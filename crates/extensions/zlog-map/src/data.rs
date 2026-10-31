use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use geo::{Area as _, Contains, Coord, LineString, Point, Polygon};
use zlog_core::Log;

const SHAPES: &[u8] = include_bytes!("../data/shapes.bin");
const JCC: &str = include_str!("../data/jcc.tsv");

pub type Ring = Vec<u32>;

pub struct Shape {
    pub code: String,
    pub rings: Vec<Vec<Ring>>,
    pub polygons: Vec<Polygon<f64>>,
}

pub struct Area {
    pub name: String,
    pub shapes: Vec<usize>,
}

pub struct MapData {
    pub arcs: Vec<Vec<[i32; 2]>>,
    pub bounds: Vec<[i32; 4]>,
    pub shapes: Vec<Shape>,
    pub areas: HashMap<String, Area>,
}

pub fn source() -> &'static str {
    JCC.lines()
        .next()
        .and_then(|l| l.strip_prefix("# source: "))
        .unwrap_or("unknown")
}

pub fn map_data() -> &'static MapData {
    static DATA: OnceLock<MapData> = OnceLock::new();
    DATA.get_or_init(|| {
        let (arcs, shapes) = parse_shapes(SHAPES);
        let index: HashMap<&str, usize> = shapes
            .iter()
            .enumerate()
            .map(|(i, s)| (s.code.as_str(), i))
            .collect();
        let areas = JCC
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|line| {
                let mut cols = line.split('\t');
                let (code, name, cities) = (cols.next()?, cols.next()?, cols.next()?);
                let shapes = cities
                    .split('|')
                    .filter_map(|c| index.get(c).copied())
                    .collect();
                Some((
                    code.to_string(),
                    Area {
                        name: name.to_string(),
                        shapes,
                    },
                ))
            })
            .collect();
        let bounds = arcs
            .iter()
            .map(|a| {
                a.iter()
                    .fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |b, p| {
                        [
                            b[0].min(p[0]),
                            b[1].min(p[1]),
                            b[2].max(p[0]),
                            b[3].max(p[1]),
                        ]
                    })
            })
            .collect();
        MapData {
            arcs,
            bounds,
            shapes,
            areas,
        }
    })
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> &[u8] {
        self.at += n;
        &self.b[self.at - n..self.at]
    }

    fn u32(&mut self) -> usize {
        u32::from_le_bytes(self.bytes(4).try_into().unwrap()) as usize
    }

    fn var(&mut self) -> u64 {
        let (mut n, mut shift) = (0u64, 0);
        loop {
            let b = self.bytes(1)[0];
            n |= ((b & 0x7f) as u64) << shift;
            if b < 0x80 {
                return n;
            }
            shift += 7;
        }
    }

    fn zig(&mut self) -> i32 {
        let n = self.var();
        ((n >> 1) as i64 ^ -((n & 1) as i64)) as i32
    }
}

fn parse_shapes(b: &[u8]) -> (Vec<Vec<[i32; 2]>>, Vec<Shape>) {
    let mut r = Reader { b, at: 0 };
    let arcs: Vec<Vec<[i32; 2]>> = (0..r.u32())
        .map(|_| {
            let mut p = [0, 0];
            (0..r.var())
                .map(|_| {
                    p = [p[0] + r.zig(), p[1] + r.zig()];
                    p
                })
                .collect()
        })
        .collect();
    let shapes = (0..r.u32())
        .map(|_| {
            let len = u16::from_le_bytes(r.bytes(2).try_into().unwrap()) as usize;
            let code = String::from_utf8_lossy(r.bytes(len)).into_owned();
            let rings: Vec<Vec<Ring>> = (0..r.var())
                .map(|_| {
                    (0..r.var())
                        .map(|_| (0..r.var()).map(|_| r.var() as u32).collect())
                        .collect()
                })
                .collect();
            let polygons = rings
                .iter()
                .map(|p| {
                    let mut rs = p.iter().map(|ring| line(&arcs, ring));
                    let outer = rs.next().unwrap();
                    Polygon::new(outer, rs.collect())
                })
                .collect();
            Shape {
                code,
                rings,
                polygons,
            }
        })
        .collect();
    (arcs, shapes)
}

