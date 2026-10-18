//! International contests of zLog (`UzLogContest.pas` and the `U*Multi.pas`,
//! `U*Score.pas` units): CQ WW, CQ WPX, IARU HF, ARRL DX (both sides), WAEDC, IOTA,
//! ARRL 10 m, All Asian DX, JIDX (both sides), AP Sprint, BARTG HF RTTY and the
//! DXpedition mode.
//!
//! All share one implementation, [`WorldRules`]; [`WorldContest`] says what differs.
//! Countries come from the user's `CTY.DAT` ([`cty`]); fixed lists from zLog's
//! `ARDX.DAT`, `ARRL10.DAT` and `IOTA.DAT` ([`lists`]).
//!
//! The own station (country, continent, zones) is taken from the log header's
//! callsign, as zLog's `AnalyzeMyCountry` takes it from the settings; the ITU zone
//! and IOTA reference the operator sends are read back from the sent numbers.

pub mod cty;
pub mod lists;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use zlog_core::{
    Band, BandScore, Contest, ContestDef, ContestMode, Extension, Log, Mode, MultiCheck, Power,
    Qso, QsoScore, ScoreSheet, Scoring, Station,
};

use crate::cty::{call_area, wpx_prefix, Cty};
use crate::lists::{extract_iota, find_state, State, PREFECTURES};

zlog_core::texts!();

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
    Pedi,
}

use WorldContest::*;

const CW_PH_RTTY: &[ContestMode] = &[ContestMode::Cw, ContestMode::Phone, ContestMode::Rtty];
const MIX_CW_PH_ALL: &[ContestMode] = &[
    ContestMode::Mix,
    ContestMode::Cw,
    ContestMode::Phone,
    ContestMode::All,
];
const CW_PH_ALL: &[ContestMode] = &[ContestMode::Cw, ContestMode::Phone, ContestMode::All];
const CW_PH: &[ContestMode] = &[ContestMode::Cw, ContestMode::Phone];
const RTTY: &[ContestMode] = &[ContestMode::Rtty];
const EVERY: &[ContestMode] = &[
    ContestMode::Mix,
    ContestMode::Cw,
    ContestMode::Phone,
    ContestMode::Rtty,
    ContestMode::All,
];

impl WorldContest {
    pub const ALL: [WorldContest; 14] = [
        Cqww, Wpx, Iaru, ArrlDxDx, ArrlDxW, Wae, Iota, Arrl10, AllAsian, Jidx, JidxDx, ApSprint,
        Bartg, Pedi,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Cqww => "cqww",
            Wpx => "cqwpx",
            Iaru => "iaru",
            ArrlDxDx => "arrldx-dx",
            ArrlDxW => "arrldx-w",
            Wae => "waedc",
            Iota => "iota",
            Arrl10 => "arrl10",
            AllAsian => "allasian",
            Jidx => "jidx",
            JidxDx => "jidx-dx",
            ApSprint => "apsprint",
            Bartg => "bartg",
            Pedi => "pedi",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.id().eq_ignore_ascii_case(id))
    }

    fn caption_key(self) -> &'static str {
        match self {
            Cqww => "caption.cqww",
            Wpx => "caption.cqwpx",
            Iaru => "caption.iaru",
            ArrlDxDx => "caption.arrldx_dx",
            ArrlDxW => "caption.arrldx_w",
            Wae => "caption.waedc",
            Iota => "caption.iota",
            Arrl10 => "caption.arrl10",
            AllAsian => "caption.allasian",
            Jidx => "caption.jidx",
            JidxDx => "caption.jidx_dx",
            ApSprint => "caption.apsprint",
            Bartg => "caption.bartg",
            Pedi => "caption.pedi",
        }
    }

    /// Bands (`FBandLow`, `FBandHigh`). JIDX (DX) keeps zLog's default up to 50 MHz
    /// in zLog, where its tables stop at 28 MHz; here it ends at 28 MHz.
    fn bands(self) -> (Band, Band) {
        match self {
            Wae | Iota | Bartg => (Band::B3_5, Band::B28),
            Arrl10 => (Band::B28, Band::B28),
            // AP Sprint rules (jsfc.org, 2026): 20 and 15 m only; zLog has 40 m too.
            ApSprint => (Band::B14, Band::B21),
            Pedi => (Band::B1_9, Band::HIGHEST),
            _ => (Band::B1_9, Band::B28),
        }
    }

    /// Whether WAE-only entities are countries (`Load_CTYDAT(True)` in `InitContest`).
    fn waedc(self) -> bool {
        !matches!(self, ArrlDxDx | ArrlDxW | Arrl10 | AllAsian | Bartg)
    }

    /// `Log.AcceptDifferentMode`.
    fn different_mode(self) -> bool {
        matches!(self, Iaru | Iota | Arrl10 | Pedi)
    }

    /// Sends a serial number (`SerialType = stAll`).
    fn serial(self) -> bool {
        matches!(self, Wpx | Wae | Iota | Arrl10 | ApSprint | Bartg)
    }
}

