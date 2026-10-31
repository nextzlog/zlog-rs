use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::LazyLock;

use zlog_core::{Band, Log, Mode, Qso};

pub fn partial_match(fragment: &str, call: &str) -> bool {
    if !fragment.contains('.') {
        return call.contains(fragment);
    }
    let (f, c) = (fragment.as_bytes(), call.as_bytes());
    f.len() <= c.len() && f.iter().zip(c).all(|(a, b)| *a == b'.' || a == b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Time,
    Band,
    Call,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialOptions {
    pub all_bands: bool,
    pub sort: SortBy,
    pub this_band_first: bool,
    pub max: usize,
}

impl Default for PartialOptions {
    fn default() -> Self {
        PartialOptions {
            all_bands: true,
            sort: SortBy::Time,
            this_band_first: false,
            max: 200,
        }
    }
}

pub fn partial_check(log: &Log, fragment: &str, band: Band, opt: &PartialOptions) -> Vec<usize> {
    if fragment.is_empty() || fragment.starts_with(',') {
        return Vec::new();
    }
    partial_list(log, band, opt, |q| partial_match(fragment, &q.callsign))
}

pub fn partial_check_number(
    log: &Log,
    number: &str,
    band: Band,
    opt: &PartialOptions,
) -> Vec<usize> {
    if number.is_empty() {
        return Vec::new();
    }
    partial_list(log, band, opt, |q| q.nr_rcvd.contains(number))
}

fn partial_list(
    log: &Log,
    band: Band,
    opt: &PartialOptions,
    hit: impl Fn(&Qso) -> bool,
) -> Vec<usize> {
    let mut hits: Vec<usize> = log
        .qsos
        .iter()
        .enumerate()
        .filter(|(_, q)| hit(q))
        .filter(|(_, q)| opt.all_bands || q.band == band)
        .map(|(i, _)| i)
        .take(opt.max.saturating_add(1))
        .collect();
    let q = |i: usize| &log.qsos[i];
    match opt.sort {
        SortBy::Time => hits.sort_by(|a, b| q(*a).time.0.total_cmp(&q(*b).time.0)),
        SortBy::Band => hits.sort_by_key(|i| q(*i).band),
        SortBy::Call => hits
            .sort_by(|a, b| compare_text(core_call(&q(*a).callsign), core_call(&q(*b).callsign))),
    }
    if opt.this_band_first {
        hits.sort_by_key(|i| q(*i).band != band);
    }
    hits
}

fn compare_text(a: &str, b: &str) -> Ordering {
    let up = |s: &str| {
        s.bytes()
            .map(|c| c.to_ascii_uppercase())
            .collect::<Vec<u8>>()
    };
    up(a).cmp(&up(b))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub callsign: String,
    pub numbers: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Database {
    entries: Vec<Entry>,
    pairs: HashMap<[u8; 2], Vec<u32>>,
    packed: Vec<u8>,
    starts: Vec<usize>,
    by_len: Vec<Vec<u32>>,
    masks: Vec<u64>,
}

pub fn parse_spc(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty() && !l.starts_with([';', '#', '!']))
        .filter_map(|l| {
            let (c, n) = match l.find(' ') {
                Some(i) => {
                    let n: String = l[i..].chars().take(30).collect();
                    (&l[..i], n.trim_start().to_string())
                }
                None => (l, String::new()),
            };
            // Departure: zLog keeps the call as written; it is upper-cased here on
            // purpose, since the entry row is upper case and matching is byte-wise.
            (c.is_ascii() && !c.is_empty()).then(|| (c.to_ascii_uppercase(), n))
        })
        .collect()
}

pub fn log_records(log: &Log) -> Vec<(String, String)> {
    log.qsos
        .iter()
        .map(|q: &Qso| (q.callsign.clone(), q.nr_rcvd.clone()))
        .filter(|(c, _)| !c.is_empty() && c.is_ascii())
        .collect()
}

impl Database {
    pub fn new(records: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut by_call: HashMap<String, Vec<String>> = HashMap::new();
        for (c, n) in records {
            let numbers = by_call.entry(c).or_default();
            let word = |s: &str| s.split(' ').next().unwrap_or("").to_string();
            if !n.is_empty()
                && !numbers
                    .iter()
                    .any(|m| word(m).eq_ignore_ascii_case(&word(&n)))
            {
                numbers.push(n);
            }
        }
        let mut entries: Vec<Entry> = by_call
            .into_iter()
            .map(|(callsign, numbers)| Entry { callsign, numbers })
            .collect();
        entries.sort_by(|a, b| a.callsign.cmp(&b.callsign));
        let mut pairs: HashMap<[u8; 2], Vec<u32>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            let mut seen: Vec<[u8; 2]> = Vec::new();
            for w in e.callsign.as_bytes().windows(2) {
                let k = [w[0], w[1]];
                if !seen.contains(&k) {
                    seen.push(k);
                    pairs.entry(k).or_default().push(i as u32);
                }
            }
        }
        let mut packed = Vec::new();
        let mut starts = vec![0];
        let mut by_len: Vec<Vec<u32>> = Vec::new();
        let masks = entries
            .iter()
            .map(|e| char_mask(e.callsign.as_bytes()))
            .collect();
        for (i, e) in entries.iter().enumerate() {
            packed.extend_from_slice(e.callsign.as_bytes());
            starts.push(packed.len());
            let n = e.callsign.len();
            if by_len.len() <= n {
                by_len.resize(n + 1, Vec::new());
            }
            by_len[n].push(i as u32);
        }
        Database {
            entries,
            pairs,
            packed,
            starts,
            by_len,
            masks,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, i: usize) -> &Entry {
        &self.entries[i]
    }

    pub fn lines(&self, fragment: &str, max: usize) -> Vec<(usize, Option<usize>)> {
        let mut out = Vec::new();
        for i in self.partial(fragment, max) {
            if out.len() >= max {
                break;
            }
            match self.entries[i].numbers.len() {
                0 => out.push((i, None)),
                n => out.extend((0..n).map(|k| (i, Some(k)))),
            }
        }
        out.truncate(max);
        out
    }

    pub fn partial(&self, fragment: &str, max: usize) -> Vec<usize> {
        if fragment.is_empty() || fragment.starts_with(',') {
            return Vec::new();
        }
        let core = core_call(fragment);
        if core.is_empty() {
            return Vec::new();
        }
        let matches = |i: &usize| partial_match(core, &self.entries[*i].callsign);
        let b = core.as_bytes();
        if b.len() >= 2 && !core.contains('.') {
            let Some(list) = self.pairs.get(&[b[0], b[1]]) else {
                return Vec::new();
            };
            list.iter()
                .map(|i| *i as usize)
                .filter(matches)
                .take(max)
                .collect()
        } else {
            (0..self.entries.len()).filter(matches).take(max).collect()
        }
    }

    pub fn n_plus_one(&self, fragment: &str, max: usize) -> Vec<NearCall> {
        if fragment.len() < 3 || fragment.starts_with(',') {
            return Vec::new();
        }
        let f = fragment.len();
        let fm = char_mask(fragment.as_bytes());
        let mut hits: Vec<NearCall> = Vec::new();
        for (len, list) in self.by_len.iter().enumerate() {
            let limit = limit_for(len.max(f)) as u32;
            if len.abs_diff(f) > limit as usize {
                continue;
            }
            hits.extend(list.iter().filter_map(|&i| {
                let i = i as usize;
                let m = self.masks[i];
                if (m & !fm).count_ones() > limit || (fm & !m).count_ones() > limit {
                    return None;
                }
                near(
                    &self.packed[self.starts[i]..self.starts[i + 1]],
                    fragment.as_bytes(),
                )
                .map(|n| NearCall { index: i, ..n })
            }));
        }
        hits.sort_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(Ordering::Equal)
                .then(a.index.cmp(&b.index))
        });
        hits.truncate(max.saturating_add(1));
        hits
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearCall {
    pub index: usize,
    pub distance: usize,
    pub score: f64,
    pub position: usize,
}

impl NearCall {
    pub fn marker(&self) -> char {
        match self.position {
            _ if self.distance == 0 => '*',
            p @ 0..=9 => char::from(b'0' + p as u8),
            _ => 'F',
        }
    }
}

fn limit_for(longer: usize) -> usize {
    static TABLE: LazyLock<Vec<usize>> = LazyLock::new(|| (0..128).map(compute_limit).collect());
    TABLE
        .get(longer)
        .copied()
        .unwrap_or_else(|| compute_limit(longer))
}

fn compute_limit(longer: usize) -> usize {
    (0..longer)
        .take_while(|&d| ((d + 1) as f64 / longer as f64) < 0.3)
        .count()
}

fn near(a: &[u8], b: &[u8]) -> Option<NearCall> {
    let longer = a.len().max(b.len());
    if longer == 0 {
        return None;
    }
    let limit = limit_for(longer);
    if a.len().abs_diff(b.len()) > limit {
        return None;
    }
    let distance = bounded_levenshtein(a, b, limit)?;
    let score = distance as f64 / longer as f64;
    let position = (0..longer)
        .find(|&j| a.get(j).unwrap_or(&b' ') != b.get(j).unwrap_or(&b' '))
        .map_or(0, |j| j + 1);
    Some(NearCall {
        index: 0,
        distance,
        score,
        position,
    })
}

pub fn bounded_levenshtein(a: &[u8], b: &[u8], limit: usize) -> Option<usize> {
    let p = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[p..], &b[p..]);
    let s = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a, b) = (&a[..a.len() - s], &b[..b.len() - s]);
    let distance = match (a.len(), b.len()) {
        (0, n) | (n, 0) => n,
        (1, 1) => 1,
        _ if limit <= 1 => return None,
        _ => {
            let mut stack = [0usize; 33];
            let mut heap = Vec::new();
            let row: &mut [usize] = if b.len() < stack.len() {
                &mut stack[..=b.len()]
            } else {
                heap.resize(b.len() + 1, 0);
                &mut heap
            };
            for (j, r) in row.iter_mut().enumerate() {
                *r = j;
            }
            for (i, ca) in a.iter().enumerate() {
                let mut diag = row[0];
                row[0] = i + 1;
                let mut best = row[0];
                for (j, cb) in b.iter().enumerate() {
                    let next = (row[j] + 1)
                        .min(row[j + 1] + 1)
                        .min(diag + usize::from(ca != cb));
                    diag = row[j + 1];
                    row[j + 1] = next;
                    best = best.min(next);
                }
                if best > limit {
                    return None;
                }
            }
            row[b.len()]
        }
    };
    (distance <= limit).then_some(distance)
}

