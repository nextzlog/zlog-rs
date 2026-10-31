use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use zlog_core::{
    Band, BandScore, Contest, ContestDef, ContestMode, Log, MultiCheck, Power, Qso, QsoScore,
    ScoreSheet, Scoring, Station,
};

use crate::cty::{self, Cty};
use crate::lists::{self, States};
use crate::texts;
use crate::{
    allasian, apsprint, arrl10, arrldx, bartg, cqwpx, cqww, iaru, iota, jarlrtty, jidx, pedi, waedc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldContest {
    Cqww,
    Wpx,
    Iaru,
    ArrlDxDx,
    ArrlDxW,
    Wae,
    Iota,
    Arrl10,
    AllAsian,
    Jidx,
    JidxDx,
    ApSprint,
    Bartg,
    JarlRtty,
    Pedi,
}

use WorldContest::*;

pub(crate) const CW_PH_RTTY: &[ContestMode] =
    &[ContestMode::Cw, ContestMode::Phone, ContestMode::Rtty];
pub(crate) const MIX_CW_PH_ALL: &[ContestMode] = &[
    ContestMode::Mix,
    ContestMode::Cw,
    ContestMode::Phone,
    ContestMode::All,
];
pub(crate) const CW_PH_ALL: &[ContestMode] =
    &[ContestMode::Cw, ContestMode::Phone, ContestMode::All];
pub(crate) const CW_PH: &[ContestMode] = &[ContestMode::Cw, ContestMode::Phone];
pub(crate) const RTTY: &[ContestMode] = &[ContestMode::Rtty];
pub(crate) const EVERY: &[ContestMode] = &[
    ContestMode::Mix,
    ContestMode::Cw,
    ContestMode::Phone,
    ContestMode::Rtty,
    ContestMode::All,
];

pub(crate) type Row = (String, String, Vec<Band>);

pub(crate) type Primary = (usize, String, String, Slot);

pub(crate) type Slot = (Band, u8);

pub(crate) trait Rule: Sync {
    fn id(&self) -> &'static str;

    fn caption_key(&self) -> &'static str;

    fn name(&self) -> &'static str;

    fn modes(&self) -> &'static [ContestMode] {
        CW_PH_RTTY
    }

    fn bands(&self) -> (Band, Band) {
        (Band::B1_9, Band::B28)
    }

    fn waedc(&self) -> bool {
        true
    }

    fn different_mode(&self) -> bool {
        false
    }

    fn serial(&self) -> bool {
        false
    }

    fn period(&self) -> u32 {
        48
    }

    fn start_hour(&self) -> Option<u32> {
        Some(0)
    }

    fn uses_utc(&self) -> bool {
        true
    }

    fn uses_warc(&self) -> bool {
        false
    }

    fn sent_with_time(&self) -> bool {
        false
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str;

    fn adif_exchange_field(&self) -> &'static str {
        ""
    }

    fn adif_extra(&self, _w: &WorldRules, _q: &Qso) -> Option<(String, String)> {
        None
    }

    fn states(&self) -> &'static States {
        lists::no_states()
    }

    fn islands(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn read_sent(&self, _my: &mut My, _sent: &str) {}

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, my: &My, s: &mut QsoScore);

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary>;

    fn valid_number(&self, _w: &WorldRules, q: &Qso) -> bool {
        !q.nr_rcvd.trim().is_empty()
    }

    fn sent_number(&self, w: &WorldRules, power: Power, station: &Station) -> String;

    fn exchange_from(&self, _w: &WorldRules, past: &Qso) -> Option<String> {
        (!self.serial()).then(|| past.nr_rcvd.clone())
    }

    fn extract_multi(&self, _w: &WorldRules, q: &Qso) -> String {
        q.nr_rcvd.clone()
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row>;

    fn dupes_score(&self) -> bool {
        false
    }

    fn counts_every_mode(&self) -> bool {
        false
    }

    fn multi_weight(&self, _band: Band) -> u32 {
        1
    }

    fn continent_factor(&self) -> bool {
        false
    }

    fn counts_qtcs(&self) -> bool {
        false
    }

    fn total(&self, total: &BandScore, _qtcs: u32, _continents: u64) -> u64 {
        total.points as u64 * total.multi as u64
    }
}

impl WorldContest {
    pub const ALL: [WorldContest; 15] = [
        Cqww, Wpx, Iaru, ArrlDxDx, ArrlDxW, Wae, Iota, Arrl10, AllAsian, Jidx, JidxDx, ApSprint,
        Bartg, JarlRtty, Pedi,
    ];