fn points(arcs: &[Vec<[i32; 2]>], refs: &[u32]) -> Vec<[i32; 2]> {
    let mut out: Vec<[i32; 2]> = Vec::new();
    for &r in refs {
        let arc = &arcs[(r >> 1) as usize];
        let mut add = |p: &[i32; 2]| {
            if out.last() != Some(p) {
                out.push(*p);
            }
        };
        if r & 1 == 1 {
            arc.iter().rev().for_each(&mut add);
        } else {
            arc.iter().for_each(&mut add);
        }
    }
    out
}

fn line(arcs: &[Vec<[i32; 2]>], refs: &[u32]) -> LineString<f64> {
    points(arcs, refs)
        .into_iter()
        .map(|[x, y]| Coord {
            x: x as f64 / 1e4,
            y: y as f64 / 1e4,
        })
        .collect()
}

/// The arcs two members of shapes share, inside their union.
pub fn inner_arcs(shapes: &[usize]) -> Vec<u32> {
    let mut n: HashMap<u32, usize> = HashMap::new();
    for &s in shapes {
        for r in map_data().shapes[s].rings.iter().flatten().flatten() {
            *n.entry(r >> 1).or_default() += 1;
        }
    }
    n.into_iter()
        .filter(|&(_, c)| c > 1)
        .map(|(a, _)| a)
        .collect()
}

/// The outline of the union of shapes: the arcs its members use once, stitched into rings.
pub fn outline(shapes: &[usize]) -> Vec<Ring> {
    let d = map_data();
    let mut net: BTreeMap<u32, i32> = BTreeMap::new();
    for &s in shapes {
        for r in d.shapes[s].rings.iter().flatten().flatten() {
            *net.entry(r >> 1).or_default() += if r & 1 == 1 { -1 } else { 1 };
        }
    }
    let ends = |r: u32| {
        let arc = &d.arcs[(r >> 1) as usize];
        let (a, b) = (arc[0], arc[arc.len() - 1]);
        if r & 1 == 1 {
            (b, a)
        } else {
            (a, b)
        }
    };
    let mut from: HashMap<[i32; 2], Vec<u32>> = HashMap::new();
    let refs: Vec<u32> = net
        .iter()
        .filter(|(_, n)| **n != 0)
        .map(|(a, n)| a * 2 + (*n < 0) as u32)
        .collect();
    for &r in refs.iter().rev() {
        from.entry(ends(r).0).or_default().push(r);
    }
    let mut rings = Vec::new();
    for &first in &refs {
        let start = ends(first).0;
        let Some(list) = from.get_mut(&start) else {
            continue;
        };
        let Some(i) = list.iter().position(|&r| r == first) else {
            continue;
        };
        list.swap_remove(i);
        let mut ring = vec![first];
        let mut at = ends(first).1;
        while at != start {
            match from.get_mut(&at).and_then(|l| l.pop()) {
                Some(r) => {
                    ring.push(r);
                    at = ends(r).1;
                }
                None => break,
            }
        }
        rings.push(ring);
    }
    rings
}

/// The union of shapes as polygons with holes, built from its outline.
pub fn dissolve(shapes: &[usize]) -> Vec<Polygon<f64>> {
    let d = map_data();
    let mut outers: Vec<Polygon<f64>> = Vec::new();
    let mut holes: Vec<LineString<f64>> = Vec::new();
    for ring in outline(shapes) {
        let l = line(&d.arcs, &ring);
        if l.0.len() < 4 {
            continue;
        }
        let p = Polygon::new(l, vec![]);
        if p.signed_area() > 0.0 {
            outers.push(p);
        } else {
            holes.push(p.exterior().clone());
        }
    }
    let mut inner: Vec<Vec<LineString<f64>>> = vec![Vec::new(); outers.len()];
    for hole in holes {
        let owner = hole
            .points()
            .find_map(|pt| {
                outers
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.contains(&pt))
                    .min_by(|a, b| a.1.unsigned_area().total_cmp(&b.1.unsigned_area()))
                    .map(|(i, _)| i)
            })
            .or_else(|| (!outers.is_empty()).then_some(0));
        if let Some(i) = owner {
            inner[i].push(hole);
        }
    }
    outers
        .into_iter()
        .zip(inner)
        .map(|(o, h)| Polygon::new(o.exterior().clone(), h))
        .collect()
}

