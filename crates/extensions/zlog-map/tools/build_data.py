"""Build the embedded map data of zlog-map.

Usage: python3 -I build_data.py N03_GEOJSON ACAG_DAT JARL_DIR OUT_DIR SOURCE [YAML]

N03_GEOJSON is the nationwide GeoJSON of the National Land Numerical Information
administrative boundaries (国土数値情報 行政区域データ N03, format v3.1), such as
N03-20250101.geojson from N03-20250101_GML.zip at
https://nlftp.mlit.go.jp/ksj/gml/datalist/KsjTmplt-N03-2025.html.
Its features carry N03_001 to N03_005 and the administrative code N03_007;
the features of one N03_007 (islands) make one municipality.
ACAG_DAT is the JCC/JCG list zLog bundles (zlog-rule-ja/data/ACAG.DAT, UTF-8 or
Shift_JIS), and JARL_DIR holds jcc-list.txt, jcg-list.txt and ku-list.txt as
published at https://www.jarl.org/Japanese/A_Shiryo/A-2_jcc-jcg/.
SOURCE names the versions of these inputs and goes into the first line of jcc.tsv.

The codes are those of ACAG.DAT plus the cities with wards and the prefectures.
A JCC city or ward maps to the municipalities of that city or ward, a city with
wards to all its wards, a JCG to the municipalities of that gun in that prefecture
(in Hokkaido also within the 振興局 in parentheses), and a prefecture to all its
municipalities. The JARL lists cross-check the codes and the JCG members.

The boundaries keep their topology: the rings are split into arcs at the vertices
where three or more boundaries meet, each arc is simplified once, and the rings
are rebuilt from the arcs, so that neighbours share identical edges.
The script stops unless every check holds. It writes:

- shapes.bin, little endian, with unsigned LEB128 varints (v) and zigzag varints (z):
  u32 arc count, then per arc v point count and z coordinates in 1e-4 degrees,
  the first point absolute and the others as deltas;
  u32 municipality count, then per municipality u16 code length, ASCII N03_007,
  v polygon count, per polygon v ring count (outer first, then holes), and per ring
  v arc count and v references 2 * arc + reversed. Outer rings run counterclockwise.
- jcc.tsv: a first line `# source: SOURCE`, then one line per code:
  code, name, N03_007 codes joined by '|'.
- YAML, if given: the same table in the format of https://qth.zlog.org/jcc.yaml,
  with the municipalities named as its index.js names the features.
"""

import collections
import json
import re
import struct
import sys

TOLERANCE = 0.0003  # degrees, about 30 m
FINEST = 16  # the largest divisor of the tolerance for arcs that overlap
ISLET = 0.0025  # degrees, the side of the smallest kept islet
GRID = 1e4  # stored units per degree
SCALE = 10**9  # input units per degree

NO_SHAPE = {}

NO_AREA = {
    "01695": "色丹郡 has no JCG code",
    "01696": "国後郡 has no JCG code",
    "01697": "国後郡 has no JCG code",
    "01698": "択捉郡 has no JCG code",
    "01699": "紗那郡 has no JCG code",
    "01700": "蘂取郡 has no JCG code",
}
UNASSIGNED = "所属未定地"

# The Tokyo islands by the first four digits of N03_007; JCG counts them by 支庁,
# which N03 v3.1 no longer records.
SUBPREFECTURES = {"1336": "大島支庁", "1338": "三宅支庁", "1340": "八丈支庁", "1342": "小笠原支庁"}


def norm(s):
    return s.replace("ヶ", "ケ").replace("（", "(").replace("）", ")").replace(" ", "")


def read(path):
    raw = open(path, "rb").read()
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        return raw.decode("cp932")


def parse_acag(text):
    codes = {}
    for line in text.splitlines()[1:]:
        cols = line.split()
        if len(cols) == 2 and cols[0].isdigit():
            codes[cols[0]] = cols[1]
    return codes


