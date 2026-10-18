//! The four major JARL contests: ALL JA, 6m AND DOWN, Field Day and ACAG.
//!
//! All four share one implementation; [`Rules`] holds what differs between them.
//! The multiplier lists are zLog's `XPO.DAT` (prefectures and Hokkaido subprefectures)
//! and `ACAG.DAT` (cities, counties and wards), converted to UTF-8.

use std::collections::{BTreeSet, HashMap};

use zlog_core::{
    Band, Contest, ContestDef, ContestMode, Extension, Log, MultiCheck, Power, Qso, Scoring,
    Station,
};
use zlog_scoring::{expand_template, renew_by_band, BandRules};

const XPO_DAT: &str = include_str!("data/XPO.DAT");
const ACAG_DAT: &str = include_str!("data/ACAG.DAT");

/// Power codes that end a JARL contest number.
const POWER_CODES: [char; 4] = ['P', 'L', 'M', 'H'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JarlContest {
    AllJa,
    SixDown,
    FieldDay,
    Acag,
}

impl JarlContest {
    pub const ALL: [JarlContest; 4] = [
        JarlContest::AllJa,
        JarlContest::SixDown,
        JarlContest::FieldDay,
        JarlContest::Acag,
    ];

    pub fn id(self) -> &'static str {
        match self {
            JarlContest::AllJa => "allja",
            JarlContest::SixDown => "6d",
            JarlContest::FieldDay => "fd",
            JarlContest::Acag => "acag",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.id().eq_ignore_ascii_case(id))
    }

    fn spec(self) -> Rules {
        use JarlContest::*;
        match self {
            AllJa => Rules {
                name: "ALL JA",
                bands: (Band::B1_9, Band::B50),
                lists: &[XPO_DAT],
                number: NumberRule::Prefecture,
                sent: "$V$P",
                coeff: false,
                shf_points: 1,
            },
            SixDown => Rules {
                name: "6m AND DOWN",
                bands: (Band::B50, Band::HIGHEST),
                lists: &[XPO_DAT, ACAG_DAT],
                number: NumberRule::ByBand,
                sent: "$Q$P",
                coeff: false,
                shf_points: 2,
            },
            FieldDay => Rules {
                name: "Field Day",
                bands: (Band::B1_9, Band::HIGHEST),
                lists: &[XPO_DAT, ACAG_DAT],
                number: NumberRule::ByBand,
                sent: "$Q$P",
                coeff: true,
                shf_points: 1,
            },
            Acag => Rules {
                name: "ACAG",
                bands: (Band::B1_9, Band::HIGHEST),
                lists: &[ACAG_DAT],
                number: NumberRule::City,
                sent: "$Q$P",
                coeff: false,
                shf_points: 1,
            },
        }
    }

    /// Build a fresh contest with nothing worked.
    pub fn build_rules(self, mode: ContestMode) -> JarlRules {
        let rules = self.spec();
        let mut entries = Vec::new();
        for text in rules.lists {
            entries.extend(parse_dat(text));
        }
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, (code, _))| (code.clone(), i))
            .collect();
        JarlRules {
            rules,
            mode,
            entries,
            index,
            worked: HashMap::new(),
        }
    }
}

/// The extension that adds the four JARL contests.
pub struct Jarl;

impl Extension for Jarl {
    fn name(&self) -> &str {
        "JARL contests"
    }

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        JarlContest::ALL
            .into_iter()
            .map(|c| Box::new(c) as Box<dyn ContestDef>)
            .collect()
    }
}

zlog_core::texts!();

impl ContestDef for JarlContest {
    fn id(&self) -> &str {
        JarlContest::id(*self)
    }

    fn caption(&self, language: &str) -> String {
        let key = match self {
            JarlContest::AllJa => "caption.allja",
            JarlContest::SixDown => "caption.sixdown",
            JarlContest::FieldDay => "caption.fieldday",
            JarlContest::Acag => "caption.acag",
        };
        texts().get(language, key).into()
    }

    fn uses_coeff(&self) -> bool {
        self.spec().coeff
    }

    fn group(&self, language: &str) -> String {
        texts().get(language, "group.jarl").into()
    }

    fn build(&self, mode: ContestMode) -> Box<dyn Contest> {
        Box::new(self.build_rules(mode))
    }
}

