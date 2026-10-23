//! What the band scope holds and how it marks stations (`UBandScope2.pas`, `USpotClass.pas`).

use std::collections::HashSet;

use zlog_core::{Band, Contest, Log, Mode, MultiCheck, Qso};
use zlog_rig::sync::band_of;
use zlog_ui::{EntryAction, Spot};

/// zLog's default lifetime of a band scope entry in minutes (`BandScopeExpire`).
pub const DEFAULT_EXPIRE_MINUTES: i64 = 60;

/// One station on the scope.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub callsign: String,
    pub hz: u64,
    pub band: Band,
    pub mode: Option<Mode>,
    /// Received or memorised, in seconds since the Unix epoch.
    pub time: i64,
    pub spotter: String,
    /// Put here by the operator (`AddSelfSpot`) rather than received.
    pub memory: bool,
    /// Number known with the entry: the entry row's number when memorised
    /// (`AddSelfSpot(.., strNrRcvd, ..)`), empty for received spots.
    pub number: String,
}

/// How a station is shown (`TBandScope2.GridDrawCell`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// Already worked on this band, in any mode (`TLog.IsWorked`).
    Worked,
    /// Not worked, and its number (known from another band) is a multiplier still needed.
    NewMulti(String),
    /// Not worked; its number is known and not a new multiplier.
    Number(String),
    /// Not worked; nothing known about its number.
    Unknown,
}

/// The entries of every band.
#[derive(Default)]
pub struct Scope {
    pub items: Vec<Item>,
    /// Spots of [`zlog_ui::View::spots`] already taken in.
    seen: HashSet<(String, u64, i64)>,
}

impl Scope {
    /// Take in spots not seen before. Spots out of every band are ignored, as zLog
    /// drops those its band plan cannot place (`BandPlan.FreqToBand`), and so are
    /// spots older than `minutes` at `now`, which [`Scope::expire`] would drop
    /// (`TBandScope2.AddClusterSpot` skips expired spots).
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
                    number: String::new(),
                });
            }
        }
    }

    /// Add an entry as `TBandScope2.Cleanup` merges a new one: the same call on the
    /// same band moves to the new frequency and time; another call on the same
    /// frequency (to 100 Hz) is removed, but only when it comes before the entry of
    /// the same call in the list (zLog drops the new spot once merged, so the loop
    /// removes nothing after it).
    ///
    /// Departure: zLog moves the entry of the call to any spot of it, an older one
    /// too (the frequency always, the time when its reliability is not lower). Here an
    /// entry older than the one held for the call is ignored, since the application
    /// hands in the spots it holds again and again ([`Scope::ingest`]), and an old
    /// spot must not move the call back off a newer frequency (see the test
    /// `old_spot_is_not_reingested_after_expire`). Spots carry no reliability here.
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
                old.number = item.number;
            }
            None => self.items.push(item),
        }
    }

    /// Drop entries older than `minutes` (`Cleanup`, `_bsexpire`).
    pub fn expire(&mut self, now: i64, minutes: i64) {
        let limit = now - minutes * 60;
        self.items.retain(|i| i.time >= limit);
        self.seen.retain(|(_, _, t)| *t >= limit);
    }

    /// Entries of one band (or all), sorted by frequency.
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

/// Callsigns compare without case: spots keep the case they came in with, the log
/// is in upper case (zLog upper-cases both).
fn same_call(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The entry memorised from the entry row at the rig's frequency
/// (`TBandScope2.AddSelfSpot`): it keeps the received number typed so far. `None`
/// when the frequency is out of every band.
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
    })
}

/// Mark an entry against the log (`SpotCheckWorked`).
///
/// An entry on a band outside the contest (or on a WARC band the contest does not
/// use) is left unmarked: only its own number is shown, as zLog exits early.
/// Worked means a valid QSO with the same core callsign on the band, in any mode
/// (`TLog.IsWorked` compares callsign and band only). A spot without a mode is
/// checked for multipliers in `entry_mode`. When the entry has no number, as zLog
/// does for contests with the same exchange on every band, the number is taken from
/// a QSO with the station on another band ([`Contest::exchange_from`],
/// `IsOtherBandWorked`), and the contest says whether it is a multiplier still
/// needed on this band ([`Contest::check_multi`]). zLog's lookups in the Super
/// Check list and the lookup server are not done.
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

/// What clicking an entry does (`TBandScope2.GridDblClick`): a worked station only
/// clears the callsign and number (`SetYourCallsign('', '')`); otherwise the rig
/// tunes to it and its callsign and known number go into the entry row
/// (`SetFreqAndCall(FreqHz, Call, Number)`).
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

/// Freshness from 0 (about to expire) to 4 (new), by the share of the lifetime left
/// (`TBandScope2.SetFreshnessType`, default type: 1/16, 1/8, 1/4 and 1/2 of it).
pub fn freshness(item: &Item, now: i64, minutes: i64) -> u8 {
    let life = minutes * 60;
    let left = (item.time + life - now).max(0);
    [16, 8, 4, 2]
        .iter()
        .position(|d| left < life / d)
        .unwrap_or(4) as u8
}