fn char_mask(s: &[u8]) -> u64 {
    s.iter().fold(0, |m, &c| m | 1 << (c & 63))
}

pub fn core_call(call: &str) -> &str {
    match call.find('/') {
        Some(p) if p > 3 => &call[..p],
        _ => call,
    }
}

pub fn check_call(log: &Log, call: &str, bands: &[Band], q: &Qso) -> Vec<(Band, Option<usize>)> {
    if call.is_empty() || call.starts_with(',') {
        return bands.iter().map(|b| (*b, None)).collect();
    }
    let mut probe = q.clone();
    probe.callsign = call.to_string();
    bands
        .iter()
        .map(|b| {
            probe.band = *b;
            (*b, log.find_dupe(&probe))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiRow {
    pub band: Band,
    pub cw: Option<bool>,
    pub hit: Option<usize>,
}

pub fn check_multi(log: &Log, multi: &str, bands: &[Band], cw_and_ph: bool) -> Vec<MultiRow> {
    let sides: &[Option<bool>] = if cw_and_ph {
        &[Some(true), Some(false)]
    } else {
        &[None]
    };
    let mut rows = Vec::new();
    for b in bands {
        for cw in sides {
            let hit = (!multi.is_empty())
                .then(|| {
                    log.qsos.iter().rposition(|q| {
                        q.band == *b
                            && q.multi1 == multi
                            && cw.is_none_or(|cw| (q.mode == Mode::Cw) == cw)
                    })
                })
                .flatten();
            rows.push(MultiRow {
                band: *b,
                cw: *cw,
                hit,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::DelphiTime;

    fn qso(call: &str, band: Band, t: f64) -> Qso {
        Qso {
            callsign: call.into(),
            band,
            mode: Mode::Cw,
            time: DelphiTime(t),
            ..Qso::default()
        }
    }

    #[test]
    fn partial_match_like_zlog() {
        assert!(partial_match("ZL", "JA1ZLO"));
        assert!(!partial_match("ZX", "JA1ZLO"));
        assert!(partial_match("JA.Z", "JA1ZLO"));
        assert!(!partial_match("JA.Y", "JA1ZLO"));
        assert!(!partial_match("JA1ZLO.", "JA1ZLO"));
    }

    #[test]
    fn partial_check_options() {
        let log = Log {
            qsos: vec![
                qso("JA1ZLO", Band::B14, 3.0),
                qso("JA1YAA", Band::B7, 2.0),
                qso("JR1ZLO", Band::B7, 1.0),
            ],
            ..Log::default()
        };
        let mut opt = PartialOptions::default();
        assert_eq!(partial_check(&log, "ZLO", Band::B7, &opt), vec![2, 0]);
        opt.sort = SortBy::Call;
        assert_eq!(partial_check(&log, "ZLO", Band::B7, &opt), vec![0, 2]);
        opt.this_band_first = true;
        assert_eq!(partial_check(&log, "ZLO", Band::B7, &opt), vec![2, 0]);
        opt.all_bands = false;
        assert_eq!(partial_check(&log, "ZLO", Band::B14, &opt), vec![0]);
        opt.max = 0;
        opt.all_bands = true;
        assert_eq!(partial_check(&log, "J", Band::B14, &opt).len(), 1);
        assert!(partial_check(&log, ",ZLO", Band::B7, &opt).is_empty());
    }

    #[test]
    fn spc_parsing() {
        let text = "# comment\r\n;x\n\nJA1ZLO 13M\r\nja1yaa\nJA1Ö\n!y\nW1AW   MA extra\n";
        assert_eq!(
            parse_spc(text),
            vec![
                ("JA1ZLO".into(), "13M".into()),
                ("JA1YAA".into(), "".into()),
                ("W1AW".into(), "MA extra".into()),
            ]
        );
    }

    fn db() -> Database {
        Database::new(
            [
                "JA1ZLO 13M",
                "JA1ZLO 13M",
                "JA1ZLO 14",
                "JA1ZLA",
                "JR1ZLO",
                "W1AW",
                "K1ZZ",
            ]
            .iter()
            .flat_map(|l| parse_spc(l)),
        )
    }

    #[test]
    fn database_merges_numbers() {
        let d = db();
        assert_eq!(d.len(), 5);
        let i = d.partial("JA1ZLO", 10)[0];
        assert_eq!(d.entry(i).numbers, vec!["13M", "14"]);
    }

    #[test]
    fn super_partial() {
        let d = db();
        let calls = |v: Vec<usize>| -> Vec<String> {
            v.into_iter().map(|i| d.entry(i).callsign.clone()).collect()
        };
        assert_eq!(
            calls(d.partial("ZL", 10)),
            vec!["JA1ZLA", "JA1ZLO", "JR1ZLO"]
        );
        assert_eq!(calls(d.partial("ZL", 1)), vec!["JA1ZLA"]);
        assert_eq!(calls(d.partial("Z", 10)).len(), 4);
        assert_eq!(calls(d.partial("J.1", 10)).len(), 3);
        assert_eq!(calls(d.partial("JA1ZLO/1", 10)), vec!["JA1ZLO"]);
        assert!(d.partial("QQ", 10).is_empty());
    }

    #[test]
    fn n_plus_one() {
        let d = db();
        let hits = d.n_plus_one("JA1ZLO", 10);
        let calls: Vec<_> = hits
            .iter()
            .map(|h| (d.entry(h.index).callsign.as_str(), h.marker()))
            .collect();
        assert_eq!(calls[0], ("JA1ZLO", '*'));
        assert!(calls.contains(&("JA1ZLA", '6')));
        assert!(calls.contains(&("JR1ZLO", '2')));
        assert!(!calls.iter().any(|c| c.0 == "W1AW"));
        assert!(d.n_plus_one("JA", 10).is_empty());
    }

    #[test]
    fn bounded_levenshtein_agrees_with_the_full_one() {
        let alphabet = b"AB1/";
        let words: Vec<Vec<u8>> = (0..400usize)
            .map(|n| {
                let len = n % 7;
                (0..len)
                    .map(|i| alphabet[(n / 7 + i * 3 + n * i) % 4])
                    .collect()
            })
            .collect();
        for a in &words {
            for b in &words {
                let d = strsim::generic_levenshtein(a, b);
                for limit in 0..4 {
                    let expect = (d <= limit).then_some(d);
                    assert_eq!(
                        bounded_levenshtein(a, b, limit),
                        expect,
                        "{a:?} {b:?} {limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn levenshtein_distance() {
        assert_eq!(bounded_levenshtein(b"JA1ZLO", b"JA1ZLO", 9), Some(0));
        assert_eq!(bounded_levenshtein(b"JA1ZLO", b"JA1ZL", 9), Some(1));
        assert_eq!(bounded_levenshtein(b"JA1ZLO", b"JR1ZLA", 9), Some(2));
        assert_eq!(bounded_levenshtein(b"", b"ABC", 9), Some(3));
    }

    #[test]
    fn check_call_and_multi() {
        let mut log = Log {
            qsos: vec![qso("JA1ZLO", Band::B7, 1.0), qso("JA1ZLO", Band::B14, 2.0)],
            ..Log::default()
        };
        log.qsos[1].multi1 = "13".into();
        let bands = [Band::B7, Band::B14, Band::B21];
        let probe = qso("", Band::B7, 0.0);
        assert_eq!(
            check_call(&log, "JA1ZLO/1", &bands, &probe),
            vec![(Band::B7, Some(0)), (Band::B14, Some(1)), (Band::B21, None)]
        );
        let hits: Vec<_> = check_multi(&log, "13", &bands, false)
            .iter()
            .map(|r| (r.band, r.hit))
            .collect();
        assert_eq!(
            hits,
            vec![(Band::B7, None), (Band::B14, Some(1)), (Band::B21, None)]
        );
    }

    fn calls(d: &Database, lines: &[(usize, Option<usize>)]) -> Vec<String> {
        lines
            .iter()
            .map(|(i, k)| {
                let e = d.entry(*i);
                match k {
                    Some(k) => format!("{} {}", e.callsign, e.numbers[*k]),
                    None => e.callsign.clone(),
                }
            })
            .collect()
    }

    #[test]
    fn super_lines_one_per_record() {
        let d = db();
        assert_eq!(
            calls(&d, &d.lines("JA1ZL", 10)),
            vec!["JA1ZLA", "JA1ZLO 13M", "JA1ZLO 14"]
        );
    }

    #[test]
    fn super_lines_limit_counts_records() {
        let d = db();
        assert_eq!(
            calls(&d, &d.lines("JA1ZL", 2)),
            vec!["JA1ZLA", "JA1ZLO 13M"]
        );
        assert_eq!(calls(&d, &d.lines("JA1ZLO", 1)), vec!["JA1ZLO 13M"]);
        assert!(d.lines("JA1ZL", 0).is_empty());
    }

    #[test]
    fn super_lines_empty_and_command() {
        let d = db();
        assert!(d.lines("", 10).is_empty());
        assert!(d.lines(",RIG", 10).is_empty());
        assert!(d.lines("QQ", 10).is_empty());
        assert!(Database::default().lines("JA", 10).is_empty());
    }

    fn delphi_n_plus_one(calls: &[&str], partial: &str, maxhit: usize) -> Vec<(String, char)> {
        let mut sorted = calls.to_vec();
        sorted.sort();
        let mut hits: Vec<(f64, String, char)> = Vec::new();
        for c in sorted {
            let n = strsim::levenshtein(c, partial);
            let (l1, l2) = (c.len(), partial.len());
            let score = n as f64 / l1.max(l2) as f64;
            if score < 0.3 {
                let w = l1.max(l2);
                let (a, b) = (format!("{c:<w$}"), format!("{partial:<w$}"));
                let pos = a.bytes().zip(b.bytes()).position(|(x, y)| x != y);
                let np1 = match pos.map_or(0, |p| p + 1) {
                    _ if n == 0 => '*',
                    p @ 0..=9 => char::from(b'0' + p as u8),
                    _ => 'F',
                };
                hits.push((score, c.to_string(), np1));
            }
        }
        hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        hits.into_iter()
            .take(maxhit + 1)
            .map(|(_, c, m)| (c, m))
            .collect()
    }

    #[test]
    fn n_plus_one_matches_tsupercheck_nplusone_thread() {
        let calls = [
            "JA1ZLO",
            "JA1ZL",
            "JA1ZLOO",
            "XA1ZLO",
            "JA1ZLX",
            "JA1YWX",
            "JA1ZLO/1",
            "JA1ZLO/10",
            "JA1ZLO/100",
            "JA1ZWO",
            "A1ZLO",
            "JA1ZLOX/P",
            "7J1ZLO",
            "JA1ZL1234",
            "JA1ZLA",
        ];
        let d = Database::new(calls.iter().map(|c| (c.to_string(), String::new())));
        for partial in [
            "JA1ZLO",
            "JA1ZL",
            "JA1ZLO/1",
            "JA1ZLO/10",
            "A1Z",
            "JA1YWX",
            "JA1ZL123",
        ] {
            for maxhit in [0, 2, 100] {
                let got: Vec<_> = d
                    .n_plus_one(partial, maxhit)
                    .iter()
                    .map(|h| (d.entry(h.index).callsign.clone(), h.marker()))
                    .collect();
                assert_eq!(
                    got,
                    delphi_n_plus_one(&calls, partial, maxhit),
                    "{partial} {maxhit}"
                );
            }
        }
        let hits = d.n_plus_one("JA1ZLO/10", 100);
        let marks: Vec<_> = hits
            .iter()
            .map(|h| (d.entry(h.index).callsign.as_str(), h.marker()))
            .collect();
        assert!(marks.contains(&("JA1ZLO/100", 'F')));
        assert!(marks.contains(&("JA1ZLO/1", '9')));
    }

    #[test]
    fn n_plus_one_shows_max_plus_one() {
        let d = db();
        assert_eq!(d.n_plus_one("JA1ZLO", 1).len(), 2);
        assert_eq!(d.n_plus_one("JA1ZLO", 0).len(), 1);
        assert_eq!(d.n_plus_one("JA1ZLO", usize::MAX).len(), 3);
    }

    #[test]
    fn partial_sort_by_core_call_ignoring_case() {
        let log = Log {
            qsos: vec![
                qso("JA1ZLO/1", Band::B7, 1.0),
                qso("ja1zla", Band::B7, 2.0),
                qso("JA1ZLB", Band::B7, 3.0),
                qso("JA1ZLO", Band::B7, 4.0),
            ],
            ..Log::default()
        };
        let opt = PartialOptions {
            sort: SortBy::Call,
            ..PartialOptions::default()
        };
        assert_eq!(partial_check(&log, "1", Band::B7, &opt), vec![1, 2, 0, 3]);
    }

    #[test]
    fn partial_check_numbers() {
        let mut log = Log {
            qsos: vec![
                qso("JA1ZLO", Band::B7, 3.0),
                qso("JA1YAA", Band::B14, 2.0),
                qso("JR1ZLO", Band::B7, 1.0),
            ],
            ..Log::default()
        };
        log.qsos[0].nr_rcvd = "1310M".into();
        log.qsos[1].nr_rcvd = "13H".into();
        log.qsos[2].nr_rcvd = "10M".into();
        let mut opt = PartialOptions::default();
        assert_eq!(partial_check_number(&log, "13", Band::B7, &opt), vec![1, 0]);
        assert_eq!(partial_check_number(&log, "0M", Band::B7, &opt), vec![2, 0]);
        assert!(partial_check_number(&log, "", Band::B7, &opt).is_empty());
        assert!(partial_check_number(&log, "99", Band::B7, &opt).is_empty());
        opt.all_bands = false;
        assert_eq!(partial_check_number(&log, "13", Band::B7, &opt), vec![0]);
        opt.all_bands = true;
        opt.max = 0;
        assert_eq!(partial_check_number(&log, "M", Band::B7, &opt).len(), 1);
    }

    #[test]
    fn spc_number_takes_30_from_the_space() {
        let line = format!("JA1ZLO   {}", "1".repeat(40));
        assert_eq!(parse_spc(&line)[0].1, "1".repeat(27));
        assert_eq!(parse_spc(&format!("W1AW {}", " ".repeat(40)))[0].1, "");
        assert_eq!(parse_spc("W1AW ")[0], ("W1AW".into(), "".into()));
        assert!(parse_spc(" 13M").is_empty());
        assert!(parse_spc("").is_empty());
    }

    #[test]
    fn spc_calls_upper_case_on_purpose() {
        assert_eq!(parse_spc("ja1zlo 13m")[0], ("JA1ZLO".into(), "13m".into()));
    }

    #[test]
    fn database_dedupes_numbers_ignoring_case() {
        let d = Database::new(parse_spc("JA1ZLO 13m\nJA1ZLO 13M\nJA1ZLO 13M x\nJA1ZLO 14"));
        assert_eq!(d.entry(0).numbers, vec!["13m", "14"]);
    }

    #[test]
    fn check_multi_counts_invalid_and_splits_modes() {
        let mut log = Log {
            qsos: vec![
                qso("W1AW", Band::B28, 1.0),
                qso("K1ZZ", Band::B28, 2.0),
                qso("N1MM", Band::B28, 3.0),
            ],
            ..Log::default()
        };
        for q in &mut log.qsos {
            q.multi1 = "MA".into();
        }
        log.qsos[1].mode = Mode::Ssb;
        log.qsos[2].invalid = true;
        let one: Vec<_> = check_multi(&log, "MA", &[Band::B28], false)
            .iter()
            .map(|r| r.hit)
            .collect();
        assert_eq!(one, vec![Some(2)]);
        let two: Vec<_> = check_multi(&log, "MA", &[Band::B28], true)
            .iter()
            .map(|r| (r.cw, r.hit))
            .collect();
        assert_eq!(two, vec![(Some(true), Some(2)), (Some(false), Some(1))]);
        assert!(check_multi(&log, "", &[Band::B28], true)
            .iter()
            .all(|r| r.hit.is_none()));
        assert!(check_multi(&log, "MA", &[], true).is_empty());
    }
}