    pub(crate) fn rule(self) -> &'static dyn Rule {
        match self {
            Cqww => &cqww::CqwwRule,
            Wpx => &cqwpx::WpxRule,
            Iaru => &iaru::IaruRule,
            ArrlDxDx => &arrldx::ArrlDxDxRule,
            ArrlDxW => &arrldx::ArrlDxWRule,
            Wae => &waedc::WaeRule,
            Iota => &iota::IotaRule,
            Arrl10 => &arrl10::Arrl10Rule,
            AllAsian => &allasian::AllAsianRule,
            Jidx => &jidx::JidxRule,
            JidxDx => &jidx::JidxDxRule,
            ApSprint => &apsprint::ApSprintRule,
            Bartg => &bartg::BartgRule,
            JarlRtty => &jarlrtty::JarlRttyRule,
            Pedi => &pedi::PediRule,
        }
    }

    pub fn id(self) -> &'static str {
        self.rule().id()
    }
}

impl ContestDef for WorldContest {
    fn id(&self) -> &str {
        WorldContest::id(*self)
    }

    fn caption(&self, language: &str) -> String {
        texts().get(language, self.rule().caption_key()).into()
    }

    fn group(&self, language: &str) -> String {
        texts().get(language, "group.world").into()
    }

    fn modes(&self) -> &[ContestMode] {
        self.rule().modes()
    }

    fn uses_utc(&self) -> bool {
        self.rule().uses_utc()
    }

