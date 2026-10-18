//! Embedded municipality shapes and the JCC/JCG code table, built by `tools/build_data.py`.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use geo::{LineString, Polygon};
use zlog_core::Log;

const SHAPES: &[u8] = include_bytes!("../data/shapes.bin");
const JCC: &str = include_str!("../data/jcc.tsv");

/// One municipality: its name (prefecture and municipality, separated by a space) and its polygons in degrees
/// (x is longitude, y is latitude).
pub struct Shape {
    pub name: String,
    pub polygons: Vec<Polygon<f64>>,
}

/// A JCC, JCG or prefecture code with its name and the shapes it covers.
pub struct Area {
    pub name: String,
    pub shapes: Vec<usize>,
}

pub struct MapData {
    pub shapes: Vec<Shape>,
    pub areas: HashMap<String, Area>,
}

/// Version of the code table, from the first line of `jcc.tsv`.
///
/// JCC/JCG codes are revised from time to time; codes not in this table are
/// counted but not drawn.
pub fn source() -> &'static str {
    JCC.lines()
        .next()
        .and_then(|l| l.strip_prefix("# source: "))
        .unwrap_or("unknown")
}

pub fn map_data() -> &'static MapData {
    static DATA: OnceLock<MapData> = OnceLock::new();
    DATA.get_or_init(|| {
        let shapes = parse_shapes(SHAPES);
        let index: HashMap<&str, usize> = shapes
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.as_str(), i))
            .collect();
        let areas = JCC
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|line| {
                let mut cols = line.split('\t');
                let (code, name, cities) = (cols.next()?, cols.next()?, cols.next()?);
                let mut shapes: Vec<usize> = cities
                    .split('|')
                    .flat_map(|c| resolve(c, &index, &shapes))
                    .collect();
                // A city may be listed twice in the source table.
                shapes.sort_unstable();
                shapes.dedup();
                Some((
                    code.to_string(),
                    Area {
                        name: name.to_string(),
                        shapes,
                    },
                ))
            })
            .collect();
        MapData { shapes, areas }
    })
}

/// Shapes for a municipality name; a designated city as a whole means all its wards.
fn resolve(city: &str, index: &HashMap<&str, usize>, shapes: &[Shape]) -> Vec<usize> {
    if let Some(&i) = index.get(city) {
        return vec![i];
    }
    let wards = format!("{city} ");
    shapes
        .iter()
        .enumerate()
        .filter(|(_, s)| s.name.starts_with(&wards))
        .map(|(i, _)| i)
        .collect()
}

fn parse_shapes(b: &[u8]) -> Vec<Shape> {
    let mut at = 0;
    let mut take = |n: usize| {
        let s = &b[at..at + n];
        at += n;
        s
    };
    let u16_ = |s: &[u8]| u16::from_le_bytes([s[0], s[1]]) as usize;
    let u32_ = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap()) as usize;
    let count = u32_(take(4));
    (0..count)
        .map(|_| {
            let len = u16_(take(2));
            let name = String::from_utf8_lossy(take(len)).into_owned();
            let polygons = (0..u16_(take(2)))
                .map(|_| {
                    let n = u32_(take(4));
                    let ring = take(n * 8)
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|p| {
                            let x = i32::from_le_bytes(p[..4].try_into().unwrap());
                            let y = i32::from_le_bytes(p[4..].try_into().unwrap());
                            (x as f64 / 1e4, y as f64 / 1e4)
                        })
                        .collect::<Vec<_>>();
                    Polygon::new(LineString::from(ring), vec![])
                })
                .collect();
            Shape { name, polygons }
        })
        .collect()
}

/// Codes received in valid QSOs with how often, as maplot does: the first run of digits.
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
    use zlog_core::Qso;

    #[test]
    fn data_loads_and_resolves() {
        let d = map_data();
        assert_eq!(d.shapes.len(), 1908);
        let chiyoda = &d.areas["100101"];
        assert_eq!(chiyoda.name, "東京都 千代田区");
        assert_eq!(chiyoda.shapes.len(), 1);
        // Sapporo as a whole is its ten wards.
        let sapporo = d
            .areas
            .values()
            .find(|a| a.name == "北海道 札幌市")
            .unwrap();
        assert_eq!(sapporo.shapes.len(), 10);
        assert!(source().starts_with("jg1vpp/qth.zlog.org"));
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
