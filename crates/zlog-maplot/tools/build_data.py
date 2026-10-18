"""Build the embedded map data of zlog-maplot from qth.zlog.org.

Usage: python3 build_data.py JP_JSON JCC_YAML OUT_DIR SOURCE

SOURCE names the version of the input, for example a commit and its date.
JCC/JCG codes are revised from time to time; rerun this script on the new table
and commit the output, and the version line in jcc.tsv records which one is used.

JP_JSON and JCC_YAML come from https://github.com/jg1vpp/qth.zlog.org (MIT, JA1ZLO);
the shapes there are simplified from the National Land Numerical Information
administrative boundaries. This script simplifies them further and writes:

- shapes.bin: for each municipality, its name and rings of points
  (little endian; u32 count; then per municipality u16 name length, UTF-8 name,
  u16 ring count, and per ring u32 point count and i32 pairs of 1e-4 degrees).
- jcc.tsv: a first line `# source: SOURCE`, then one line per code:
  code, name, municipality names joined by '|'.
"""

import json
import struct
import sys

TOLERANCE = 0.002  # degrees, about 200 m


def simplify(points, tol):
    """Douglas-Peucker on a list of (x, y)."""
    if len(points) < 3:
        return points
    keep = [False] * len(points)
    keep[0] = keep[-1] = True
    stack = [(0, len(points) - 1)]
    while stack:
        a, b = stack.pop()
        ax, ay = points[a]
        bx, by = points[b]
        dx, dy = bx - ax, by - ay
        norm = (dx * dx + dy * dy) ** 0.5
        best, index = 0.0, None
        for i in range(a + 1, b):
            px, py = points[i]
            if norm == 0:
                d = ((px - ax) ** 2 + (py - ay) ** 2) ** 0.5
            else:
                d = abs(dy * px - dx * py + bx * ay - by * ax) / norm
            if d > best:
                best, index = d, i
        if index is not None and best > tol:
            keep[index] = True
            stack += [(a, index), (index, b)]
    return [p for p, k in zip(points, keep) if k]


def name_of(props):
    """Same key as qth.zlog.org's index.js."""
    pref, city, ward = props["N03_001"], props.get("N03_003"), props.get("N03_004")
    if ward is not None and city is not None:
        ward = ward.replace(city, city + " ")
    ward = (ward or "None").replace("七ヶ宿町", "七ケ宿町")
    return f"{pref} {ward}"


def parse_jcc(text):
    """The YAML has only `'code':`, `  name: x`, `  cities:` and `  - city` lines."""
    entries, code = {}, None
    for line in text.splitlines():
        if line.startswith("'"):
            code = line.split("'")[1]
            entries[code] = {"name": "", "cities": []}
        elif line.startswith("  name:"):
            entries[code]["name"] = line.split(":", 1)[1].strip()
        elif line.startswith("  - "):
            entries[code]["cities"].append(line[4:].strip())
    return entries


def main(jp_json, jcc_yaml, out_dir, source):
    with open(jp_json, encoding="utf-8") as f:
        features = json.load(f)["features"]
    shapes = {}
    for feat in features:
        geom = feat["geometry"]
        polys = [geom["coordinates"]] if geom["type"] == "Polygon" else geom["coordinates"]
        rings = shapes.setdefault(name_of(feat["properties"]), [])
        for poly in polys:
            outer = simplify([tuple(p) for p in poly[0]], TOLERANCE)
            if len(outer) >= 4:
                rings.append(outer)
    out = [struct.pack("<I", len(shapes))]
    points = 0
    for name, rings in shapes.items():
        raw = name.encode("utf-8")
        out.append(struct.pack("<H", len(raw)) + raw + struct.pack("<H", len(rings)))
        for ring in rings:
            out.append(struct.pack("<I", len(ring)))
            out.append(b"".join(struct.pack("<ii", round(x * 1e4), round(y * 1e4)) for x, y in ring))
            points += len(ring)
    with open(f"{out_dir}/shapes.bin", "wb") as f:
        f.write(b"".join(out))
    with open(jcc_yaml, encoding="utf-8") as f:
        jcc = parse_jcc(f.read())
    with open(f"{out_dir}/jcc.tsv", "w", encoding="utf-8") as f:
        f.write(f"# source: {source}\n")
        for code, e in jcc.items():
            f.write(f"{code}\t{e['name']}\t{'|'.join(e['cities'])}\n")
    print(f"{len(shapes)} municipalities, {points} points, {len(jcc)} codes")


if __name__ == "__main__":
    main(*sys.argv[1:5])