/// How a received number (without the power code) is checked.
#[derive(Debug, Clone, Copy)]
enum NumberRule {
    /// Prefecture number, 2 or 3 digits (ALL JA).
    Prefecture,
    /// City number, 4 to 6 digits (ACAG).
    City,
    /// Prefecture up to 1200 MHz, city from 2400 MHz (Field Day, 6m AND DOWN).
    ByBand,
}

/// What differs between the four contests.
#[derive(Debug, Clone)]
struct Rules {
    name: &'static str,
    bands: (Band, Band),
    lists: &'static [&'static str],
    number: NumberRule,
    sent: &'static str,
    coeff: bool,
    /// Points from 2400 MHz up (6m AND DOWN gives 2; 10.4G gives 0 there).
    shf_points: u8,
}

/// A JARL contest in progress.
pub struct JarlRules {
    rules: Rules,
    mode: ContestMode,
    /// (code, name) in list order.
    entries: Vec<(String, String)>,
    index: HashMap<String, usize>,
    worked: HashMap<usize, BTreeSet<Band>>,
}

fn parse_dat(text: &str) -> impl Iterator<Item = (String, String)> + '_ {
    text.lines()
        .skip(1)
        .take_while(|l| !l.to_lowercase().contains("end of file"))
        .filter_map(|l| {
            let (code, name) = l.split_once(' ')?;
            Some((code.to_string(), name.trim().to_string()))
        })
}

/// Split `25M` into `("25", Some('M'))`.
fn split_power(nr: &str) -> (&str, Option<char>) {
    match nr.chars().last() {
        Some(c) if POWER_CODES.contains(&c) => (&nr[..nr.len() - 1], Some(c)),
        _ => (nr, None),
    }
}

impl JarlRules {
    /// Normalise a prefecture number as zLog's `KenToInt` does (`010` is `10`).
    fn prefecture_code(nr: &str) -> Option<String> {
        match nr.parse::<u32>().ok()? {
            n @ 101..=114 => Some(n.to_string()),
            n @ 2..=48 => Some(format!("{n:02}")),
            _ => None,
        }
    }

    /// Multiplier entry for a number without its power code.
    fn lookup(&self, code: &str) -> Option<usize> {
        match self.rules.number {
            NumberRule::Prefecture => self.index.get(&Self::prefecture_code(code)?).copied(),
            NumberRule::City | NumberRule::ByBand => self.index.get(code).copied(),
        }
    }

    fn number_length_ok(&self, band: Band, len: usize) -> bool {
        match self.rules.number {
            NumberRule::Prefecture => (3..=4).contains(&len),
            NumberRule::City => (5..=7).contains(&len),
            NumberRule::ByBand if band <= Band::B1200 => (3..=4).contains(&len),
            NumberRule::ByBand => (5..=7).contains(&len),
        }
    }
}

impl Contest for JarlRules {
    fn name(&self) -> &str {
        self.rules.name
    }

    fn band_range(&self) -> (Band, Band) {
        self.rules.bands
    }

    fn contest_mode(&self) -> ContestMode {
        self.mode
    }

    fn sent_number(&self, band: Band, power: Power, station: &Station) -> String {
        // QTHString: the prefecture or the city, depending on the contest and band.
        let qth = match self.rules.number {
            NumberRule::Prefecture => &station.prov,
            NumberRule::City => &station.city,
            NumberRule::ByBand if band <= Band::B1200 => &station.prov,
            NumberRule::ByBand => &station.city,
        };
        expand_template(self.rules.sent, &station.prov, qth, power)
    }

    fn score(&mut self, log: &Log) -> Scoring {
        let coeff = self.rules.coeff.then(|| log.score_coeff());
        renew_by_band(log, self, coeff)
    }

