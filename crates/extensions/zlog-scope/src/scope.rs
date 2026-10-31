use std::collections::HashSet;

use zlog_core::band_of;
use zlog_core::{Band, Contest, Log, Mode, MultiCheck, Qso};
use zlog_ui::{EntryAction, Spot};

pub const DEFAULT_EXPIRE_MINUTES: i64 = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub callsign: String,
    pub hz: u64,
    pub band: Band,
    pub mode: Option<Mode>,
    pub time: i64,
    pub spotter: String,
    pub memory: bool,
    pub number: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    Worked,
    NewMulti(String),
    Number(String),
    Unknown,
}

#[derive(Default)]
pub struct Scope {
    pub items: Vec<Item>,
    seen: HashSet<(String, u64, i64)>,
}

impl Scope {
    pub fn ingest(&mut self, spots: &[Spot], now: i64, minutes: i64) {
        let limit = now - minutes * 60;
        for s in spots {
            if s.time < limit {
                continue;
            }
            if !self.seen.insert((s.callsign.clone(), s.hz, s.time)) {
                continue;
            }
            if let Some(band) = band_of(s.hz) {
                self.add(Item {
                    callsign: s.callsign.clone(),
                    hz: s.hz,
                    band,
                    mode: s.mode,
                    time: s.time,
                    spotter: s.spotter.clone(),
                    memory: false,
                    number: s.number.clone(),
                    source: s.source.clone(),
                });
            }
        }
    }

    pub fn add(&mut self, item: Item) {
        let is_call = |i: &Item| same_call(&i.callsign, &item.callsign) && i.band == item.band;
        if self.items.iter().any(|i| is_call(i) && i.time > item.time) {
            return;
        }
        let same_slot = |h: u64| (h + 50) / 100 == (item.hz + 50) / 100;
        let merged_at = self.items.iter().position(is_call);
        let mut n = 0;
        self.items.retain(|i| {
            let before = merged_at.is_none_or(|m| n < m);
            n += 1;
            is_call(i) || !before || i.band != item.band || !same_slot(i.hz)
        });
        match self.items.iter_mut().find(|i| is_call(i)) {
            Some(old) => {
                old.hz = item.hz;
                old.time = item.time;
                old.mode = item.mode.or(old.mode);
                old.spotter = item.spotter;
                old.memory = item.memory;
                old.source = item.source;
                if !item.number.is_empty() {
                    old.number = item.number;
                }
            }
            None => self.items.push(item),
        }
    }

    pub fn expire(&mut self, now: i64, minutes: i64) {
        let limit = now - minutes * 60;
        self.items.retain(|i| i.time >= limit);
        self.seen.retain(|(_, _, t)| *t >= limit);
    }

    pub fn rows(&self, band: Option<Band>) -> Vec<&Item> {
        let mut rows: Vec<&Item> = self
            .items
            .iter()
            .filter(|i| band.is_none_or(|b| i.band == b))
            .collect();
        rows.sort_by_key(|i| i.hz);
        rows
    }
}

