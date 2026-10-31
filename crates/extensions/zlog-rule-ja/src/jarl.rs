use std::collections::{BTreeSet, HashMap};

use crate::texts;
use crate::{acag, allja, field_day, six_down};
use zlog_core::{
    Band, Contest, ContestDef, ContestMode, Log, MultiCheck, Power, Qso, Scoring, Station,
};
use zlog_rule::{expand_template, renew_by_band, BandRules};

pub(crate) const XPO_DAT: &str = include_str!("../data/XPO.DAT");
pub(crate) const ACAG_DAT: &str = include_str!("../data/ACAG.DAT");

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
        match self {
            JarlContest::AllJa => allja::RULES,
            JarlContest::SixDown => six_down::RULES,
            JarlContest::FieldDay => field_day::RULES,
            JarlContest::Acag => acag::RULES,
        }
    }

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

#[derive(Debug, Clone, Copy)]
pub(crate) enum NumberRule {
    Prefecture,
    City,
    ByBand,
}

#[derive(Debug, Clone)]
pub(crate) struct Rules {
    pub(crate) name: &'static str,
    pub(crate) bands: (Band, Band),
    pub(crate) lists: &'static [&'static str],
    pub(crate) number: NumberRule,
    pub(crate) sent: &'static str,
    pub(crate) coeff: bool,
    pub(crate) shf_points: u8,
    pub(crate) period: u32,
}

pub struct JarlRules {
    rules: Rules,
    mode: ContestMode,
    entries: Vec<(String, String)>,
    index: HashMap<String, usize>,
    worked: HashMap<usize, BTreeSet<Band>>,
}

fn parse_dat(text: &str) -> impl Iterator<Item = (String, String)> + '_ {
    zlog_cfg::parse_dat(text)
        .into_iter()
        .map(|e| (e.code, e.name))
}

fn split_power(nr: &str) -> (&str, Option<char>) {
    match nr.chars().last() {
        Some(c) if POWER_CODES.contains(&c) => (&nr[..nr.len() - 1], Some(c)),
        _ => (nr, None),
    }
}

impl JarlRules {
    fn prefecture_code(nr: &str) -> Option<String> {
        match nr.parse::<u32>().ok()? {
            n @ 101..=114 => Some(n.to_string()),
            n @ 2..=48 => Some(format!("{n:02}")),
            _ => None,
        }
    }

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

    fn exchange_label(&self, language: &str) -> Option<String> {
        let city = self.rules.lists.contains(&ACAG_DAT);
        let label = match (city, language) {
            (true, "ja") => "市郡区 + 電力",
            (true, _) => "City + power",
            (false, "ja") => "都府県支庁 + 電力",
            (false, _) => "Prefecture + power",
        };
        Some(label.into())
    }

    fn band_range(&self) -> (Band, Band) {
        self.rules.bands
    }

    fn contest_mode(&self) -> ContestMode {
        self.mode
    }

    fn period_hours(&self) -> u32 {
        self.rules.period
    }

    fn start_hour(&self) -> Option<u32> {
        Some(21)
    }

    fn adif_fields(&self, _q: &Qso) -> Vec<(String, String)> {
        vec![("contest_id".into(), "JA_DOMESTIC".into())]
    }

    fn sent_number(&self, band: Band, power: Power, station: &Station) -> String {
        let qth = match self.rules.number {
            NumberRule::Prefecture => &station.prov,
            NumberRule::City => &station.city,
            NumberRule::ByBand if band <= Band::B1200 => &station.prov,
            NumberRule::ByBand => &station.city,
        };
        expand_template(self.rules.sent, &station.prov, qth, power)
    }

    fn sent_template(&self, _station: &Station) -> Option<String> {
        Some(self.rules.sent.into())
    }

    fn score(&mut self, log: &Log) -> Scoring {
        let coeff = self.rules.coeff.then(|| log.score_coeff());
        renew_by_band(log, self, coeff)
    }

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

    fn extract_multi(&self, q: &Qso) -> String {
        split_power(&q.nr_rcvd).0.to_string()
    }