    /// Field Day and 6m AND DOWN send a prefecture up to 1200 MHz and a city above
    /// (`TFDContest.DispExchangeOnOtherBands`). A city worked above 1200 MHz gives the
    /// prefecture below: its first two digits and the power code, except in Hokkaido,
    /// whose city numbers do not tell the subprefecture. A prefecture does not tell
    /// the city, so nothing carries over upwards.
    fn exchange_from(&self, band: Band, past: &Qso) -> Option<String> {
        let shf = |b: Band| b > Band::B1200;
        match self.rules.number {
            NumberRule::ByBand if shf(band) && !shf(past.band) => None,
            NumberRule::ByBand if !shf(band) && shf(past.band) => {
                let (city, power) = split_power(&past.nr_rcvd);
                let pref = city.get(..2).filter(|p| *p != "01")?;
                Some(format!(
                    "{pref}{}",
                    power.map(String::from).unwrap_or_default()
                ))
            }
            _ => Some(past.nr_rcvd.clone()),
        }
    }

    fn valid_number(&self, q: &Qso) -> bool {
        let nr = q.nr_rcvd.as_str();
        if !self.number_length_ok(q.band, nr.len()) {
            return false;
        }
        match split_power(nr) {
            (code, Some(_)) => self.lookup(code).is_some(),
            (_, None) => false,
        }
    }

    fn check_multi(&self, q: &Qso) -> MultiCheck {
        // Field Day and 6m AND DOWN: a number of the wrong length for the band (`TFDMulti.CheckMulti`).
        // Lengths count the power code, which may not be typed yet.
        let (bare, _) = split_power(&q.nr_rcvd);
        if matches!(self.rules.number, NumberRule::ByBand)
            && !self.number_length_ok(q.band, bare.len() + 1)
        {
            return MultiCheck::Invalid;
        }
        let (code, _) = split_power(&q.nr_rcvd);
        let Some(i) = self.lookup(code) else {
            return MultiCheck::Invalid;
        };
        let worked: Vec<Band> = self
            .worked
            .get(&i)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        let (code, name) = &self.entries[i];
        MultiCheck::Known {
            code: code.clone(),
            name: name.clone(),
            needed: !worked.contains(&q.band),
            worked_on: worked,
        }
    }

    fn multi_table(&self) -> Vec<(String, String, Vec<Band>)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, (code, name))| {
                let bands = self
                    .worked
                    .get(&i)
                    .map(|s| s.iter().copied().collect())
                    .unwrap_or_default();
                (code.clone(), name.clone(), bands)
            })
            .collect()
    }
}

impl BandRules for JarlRules {
    fn points(&self, band: Band) -> u8 {
        match band {
            Band::B10_4G if self.rules.shf_points != 1 => 0,
            b if b >= Band::B2400 => self.rules.shf_points,
            _ => 1,
        }
    }

    fn reset(&mut self) {
        self.worked.clear();
    }

    fn add_multi(&mut self, q: &Qso, dupe: bool) -> (String, bool) {
        // zLog drops the last character whether or not it is a power code.
        let mut code = q.nr_rcvd.clone();
        code.pop();
        if dupe || !self.mode.accepts(q.mode) || q.band.is_warc() {
            return (code, false);
        }
        let new = self
            .lookup(&code)
            .is_some_and(|i| self.worked.entry(i).or_default().insert(q.band));
        (code, new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
        assert_eq!(JarlContest::Acag.caption("en"), "ACAG");
    }
    use zlog_core::{rescore, Mode};

    fn qso(call: &str, band: Band, mode: Mode, nr: &str) -> Qso {
        Qso {
            callsign: call.into(),
            band,
            mode,
            nr_rcvd: nr.into(),
            ..Qso::default()
        }
    }

    #[test]
    fn lists_are_loaded() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        assert_eq!(c.entries.len(), 61);
        let c = JarlContest::Acag.build_rules(ContestMode::Mix);
        assert!(c.entries.len() > 1300);
    }

    #[test]
    fn allja_score() {
        let mut log = Log {
            qsos: vec![
                qso("JA1AAA", Band::B7, Mode::Cw, "10H"),
                qso("JA1BBB", Band::B7, Mode::Cw, "10M"),
                qso("JA1AAA", Band::B7, Mode::Ssb, "10H"), // dupe
                qso("JA8CCC", Band::B14, Mode::Cw, "101P"),
                qso("JA2DDD", Band::B10, Mode::Cw, "18H"), // WARC
                qso("JA3EEE", Band::B144, Mode::Fm, "25M"), // out of band
            ],
            ..Log::default()
        };
        let mut c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        let sheet = rescore(&mut log, &mut c).unwrap();
        let t = sheet.total;
        assert_eq!((t.qsos, t.points, t.multi), (4, 3, 2));
        assert_eq!(sheet.score, 6);
        assert!(log.qsos[2].dupe);
        assert_eq!(log.qsos[3].multi1, "101");
    }