def parse_jarl(text):
    """Current entries {code: kanji}, members {code: [kanji]} and prefectures {code: name}."""
    current, members, prefs, last = {}, collections.defaultdict(list), {}, None
    for line in text.splitlines():
        m = re.match(r"^(\*?)\s*(\d{4,6})(※?)\s+(.*)$", line)
        if m:
            words = [w for w in m[4].split() if not w.isascii()]
            dead = m[1] or m[3] or '"' in m[4] or "以前" in m[4]
            last = None if dead else m[2]
            if last:
                current[last] = words[0]
            continue
        m = re.match(r"^(\S[^\d(（]*?)\s*[(（]?(\d{2,4})[)）]?\s*$", line)
        if m:
            if len(m[2]) == 2:
                prefs[m[2]] = norm(m[1])
            last = None
            continue
        words = line.split()
        if last and len(words) == 2 and words[0].isascii() and not words[1].isascii():
            members[last].append(words[1])
    return current, members, prefs


def parse_wards(text):
    """Cities with wards {code: name} from the headers of ku-list.txt."""
    return {m[2]: m[1] for m in re.finditer(r"^(\S+市)[(（](\d{4})[)）]", text, re.M)}


def fail(problems):
    if problems:
        sys.exit("build_data.py: " + "\n  ".join(["checks failed:"] + problems[:200]))


def resolve(code, name, pref, recs, parents):
    """The records a code maps to."""
    mine = [r for r in recs if r["N03_001"] == pref]
    if len(code) == 5:
        gun = re.match(r"^(.+?)(?:\((.+)\))?$", norm(name))
        return [
            r
            for r in mine
            if r["N03_003"] == gun[1]
            and (gun[2] is None or (r["N03_002"] or "").startswith(gun[2]))
        ]
    if code in parents:
        return [r for r in mine if r["N03_003"] == name]
    return [r for r in mine if norm(r["N03_004"]) == norm(name)]


def load(path):
    """Municipalities in input order, each with its attributes and polygons of integer rings.

    N03_003 and N03_004 follow the older layout: N03_003 is the gun or the city with
    wards, N03_004 the municipality with the city prefixed to a ward.
    """
    with open(path, encoding="utf-8") as f:
        features = json.load(f)["features"]
    recs, problems = {}, []
    for feat in features:
        p = feat["properties"]
        code = p.get("N03_007")
        if not code or not p.get("N03_004"):
            problems.append(f"record without a code or name: {p}")
            continue
        city, ward = p["N03_004"], p.get("N03_005")
        attrs = {
            "N03_001": p["N03_001"],
            "N03_002": p.get("N03_002"),
            "N03_003": city if ward else p.get("N03_003") or SUBPREFECTURES.get(code[:4]),
            "N03_004": city + ward if ward else city,
            "N03_007": code,
        }
        rec = recs.setdefault(code, dict(attrs, polygons=[]))
        if any(rec[k] != v for k, v in attrs.items()):
            problems.append(f"{code} has differing attributes: {attrs}")
        geom = feat["geometry"]
        polys = [geom["coordinates"]] if geom["type"] == "Polygon" else geom["coordinates"]
        for poly in polys:
            rings = []
            for i, ring in enumerate(poly):
                pts = []
                for x, y in ring:
                    q = (round(x * SCALE) << 38) | round(y * SCALE)
                    if not pts or pts[-1] != q:
                        pts.append(q)
                if len(pts) > 1 and pts[0] == pts[-1]:
                    pts.pop()
                if len(pts) < 3 or area(pts) == 0:
                    if i == 0:
                        break
                    continue
                if (area(pts) > 0) != (i == 0):
                    pts.reverse()
                rings.append(pts)
            if rings:
                rec["polygons"].append(rings)
    fail(problems)
    return list(recs.values())


def xy(q):
    return q >> 38, q & ((1 << 38) - 1)


def area(pts):
    s = 0
    for i in range(len(pts)):
        x0, y0 = xy(pts[i - 1])
        x1, y1 = xy(pts[i])
        s += x0 * y1 - x1 * y0
    return s