pub fn ring_points(ring: &[u32]) -> Vec<(f64, f64)> {
    line(&map_data().arcs, ring)
        .points()
        .map(|p: Point<f64>| (p.x(), p.y()))
        .collect()
}

pub fn worked_codes(log: &Log) -> BTreeMap<String, usize> {
    let mut codes = BTreeMap::new();
    for q in log.qsos.iter().filter(|q| !q.invalid) {
        let digits: String = q
            .nr_rcvd
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            *codes.entry(digits).or_insert(0) += 1;
        }
    }
    codes
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::InteriorPoint;
    use zlog_core::Qso;

    fn area(code: &str) -> Vec<&str> {
        let d = map_data();
        d.areas[code]
            .shapes
            .iter()
            .map(|&s| d.shapes[s].code.as_str())
            .collect()
    }

    #[test]
    fn data_loads_and_resolves() {
        let d = map_data();
        assert_eq!(d.shapes.len(), 1905);
        assert_eq!(d.areas["100101"].name, "東京都 千代田区");
        assert_eq!(area("100101"), ["13101"]);
        assert_eq!(d.areas["0101"].name, "北海道 札幌市");
        assert_eq!(area("0101").len(), 10);
        assert_eq!(area("1112"), ["14211"]);
        assert!(!source().is_empty() && source() != "unknown");
    }

    #[test]
    fn hamamatsu_has_its_new_wards() {
        let d = map_data();
        assert_eq!(d.areas["180208"].name, "静岡県 浜松市 中央区");
        assert_eq!(area("180208"), ["22138"]);
        assert_eq!(d.areas["180209"].name, "静岡県 浜松市 浜名区");
        assert_eq!(area("180209"), ["22139"]);
        assert_eq!(area("1802"), ["22138", "22139", "22140"]);
    }

    fn uses(shapes: &[usize]) -> HashMap<u32, usize> {
        let mut n = HashMap::new();
        for &s in shapes {
            for r in map_data().shapes[s].rings.iter().flatten().flatten() {
                *n.entry(r >> 1).or_default() += 1;
            }
        }
        n
    }

    fn closed(ring: &[u32]) -> bool {
        let p = ring_points(ring);
        p.len() >= 4 && p[0] == p[p.len() - 1]
    }

    #[test]
    fn hokkaido_outline_has_no_internal_edges() {
        let shapes = &map_data().areas["01"].shapes;
        let n = uses(shapes);
        let rings = outline(shapes);
        assert!(rings.iter().all(|r| closed(r)));
        let arcs: Vec<u32> = rings.iter().flatten().map(|r| r >> 1).collect();
        assert!(arcs.iter().all(|a| n[a] == 1));
        assert_eq!(arcs.len(), n.values().filter(|&&c| c == 1).count());
        assert!(n.values().any(|&c| c == 2));
        let polygons = dissolve(shapes);
        let at = |x: f64, y: f64| polygons.iter().any(|p| p.contains(&Point::new(x, y)));
        assert!(at(141.35, 43.06) && at(142.8, 43.5));
        let kunashiri = &map_data().shapes[shapes
            .iter()
            .copied()
            .find(|&s| map_data().shapes[s].code == "01696")
            .unwrap()];
        let inside = kunashiri.polygons[0].interior_point().unwrap();
        assert!(at(inside.x(), inside.y()));
        assert!(!at(142.0, 41.6));
    }

    #[test]
    fn two_neighbours_dissolve_to_one_ring() {
        let d = map_data();
        let simple: Vec<usize> = (0..d.shapes.len())
            .filter(|&s| d.shapes[s].rings.len() == 1 && d.shapes[s].rings[0].len() == 1)
            .collect();
        let arcs =
            |s: usize| -> Vec<u32> { d.shapes[s].rings[0][0].iter().map(|r| r >> 1).collect() };
        let (a, b) = simple
            .iter()
            .flat_map(|&a| simple.iter().map(move |&b| (a, b)))
            .find(|&(a, b)| a < b && arcs(a).iter().filter(|x| arcs(b).contains(x)).count() == 1)
            .unwrap();
        let rings = outline(&[a, b]);
        assert_eq!(rings.len(), 1, "{} {}", d.shapes[a].code, d.shapes[b].code);
        assert!(closed(&rings[0]));
        let polygons = dissolve(&[a, b]);
        assert_eq!(polygons.len(), 1);
        assert!(polygons[0].interiors().is_empty());
    }

    #[test]
    fn enclaves_are_holes_until_worked_together() {
        let d = map_data();
        let (s, hole) = (0..d.shapes.len())
            .find_map(|s| {
                let ring = d.shapes[s].rings.iter().find_map(|p| p.get(1))?;
                let enclave = (0..d.shapes.len()).find(|&e| {
                    e != s
                        && d.shapes[e].rings.iter().any(|p| {
                            p[0].len() == ring.len() && p[0].iter().all(|r| ring.contains(&(r ^ 1)))
                        })
                })?;
                Some((s, enclave))
            })
            .unwrap();
        assert!(dissolve(&[s]).iter().any(|p| !p.interiors().is_empty()));
        let both = dissolve(&[s, hole]);
        let alone = dissolve(&[s]);
        let holes = |ps: &[Polygon<f64>]| ps.iter().map(|p| p.interiors().len()).sum::<usize>();
        assert_eq!(holes(&both) + 1, holes(&alone));
    }

    #[test]
    fn shared_edges_are_identical() {
        let d = map_data();
        let mut edges: HashMap<([i32; 2], [i32; 2]), usize> = HashMap::new();
        for shape in &d.shapes {
            for ring in shape.rings.iter().flatten() {
                let p = points(&d.arcs, ring);
                assert_eq!(p.first(), p.last(), "{}", shape.code);
                for w in p.windows(2) {
                    *edges.entry((w[0].min(w[1]), w[0].max(w[1]))).or_default() += 1;
                }
            }
        }
        assert!(edges.values().all(|&n| n <= 2));
        assert!(edges.values().filter(|&&n| n == 2).count() > 100_000);
    }

    #[test]
    fn every_area_has_shapes() {
        let d = map_data();
        assert!(d.areas.len() > 1400);
        for (code, a) in &d.areas {
            assert!(!a.shapes.is_empty(), "{code} {}", a.name);
        }
    }

    #[test]
    fn northern_territories_are_in_no_gun() {
        let tomari = ["01403", "01696"];
        assert_eq!(area("01062"), ["01403", "01404"]);
        let d = map_data();
        let shapes: Vec<usize> = tomari
            .iter()
            .map(|c| d.shapes.iter().position(|s| s.code == *c).unwrap())
            .collect();
        assert_ne!(shapes[0], shapes[1]);
        assert_ne!(
            d.shapes[shapes[0]].polygons[0],
            d.shapes[shapes[1]].polygons[0]
        );
        assert!(!area("01023").contains(&"01698"));
        assert!(!area("01036").contains(&"01699"));
        assert!(!area("01059").contains(&"01700"));
        let territories = ["01695", "01696", "01697", "01698", "01699", "01700"];
        for (code, a) in &d.areas {
            if code.len() == 5 {
                assert!(a
                    .shapes
                    .iter()
                    .all(|&s| !territories.contains(&d.shapes[s].code.as_str())));
            }
        }
        assert!(territories.iter().all(|t| area("01").contains(t)));
    }

    #[test]
    fn codes_are_counted_from_numbers() {
        let qso = |nr: &str, invalid| Qso {
            nr_rcvd: nr.into(),
            invalid,
            ..Qso::default()
        };
        let log = Log {
            qsos: vec![
                qso("100101H", false),
                qso("100101M", false),
                qso("10P", true),
            ],
            ..Log::default()
        };
        let codes = worked_codes(&log);
        assert_eq!(codes.len(), 1);
        assert_eq!(codes["100101"], 2);
    }
}