    #[test]
    fn allja_validates_numbers() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        assert!(c.valid_number(&qso("X", Band::B7, Mode::Cw, "10H")));
        assert!(c.valid_number(&qso("X", Band::B7, Mode::Cw, "101P")));
        assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "10")));
        assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "49H")));
    }

    #[test]
    fn field_day_uses_city_from_2400() {
        let c = JarlContest::FieldDay.build_rules(ContestMode::Mix);
        assert!(c.valid_number(&qso("X", Band::B430, Mode::Fm, "10M")));
        assert!(!c.valid_number(&qso("X", Band::B2400, Mode::Fm, "10M")));
        assert!(c.valid_number(&qso("X", Band::B2400, Mode::Fm, "100101M")));
        assert!(JarlContest::FieldDay.uses_coeff());
    }

    #[test]
    fn sent_number_follows_band() {
        let st = Station {
            prov: "10".into(),
            city: "100101".into(),
            ..Station::default()
        };
        let fd = JarlContest::FieldDay.build_rules(ContestMode::Mix);
        let m = Power::P005;
        assert_eq!(fd.sent_number(Band::B430, m, &st), "10M");
        assert_eq!(fd.sent_number(Band::B2400, m, &st), "100101M");
        let acag = JarlContest::Acag.build_rules(ContestMode::Mix);
        assert_eq!(
            acag.sent_number(Band::B7, st.power_for(Band::B7), &st),
            "100101H"
        );
    }

    #[test]
    fn field_day_check_multi_wants_the_right_length() {
        let c = JarlContest::FieldDay.build_rules(ContestMode::Mix);
        let at = |band, nr: &str| c.check_multi(&qso("X", band, Mode::Cw, nr));
        assert_eq!(at(Band::B430, "100101M"), MultiCheck::Invalid);
        assert_eq!(at(Band::B2400, "10M"), MultiCheck::Invalid);
        assert!(matches!(at(Band::B430, "10M"), MultiCheck::Known { .. }));
        assert!(matches!(
            at(Band::B2400, "100101"),
            MultiCheck::Known { .. }
        ));
    }

    #[test]
    fn exchange_carries_over_on_the_same_side_of_1200() {
        let fd = JarlContest::FieldDay.build_rules(ContestMode::Mix);
        let past = qso("X", Band::B430, Mode::Cw, "10M");
        assert_eq!(fd.exchange_from(Band::B144, &past), Some("10M".into()));
        assert_eq!(fd.exchange_from(Band::B2400, &past), None);
        let city = qso("X", Band::B2400, Mode::Cw, "100101M");
        assert_eq!(fd.exchange_from(Band::B430, &city), Some("10M".into()));
        let hokkaido = qso("X", Band::B2400, Mode::Cw, "010101M");
        assert_eq!(fd.exchange_from(Band::B430, &hokkaido), None);
    }

    #[test]
    fn six_down_points() {
        let c = JarlContest::SixDown.build_rules(ContestMode::Mix);
        assert_eq!(c.points(Band::B50), 1);
        assert_eq!(c.points(Band::B5600), 2);
        assert_eq!(c.points(Band::B10_4G), 0);
    }

    #[test]
    fn check_multi_reports_worked_bands() {
        let mut log = Log {
            qsos: vec![qso("JA1AAA", Band::B7, Mode::Cw, "100101H")],
            ..Log::default()
        };
        let mut c = JarlContest::Acag.build_rules(ContestMode::Mix);
        rescore(&mut log, &mut c).unwrap();
        match c.check_multi(&qso("JA1BBB", Band::B14, Mode::Cw, "100101")) {
            MultiCheck::Known {
                name,
                needed,
                worked_on,
                ..
            } => {
                assert!(name.contains("千代田"));
                assert!(needed);
                assert_eq!(worked_on, vec![Band::B7]);
            }
            MultiCheck::Invalid => panic!("100101 should be known"),
        }
    }
}