fn same_call(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn memorised(callsign: &str, number: &str, hz: u64, mode: Mode, now: i64) -> Option<Item> {
    Some(Item {
        callsign: callsign.to_string(),
        hz,
        band: band_of(hz)?,
        mode: Some(mode),
        time: now,
        spotter: String::new(),
        memory: true,
        number: number.to_string(),
        source: String::new(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub row: usize,
    pub on: bool,
}

pub fn marker(rows: &[&Item], hz: Option<u64>, band: Band) -> Option<Marker> {
    let hz = hz.filter(|&hz| band_of(hz) == Some(band))?;
    let row = rows.iter().position(|i| i.hz >= hz).unwrap_or(rows.len());
    let on = rows.get(row).is_some_and(|i| i.hz == hz);
    Some(Marker { row, on })
}

pub fn mark(item: &Item, log: &Log, contest: &dyn Contest, entry_mode: Mode) -> Mark {
    let plain = || match item.number.is_empty() {
        true => Mark::Unknown,
        false => Mark::Number(item.number.clone()),
    };
    let (low, high) = contest.band_range();
    if item.band < low || item.band > high || (item.band.is_warc() && !contest.uses_warc()) {
        return plain();
    }
    let mut q = Qso {
        callsign: item.callsign.clone(),
        band: item.band,
        mode: item.mode.unwrap_or(entry_mode),
        ..Qso::default()
    };
    let core = q.core_call().to_string();
    let worked = log
        .qsos
        .iter()
        .any(|p| !p.invalid && p.band == item.band && same_call(p.core_call(), &core));
    if worked {
        return Mark::Worked;
    }
    let number = match item.number.is_empty() {
        false => Some(item.number.clone()),
        true => log
            .qsos
            .iter()
            .rev()
            .filter(|p| !p.invalid && same_call(p.core_call(), &core))
            .find_map(|p| contest.exchange_from(item.band, p)),
    };
    let Some(number) = number.filter(|n| !n.is_empty()) else {
        return Mark::Unknown;
    };
    q.nr_rcvd = number.clone();
    match contest.check_multi(&q) {
        MultiCheck::Known { needed: true, .. } => Mark::NewMulti(number),
        _ => Mark::Number(number),
    }
}

pub fn click(item: &Item, mark: &Mark) -> Vec<EntryAction> {
    match mark {
        Mark::Worked => vec![
            EntryAction::Callsign(String::new()),
            EntryAction::Number(String::new()),
        ],
        Mark::NewMulti(n) | Mark::Number(n) => vec![
            EntryAction::Qsy(item.hz, item.mode),
            EntryAction::Callsign(item.callsign.clone()),
            EntryAction::Number(n.clone()),
        ],
        Mark::Unknown => vec![
            EntryAction::Qsy(item.hz, item.mode),
            EntryAction::Callsign(item.callsign.clone()),
        ],
    }
}

pub fn freshness(item: &Item, now: i64, minutes: i64) -> u8 {
    let life = minutes * 60;
    let left = (item.time + life - now).max(0);
    [16, 8, 4, 2]
        .iter()
        .position(|d| left < life / d)
        .unwrap_or(4) as u8
}

pub fn is_near(item: &Item, hz: Option<u64>) -> bool {
    hz.is_some_and(|hz| (item.hz / 100).abs_diff(hz / 100) <= 1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use zlog_core::{ContestMode, Power, Scoring, Station};

    pub(crate) struct Test;

    impl Contest for Test {
        fn name(&self) -> &str {
            "test"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::B50)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Mix
        }
        fn sent_number(&self, _: Band, _: Power, _: &Station) -> String {
            String::new()
        }
        fn valid_number(&self, _: &Qso) -> bool {
            true
        }
        fn score(&mut self, _: &Log) -> Scoring {
            Scoring::default()
        }
        fn check_multi(&self, q: &Qso) -> MultiCheck {
            MultiCheck::Known {
                code: q.nr_rcvd.clone(),
                name: q.nr_rcvd.clone(),
                needed: !(q.nr_rcvd == "10" && q.band == Band::B7),
                worked_on: vec![],
            }
        }
    }

    pub(crate) fn spot(call: &str, hz: u64, time: i64) -> Spot {
        Spot {
            callsign: call.into(),
            hz,
            mode: Some(Mode::Cw),
            spotter: "JA1RL".into(),
            time,
            ..Spot::default()
        }
    }

    fn heard(call: &str, hz: u64, number: &str, source: &str, time: i64) -> Spot {
        Spot {
            callsign: call.into(),
            hz,
            number: number.into(),
            source: source.into(),
            time,
            ..Spot::default()
        }
    }

    #[test]
    fn decoded_spots_keep_their_source_and_number() {
        let mut s = Scope::default();
        s.ingest(
            &[
                heard("JA1ZLO", 7_010_100, "13M", "CW4I", 100),
                heard("JA1YWX", 7_074_900, "", "FT8", 100),
            ],
            100,
            60,
        );
        let rows = s.rows(Some(Band::B7));
        let got: Vec<_> = rows
            .iter()
            .map(|i| (i.callsign.as_str(), i.number.as_str(), i.source.as_str()))
            .collect();
        assert_eq!(got, [("JA1ZLO", "13M", "CW4I"), ("JA1YWX", "", "FT8")]);
    }

    #[test]
    fn several_sources_merge_into_the_newest_and_keep_the_number() {
        let mut s = Scope::default();
        s.ingest(&[heard("JA1ZLO", 7_010_100, "13M", "CW4I", 100)], 100, 60);
        s.ingest(&[heard("ja1zlo", 7_010_300, "", "DXC", 110)], 110, 60);
        s.ingest(&[heard("JA1ZLO", 7_010_000, "", "RTTY", 105)], 110, 60);
        assert_eq!(s.items.len(), 1);
        let i = &s.items[0];
        assert_eq!(
            (i.hz, i.number.as_str(), i.source.as_str()),
            (7_010_300, "13M", "DXC")
        );
    }

    #[test]
    fn decoded_spots_expire_with_the_scope_setting() {
        let mut s = Scope::default();
        s.ingest(&[heard("JA1ZLO", 7_010_100, "", "FT8", 0)], 30 * 60, 20);
        assert!(s.items.is_empty(), "older than 20 minutes");
        s.ingest(&[heard("JA1ZLO", 7_010_100, "", "FT8", 0)], 30 * 60, 40);
        assert_eq!(s.items.len(), 1);
        s.expire(41 * 60, 40);
        assert!(s.items.is_empty());
    }

    #[test]
    fn merge_and_sort() {
        let mut s = Scope::default();
        s.ingest(
            &[
                spot("JA1YAA", 7_012_500, 0),
                spot("JA1ZLO", 7_005_000, 0),
                spot("W1AW", 14_025_000, 0),
                spot("XX0XX", 99_000, 0),
            ],
            0,
            60,
        );
        s.ingest(&[spot("JA1YAA", 7_012_500, 0)], 0, 60);
        s.ingest(
            &[spot("JA1YAA", 7_020_000, 10), spot("JA2AAA", 7_005_040, 10)],
            10,
            60,
        );
        let rows: Vec<_> = s
            .rows(Some(Band::B7))
            .iter()
            .map(|i| (i.callsign.as_str(), i.hz))
            .collect();
        assert_eq!(rows, vec![("JA2AAA", 7_005_040), ("JA1YAA", 7_020_000)]);
        assert_eq!(s.rows(None).len(), 3);
        s.expire(60 * 60 + 5, 60);
        assert_eq!(s.rows(None).len(), 2);
    }

    #[test]
    fn marks() {
        let mut log = Log::default();
        log.qsos.push(Qso {
            callsign: "JA1YAA".into(),
            band: Band::B7,
            mode: Mode::Cw,
            nr_rcvd: "10".into(),
            ..Qso::default()
        });
        let item = |call: &str, band| Item {
            callsign: call.into(),
            hz: 0,
            band,
            mode: Some(Mode::Cw),
            time: 0,
            spotter: String::new(),
            memory: false,
            number: String::new(),
            source: String::new(),
        };
        let m = |i: &Item| mark(i, &log, &Test, Mode::Ssb);
        assert_eq!(m(&item("JA1YAA", Band::B7)), Mark::Worked);
        assert_eq!(m(&item("JA1YAA/1", Band::B7)), Mark::Worked);
        assert_eq!(m(&item("JA1YAA", Band::B14)), Mark::NewMulti("10".into()));
        assert_eq!(m(&item("JA1ZLO", Band::B14)), Mark::Unknown);
        assert_eq!(m(&item("ja1yaa", Band::B7)), Mark::Worked);
        assert_eq!(m(&item("ja1yaa/1", Band::B7)), Mark::Worked);
        assert_eq!(m(&item("Ja1Yaa", Band::B14)), Mark::NewMulti("10".into()));
    }

    #[test]
    fn same_call_in_another_case_merges() {
        let mut s = Scope::default();
        s.ingest(&[spot("JA1YAA", 7_012_500, 0)], 0, 60);
        s.ingest(&[spot("ja1yaa", 7_020_000, 10)], 10, 60);
        let rows: Vec<_> = s.rows(None).iter().map(|i| i.hz).collect();
        assert_eq!(rows, vec![7_020_000]);
    }

    #[test]
    fn freshness_and_nearness() {
        let i = Item {
            callsign: "JA1YAA".into(),
            hz: 7_012_500,
            band: Band::B7,
            mode: None,
            time: 0,
            spotter: String::new(),
            memory: true,
            number: String::new(),
            source: String::new(),
        };
        assert_eq!(freshness(&i, 0, 60), 4);
        assert_eq!(freshness(&i, 40 * 60, 60), 3);
        assert_eq!(freshness(&i, 59 * 60, 60), 0);
        assert!(is_near(&i, Some(7_012_600)));
        assert!(!is_near(&i, Some(7_013_000)));
        assert!(!is_near(&i, None));
    }

    #[test]
    fn old_spot_is_not_reingested_after_expire() {
        let mut s = Scope::default();
        let spots = [spot("JA1YAA", 7_010_000, 0)];
        s.ingest(&spots, 0, 10);
        s.ingest(&[spot("JA1YAA", 7_020_000, 15 * 60)], 15 * 60, 10);
        s.expire(15 * 60, 10);
        s.ingest(&spots, 15 * 60, 10);
        let rows = s.rows(Some(Band::B7));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].hz, 7_020_000);
    }

    #[test]
    fn merge_stops_removing_other_calls() {
        let item = |call: &str, hz, time| Item {
            callsign: call.into(),
            hz,
            band: Band::B7,
            mode: Some(Mode::Cw),
            time,
            spotter: String::new(),
            memory: false,
            number: String::new(),
            source: String::new(),
        };
        let mut s = Scope {
            items: vec![
                item("JA1AAA", 7_010_000, 0),
                item("JA1YAA", 7_020_000, 0),
                item("JA1BBB", 7_010_000, 0),
            ],
            ..Scope::default()
        };
        s.add(item("JA1YAA", 7_010_040, 10));
        assert_eq!(
            s.items,
            vec![item("JA1YAA", 7_010_040, 10), item("JA1BBB", 7_010_000, 0)]
        );
        s.add(item("JA1CCC", 7_009_960, 20));
        assert_eq!(s.items, vec![item("JA1CCC", 7_009_960, 20)]);
        s.add(item("JA1DDD", 7_010_050, 30));
        s.add(Item {
            band: Band::B14,
            ..item("JA1EEE", 7_009_960, 30)
        });
        assert_eq!(s.items.len(), 3);
    }

    #[test]
    fn add_to_empty_scope() {
        let mut s = Scope::default();
        let i = Item {
            callsign: "JA1YAA".into(),
            hz: 0,
            band: Band::B7,
            mode: None,
            time: 0,
            spotter: String::new(),
            memory: false,
            number: String::new(),
            source: String::new(),
        };
        s.add(i.clone());
        assert_eq!(s.items, vec![i]);
    }

    #[test]
    fn older_entry_does_not_move_newer() {
        let mut s = Scope::default();
        let item = |hz, time, spotter: &str| Item {
            callsign: "JA1YAA".into(),
            hz,
            band: Band::B7,
            mode: Some(Mode::Cw),
            time,
            spotter: spotter.into(),
            memory: false,
            number: String::new(),
            source: String::new(),
        };
        s.add(item(7_020_000, 100, "NEW"));
        s.add(item(7_010_000, 50, "OLD"));
        assert_eq!(s.items, vec![item(7_020_000, 100, "NEW")]);
        s.add(item(7_021_000, 100, "SAME"));
        assert_eq!(s.items, vec![item(7_021_000, 100, "SAME")]);
        s.add(item(7_022_000, 200, "NEWER"));
        assert_eq!(s.items, vec![item(7_022_000, 200, "NEWER")]);
        s.add(Item {
            band: Band::B14,
            hz: 14_020_000,
            ..item(0, 0, "")
        });
        assert_eq!(s.items.len(), 2);
    }

    #[test]
    fn ingest_skips_expired_and_keeps_boundary() {
        let mut s = Scope::default();
        s.ingest(&[], 0, 60);
        assert!(s.items.is_empty());
        s.ingest(
            &[spot("JA1YAA", 7_010_000, 0), spot("JA1ZLO", 7_005_000, -1)],
            3600,
            60,
        );
        let calls: Vec<_> = s.items.iter().map(|i| i.callsign.as_str()).collect();
        assert_eq!(calls, vec!["JA1YAA"]);
        s.ingest(&[spot("JA1ZLO", 7_005_000, -1)], 3600, 60);
        assert_eq!(s.items.len(), 1);
    }

    fn entry(mode: Option<Mode>) -> Item {
        Item {
            callsign: "JA1YAA".into(),
            hz: 7_012_500,
            band: Band::B7,
            mode,
            time: 0,
            spotter: String::new(),
            memory: false,
            number: String::new(),
            source: String::new(),
        }
    }

    #[test]
    fn click_worked_clears_without_qsy() {
        assert_eq!(
            click(&entry(Some(Mode::Cw)), &Mark::Worked),
            vec![
                EntryAction::Callsign(String::new()),
                EntryAction::Number(String::new())
            ]
        );
    }

    #[test]
    fn click_passes_known_number() {
        let i = entry(Some(Mode::Cw));
        let want = |n: &str| {
            vec![
                EntryAction::Qsy(7_012_500, Some(Mode::Cw)),
                EntryAction::Callsign("JA1YAA".into()),
                EntryAction::Number(n.into()),
            ]
        };
        assert_eq!(click(&i, &Mark::NewMulti("10".into())), want("10"));
        assert_eq!(click(&i, &Mark::Number("11".into())), want("11"));
    }

    #[test]
    fn click_unknown_tunes_and_fills_call_only() {
        assert_eq!(
            click(&entry(None), &Mark::Unknown),
            vec![
                EntryAction::Qsy(7_012_500, None),
                EntryAction::Callsign("JA1YAA".into())
            ]
        );
    }

    fn qso(call: &str, band: Band, mode: Mode) -> Qso {
        Qso {
            callsign: call.into(),
            band,
            mode,
            nr_rcvd: "10".into(),
            ..Qso::default()
        }
    }

    fn at(call: &str, band: Band, mode: Option<Mode>) -> Item {
        Item {
            callsign: call.into(),
            band,
            mode,
            ..entry(None)
        }
    }

    #[test]
    fn worked_in_any_mode() {
        let mut log = Log::default();
        log.qsos.push(qso("JA1YAA", Band::B7, Mode::Cw));
        let m = |i: &Item| mark(i, &log, &Test, Mode::Cw);
        assert_eq!(m(&at("JA1YAA", Band::B7, Some(Mode::Ssb))), Mark::Worked);
        assert_eq!(m(&at("JA1YAA", Band::B7, None)), Mark::Worked);
        assert_eq!(m(&at("JA1YAA/P", Band::B7, Some(Mode::Fm))), Mark::Worked);
        assert_ne!(m(&at("JA1YAA", Band::B14, Some(Mode::Cw))), Mark::Worked);
    }

    #[test]
    fn invalid_qso_is_not_worked() {
        let mut log = Log::default();
        log.qsos.push(Qso {
            invalid: true,
            ..qso("JA1YAA", Band::B7, Mode::Cw)
        });
        let m = mark(
            &at("JA1YAA", Band::B7, Some(Mode::Cw)),
            &log,
            &Test,
            Mode::Cw,
        );
        assert_eq!(m, Mark::Unknown);
    }

    #[test]
    fn empty_log_marks_unknown() {
        let m = mark(
            &at("JA1YAA", Band::B7, None),
            &Log::default(),
            &Test,
            Mode::Cw,
        );
        assert_eq!(m, Mark::Unknown);
    }

    #[test]
    fn out_of_contest_band_is_unmarked() {
        let mut log = Log::default();
        log.qsos.push(qso("JA1YAA", Band::B144, Mode::Cw));
        log.qsos.push(qso("JA1YAA", Band::B10, Mode::Cw));
        let m = |i: &Item| mark(i, &log, &Test, Mode::Cw);
        assert_eq!(m(&at("JA1YAA", Band::B144, None)), Mark::Unknown);
        assert_eq!(m(&at("JA1YAA", Band::B10, None)), Mark::Unknown);
        let i = Item {
            number: "10".into(),
            source: String::new(),
            ..at("JA1ZLO", Band::B144, None)
        };
        assert_eq!(m(&i), Mark::Number("10".into()));
    }

    #[test]
    fn band_range_edges_are_marked() {
        let mut log = Log::default();
        log.qsos.push(qso("JA1YAA", Band::B1_9, Mode::Cw));
        log.qsos.push(qso("JA1YAA", Band::B50, Mode::Cw));
        let m = |i: &Item| mark(i, &log, &Test, Mode::Cw);
        assert_eq!(m(&at("JA1YAA", Band::B1_9, None)), Mark::Worked);
        assert_eq!(m(&at("JA1YAA", Band::B50, None)), Mark::Worked);
    }

    #[test]
    fn memorised_keeps_number() {
        let i = memorised("JA1ZLO", "11", 7_010_000, Mode::Cw, 5).unwrap();
        assert_eq!(i.number, "11");
        assert!(i.memory);
        assert_eq!(i.band, Band::B7);
        let log = Log::default();
        assert_eq!(mark(&i, &log, &Test, Mode::Cw), Mark::NewMulti("11".into()));
        let i = memorised("JA1ZLO", "", 7_010_000, Mode::Cw, 5).unwrap();
        assert_eq!(mark(&i, &log, &Test, Mode::Cw), Mark::Unknown);
        assert!(memorised("JA1ZLO", "11", 99_000, Mode::Cw, 5).is_none());
    }

    #[test]
    fn merge_keeps_new_number() {
        let mut s = Scope::default();
        s.add(memorised("JA1ZLO", "11", 7_010_000, Mode::Cw, 5).unwrap());
        s.add(memorised("JA1ZLO", "12", 7_011_000, Mode::Cw, 6).unwrap());
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].number, "12");
    }

    #[test]
    fn rig_marker_position() {
        let mut s = Scope::default();
        s.add(memorised("JA1ZLO", "10", 7_010_000, Mode::Cw, 0).unwrap());
        s.add(memorised("JA1YWX", "11", 7_020_000, Mode::Cw, 0).unwrap());
        let rows = s.rows(Some(Band::B7));
        let at = |hz| marker(&rows, hz, Band::B7);
        assert_eq!(at(Some(7_000_000)), Some(Marker { row: 0, on: false }));
        assert_eq!(at(Some(7_010_000)), Some(Marker { row: 0, on: true }));
        assert_eq!(at(Some(7_015_000)), Some(Marker { row: 1, on: false }));
        assert_eq!(at(Some(7_020_000)), Some(Marker { row: 1, on: true }));
        assert_eq!(at(Some(7_030_000)), Some(Marker { row: 2, on: false }));
        assert_eq!(
            marker(&[], Some(7_010_000), Band::B7),
            Some(Marker { row: 0, on: false })
        );
    }

    #[test]
    fn no_rig_marker_without_rig_or_on_other_band() {
        let mut s = Scope::default();
        s.add(memorised("JA1ZLO", "10", 7_010_000, Mode::Cw, 0).unwrap());
        let rows = s.rows(Some(Band::B7));
        assert_eq!(marker(&rows, None, Band::B7), None);
        assert_eq!(marker(&rows, Some(14_010_000), Band::B7), None);
        assert_eq!(marker(&rows, Some(99_000), Band::B7), None);
    }
}