/// The extension that adds the international contests.
pub struct World;

impl Extension for World {
    fn name(&self) -> &str {
        "World contests"
    }

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        WorldContest::ALL
            .into_iter()
            .map(|c| Box::new(c) as Box<dyn ContestDef>)
            .collect()
    }
}

impl ContestDef for WorldContest {
    fn id(&self) -> &str {
        WorldContest::id(*self)
    }

    fn caption(&self, language: &str) -> String {
        texts().get(language, self.caption_key()).into()
    }

    fn group(&self, language: &str) -> String {
        texts().get(language, "group.world").into()
    }

    /// Modes of the zLog Menu (`TMenuForm.EnableEveryThing`).
    fn modes(&self) -> &[ContestMode] {
        match self {
            Iaru | Iota | Arrl10 => MIX_CW_PH_ALL,
            ApSprint => CW_PH_ALL,
            AllAsian => CW_PH,
            Bartg => RTTY,
            Pedi => EVERY,
            _ => CW_PH_RTTY,
        }
    }

    fn uses_utc(&self) -> bool {
        *self != Pedi
    }

    fn build(&self, mode: ContestMode) -> Box<dyn Contest> {
        Box::new(WorldRules::new(*self, mode, Cty::global()))
    }
}

/// What the scorer knows about the own station (`MyCountry`, `MyContinent`,
/// `MyITUZone`, `_myiota`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct My {
    country: String,
    continent: String,
    itu: String,
    iota: String,
}

/// Where a multiplier was worked: a band and a mode slot (0, or CW/phone where a
/// contest counts them apart).
type Slot = (Band, u8);

/// A contest in progress.
pub struct WorldRules {
    kind: WorldContest,
    mode: ContestMode,
    cty: Arc<Cty>,
    states: Vec<State>,
    islands: Vec<(String, String)>,
    /// Worked multipliers of multi1, multi2 and multi3, by key.
    worked: [HashMap<String, BTreeSet<Slot>>; 3],
    next_serial: i32,
}