    fn check_multi(&self, q: &Qso) -> MultiCheck {
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
pub(crate) mod tests {
    pub(crate) use super::*;
    pub(crate) use zlog_core::{
        rescore, Band, Contest, ContestDef, ContestMode, Log, Mode, MultiCheck, Power, Qso, Station,
    };
    pub(crate) use zlog_rule::BandRules;

    #[test]
    fn city_dat_follows_tcitylist() {
        let text = "h\n101 \t東京\u{3000}\r\nnospace\n 102 x\nend of file\n";
        let got: Vec<_> = super::parse_dat(text).collect();
        assert_eq!(
            got,
            vec![
                ("101".into(), "東京\u{3000}".into()),
                (String::new(), "nospace".into()),
                (String::new(), "102 x".into()),
            ]
        );
    }

    pub(crate) fn qso(call: &str, band: Band, mode: Mode, nr: &str) -> Qso {
        Qso {
            callsign: call.into(),
            band,
            mode,
            nr_rcvd: nr.into(),
            ..Qso::default()
        }
    }

    #[test]
    fn caption_from_texts() {
        assert_eq!(JarlContest::Acag.caption("en"), "ACAG");
    }

    #[test]
    fn the_exchange_is_named_by_the_list() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        assert_eq!(c.exchange_label("ja").as_deref(), Some("都府県支庁 + 電力"));
        let c = JarlContest::Acag.build_rules(ContestMode::Mix);
        assert_eq!(c.exchange_label("en").as_deref(), Some("City + power"));
    }

    #[test]
    fn lists_are_loaded() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        assert_eq!(c.entries.len(), 61);
        let c = JarlContest::Acag.build_rules(ContestMode::Mix);
        assert!(c.entries.len() > 1300);
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
    fn extract_multi_drops_the_power_code() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        let x = |nr: &str| c.extract_multi(&qso("JA1AAA", Band::B7, Mode::Cw, nr));
        assert_eq!(x("10H"), "10");
        assert_eq!(x("10"), "10");
        assert_eq!(x("99P"), "99");
        assert_eq!(x("H"), "");
        assert_eq!(x(""), "");
        let c = JarlContest::Acag.build_rules(ContestMode::Mix);
        let x = |nr: &str| c.extract_multi(&qso("JA1AAA", Band::B7, Mode::Cw, nr));
        assert_eq!(x("100101M"), "100101");
    }

    #[test]
    fn exchange_from_repeats_the_number_in_all_ja_and_acag() {
        for k in [JarlContest::AllJa, JarlContest::Acag] {
            let c = k.build_rules(ContestMode::Mix);
            for nr in ["10H", "100101M", "", "X"] {
                let past = qso("JA1AAA", Band::B7, Mode::Cw, nr);
                assert_eq!(c.exchange_from(Band::B14, &past), Some(nr.into()), "{k:?}");
            }
        }
    }

    #[test]
    fn check_multi_invalid_numbers() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        let at = |c: &JarlRules, b, nr: &str| c.check_multi(&qso("JA1AAA", b, Mode::Cw, nr));
        assert_eq!(at(&c, Band::B7, ""), MultiCheck::Invalid);
        assert_eq!(at(&c, Band::B7, "H"), MultiCheck::Invalid);
        assert_eq!(at(&c, Band::B7, "99H"), MultiCheck::Invalid);
        assert_eq!(at(&c, Band::B7, "1O0H"), MultiCheck::Invalid);
        assert!(matches!(at(&c, Band::B7, "10"), MultiCheck::Known { .. }));
        let c = JarlContest::Acag.build_rules(ContestMode::Mix);
        assert_eq!(at(&c, Band::B7, "999999H"), MultiCheck::Invalid);
        assert_eq!(at(&c, Band::B7, "10"), MultiCheck::Invalid);
    }

    #[test]
    fn multi_table_lists_entries_with_worked_bands() {
        let mut c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        let empty = c.multi_table();
        assert!(!empty.is_empty());
        assert!(empty.iter().all(|r| r.2.is_empty()));
        let mut log = Log {
            qsos: vec![
                qso("JA1AAA", Band::B7, Mode::Cw, "10H"),
                qso("JA1BBB", Band::B21, Mode::Ssb, "10M"),
            ],
            ..Log::default()
        };
        rescore(&mut log, &mut c).unwrap();
        let t = c.multi_table();
        assert_eq!(t.len(), empty.len());
        let row = t.iter().find(|r| r.0 == "10").unwrap();
        assert_eq!(row.2, vec![Band::B7, Band::B21]);
        assert_eq!(t.iter().filter(|r| !r.2.is_empty()).count(), 1);
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

    #[test]
    fn periods_start_hours_and_adif() {
        for (c, hours) in [
            (JarlContest::AllJa, 24),
            (JarlContest::SixDown, 18),
            (JarlContest::FieldDay, 18),
            (JarlContest::Acag, 24),
        ] {
            let r = c.build_rules(ContestMode::Mix);
            assert_eq!(r.period_hours(), hours, "{c:?}");
            assert_eq!(r.start_hour(), Some(21));
            let x = qso("JA1AAA", Band::B7, Mode::Cw, "10H");
            assert_eq!(
                r.adif_fields(&x),
                vec![("contest_id".to_string(), "JA_DOMESTIC".to_string())]
            );
        }
    }
}