/// Whether the rig is on this entry (`TBandScope2.SetCurrentFreq`: within 100 Hz).
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
        /// Multiplier 10 is worked on 7 MHz only.
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
            comment: String::new(),
            time,
        }
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
        // Seen again: ignored. Same call moves; another call on the same frequency replaces.
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
        };
        let m = |i: &Item| mark(i, &log, &Test, Mode::Ssb);
        assert_eq!(m(&item("JA1YAA", Band::B7)), Mark::Worked);
        assert_eq!(m(&item("JA1YAA/1", Band::B7)), Mark::Worked);
        assert_eq!(m(&item("JA1YAA", Band::B14)), Mark::NewMulti("10".into()));
        assert_eq!(m(&item("JA1ZLO", Band::B14)), Mark::Unknown);
        // Spots keep their case; the log is upper case.
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
        };
        assert_eq!(freshness(&i, 0, 60), 4);
        assert_eq!(freshness(&i, 40 * 60, 60), 3);
        assert_eq!(freshness(&i, 59 * 60, 60), 0);
        assert!(is_near(&i, Some(7_012_600)));
        assert!(!is_near(&i, Some(7_013_000)));
        assert!(!is_near(&i, None));
    }

    /// C2: with a short expiry, an old spot still in `View::spots` comes back after
    /// `expire` prunes `seen`; it must not move the call off its newer frequency.
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

    /// K5: once the new spot merged into the entry of its call, zLog's `Cleanup`
    /// removes no other call after that entry; before it, it does.
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
        // A new call removes every other call on its frequency.
        s.add(item("JA1CCC", 7_009_960, 20));
        assert_eq!(s.items, vec![item("JA1CCC", 7_009_960, 20)]);
        // Another band, or 100 Hz away, is kept.
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
        };
        s.add(item(7_020_000, 100, "NEW"));
        s.add(item(7_010_000, 50, "OLD"));
        assert_eq!(s.items, vec![item(7_020_000, 100, "NEW")]);
        // Same time: the later one wins, as before.
        s.add(item(7_021_000, 100, "SAME"));
        assert_eq!(s.items, vec![item(7_021_000, 100, "SAME")]);
        // Newer: moves.
        s.add(item(7_022_000, 200, "NEWER"));
        assert_eq!(s.items, vec![item(7_022_000, 200, "NEWER")]);
        // Same call on another band is a separate entry.
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
        // A skipped spot is not remembered as seen; it is still skipped later.
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
        }
    }

    /// C4: zLog clears the entry row and does not tune to a worked station.
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

    /// C7: the known number is passed along (`SetFreqAndCall`).
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

    /// N1: worked on the band in another mode is still worked (`TLog.IsWorked`).
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

    /// N1: an invalid QSO does not make the station worked.
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

    /// N2: outside the contest's bands nothing is marked (`SpotCheckWorked` exits).
    #[test]
    fn out_of_contest_band_is_unmarked() {
        let mut log = Log::default();
        log.qsos.push(qso("JA1YAA", Band::B144, Mode::Cw));
        log.qsos.push(qso("JA1YAA", Band::B10, Mode::Cw));
        let m = |i: &Item| mark(i, &log, &Test, Mode::Cw);
        assert_eq!(m(&at("JA1YAA", Band::B144, None)), Mark::Unknown);
        // WARC band inside the range, contest without WARC.
        assert_eq!(m(&at("JA1YAA", Band::B10, None)), Mark::Unknown);
        // Its own number is still shown, never as a new multiplier.
        let i = Item {
            number: "10".into(),
            ..at("JA1ZLO", Band::B144, None)
        };
        assert_eq!(m(&i), Mark::Number("10".into()));
    }

    /// Boundaries of the band range are inside it.
    #[test]
    fn band_range_edges_are_marked() {
        let mut log = Log::default();
        log.qsos.push(qso("JA1YAA", Band::B1_9, Mode::Cw));
        log.qsos.push(qso("JA1YAA", Band::B50, Mode::Cw));
        let m = |i: &Item| mark(i, &log, &Test, Mode::Cw);
        assert_eq!(m(&at("JA1YAA", Band::B1_9, None)), Mark::Worked);
        assert_eq!(m(&at("JA1YAA", Band::B50, None)), Mark::Worked);
    }

    /// N5: a memorised entry keeps the number typed in the entry row.
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

    /// N5: the number survives a merge into an existing entry.
    #[test]
    fn merge_keeps_new_number() {
        let mut s = Scope::default();
        s.add(memorised("JA1ZLO", "11", 7_010_000, Mode::Cw, 5).unwrap());
        s.add(memorised("JA1ZLO", "12", 7_011_000, Mode::Cw, 6).unwrap());
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].number, "12");
    }
}