/// Leading digits of a sent number, the serial zLog keeps in `TQSO.Serial`.
fn serial_of(q: &Qso) -> i32 {
    if q.serial > 0 {
        return q.serial;
    }
    let digits: String = q
        .nr_sent
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

/// `StrToIntDef(s, default)` for decimal numbers.
fn int_of(s: &str) -> Option<i64> {
    let s = s.trim();
    let d = s.strip_prefix(['+', '-']).unwrap_or(s);
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Zone and state of a CQ WW exchange (`TWWMulti.SplitZoneState`): up to two leading digits.
fn split_zone_state(nr: &str) -> (String, String) {
    let s = nr.trim().to_ascii_uppercase();
    let n = s.bytes().take(2).take_while(u8::is_ascii_digit).count();
    (s[..n].to_string(), s[n..].trim().to_string())
}

fn is_wve(code: &str) -> bool {
    matches!(code, "K" | "W" | "N" | "KH6" | "KL7" | "KL" | "VE")
}

/// Points by band for JIDX (`TJIDXScore2.CalcPoints`, `TJIDX_DX_Score.CalcPoints`).
fn jidx_points(band: Band) -> u8 {
    match band {
        Band::B1_9 => 4,
        Band::B3_5 | Band::B28 => 2,
        b if b >= Band::B7 && b <= Band::B21 => 1,
        _ => 0,
    }
}

/// WAEDC multiplier bonus per band (`UWAEScore.BandFactor`).
fn wae_factor(band: Band) -> u32 {
    match band {
        Band::B3_5 => 4,
        Band::B7 => 3,
        Band::B14 | Band::B21 | Band::B28 => 2,
        _ => 0,
    }
}

/// Number of multipliers in a multiplier text (`TStringList.CommaText`).
fn count(text: &str) -> u32 {
    text.split(',').filter(|s| !s.trim().is_empty()).count() as u32
}

impl WorldRules {
    pub fn new(kind: WorldContest, mode: ContestMode, cty: Arc<Cty>) -> Self {
        let states = match kind {
            Arrl10 => lists::arrl10_states(),
            ArrlDxDx | Cqww => lists::ardx_states(),
            _ => Vec::new(),
        };
        let islands = if kind == Iota {
            lists::islands()
        } else {
            Vec::new()
        };
        WorldRules {
            kind,
            mode,
            cty,
            states,
            islands,
            worked: Default::default(),
            next_serial: 1,
        }
    }

    /// Mark a multiplier worked; whether it is new on its slot, or anywhere when
    /// `per_contest`.
    fn mark(&mut self, t: usize, key: &str, slot: Slot, per_contest: bool) -> bool {
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

    fn worked_bands(&self, t: usize, key: &str) -> Vec<Band> {
        let bands: BTreeSet<Band> = self.worked[t]
            .get(key)
            .map(|s| s.iter().map(|(b, _)| *b).collect())
            .unwrap_or_default();
        bands.into_iter().collect()
    }

    fn my(&self, log: &Log) -> My {
        let call = log.header.callsign.trim();
        let mut my = if !self.cty.is_loaded() || call.is_empty() {
            My {
                country: "JA".into(),
                continent: "AS".into(),
                itu: "45".into(),
                iota: String::new(),
            }
        } else {
            let f = self.cty.lookup(call, self.kind.waedc());
            My {
                country: f.country.code.clone(),
                continent: f.continent().to_string(),
                itu: self.cty.guess_itu_zone(call, self.kind.waedc()),
                iota: String::new(),
            }
        };
        if let Some(last) = log.qsos.iter().rev().find(|q| !q.nr_sent.is_empty()) {
            match self.kind {
                Iaru => {
                    if let Some(z) = int_of(&last.nr_sent).filter(|z| (1..=90).contains(z)) {
                        my.itu = z.to_string();
                    }
                }
                Iota => my.iota = extract_iota(&last.nr_sent),
                _ => {}
            }
        }
        my
    }

    /// Multipliers and points of one in-band, valid QSO (`AddNoUpdate` of the
    /// multiplier form, then of the score form).
    fn add(&mut self, q: &Qso, dupe: bool, mode_ok: bool, my: &My, s: &mut QsoScore) {
        let cty = self.cty.clone();
        let f = cty.lookup(&q.callsign, self.kind.waedc());
        let code = f.country.code.as_str();
        let cont = f.continent();
        let band = q.band;
        let skip = dupe || !mode_ok;
        match self.kind {
            // UWWMulti.pas TWWMulti.AddNoUpdate
            Cqww => {
                let (zone, state) = split_zone_state(&q.nr_rcvd);
                s.multi1 = zone.clone();
                if skip {
                    return;
                }
                if self.mode == ContestMode::Rtty && q.mode == Mode::Rtty && !state.is_empty() {
                    if let Some(st) = find_state(&self.states, &state) {
                        s.multi3 = st.abbrev.clone();
                        s.new_multi3 = self.mark(2, &st.abbrev.clone(), (band, 0), false);
                    }
                }
                if let Some(z) = int_of(&zone).filter(|z| (1..=40).contains(z)) {
                    s.new_multi1 = self.mark(0, &z.to_string(), (band, 0), false);
                }
                s.multi2 = code.to_string();
                s.new_multi2 = self.mark(1, code, (band, 0), false);
                s.points = if q.mode == Mode::Rtty {
                    if my.continent != cont {
                        3
                    } else if my.country == code {
                        1
                    } else {
                        2
                    }
                } else if my.country == code {
                    0
                } else if my.continent == cont {
                    if my.continent == "NA" {
                        2
                    } else {
                        1
                    }
                } else {
                    3
                };
            }
            // UWPXMulti.pas TWPXMulti.AddNoUpdate; UAPSprintScore.pas gives 1 point.
            Wpx | ApSprint => {
                let px = wpx_prefix(&q.callsign);
                s.multi1 = px.clone();
                if skip {
                    return;
                }
                s.new_multi1 = self.mark(0, &px, (band, 0), true);
                if self.kind == ApSprint {
                    s.points = 1;
                    return;
                }
                let low = band <= Band::B7;
                s.points = if q.mode == Mode::Rtty {
                    let base = if my.continent != cont {
                        3
                    } else if my.country == code {
                        1
                    } else {
                        2
                    };
                    match band {
                        Band::B14 | Band::B21 | Band::B28 => base,
                        Band::B3_5 | Band::B7 => base * 2,
                        _ => 0,
                    }
                } else if my.country == code {
                    1
                } else if my.continent == cont {
                    match (my.continent == "NA", low) {
                        (true, true) => 4,
                        (true, false) | (false, true) => 2,
                        (false, false) => 1,
                    }
                } else if low {
                    6
                } else {
                    3
                };
            }
            // UIARUMulti.pas TIARUMulti.AddNoUpdate
            Iaru => {
                s.multi1 = q.nr_rcvd.clone();
                if skip {
                    return;
                }
                let (key, hq) = match int_of(&q.nr_rcvd).filter(|z| (1..=90).contains(z)) {
                    Some(z) => (z.to_string(), false),
                    None => (q.nr_rcvd.clone(), true),
                };
                s.new_multi1 = self.mark(0, &key, (band, 0), false);
                s.points = if my.itu == key || hq {
                    1
                } else if my.continent == cont {
                    3
                } else {
                    5
                };
            }
            // UARRLDXMulti.pas TARRLDXMulti.AddNoUpdate; UARRLDXScore.pas: 3 points.
            ArrlDxDx => {
                let st = find_state(&self.states, &q.nr_rcvd).map(|s| s.abbrev.clone());
                s.multi1 = st.clone().unwrap_or_default();
                if skip {
                    return;
                }
                if let Some(st) = st {
                    s.new_multi1 = self.mark(0, &st, (band, 0), false);
                }
                s.points = 3;
            }
            // UARRLWMulti.pas TARRLWMulti.AddNoUpdate (ALLASIANFLAG off, then on).
            ArrlDxW | AllAsian => {
                if !f.country.is_waedc() {
                    s.multi1 = code.to_string();
                }
                if skip {
                    return;
                }
                if self.kind == ArrlDxW {
                    s.points = 3;
                    // ARRL DX rules 2026 (v2.0) 5.2.2: W/VE count DXCC entities except
                    // USA and Canada; zLog counts them too.
                    if matches!(code, "K" | "VE") {
                        return;
                    }
                } else if my.country == code {
                    s.points = 0;
                } else {
                    let asia = f.country.continent == "AS";
                    s.points = match (band, asia) {
                        (Band::B1_9, true) => 3,
                        (Band::B3_5 | Band::B28, true) => 2,
                        (_, true) => 1,
                        (Band::B1_9, false) => 9,
                        (Band::B3_5 | Band::B28, false) => 6,
                        (_, false) => 3,
                    };
                }
                if !f.country.is_waedc() {
                    s.new_multi1 = self.mark(0, code, (band, 0), false);
                }
            }
            // UWAEMulti.pas TWAEMulti.AddNoUpdate; UWAEScore.pas: 1 point.
            // WAE rules 2025 section 12: in RTTY everybody works everybody; QSOs
            // outside Europe score a point but give no multiplier to a non-European.
            Wae => {
                if f.country.continent != "EU" {
                    s.multi1 = "Non-EU".into();
                    if q.mode == Mode::Rtty && !skip {
                        s.points = 1;
                    }
                    return;
                }
                s.multi1 = code.to_string();
                if skip {
                    return;
                }
                s.new_multi1 = self.mark(0, code, (band, 0), false);
                s.points = 1;
            }
            // UIOTAMulti.pas TIOTAMulti.AddNoUpdate: islands by band and mode.
            Iota => {
                let r = extract_iota(&q.nr_rcvd);
                // IOTA rules 2023/2024: island 15, own reference 5, island station
                // working a world station 5, world working world 2 (zLog: 3 and 15).
                s.points = match (my.iota.is_empty(), r.is_empty()) {
                    (_, false) if r == my.iota => 5,
                    (_, false) => 15,
                    (false, true) => 5,
                    (true, true) => 2,
                };
                s.multi1 = r.clone();
                if skip || r.is_empty() {
                    return;
                }
                s.new_multi1 = self.mark(0, &r, (band, q.mode as u8), false);
            }
            // UARRL10Multi.pas TARRL10Multi.AddNoUpdate; UARRL10Score.pas points.
            Arrl10 => {
                let slot = (band, if q.mode == Mode::Cw { 1 } else { 2 });
                if skip {
                    return;
                }
                if is_wve(code) || q.callsign.contains("/MM") || code == "XE" {
                    if let Some(st) = find_state(&self.states, &q.nr_rcvd) {
                        s.multi1 = st.abbrev.clone();
                        s.new_multi1 = self.mark(0, &st.abbrev.clone(), slot, false);
                    }
                } else if !f.country.is_waedc() {
                    s.multi1 = code.to_string();
                    s.new_multi1 = self.mark(1, code, slot, false);
                }
                s.points = match q.mode {
                    Mode::Cw => 4,
                    Mode::Ssb | Mode::Fm | Mode::Am => 2,
                    _ => 0,
                };
            }
            // UJIDXMulti.pas TJIDXMulti.AddNoUpdate; UJIDXScore2.pas points.
            Jidx => {
                s.multi1 = q.nr_rcvd.clone();
                // JIDX rules 2026: only JA-DX QSOs count; JD1 is no DXCC multiplier.
                if skip || code == "JA" {
                    return;
                }
                if let Some(z) = int_of(&q.nr_rcvd).filter(|z| (1..=40).contains(z)) {
                    s.new_multi1 = self.mark(0, &z.to_string(), (band, 0), false);
                }
                s.points = jidx_points(band);
                if code.is_empty() || f.country.is_waedc() || code.starts_with("JD") {
                    return;
                }
                s.multi2 = code.to_string();
                s.new_multi2 = self.mark(1, code, (band, 0), false);
            }
            // UJIDX_DX_Multi.pas TJIDX_DX_Multi.AddNoUpdate; UJIDX_DX_Score.pas points.
            JidxDx => {
                s.multi1 = q.nr_rcvd.clone();
                let japan = code == "JA" || code.starts_with("JD");
                if skip || (!f.is_unknown() && !japan) {
                    return;
                }
                if let Some(p) = int_of(&q.nr_rcvd).filter(|p| (1..=50).contains(p)) {
                    s.new_multi1 = self.mark(0, &p.to_string(), (band, 0), false);
                }
                s.points = jidx_points(band);
            }
            // UJarlWorldWideRTTYMulti.pas AddNoUpdate with the BARTG rule.
            Bartg => {
                if skip || q.mode != Mode::Rtty {
                    return;
                }
                let area = call_area(&q.callsign, code);
                let mut new = Vec::new();
                if !f.country.is_waedc() && self.mark(0, code, (band, 0), false) {
                    new.push(code.to_string());
                }
                // zLog's call-area list: JA0-9, K0-9, VE0-9, VK1-8.
                let areas =
                    !(area.starts_with("VK") && (area.ends_with('0') || area.ends_with('9')));
                if !area.is_empty() && areas && self.mark(0, &area, (band, 0), false) {
                    new.push(area);
                }
                s.multi1 = new.join(",");
                s.new_multi1 = !new.is_empty();
                if !cont.is_empty() && self.mark(1, cont, (band, 0), true) {
                    s.multi2 = cont.to_string();
                    s.new_multi2 = true;
                }
                s.points = 1;
            }
            Pedi => s.points = 1,
        }
    }

    fn score_band(&self, band: Band) -> bool {
        let (lo, hi) = self.kind.bands();
        band >= lo && band <= hi && (self.uses_warc() || !band.is_warc()) && band != Band::B10_4G
    }

    /// Dupe flags with this contest's rule (`SetDupeFlags`; `AcceptDifferentMode`).
    fn dupes(&self, log: &Log) -> Vec<bool> {
        let mut seen = std::collections::HashSet::new();
        log.qsos
            .iter()
            .map(|q| {
                let mut key = format!("{}\t{}", q.core_call(), q.band.label());
                if self.kind.different_mode() {
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

    fn country_rows(&self, t: usize, filter: impl Fn(&cty::Country) -> bool) -> Vec<Row> {
        self.cty
            .countries()
            .iter()
            .skip(1)
            .filter(|c| self.kind.waedc() || !c.is_waedc())
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

    /// Worked keys of a table that no fixed list names, in order.
    fn extra_rows(&self, t: usize, known: &[Row]) -> Vec<Row> {
        let mut keys: Vec<&String> = self.worked[t]
            .keys()
            .filter(|k| !known.iter().any(|(c, _, _)| c == *k))
            .collect();
        keys.sort();
        keys.into_iter()
            .map(|k| (k.clone(), String::new(), self.worked_bands(t, k)))
            .collect()
    }

    fn zone_rows(&self, n: u32, label: &str) -> Vec<Row> {
        (1..=n)
            .map(|z| {
                let k = z.to_string();
                let bands = self.worked_bands(0, &k);
                (k, format!("{label} {z}"), bands)
            })
            .collect()
    }

    fn state_rows(&self, t: usize) -> Vec<Row> {
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

    /// The primary multiplier a QSO being typed would give: (table, key, name, slot).
    fn primary(&self, q: &Qso) -> Option<(usize, String, String, Slot)> {
        let f = self.cty.lookup(&q.callsign, self.kind.waedc());
        let country = || {
            (!f.is_unknown()).then(|| {
                (
                    0,
                    f.country.code.clone(),
                    f.country.name.clone(),
                    (q.band, 0),
                )
            })
        };
        match self.kind {
            Cqww | Jidx => {
                let (zone, _) = split_zone_state(&q.nr_rcvd);
                let z = int_of(&zone).filter(|z| (1..=40).contains(z))?;
                Some((0, z.to_string(), format!("Zone {z}"), (q.band, 0)))
            }
            Iaru => {
                let key = match int_of(&q.nr_rcvd).filter(|z| (1..=90).contains(z)) {
                    Some(z) => z.to_string(),
                    None if q.nr_rcvd.is_empty() => return None,
                    None => q.nr_rcvd.clone(),
                };
                Some((0, key.clone(), key, (q.band, 0)))
            }
            ArrlDxDx => {
                let st = find_state(&self.states, &q.nr_rcvd)?;
                Some((0, st.abbrev.clone(), st.name.clone(), (q.band, 0)))
            }
            Arrl10 => {
                let slot = (q.band, if q.mode == Mode::Cw { 1 } else { 2 });
                if is_wve(&f.country.code) || q.callsign.contains("/MM") || f.country.code == "XE" {
                    let st = find_state(&self.states, &q.nr_rcvd)?;
                    Some((0, st.abbrev.clone(), st.name.clone(), slot))
                } else {
                    country().map(|(_, k, n, _)| (1, k, n, slot))
                }
            }
            ArrlDxW | AllAsian | Bartg => country(),
            Wae => country().filter(|_| f.country.continent == "EU"),
            Wpx | ApSprint => {
                let px = wpx_prefix(&q.callsign);
                (!q.callsign.is_empty()).then(|| (0, px.clone(), px, (Band::Unknown, 0)))
            }
            Iota => {
                let r = extract_iota(&q.nr_rcvd);
                if r.is_empty() {
                    return None;
                }
                let name = self
                    .islands
                    .iter()
                    .find(|(k, _)| *k == r)
                    .map(|(_, n)| n.clone())?;
                Some((0, r, name, (q.band, q.mode as u8)))
            }
            JidxDx => {
                let p = int_of(&q.nr_rcvd).filter(|p| (1..=50).contains(p))?;
                Some((
                    0,
                    p.to_string(),
                    PREFECTURES[p as usize - 1].to_string(),
                    (q.band, 0),
                ))
            }
            Pedi => None,
        }
    }

    /// Next serial number (`TLog.GetCurrentSerial`, `stAll`), from the last QSO.
    pub fn next_serial(&self) -> i32 {
        self.next_serial
    }
}

type Row = (String, String, Vec<Band>);

impl Contest for WorldRules {
    fn name(&self) -> &str {
        match self.kind {
            Cqww => "CQWW DX Contest",
            Wpx => "CQ WPX Contest",
            Iaru => "IARU HF Championship",
            ArrlDxDx => "ARRL International DX Contest (DX)",
            ArrlDxW => "ARRL International DX Contest (W/VE)",
            Wae => "WAEDC Contest",
            Iota => "IOTA Contest",
            Arrl10 => "ARRL 10m Contest",
            AllAsian => "All Asian DX Contest (Asia)",
            Jidx => "JIDX Contest (JA)",
            JidxDx => "JIDX Contest (DX)",
            ApSprint => "Asia Pacific Sprint",
            Bartg => "BARTG HF RTTY",
            Pedi => "Pedition mode",
        }
    }

    fn band_range(&self) -> (Band, Band) {
        self.kind.bands()
    }

    fn uses_warc(&self) -> bool {
        self.kind == Pedi
    }

    fn contest_mode(&self) -> ContestMode {
        self.mode
    }

    /// The sent template (`FSentStr`): `$S` serial, `$Z` CQ zone, `$I` ITU zone,
    /// `$V` prefecture or state, `$N` power. Age (`$A`) and IOTA (`$T`) are not in
    /// [`Station`] and stay empty.
    fn sent_number(&self, _band: Band, power: Power, station: &Station) -> String {
        let serial = || format!("{:03}", self.next_serial);
        let w = self.kind.waedc();
        match self.kind {
            Cqww => self.cty.guess_cq_zone(&station.callsign, w),
            Iaru => self.cty.guess_itu_zone(&station.callsign, w),
            ArrlDxDx => match power {
                Power::P1000 => "1KW".into(),
                p => p.label().trim_end_matches('W').to_string(),
            },
            ArrlDxW | Jidx | JidxDx => station.prov.clone(),
            Arrl10 => {
                let code = &self.cty.lookup(&station.callsign, w).country.code;
                if matches!(code.as_str(), "K" | "N" | "W") {
                    station.prov.clone()
                } else {
                    serial()
                }
            }
            AllAsian | Pedi => String::new(),
            Wpx | Wae | Iota | ApSprint | Bartg => serial(),
        }
    }

    fn exchange_from(&self, _band: Band, past: &Qso) -> Option<String> {
        match self.kind {
            k if k.serial() && k != Iota => None,
            Cqww | Jidx => {
                let z = self.cty.guess_cq_zone(&past.callsign, self.kind.waedc());
                Some(if z.is_empty() {
                    past.nr_rcvd.clone()
                } else {
                    z
                })
            }
            Iota => (!extract_iota(&past.nr_rcvd).is_empty()).then(|| past.nr_rcvd.clone()),
            _ => Some(past.nr_rcvd.clone()),
        }
    }

    /// `ValidMulti` of each multiplier form.
    fn valid_number(&self, q: &Qso) -> bool {
        let nr = q.nr_rcvd.trim();
        match self.kind {
            Cqww | Jidx => {
                let (zone, state) = split_zone_state(nr);
                if !int_of(&zone).is_some_and(|z| (1..=40).contains(&z)) {
                    return false;
                }
                if self.kind == Jidx || self.mode != ContestMode::Rtty {
                    return true;
                }
                let code = &self.cty.lookup(&q.callsign, true).country.code;
                let known = find_state(&self.states, &state).is_some();
                if matches!(code.as_str(), "K" | "N" | "W" | "VE") {
                    known
                } else {
                    state.is_empty() || known
                }
            }
            Wpx | ApSprint => int_of(nr).is_some_and(|n| n >= 0),
            ArrlDxDx => find_state(&self.states, nr).is_some(),
            Arrl10 => {
                if nr.is_empty() {
                    return false;
                }
                if q.callsign.contains("/MM") {
                    return matches!(nr, "1" | "2" | "3");
                }
                let code = &self.cty.lookup(&q.callsign, false).country.code;
                if is_wve(code) || code == "XE" {
                    find_state(&self.states, nr).is_some()
                } else {
                    int_of(nr).is_some_and(|n| n > 0)
                }
            }
            JidxDx => !q.band.is_warc() && int_of(nr).is_some_and(|n| (1..=50).contains(&n)),
            Iota | Pedi => true,
            Iaru | ArrlDxW | AllAsian | Wae | Bartg => !nr.is_empty(),
        }
    }

    fn score(&mut self, log: &Log) -> Scoring {
        for t in &mut self.worked {
            t.clear();
        }
        self.next_serial = log.qsos.last().map(serial_of).unwrap_or(0) + 1;
        let my = self.my(log);
        let dupes = self.dupes(log);
        let (lo, hi) = self.kind.bands();
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
            self.add(q, dupe, mode_ok, &my, &mut s);
            let pedi = self.kind == Pedi;
            if !pedi && (dupe || !mode_ok) {
                s.points = 0;
            }
            let row = rows.entry(q.band).or_default();
            if mode_ok || matches!(self.kind, AllAsian | Pedi) {
                row.qsos += 1;
                if q.mode == Mode::Cw {
                    row.cw_qsos += 1;
                }
            }
            let weight = if self.kind == Wae {
                wae_factor(q.band)
            } else {
                1
            };
            for (i, (new, text)) in [
                (s.new_multi1, &s.multi1),
                (s.new_multi2, &s.multi2),
                (s.new_multi3, &s.multi3),
            ]
            .into_iter()
            .enumerate()
            {
                if new && self.kind == Bartg && i == 1 {
                    continents += 1;
                } else if new {
                    row.multi += weight * count(text);
                }
            }
            row.points += s.points as u32;
            if self.kind == Wae && s.points > 0 && q.memo.contains("[QTC") {
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
        let score = match self.kind {
            Pedi => total.points as u64,
            Wae => (total.points + qtcs) as u64 * total.multi as u64,
            // BARTG HF RTTY rules 2025 v3: points x (countries + call areas) x
            // continents; zLog adds the continents to the multipliers.
            Bartg => total.points as u64 * total.multi as u64 * continents,
            _ => total.points as u64 * total.multi as u64,
        };
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

    fn check_multi(&self, q: &Qso) -> MultiCheck {
        let Some((t, code, name, slot)) = self.primary(q) else {
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
        match self.kind {
            Cqww | Jidx => {
                let mut v = self.zone_rows(40, "Zone");
                v.extend(self.country_rows(1, |_| true));
                if self.kind == Cqww {
                    v.extend(self.state_rows(2).into_iter().filter(|r| !r.2.is_empty()));
                }
                v
            }
            Iaru => {
                let mut v = self.zone_rows(90, "ITU Zone");
                for (k, n) in [
                    ("AC", "Admin. Council"),
                    ("R1", "Reg. Exec. Committee"),
                    ("R2", "Reg. Exec. Committee"),
                    ("R3", "Reg. Exec. Committee"),
                ] {
                    v.push((k.into(), n.into(), self.worked_bands(0, k)));
                }
                let extra = self.extra_rows(0, &v);
                v.extend(extra);
                v
            }
            ArrlDxDx => self.state_rows(0),
            Arrl10 => {
                let mut v = self.state_rows(0);
                v.extend(self.country_rows(1, |c| !is_wve(&c.code) && c.code != "XE"));
                v
            }
            ArrlDxW | AllAsian | Bartg => {
                let mut v = self.country_rows(0, |_| true);
                let extra = self.extra_rows(0, &v);
                v.extend(extra);
                v
            }
            Wae => self.country_rows(0, |c| c.continent == "EU"),
            Wpx | ApSprint => self.extra_rows(0, &[]),
            Iota => {
                let v: Vec<Row> = self
                    .islands
                    .iter()
                    .map(|(k, n)| (k.clone(), n.clone(), self.worked_bands(0, k)))
                    .collect();
                let mut v2 = v.clone();
                v2.extend(self.extra_rows(0, &v));
                v2
            }
            JidxDx => (1..=50)
                .map(|p| {
                    let k = p.to_string();
                    let b = self.worked_bands(0, &k);
                    (k, PREFECTURES[p - 1].to_string(), b)
                })
                .collect(),
            Pedi => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