def topology(recs):
    """Arcs (lists of vertices) and, per polygon ring, a list of arc references."""
    degree = collections.Counter()
    edges = set()
    for r in recs:
        for poly in r["polygons"]:
            for ring in poly:
                for i in range(len(ring)):
                    a, b = ring[i - 1], ring[i]
                    e = (a, b) if a < b else (b, a)
                    if e not in edges:
                        edges.add(e)
                        degree[a] += 1
                        degree[b] += 1
    shared = collections.Counter()
    for r in recs:
        for poly in r["polygons"]:
            for ring in poly:
                for i in range(len(ring)):
                    a, b = ring[i - 1], ring[i]
                    shared[(a, b) if a < b else (b, a)] += 1
    stats = collections.Counter(shared.values())
    del shared
    nodes = {v for v, d in degree.items() if d != 2}
    del degree, edges

    arcs, index = [], {}
    for r in recs:
        refs_polys = []
        for poly in r["polygons"]:
            refs_poly = []
            for ring in poly:
                n = len(ring)
                starts = [i for i in range(n) if ring[i] in nodes]
                if not starts:
                    m = min(range(n), key=ring.__getitem__)
                    ring = ring[m:] + ring[:m]
                    if ring[-1] < ring[1]:
                        ring = ring[:1] + ring[1:][::-1]
                        flip = True
                    else:
                        flip = False
                    key = (ring[0], ring[1])
                    if key not in index:
                        index[key] = (len(arcs), False)
                        index[(ring[0], ring[-1])] = (len(arcs), True)
                        arcs.append(ring + ring[:1])
                    i, rev = index[key]
                    refs_poly.append([2 * i + (rev != flip)])
                    continue
                refs = []
                for k, s in enumerate(starts):
                    e = starts[k + 1] if k + 1 < len(starts) else starts[0] + n
                    chain = [ring[j % n] for j in range(s, e + 1)]
                    key = (chain[0], chain[1])
                    if key not in index:
                        index[key] = (len(arcs), False)
                        index[(chain[-1], chain[-2])] = (len(arcs), True)
                        arcs.append(chain)
                    i, rev = index[key]
                    refs.append(2 * i + rev)
                refs_poly.append(refs)
            refs_polys.append(refs_poly)
        r["refs"] = refs_polys
    return arcs, stats


def dp(pts, tol):
    """Douglas-Peucker on integer vertices, endpoints kept."""
    if len(pts) < 3:
        return pts
    c = [xy(p) for p in pts]
    keep = [False] * len(c)
    keep[0] = keep[-1] = True
    stack = [(0, len(c) - 1)]
    while stack:
        a, b = stack.pop()
        ax, ay = c[a]
        bx, by = c[b]
        dx, dy = bx - ax, by - ay
        n2 = dx * dx + dy * dy
        best, index = 0.0, None
        for i in range(a + 1, b):
            px, py = c[i]
            if n2 == 0:
                d = (px - ax) ** 2 + (py - ay) ** 2
            else:
                d = (dy * px - dx * py + bx * ay - by * ax) ** 2 / n2
            if d > best:
                best, index = d, i
        if index is not None and best > tol * tol:
            keep[index] = True
            stack += [(a, index), (index, b)]
    return [p for p, k in zip(pts, keep) if k]


def farthest(pts, a, b):
    """The inner vertex of pts farthest from the line through a and b."""
    (ax, ay), (bx, by) = xy(a), xy(b)
    if a == b:
        return max(pts[1:-1], key=lambda p: (xy(p)[0] - ax) ** 2 + (xy(p)[1] - ay) ** 2)
    return max(pts[1:-1], key=lambda p: abs((bx - ax) * (xy(p)[1] - ay) - (by - ay) * (xy(p)[0] - ax)))


def thin(pts, t):
    """Drops vertices closer than t to the last kept one, endpoints kept."""
    out = [pts[0]]
    lx, ly = xy(pts[0])
    for p in pts[1:-1]:
        x, y = xy(p)
        if (x - lx) ** 2 + (y - ly) ** 2 >= t * t:
            out.append(p)
            lx, ly = x, y
    out.append(pts[-1])
    return out


def simplify(arcs, tol, lone, finer, ends, prev=None):
    """Each arc simplified once and snapped to the grid; a dropped arc is None.

    Closed arcs in lone, the islets used by a single ring, are dropped when smaller
    than a square of side ISLET. An arc in finer uses the tolerance divided by its
    value; with prev, only the arcs in finer are redone.
    """
    step = SCALE / GRID
    out = []
    for k, arc in enumerate(arcs):
        if prev is not None and k not in finer:
            out.append(prev[k])
            continue
        t = tol * SCALE / finer.get(k, 1)
        if k in lone and arc[0] == arc[-1] and abs(area(arc[:-1])) < 2 * (ISLET * SCALE) ** 2:
            out.append(None)
            continue
        s = dp(thin(arc, t / 3), t)
        closed = arc[0] == arc[-1]
        if closed and len(s) < 4 and len(arc) >= 4:
            far = farthest(arc, arc[0], arc[0])
            third = farthest(arc, arc[0], far)
            s = [p for i, p in enumerate(arc) if p in (far, third) or i in (0, len(arc) - 1)]
        elif not closed and len(s) == 2 and len(arc) > 2:
            key = (min(s), max(s))
            if key in ends and prev is None:
                s = [s[0], farthest(arc, s[0], s[1]), s[1]]
            ends.add(key)
        q = []
        for p in s:
            x, y = xy(p)
            g = (round(x / step), round(y / step))
            if not q or q[-1] != g:
                q.append(g)
        if closed and (len(q) < 4 or abs(area_xy(q[:-1])) == 0):
            out.append(None)
        elif not closed and len(q) < 2:
            out.append(None)
        else:
            out.append(q)
    return out