    fn build(&self, mode: ContestMode) -> Box<dyn Contest> {
        Box::new(WorldRules::new(*self, mode, Cty::global()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct My {
    pub(crate) country: String,
    pub(crate) continent: String,
    pub(crate) itu: String,
    pub(crate) iota: String,
}

pub struct WorldRules {
    pub(crate) kind: WorldContest,
    pub(crate) mode: ContestMode,
    pub(crate) cty: Arc<Cty>,
    pub(crate) states: &'static States,
    pub(crate) islands: Vec<(String, String)>,
    worked: [HashMap<String, BTreeSet<Slot>>; 3],
    last_serials: BTreeMap<u8, i32>,
}

fn serial_of(q: &Qso, with_time: bool) -> i32 {
    if q.serial > 0 {
        return q.serial;
    }
    let mut digits: String = q
        .nr_sent
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if with_time {
        let keep = digits.len().saturating_sub(4);
        digits.truncate(keep);
    }
    digits.parse().unwrap_or(0)
}

pub(crate) fn int_of(s: &str) -> Option<i64> {
    let s = s.trim();
    let d = s.strip_prefix(['+', '-']).unwrap_or(s);
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

pub(crate) fn split_zone_state(nr: &str) -> (String, String) {
    let s = nr.trim().to_ascii_uppercase();
    let n = s.bytes().take(2).take_while(u8::is_ascii_digit).count();
    (s[..n].to_string(), s[n..].trim().to_string())
}

pub(crate) fn cq_zone(nr: &str) -> Option<i64> {
    let (zone, _) = split_zone_state(nr);
    int_of(&zone).filter(|z| (1..=40).contains(z))
}

pub(crate) fn listed_area(area: &str) -> bool {
    !(area.starts_with("VK") && (area.ends_with('0') || area.ends_with('9')))
}

pub(crate) fn area_rows(code: &str) -> Option<impl Iterator<Item = (String, String)>> {
    let (name, digits) = match code {
        "JA" => ("Japan", 0..=9),
        "K" => ("United States", 0..=9),
        "VE" => ("Canada", 0..=9),
        "VK" => ("Australia", 1..=8),
        _ => return None,
    };
    let code = code.to_string();
    Some(digits.map(move |d| (format!("{code}{d}"), format!("{name} (Area {d})"))))
}

fn count(text: &str) -> u32 {
    text.split(',').filter(|s| !s.trim().is_empty()).count() as u32
}

impl WorldRules {
    pub fn new(kind: WorldContest, mode: ContestMode, cty: Arc<Cty>) -> Self {
        let rule = kind.rule();
        WorldRules {
            kind,
            mode,
            cty,
            states: rule.states(),
            islands: rule.islands(),
            worked: Default::default(),
            last_serials: BTreeMap::new(),
        }
    }

    fn rule(&self) -> &'static dyn Rule {
        self.kind.rule()
    }

    pub(crate) fn mark(&mut self, t: usize, key: &str, slot: Slot, per_contest: bool) -> bool {
        if key.is_empty() {
            return false;
        }
        let set = self.worked[t].entry(key.to_string()).or_default();
        let new = if per_contest {
            set.is_empty()
        } else {
            !set.contains(&slot)
        };
        set.insert(slot);
        new
    }

    pub(crate) fn worked_bands(&self, t: usize, key: &str) -> Vec<Band> {
        let bands: BTreeSet<Band> = self.worked[t]
            .get(key)
            .map(|s| s.iter().map(|(b, _)| *b).collect())
            .unwrap_or_default();
        bands.into_iter().collect()
    }

    fn my(&self, log: &Log) -> My {
        let call = log.header.callsign.trim();
        let waedc = self.rule().waedc();
        let mut my = if !self.cty.is_loaded() || call.is_empty() {
            My {
                country: "JA".into(),
                continent: "AS".into(),
                itu: "45".into(),
                iota: String::new(),
            }
        } else {
            let f = self.cty.lookup(call, waedc);
            My {
                country: f.country.code.clone(),
                continent: f.continent().to_string(),
                itu: self.cty.guess_itu_zone(call, waedc),
                iota: String::new(),
            }
        };
        if let Some(last) = log.qsos.iter().rev().find(|q| !q.nr_sent.is_empty()) {
            self.rule().read_sent(&mut my, &last.nr_sent);
        }
        my
    }

    fn score_band(&self, band: Band) -> bool {
        let (lo, hi) = self.rule().bands();
        band >= lo && band <= hi && (self.uses_warc() || !band.is_warc()) && band != Band::B10_4G
    }

    fn dupes(&self, log: &Log) -> Vec<bool> {
        let mut seen = std::collections::HashSet::new();
        log.qsos
            .iter()
            .map(|q| {
                let mut key = format!("{}\t{}", q.core_call(), q.band.label());
                if self.rule().different_mode() {
                    key.push('\t');
                    key.push_str(if log.dupe_rule.all_phone {
                        q.mode_group()
                    } else {
                        q.mode.label()
                    });
                }
                !q.invalid && !seen.insert(key)
            })
            .collect()
    }

    pub(crate) fn country_rows(
        &self,
        t: usize,
        filter: impl Fn(&cty::Country) -> bool,
    ) -> Vec<Row> {
        let waedc = self.rule().waedc();
        self.cty
            .countries()
            .iter()
            .skip(1)
            .filter(|c| waedc || !c.is_waedc())
            .filter(|c| filter(c))
            .map(|c| {
                (
                    c.code.clone(),
                    c.name.clone(),
                    self.worked_bands(t, &c.code),
                )
            })
            .collect()
    }

    pub(crate) fn extra_rows(&self, t: usize, known: &[Row]) -> Vec<Row> {
        let mut keys: Vec<&String> = self.worked[t]
            .keys()
            .filter(|k| !known.iter().any(|(c, _, _)| c == *k))
            .collect();
        keys.sort();
        keys.into_iter()
            .map(|k| (k.clone(), String::new(), self.worked_bands(t, k)))
            .collect()
    }

    pub(crate) fn zone_rows(&self, n: u32, label: &str) -> Vec<Row> {
        (1..=n)
            .map(|z| {
                let k = z.to_string();
                let bands = self.worked_bands(0, &k);
                (k, format!("{label} {z}"), bands)
            })
            .collect()
    }

    pub(crate) fn state_rows(&self, t: usize) -> Vec<Row> {
        self.states
            .iter()
            .map(|s| {
                (
                    s.abbrev.clone(),
                    s.name.clone(),
                    self.worked_bands(t, &s.abbrev),
                )
            })
            .collect()
    }

    pub(crate) fn country_primary(&self, q: &Qso) -> Option<Primary> {
        let f = self.cty.lookup(&q.callsign, self.rule().waedc());
        (!f.is_unknown()).then(|| {
            (
                0,
                f.country.code.clone(),
                f.country.name.clone(),
                (q.band, 0),
            )
        })
    }

    pub(crate) fn serial_text(&self, station: &Station) -> String {
        format!("{:03}", self.next_serial_for(station.tx_nr))
    }

    pub(crate) fn zone_exchange(&self, past: &Qso) -> Option<String> {
        let z = self.cty.guess_cq_zone(&past.callsign, self.rule().waedc());
        Some(if z.is_empty() {
            past.nr_rcvd.clone()
        } else {
            z
        })
    }

    pub fn next_serial(&self) -> i32 {
        self.next_serial_for(0)
    }

    pub fn next_serial_for(&self, tx: u8) -> i32 {
        self.last_serials
            .get(&tx)
            .copied()
            .unwrap_or(0)
            .saturating_add(1)
    }
}

impl Contest for WorldRules {
    fn name(&self) -> &str {
        self.rule().name()
    }

    fn band_range(&self) -> (Band, Band) {
        self.rule().bands()
    }

    fn uses_warc(&self) -> bool {
        self.rule().uses_warc()
    }

    fn contest_mode(&self) -> ContestMode {
        self.mode
    }

    fn period_hours(&self) -> u32 {
        self.rule().period()
    }

    fn start_hour(&self) -> Option<u32> {
        self.rule().start_hour()
    }

    fn adif_fields(&self, q: &Qso) -> Vec<(String, String)> {
        let rule = self.rule();
        let mut v = vec![(
            "contest_id".to_string(),
            rule.adif_contest_id(self.mode).to_string(),
        )];
        let field = rule.adif_exchange_field();
        if !field.is_empty() {
            v.push((field.into(), q.nr_rcvd.clone()));
        }
        v.extend(rule.adif_extra(self, q));
        v
    }

    fn sent_number(&self, _band: Band, power: Power, station: &Station) -> String {
        self.rule().sent_number(self, power, station)
    }

    fn sent_template(&self, station: &Station) -> Option<String> {
        let usa = || {
            let code = &self
                .cty
                .lookup(&station.callsign, self.rule().waedc())
                .country
                .code;
            matches!(code.as_str(), "K" | "N" | "W")
        };
        let template = match self.kind {
            Cqww => "$Z",
            Wpx | Wae | ApSprint => "$S",
            Iaru => "$I",
            ArrlDxDx => "$N",
            ArrlDxW | Jidx | JidxDx => "$V",
            Iota => "$S$T",
            Arrl10 if usa() => "$V",
            Arrl10 => "$S",
            AllAsian | JarlRtty => "$A",
            Bartg => "$S$D",
            Pedi => "",
        };
        Some(template.into())
    }

    fn sent_with_time(&self) -> bool {
        self.rule().sent_with_time()
    }

    fn next_serial_on(&self, band: Band) -> Option<i32> {
        self.next_serial_of_tx(band, 0)
    }

    fn next_serial_of_tx(&self, _band: Band, tx_nr: u8) -> Option<i32> {
        self.rule().serial().then(|| self.next_serial_for(tx_nr))
    }

    fn exchange_from(&self, _band: Band, past: &Qso) -> Option<String> {
        self.rule().exchange_from(self, past)
    }

    fn valid_number(&self, q: &Qso) -> bool {
        self.rule().valid_number(self, q)
    }

    fn score(&mut self, log: &Log) -> Scoring {
        let rule = self.rule();
        for t in &mut self.worked {
            t.clear();
        }
        let with_time = self.sent_with_time();
        self.last_serials = log
            .qsos
            .iter()
            .map(|q| (q.tx, serial_of(q, with_time)))
            .collect();
        let my = self.my(log);
        let dupes = self.dupes(log);
        let (lo, hi) = rule.bands();
        let mut rows: BTreeMap<Band, BandScore> = Band::range(lo, hi)
            .filter(|b| self.score_band(*b))
            .map(|b| (b, BandScore::default()))
            .collect();
        let mut qtcs = 0u32;
        let mut continents = 0u64;
        let mut qsos = Vec::with_capacity(log.qsos.len());
        for (q, dupe) in log.qsos.iter().zip(dupes) {
            let mut s = QsoScore {
                dupe,
                ..QsoScore::default()
            };
            let in_contest =
                q.band >= lo && q.band <= hi && (self.uses_warc() || !q.band.is_warc());
            if !in_contest || q.invalid {
                qsos.push(s);
                continue;
            }
            let mode_ok = self.mode.accepts(q.mode);
            rule.add(self, q, dupe || !mode_ok, &my, &mut s);
            if !rule.dupes_score() && (dupe || !mode_ok) {
                s.points = 0;
            }
            let row = rows.entry(q.band).or_default();
            if mode_ok || rule.counts_every_mode() {
                row.qsos += 1;
                if q.mode == zlog_core::Mode::Cw {
                    row.cw_qsos += 1;
                }
            }
            let weight = rule.multi_weight(q.band);
            for (i, (new, text)) in [
                (s.new_multi1, &s.multi1),
                (s.new_multi2, &s.multi2),
                (s.new_multi3, &s.multi3),
            ]
            .into_iter()
            .enumerate()
            {
                if new && rule.continent_factor() && i == 1 {
                    continents += 1;
                } else if new {
                    row.multi += weight * count(text);
                }
            }
            row.points += s.points as u32;
            if rule.counts_qtcs() && s.points > 0 && q.memo.contains("[QTC") {
                qtcs += 1;
            }
            qsos.push(s);
        }
        let bands: BTreeMap<Band, BandScore> = rows
            .into_iter()
            .filter(|(b, _)| self.score_band(*b))
            .collect();
        let total = bands.values().fold(BandScore::default(), |a, b| BandScore {
            qsos: a.qsos + b.qsos,
            cw_qsos: a.cw_qsos + b.cw_qsos,
            points: a.points + b.points,
            multi: a.multi + b.multi,
        });
        let score = rule.total(&total, qtcs, continents);
        Scoring {
            qsos,
            sheet: ScoreSheet {
                bands,
                total,
                score,
                with_coeff: None,
            },
        }
    }

    fn extract_multi(&self, q: &Qso) -> String {
        self.rule().extract_multi(self, q)
    }

    fn check_multi(&self, q: &Qso) -> MultiCheck {
        let Some((t, code, name, slot)) = self.rule().primary(self, q) else {
            return MultiCheck::Invalid;
        };
        let set = self.worked[t].get(&code);
        let needed = match slot.0 {
            Band::Unknown => set.is_none_or(|s| s.is_empty()),
            _ => !set.is_some_and(|s| s.contains(&slot)),
        };
        MultiCheck::Known {
            worked_on: self.worked_bands(t, &code),
            code,
            name,
            needed,
        }
    }

    fn multi_table(&self) -> Vec<(String, String, Vec<Band>)> {
        self.rule().multi_table(self)
    }
}

#[cfg(test)]
pub(crate) mod tests {

    pub(crate) use std::sync::Arc;

    pub(crate) use zlog_core::{
        rescore, Band, Contest, ContestDef, ContestMode, Log, Mode, MultiCheck, Power, Qso,
        ScoreSheet, Station,
    };

    pub(crate) use crate::cty::tests::FIXTURE;
    pub(crate) use crate::cty::Cty;
    pub(crate) use crate::world::{WorldContest, WorldContest::*, WorldRules};

    pub(crate) fn rules(kind: WorldContest, mode: ContestMode) -> WorldRules {
        WorldRules::new(kind, mode, Arc::new(Cty::parse(FIXTURE)))
    }

    pub(crate) fn q(call: &str, band: Band, mode: Mode, nr: &str) -> Qso {
        Qso {
            callsign: call.into(),
            band,
            mode,
            nr_rcvd: nr.into(),
            ..Qso::default()
        }
    }

    pub(crate) fn log(me: &str, qsos: Vec<Qso>) -> Log {
        let mut l = Log::default();
        l.header.callsign = me.into();
        l.qsos = qsos;
        l
    }

    pub(crate) fn run(
        kind: WorldContest,
        mode: ContestMode,
        l: &mut Log,
    ) -> (WorldRules, ScoreSheet) {
        let mut c = rules(kind, mode);
        let sheet = rescore(l, &mut c).unwrap();
        (c, sheet)
    }

    pub(crate) fn totals(s: &ScoreSheet) -> (u32, u32, u32, u64) {
        (s.total.qsos, s.total.points, s.total.multi, s.score)
    }

    #[test]
    fn without_cty_everything_is_unknown() {
        let mut c = WorldRules::new(Cqww, ContestMode::Cw, Arc::new(Cty::default()));
        let mut l = log("JA1ZLO", vec![q("W1AW", Band::B14, Mode::Cw, "5")]);
        let s = rescore(&mut l, &mut c).unwrap();
        assert_eq!(totals(&s), (1, 3, 1, 3));
    }

    pub(crate) fn station_aged(age: &str) -> Station {
        Station {
            callsign: "JA1ZLO".into(),
            age: age.into(),
            ..Station::default()
        }
    }

    pub(crate) fn invalid(mut x: Qso) -> Qso {
        x.invalid = true;
        x
    }

    #[test]
    fn periods_and_start_hours() {
        let expect = [
            (Cqww, 48, Some(0)),
            (Wpx, 48, Some(0)),
            (Iaru, 24, Some(12)),
            (ArrlDxDx, 48, Some(0)),
            (ArrlDxW, 48, Some(0)),
            (Wae, 48, Some(0)),
            (Iota, 24, Some(12)),
            (Arrl10, 48, Some(0)),
            (AllAsian, 48, Some(0)),
            (Jidx, 30, Some(7)),
            (JidxDx, 30, Some(7)),
            (ApSprint, 0, None),
            (Bartg, 48, Some(0)),
            (JarlRtty, 48, Some(0)),
            (Pedi, 0, None),
        ];
        assert_eq!(expect.len(), WorldContest::ALL.len());
        for (kind, hours, start) in expect {
            let c = rules(kind, ContestMode::Cw);
            assert_eq!(c.period_hours(), hours, "{kind:?}");
            assert_eq!(c.start_hour(), start, "{kind:?}");
        }
    }

    pub(crate) fn adif(kind: WorldContest, mode: ContestMode, x: &Qso) -> Vec<(String, String)> {
        rules(kind, mode).adif_fields(x)
    }

    pub(crate) fn field(v: &[(String, String)], name: &str) -> Option<String> {
        v.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    #[test]
    fn adif_fields_follow_the_mode() {
        let x = q("W1AW", Band::B14, Mode::Cw, "5");
        let id = |k, m| field(&adif(k, m, &x), "contest_id").unwrap();
        assert_eq!(id(Cqww, ContestMode::Cw), "CQWW-CW");
        assert_eq!(id(Cqww, ContestMode::Phone), "CQWW-SSB");
        assert_eq!(id(Cqww, ContestMode::Rtty), "CQWW-RTTY");
        assert_eq!(id(Cqww, ContestMode::Mix), "");
        assert_eq!(id(Wpx, ContestMode::Phone), "CQWPX-SSB");
        assert_eq!(id(Wae, ContestMode::Cw), "DARC-WAEDC-CW");
        assert_eq!(id(ArrlDxDx, ContestMode::Phone), "ARRLDX-SSB");
        assert_eq!(id(ArrlDxW, ContestMode::Rtty), "");
        assert_eq!(id(AllAsian, ContestMode::Cw), "ALL-ASIAN-DX-CW");
        assert_eq!(id(Jidx, ContestMode::Phone), "JIDX-SSB");
        assert_eq!(id(JidxDx, ContestMode::Rtty), "");
        assert_eq!(id(Iaru, ContestMode::Mix), "IARU-HF");
        assert_eq!(id(Iota, ContestMode::Cw), "RSGB-IOTA");
        assert_eq!(id(Arrl10, ContestMode::Mix), "ARRL-10");
        assert_eq!(id(ApSprint, ContestMode::Cw), "AP-SPRINT");
        assert_eq!(id(Bartg, ContestMode::Rtty), "BARTG-RTTY");
        assert_eq!(id(Pedi, ContestMode::All), "");
    }

    #[test]
    fn adif_exchange_and_extra_fields() {
        let x = q("W1AW", Band::B14, Mode::Cw, "5");
        let f = |k, n| field(&adif(k, ContestMode::Cw, &x), n);
        assert_eq!(f(Cqww, "cqz").as_deref(), Some("5"));
        assert_eq!(f(Iaru, "ituz").as_deref(), Some("5"));
        assert_eq!(f(ArrlDxDx, "state").as_deref(), Some("5"));
        assert_eq!(f(ArrlDxW, "rx_pwr").as_deref(), Some("5"));
        assert_eq!(f(AllAsian, "age").as_deref(), Some("5"));
        assert_eq!(f(Bartg, "cont").as_deref(), Some("NA"));
        assert_eq!(adif(Wae, ContestMode::Cw, &x).len(), 1);
        let mut scored = x.clone();
        scored.multi1 = "W1".into();
        let v = adif(Wpx, ContestMode::Cw, &scored);
        assert_eq!(field(&v, "pfx").as_deref(), Some("W1"));
        let u = q("ZZ9ZZ", Band::B14, Mode::Rtty, "001");
        assert_eq!(
            field(&adif(Bartg, ContestMode::Rtty, &u), "cont").as_deref(),
            Some("")
        );
    }

    pub(crate) fn on_tx(mut x: Qso, tx: u8, serial: i32) -> Qso {
        x.tx = tx;
        x.serial = serial;
        x
    }

    #[test]
    fn serials_follow_the_own_tx() {
        let mut l = log(
            "JA1ZLO",
            vec![
                on_tx(q("W1AW", Band::B7, Mode::Cw, "001"), 0, 1),
                on_tx(q("K6XX", Band::B7, Mode::Cw, "001"), 1, 1),
                on_tx(q("DL1AA", Band::B14, Mode::Cw, "002"), 1, 2),
                on_tx(q("BY1AA", Band::B14, Mode::Cw, "003"), 0, 2),
            ],
        );
        let (c, _) = run(Wpx, ContestMode::Cw, &mut l);
        assert_eq!(c.next_serial_of_tx(Band::B7, 0), Some(3));
        assert_eq!(c.next_serial_of_tx(Band::B7, 1), Some(3));
        assert_eq!(c.next_serial_of_tx(Band::B7, 5), Some(1));
        assert_eq!(c.next_serial_on(Band::B7), Some(3));
        let st = Station {
            tx_nr: 5,
            ..Station::default()
        };
        assert_eq!(c.sent_number(Band::B7, Power::P100, &st), "001");
        let (c, _) = run(Cqww, ContestMode::Cw, &mut l);
        assert_eq!(c.next_serial_of_tx(Band::B7, 1), None);
    }

    #[test]
    fn serial_of_an_empty_log_and_of_unnumbered_qsos() {
        let c = rules(Wae, ContestMode::Cw);
        assert_eq!(c.next_serial_on(Band::B7), Some(1));
        let mut l = log("JA1ZLO", vec![q("DL1AA", Band::B7, Mode::Cw, "")]);
        l.qsos[0].nr_sent = "012".into();
        let (c, _) = run(Wae, ContestMode::Cw, &mut l);
        assert_eq!(c.next_serial_on(Band::B7), Some(13));
        l.qsos[0].nr_sent = "ABC".into();
        let (c, _) = run(Wae, ContestMode::Cw, &mut l);
        assert_eq!(c.next_serial_on(Band::B7), Some(1));
    }

    #[test]
    fn serial_after_a_number_sent_with_the_time() {
        let mut l = log("JA1ZLO", vec![q("G3AAA", Band::B14, Mode::Rtty, "")]);
        l.qsos[0].nr_sent = "0011307".into();
        let (c, _) = run(Bartg, ContestMode::Mix, &mut l);
        assert_eq!(c.next_serial_on(Band::B14), Some(2));
        let (c, _) = run(Wae, ContestMode::Mix, &mut l);
        assert_eq!(c.next_serial_on(Band::B14), Some(11308));
    }

    #[test]
    fn sent_templates_match_zlog() {
        let me = Station {
            callsign: "JA1ZLO".into(),
            ..Station::default()
        };
        let zlog = [
            (Cqww, "$Z"),
            (Wpx, "$S"),
            (Iaru, "$I"),
            (ArrlDxDx, "$N"),
            (ArrlDxW, "$V"),
            (Wae, "$S"),
            (Iota, "$S$T"),
            (Arrl10, "$S"),
            (AllAsian, "$A"),
            (Jidx, "$V"),
            (JidxDx, "$V"),
            (ApSprint, "$S"),
            (Bartg, "$S$D"),
            (JarlRtty, "$A"),
            (Pedi, ""),
        ];
        assert_eq!(zlog.len(), WorldContest::ALL.len());
        for (k, want) in zlog {
            let t = rules(k, ContestMode::Mix).sent_template(&me);
            assert_eq!(t.as_deref(), Some(want), "{k:?}");
        }
        let us = Station {
            callsign: "K1ZZ".into(),
            ..Station::default()
        };
        let t = rules(Arrl10, ContestMode::Mix).sent_template(&us);
        assert_eq!(t.as_deref(), Some("$V"));
    }

    #[test]
    fn sent_number_of_every_contest() {
        let me = Station {
            callsign: "JA1ZLO".into(),
            prov: "10".into(),
            age: "45".into(),
            ..Station::default()
        };
        let sent = |k| rules(k, ContestMode::Mix).sent_number(Band::B14, Power::P100, &me);
        assert_eq!(sent(Cqww), "25");
        assert_eq!(sent(Iaru), "45");
        assert_eq!(sent(ArrlDxDx), "100");
        assert_eq!(
            rules(ArrlDxDx, ContestMode::Mix).sent_number(Band::B14, Power::P1000, &me),
            "1KW"
        );
        for k in [Wpx, Wae, Iota, ApSprint, Bartg] {
            assert_eq!(sent(k), "001", "{k:?}");
        }
        assert_eq!(sent(AllAsian), "45");
        assert_eq!(sent(JarlRtty), "45");
        assert_eq!(sent(Pedi), "");
        assert_eq!(sent(Arrl10), "001");
        for k in WorldContest::ALL {
            let _ =
                rules(k, ContestMode::Mix).sent_number(Band::B14, Power::P100, &Station::default());
        }
    }

    #[test]
    fn exchange_from_past_qsos() {
        let past = |call: &str, nr: &str| q(call, Band::B14, Mode::Cw, nr);
        let c = rules(Cqww, ContestMode::Cw);
        assert_eq!(
            c.exchange_from(Band::B7, &past("W1AW", "4")),
            Some("5".into())
        );
        assert_eq!(
            c.exchange_from(Band::B7, &past("K1MM", "")),
            Some("4".into())
        );
        assert_eq!(
            c.exchange_from(Band::B7, &past("ZZ1ZZ", "33")),
            Some("33".into())
        );
        let c = rules(Jidx, ContestMode::Cw);
        assert_eq!(
            c.exchange_from(Band::B7, &past("DL1AA", "")),
            Some("14".into())
        );
        assert_eq!(
            c.exchange_from(Band::B7, &past("ZZ1ZZ", "")),
            Some("".into())
        );
        assert_eq!(
            rules(Wpx, ContestMode::Cw).exchange_from(Band::B7, &past("W1AW", "1")),
            None
        );
        let iota = rules(Iota, ContestMode::Cw);
        assert_eq!(iota.exchange_from(Band::B7, &past("W1AW", "001")), None);
        assert_eq!(
            iota.exchange_from(Band::B7, &past("W1AW", "001AS007")),
            Some("001AS007".into())
        );
        let c = rules(ArrlDxDx, ContestMode::Cw);
        assert_eq!(
            c.exchange_from(Band::B7, &past("W1AW", "CT")),
            Some("CT".into())
        );
    }

    #[test]
    fn extract_multi_of_the_arrl_contests() {
        let dx = rules(ArrlDxDx, ContestMode::Cw);
        assert_eq!(
            dx.extract_multi(&q("W1AW", Band::B14, Mode::Cw, "CT")),
            "CT"
        );
        assert_eq!(dx.extract_multi(&q("W1AW", Band::B14, Mode::Cw, "XX")), "");
        assert_eq!(dx.extract_multi(&q("W1AW", Band::B14, Mode::Cw, "")), "");
        let ten = rules(Arrl10, ContestMode::Mix);
        assert_eq!(
            ten.extract_multi(&q("W1AW", Band::B28, Mode::Cw, "CT")),
            "CT"
        );
        assert_eq!(
            ten.extract_multi(&q("DL1AA", Band::B28, Mode::Cw, "001")),
            "DL"
        );
        assert_eq!(
            ten.extract_multi(&q("ZZ1ZZ", Band::B28, Mode::Cw, "001")),
            ""
        );
        assert_eq!(ten.extract_multi(&q("W1AW", Band::B28, Mode::Cw, "XX")), "");
        let ww = rules(Cqww, ContestMode::Cw);
        assert_eq!(ww.extract_multi(&q("W1AW", Band::B14, Mode::Cw, "5")), "5");
    }

    #[test]
    fn check_multi_invalid_paths() {
        let x = |call: &str, nr: &str| q(call, Band::B14, Mode::Cw, nr);
        let invalid = |k, call: &str, nr: &str| {
            rules(k, ContestMode::Cw).check_multi(&x(call, nr)) == MultiCheck::Invalid
        };
        assert!(invalid(Cqww, "W1AW", "0"));
        assert!(invalid(Cqww, "W1AW", "41"));
        assert!(invalid(Cqww, "W1AW", ""));
        assert!(invalid(Iaru, "W1AW", ""));
        assert!(invalid(ArrlDxDx, "W1AW", "XX"));
        assert!(invalid(ArrlDxW, "ZZ1ZZ", "100"));
        assert!(invalid(Wae, "W1AW", "001"), "outside Europe");
        assert!(invalid(Wpx, "", "001"));
        assert!(invalid(Iota, "W1AW", "001"));
        assert!(invalid(Iota, "W1AW", "001ZZ999"));
        assert!(invalid(JidxDx, "JA1ZLO", "51"));
        assert!(invalid(JidxDx, "JA1ZLO", "0"));
        assert!(invalid(Pedi, "W1AW", "1"));
        let mut l = log("JA1ZLO", vec![x("W1AW", "5")]);
        let (c, _) = run(Cqww, ContestMode::Cw, &mut l);
        match c.check_multi(&x("K1AA", "5")) {
            MultiCheck::Known {
                needed, worked_on, ..
            } => {
                assert!(!needed);
                assert_eq!(worked_on, vec![Band::B14]);
            }
            MultiCheck::Invalid => panic!("zone 5 is known"),
        }
        assert!(matches!(
            c.check_multi(&q("K1AA", Band::B7, Mode::Cw, "5")),
            MultiCheck::Known { needed: true, .. }
        ));
    }

    #[test]
    fn multi_tables_list_every_multiplier() {
        let mut l = log("JA1ZLO", vec![q("W1AW", Band::B14, Mode::Cw, "5")]);
        let (c, _) = run(Cqww, ContestMode::Cw, &mut l);
        let t = c.multi_table();
        assert_eq!(t[4].0, "5");
        assert_eq!(t[4].2, vec![Band::B14]);
        assert!(t[0].2.is_empty());
        assert!(t.iter().any(|r| r.0 == "K" && r.2 == vec![Band::B14]));
        let iaru = rules(Iaru, ContestMode::Cw).multi_table();
        assert_eq!(iaru.len(), 94);
        assert_eq!(iaru[90].0, "AC");
        assert_eq!(rules(JidxDx, ContestMode::Cw).multi_table().len(), 50);
        assert!(rules(Pedi, ContestMode::All).multi_table().is_empty());
        assert!(rules(Wpx, ContestMode::Cw).multi_table().is_empty());
        let wae = rules(Wae, ContestMode::Cw).multi_table();
        assert!(!wae.is_empty() && wae.iter().all(|r| r.0 != "K"));
        let ten = rules(Arrl10, ContestMode::Mix).multi_table();
        assert!(ten.iter().all(|r| r.0 != "K" && r.0 != "VE" && r.0 != "XE"));
        assert!(ten.iter().any(|r| r.0 == "DL"));
    }
}