def crossings(simple):
    """Arcs with a segment that properly crosses another segment."""
    cell = 50
    buckets = collections.defaultdict(list)
    segs = []
    for i, arc in enumerate(simple):
        for a, b in zip(arc or [], (arc or [])[1:]):
            n = len(segs)
            segs.append((i, a, b))
            for cx in range(min(a[0], b[0]) // cell, max(a[0], b[0]) // cell + 1):
                for cy in range(min(a[1], b[1]) // cell, max(a[1], b[1]) // cell + 1):
                    buckets[(cx, cy)].append(n)

    def orient(p, q, r):
        v = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
        return (v > 0) - (v < 0)

    bad = set()
    for members in buckets.values():
        for j in range(len(members)):
            i1, a, b = segs[members[j]]
            for k in range(j + 1, len(members)):
                i2, c, d = segs[members[k]]
                if (i1 in bad and i2 in bad) or len({a, b, c, d}) < 4:
                    continue
                if orient(a, b, c) * orient(a, b, d) < 0 and orient(c, d, a) * orient(c, d, b) < 0:
                    bad.update((i1, i2))
    return bad


def area_xy(pts):
    return sum(pts[i - 1][0] * pts[i][1] - pts[i][0] * pts[i - 1][1] for i in range(len(pts)))


def ring_points(refs, arcs):
    pts = []
    for ref in refs:
        arc = arcs[ref >> 1]
        if arc is None:
            continue
        arc = arc[::-1] if ref & 1 else arc
        if pts and pts[-1] != arc[0]:
            return None
        pts.extend(arc if not pts else arc[1:])
    return pts


def varint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def zigzag(n):
    return varint(n * 2 if n >= 0 else -n * 2 - 1)


def build_shapes(recs, tol):
    """Simplified topology, the encoded shapes.bin and a list of problems."""
    arcs, stats = topology(recs)
    print(f"{len(arcs)} arcs; edges by number of rings using them: {dict(sorted(stats.items()))}")
    count = collections.Counter(ref >> 1 for r in recs for poly in r["refs"] for refs in poly for ref in refs)
    lone = {i for i, n in count.items() if n == 1}
    loose = sum(1 for i in lone if arcs[i][0] != arcs[i][-1])
    print(f"{loose} open arcs used by one ring (coasts and the land border with no neighbour)")
    finer, ends, simple = {}, set(), None
    while True:
        simple = simplify(arcs, tol, lone, finer, ends, simple)
        edge_count, edge_arcs = collections.Counter(), collections.defaultdict(set)
        for i, n in count.items():
            arc = simple[i]
            for a, b in zip(arc or [], (arc or [])[1:]):
                e = (min(a, b), max(a, b))
                edge_count[e] += n
                edge_arcs[e].add(i)
        bad = {i for e, n in edge_count.items() if n > 2 for i in edge_arcs[e]}
        print(f"{len(bad)} arcs overlap after simplification")
        bad = {i for i in bad if finer.get(i, 1) < FINEST}
        if not bad:
            break
        for i in bad:
            finer[i] = finer.get(i, 1) * 4
    problems, users = [], collections.Counter()
    for r in recs:
        polys = []
        for poly in r["refs"]:
            rings = []
            for k, refs in enumerate(poly):
                refs = [ref for ref in refs if simple[ref >> 1] is not None]
                pts = ring_points(refs, simple)
                if pts is None or (pts and pts[0] != pts[-1]):
                    problems.append(f"{r['N03_007']}: ring does not close")
                    continue
                if len(set(pts)) < 3 or area_xy(pts[:-1]) == 0:
                    if k == 0:
                        break
                    continue
                rings.append(refs)
            if rings:
                polys.append(rings)
        r["final"] = polys
        if not polys and r["N03_004"] == UNASSIGNED:
            print(f"{r['N03_007']} {UNASSIGNED}: only islets, dropped")
        elif not polys:
            problems.append(f"{r['N03_007']} {r['N03_004']} has no ring left")
        for poly in polys:
            for refs in poly:
                for ref in refs:
                    users[ref >> 1] += 1

    expected, seen = collections.Counter(), collections.Counter()
    for i, n in users.items():
        arc = simple[i]
        for a, b in zip(arc, arc[1:]):
            expected[(min(a, b), max(a, b))] += n
    for r in recs:
        for poly in r["final"]:
            for refs in poly:
                pts = ring_points(refs, simple)
                for a, b in zip(pts, pts[1:]):
                    seen[(min(a, b), max(a, b))] += 1
    if seen != expected:
        problems.append("edges of the rebuilt rings differ from the arcs")
    snapped = [e for e, n in seen.items() if n > 2]
    if len(snapped) > 10:
        problems.append(f"{len(snapped)} edges are used by more than two rings")
    for e in snapped:
        print(f"distinct arcs snap together on the grid at {e}")
    over = [i for i, n in users.items() if n > 2]
    if over:
        problems.append(f"{len(over)} arcs are used by more than two rings")
    by = collections.Counter(users.values())
    print(f"{by[2]} arcs shared by two rings, {by[1]} used by one")
    print(f"{len(crossings(simple))} arcs cross another arc on the grid (filled as unions in zlog-map)")

    used = sorted(users)
    renum = {old: new for new, old in enumerate(used)}
    out = [struct.pack("<I", len(used))]
    points = 0
    for i in used:
        arc = simple[i]
        b = bytearray(varint(len(arc)))
        px = py = 0
        for x, y in arc:
            b += zigzag(x - px) + zigzag(y - py)
            px, py = x, y
        out.append(bytes(b))
        points += len(arc)
    out.append(struct.pack("<I", len(recs)))
    for r in recs:
        raw = r["N03_007"].encode("ascii")
        b = bytearray(struct.pack("<H", len(raw)) + raw)
        b += varint(len(r["final"]))
        for poly in r["final"]:
            b += varint(len(poly))
            for refs in poly:
                b += varint(len(refs))
                for ref in refs:
                    b += varint(2 * renum[ref >> 1] + (ref & 1))
        out.append(bytes(b))
    print(f"{len(used)} arcs kept, {points} points")
    return b"".join(out), problems


def main(n03, acag_dat, jarl_dir, out_dir, source, yaml=None):
    recs = load(n03)
    problems = [f"{r['N03_007']} has no polygon" for r in recs if not r["polygons"]]
    fail(problems)

    acag = parse_acag(read(acag_dat))
    jarl, members, jarl_prefs = {}, {}, {}
    for name in ["jcc-list.txt", "jcg-list.txt"]:
        current, more, prefs = parse_jarl(read(f"{jarl_dir}/{name}"))
        jarl.update(current)
        members.update(more)
        jarl_prefs.update(prefs)
    ku = read(f"{jarl_dir}/ku-list.txt")
    parents = parse_wards(ku)
    jarl.update(parse_jarl(ku)[0])
    if set(jarl) != set(acag) | set(parents):
        problems.append(f"only in ACAG.DAT: {sorted(set(acag) - set(jarl))}")
        problems.append(f"only in the JARL lists: {sorted(set(jarl) - set(acag) - set(parents))}")
    n03_prefs = sorted({r["N03_001"] for r in recs})
    prefs = {}
    for code, name in jarl_prefs.items():
        hits = [p for p in n03_prefs if p.startswith(name) or name.startswith(p)]
        if len(hits) != 1:
            problems.append(f"prefecture {code} {name} matches {hits}")
        else:
            prefs[code] = hits[0]
    if sorted(prefs.values()) != n03_prefs or len(prefs) != 47:
        problems.append(f"prefectures do not map one to one: {prefs}")
    fail(problems)

    areas, notes = {}, []
    for code in sorted(set(acag) | set(parents)):
        pref = prefs[code[:2]]
        name = parents.get(code) or acag[code]
        hit = resolve(code, name, pref, recs, parents)
        if not hit and code in jarl and len(code) != 5:
            alt = jarl[code] + ("区" if len(code) == 6 else "市")
            if len(code) == 6 and code[:4] in parents:
                alt = parents[code[:4]] + alt
            hit = resolve(code, alt, pref, recs, parents)
            if hit:
                notes.append(f"{code}: ACAG.DAT names it {name}, the JARL list and N03 {alt}")
        if code in NO_SHAPE:
            if hit:
                problems.append(f"{code} is listed in NO_SHAPE but resolves to {len(hit)} shapes")
            notes.append(f"{code}: no shape, {NO_SHAPE[code]}")
            continue
        if not hit:
            problems.append(f"{code} {name} resolves to no shape")
            continue
        if len(code) == 5:
            label = name
        elif code in parents:
            label = hit[0]["N03_003"]
        else:
            r = hit[0]
            label = r["N03_004"].replace(r["N03_003"] or "\0", (r["N03_003"] or "") + " ")
        areas[code] = (label, hit)
    for code, pref in prefs.items():
        areas[code] = (None, [r for r in recs if r["N03_001"] == pref])

    owner = collections.defaultdict(list)
    for code, (_, hit) in areas.items():
        if len(code) == 5 or (len(code) > 2 and code not in parents):
            for r in hit:
                owner[r["N03_007"]].append(code)
        if len(code) == 5:
            guns = {(r["N03_001"], r["N03_003"]) for r in hit}
            offices = {r["N03_002"] for r in hit}
            if len(guns) != 1:
                problems.append(f"JCG {code} spans several guns: {guns}")
            if "(" not in norm(acag[code]) and len(offices) > 1:
                problems.append(f"JCG {code} spans several 振興局 without telling which: {offices}")
    for r in recs:
        mine = owner.get(r["N03_007"], [])
        outside = r["N03_007"] in NO_AREA or r["N03_004"] == UNASSIGNED
        if len(mine) > 1:
            problems.append(f"{r['N03_007']} {r['N03_004']} is in {mine}")
        if not mine and not outside:
            problems.append(f"{r['N03_007']} {r['N03_003']} {r['N03_004']} is in no JCC/JCG")
        if mine and outside:
            problems.append(f"{r['N03_007']} is outside any JCC/JCG but is in {mine}")
    for code, towns in members.items():
        if code not in areas:
            continue
        pref = prefs[code[:2]]
        mine = {norm(r["N03_004"]) for r in areas[code][1]}
        for town in map(norm, towns):
            elsewhere = [r for r in recs if r["N03_001"] == pref and norm(r["N03_004"]) == town]
            if town in mine:
                continue
            if elsewhere:
                problems.append(f"JARL lists {town} in JCG {code}, N03 puts it elsewhere")
            else:
                notes.append(f"{code}: JARL member {town} is no N03 municipality")
    fail(problems)

    blob, problems = build_shapes(recs, TOLERANCE)
    fail(problems)
    with open(f"{out_dir}/shapes.bin", "wb") as f:
        f.write(blob)

    def display(code, label):
        pref = prefs[code[:2]]
        pref += "" if pref.endswith(("都", "道", "府", "県")) else "県"
        return pref if label is None else f"{pref} {label}"

    rows = sorted(areas.items())
    with open(f"{out_dir}/jcc.tsv", "w", encoding="utf-8") as f:
        f.write(f"# source: {source}\n")
        for code, (label, hit) in rows:
            f.write(f"{code}\t{display(code, label)}\t{'|'.join(r['N03_007'] for r in hit)}\n")
    if yaml:
        with open(yaml, "w", encoding="utf-8") as f:
            f.write("'JA':\n  cities:\n  - JA\n")
            for code, (label, hit) in rows:
                f.write(f"'{code}':\n  name: {display(code, label)}\n  cities:\n")
                names = []
                for r in hit:
                    city, ward = r["N03_003"], r["N03_004"]
                    ward = ward.replace(city, city + " ") if city else ward
                    names.append(f"{r['N03_001']} {ward}".replace("七ヶ宿町", "七ケ宿町"))
                for name in dict.fromkeys(names):
                    f.write(f"  - {name}\n")
    for note in notes:
        print(note)
    print(f"{len(recs)} municipalities, {len(blob)} bytes of shapes, {len(areas)} codes")


if __name__ == "__main__":
    main(*sys.argv[1:7])
