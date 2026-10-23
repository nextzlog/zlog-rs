//! User-defined contests (`TGeneralContest`, UzLogContest.pas:1460) from a CFG file
//! and its DAT multiplier list, with zLog's multipliers (`TGeneralMulti2`,
//! UGeneralMulti2.pas) and points (`TGeneralScore`, UGeneralScore.pas).
//!
//! The operator chooses the CFG file; its DAT file is read through the lookup the
//! caller passes, so nothing is loaded without the operator's choice.

use std::collections::BTreeSet;
use std::sync::Arc;

use zlog_core::{
    Band, Contest, ContestDef, ContestMode, Log, MultiCheck, Power, Qso, QsoScore, ScoreSheet,
    Scoring, Station,
};

use crate::cfg::{comma_text, Config, PrefixMulti, SerialType, SpecialCallMatch};
use crate::common::{
    count_qso, dupe_flags, empty_rows, expand_sent, in_contest, is_domestic, multi_count, sum,
    SentContext, Serials,
};
use crate::texts;

/// One entry of a DAT list (`TCity`): the code (several codes separated by commas)
/// and the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatEntry {
    pub code: String,
    pub name: String,
}

/// Read a DAT list (`TCityList.LoadFromFile`, UMultipliers.pas:1006): the first line is
/// a title, then `code name` lines up to a line containing `end of file`.
pub fn parse_dat(text: &str) -> Vec<DatEntry> {
    text.lines()
        .skip(1)
        .take_while(|l| !l.to_lowercase().contains("end of file"))
        .map(|l| match l.find(' ') {
            Some(i) if i > 0 => DatEntry {
                code: l[..i].to_string(),
                name: l[i + 1..].trim().to_string(),
            },
            Some(i) => DatEntry {
                code: String::new(),
                name: l[i + 1..].trim().to_string(),
            },
            None => DatEntry {
                code: String::new(),
                name: l.trim().to_string(),
            },
        })
        .collect()
}

/// What a contest's own rules decide about one QSO: its points and multipliers, or
/// `None` for a QSO they reject (no points, empty multipliers).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    pub points: i32,
    pub multi1: String,
    pub multi2: String,
}

/// Rules of a contest on top of its CFG file, where zLog would ask a rule DLL
/// (`zyloRequestScore`, `zyloRequestMulti`, `zyloRequestValid`, `zyloRequestTotal`,
/// UzLogExtension.pas:950-1031).
pub trait RuleHook: Send + Sync {
    /// Points and multipliers of a QSO; `dupe` is its dupe flag.
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict>;

    /// Score from total points and multipliers; `scored` has the
    /// QSOs of the log with their results so far.
    fn total(&self, points: u32, multis: u32, log: &Log, scored: &[QsoScore]) -> u64 {
        let _ = (log, scored);
        points as u64 * multis as u64
    }
}

/// A user-defined contest read from a CFG file, ready for the zLog Menu.
#[derive(Clone)]
pub struct UserDefinedDef {
    id: String,
    hook: Option<Arc<dyn RuleHook>>,
    config: Arc<Config>,
    entries: Arc<Vec<DatEntry>>,
    dat_loaded: bool,
}

/// Build a user-defined contest from the text of a CFG file. `dat_lookup` returns the
/// text of the DAT file the CFG names (`DAT`), looked up next to the CFG file.
pub fn from_cfg(cfg: &str, dat_lookup: impl Fn(&str) -> Option<String>) -> UserDefinedDef {
    UserDefinedDef::from_cfg(cfg, dat_lookup)
}

impl UserDefinedDef {
    pub fn from_cfg(cfg: &str, dat_lookup: impl Fn(&str) -> Option<String>) -> Self {
        let config = Config::parse(cfg);
        let (entries, dat_loaded) = if config.dat.is_empty() {
            (Vec::new(), true)
        } else {
            let entries = dat_lookup(&config.dat)
                .map(|t| parse_dat(&t))
                .unwrap_or_default();
            let loaded = !entries.is_empty();
            (entries, loaded)
        };
        let id = format!("user:{}", config.contest_name);
        UserDefinedDef {
            id,
            hook: None,
            config: Arc::new(config),
            entries: Arc::new(entries),
            dat_loaded,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Replace the multiplier list, as a rule plugin's own table does (`zyloRequestTable`).
    pub fn with_entries(mut self, entries: Vec<DatEntry>) -> Self {
        self.dat_loaded = !entries.is_empty();
        self.entries = Arc::new(entries);
        self
    }

    /// Use a rule plugin, under a new identifier.
    pub fn with_hook(mut self, id: &str, hook: Arc<dyn RuleHook>) -> Self {
        self.id = id.to_string();
        self.hook = Some(hook);
        self
    }

    /// Whether the DAT list was found and has entries (`UserDatLoaded`); zLog warns
    /// on the status line when it was not, and runs the contest anyway.
    pub fn dat_loaded(&self) -> bool {
        self.dat_loaded
    }

    pub fn build_rules(&self, mode: ContestMode) -> UserContest {
        UserContest {
            name: self.config.contest_name.clone(),
            config: self.config.clone(),
            hook: self.hook.clone(),
            mode,
            base: self.entries.clone(),
            entries: self.entries.to_vec(),
            worked: vec![BTreeSet::new(); self.entries.len()],
            serials: Serials::new(self.config.serial_start),
        }
    }
}

impl ContestDef for UserDefinedDef {
    fn id(&self) -> &str {
        &self.id
    }

    fn caption(&self, language: &str) -> String {
        if self.config.contest_name.is_empty() {
            texts().get(language, "caption.user").into()
        } else {
            self.config.contest_name.clone()
        }
    }

    fn group(&self, language: &str) -> String {
        texts().get(language, "group.local").into()
    }

    fn modes(&self) -> &[ContestMode] {
        &[
            ContestMode::Mix,
            ContestMode::Cw,
            ContestMode::Phone,
            ContestMode::Rtty,
            ContestMode::All,
        ]
    }

    fn uses_coeff(&self) -> bool {
        self.config.coeff
    }

    fn uses_utc(&self) -> bool {
        self.config.use_utc
    }

    fn build(&self, mode: ContestMode) -> Box<dyn Contest> {
        Box::new(self.build_rules(mode))
    }
}

/// A user-defined contest in progress.
pub struct UserContest {
    name: String,
    hook: Option<Arc<dyn RuleHook>>,
    config: Arc<Config>,
    mode: ContestMode,
    base: Arc<Vec<DatEntry>>,
    /// The DAT list plus multipliers added by `UNDEFMULTI` or `PXMULTI`, in order.
    entries: Vec<DatEntry>,
    worked: Vec<BTreeSet<Band>>,
    /// Serial numbers after the scored log.
    serials: Serials,
}

/// Delphi `Copy(s, from, count)` on characters, 1-based, clamped.
/// Delphi's `StrToInt` (`Val`): leading blanks, an optional sign, then decimal
/// digits or hexadecimal ones after `$`, `0x` or `x`; nothing may follow. `None`
/// when it is not a number or does not fit in 32 bits.
pub fn delphi_str_to_int(s: &str) -> Option<i32> {
    let s = s.trim_start_matches(' ');
    let (neg, s) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let (radix, digits) = if let Some(h) = s.strip_prefix('$') {
        (16, h)
    } else if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (16, h)
    } else if let Some(h) = s.strip_prefix('x').or_else(|| s.strip_prefix('X')) {
        (16, h)
    } else {
        (10, s)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let v = i64::from_str_radix(digits, radix).ok()?;
    let v = if neg { -v } else { v };
    i32::try_from(v).ok()
}

fn copy(s: &[char], from: i64, count: i64) -> Vec<char> {
    let from = from.max(1) as usize - 1;
    if count <= 0 || from >= s.len() {
        return Vec::new();
    }
    s[from..(from + count as usize).min(s.len())].to_vec()
}

/// Delphi `Delete(s, index, count)`, 1-based; an index outside the text deletes nothing.
fn delete(s: &mut Vec<char>, index: i64, count: i64) {
    if index < 1 || index as usize > s.len() || count <= 0 {
        return;
    }
    let from = index as usize - 1;
    let to = (from + count as usize).min(s.len());
    s.drain(from..to);
}

/// `StrMore`: true if a > b, by characters, a longer text being greater.
fn str_more(a: &str, b: &str) -> bool {
    for (x, y) in a.chars().zip(b.chars()) {
        if x != y {
            return x > y;
        }
    }
    a.chars().count() > b.chars().count()
}

/// WPX prefix of a callsign (`GetWPXPrefix`, UWPXMulti.pas:69).
pub fn wpx_prefix(call: &str) -> String {
    let mut s: Vec<char> = call.chars().collect();
    let digit = |c: char| c.is_ascii_digit();
    if let Some(i) = s.iter().position(|c| *c == '/') {
        let temp: Vec<char> = s[i + 1..].to_vec();
        let t: String = temp.iter().collect();
        if [
            "AA", "AT", "AG", "AE", "M", "P", "AM", "QRP", "A", "KT", "MM",
        ]
        .contains(&t.as_str())
        {
            s.truncate(i);
        } else if temp.len() == 1 && digit(temp[0]) {
            // JA1ZLO/2: the area digit replaces the first digit.
            let j = (1..s.len()).find(|j| digit(s[*j])).unwrap_or(s.len());
            let mut out: Vec<char> = s[..j.min(s.len())].to_vec();
            out.push(temp[0]);
            return out.into_iter().collect();
        } else {
            let mut part = if i + 1 > 4 { temp } else { s[..i].to_vec() };
            match part.iter().rposition(|c| digit(*c)) {
                None => part.push('0'),
                Some(k) => {
                    if part.last().is_some_and(|c| c.is_ascii_uppercase()) {
                        part.truncate(k + 1);
                    }
                }
            }
            return part.into_iter().collect();
        }
    }
    if !s.iter().skip(1).any(|c| digit(*c)) {
        return s.iter().take(2).chain(['0'].iter()).collect();
    }
    let j = s.iter().rposition(|c| digit(*c)).unwrap_or(0);
    s[..=j].iter().collect()
}

impl UserContest {
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Prefix of a callsign (`TGeneralMulti2.GetPX`, UGeneralMulti2.pas:49).
    fn prefix(&self, call: &str) -> String {
        if call.is_empty() {
            return String::new();
        }
        if self.config.prefix_multi == PrefixMulti::Wpx {
            return wpx_prefix(call);
        }
        let mut s: Vec<char> = call.chars().collect();
        if let Some(p) = s.iter().position(|c| *c == '/') {
            let slash = p + 1;
            if slash > 4 {
                s.truncate(p);
            } else {
                s.drain(..slash);
            }
        }
        match s.iter().rposition(|c| c.is_ascii_digit()) {
            Some(i) => s[..=i].iter().collect(),
            None => String::new(),
        }
    }

    /// Whether the received number is local (`TUserDefinedContest.IsLocal`,
    /// UserDefinedContest.pas:1268). The country part (`LOCALCTY`, `LOCALCONT`) needs
    /// CTY.DAT and is not checked.
    pub fn is_local(&self, nr: &str) -> bool {
        let c = &self.config;
        let s: String = if c.min_local_len > 0 {
            nr.chars().take(c.min_local_len as usize).collect()
        } else {
            nr.to_string()
        };
        match c.local.as_slice() {
            [] => false,
            [(key, _)] => s.starts_with(key.as_str()),
            many => many.iter().any(|(k, _)| *k == s),
        }
    }

    /// The multiplier of a QSO (`ExtractMulti`, UGeneralMulti2.pas:217).
    pub fn extract_multi(&self, q: &Qso) -> String {
        self.extract_with(q, q.dupe)
    }

    fn extract_with(&self, q: &Qso, dupe: bool) -> String {
        if let Some(h) = &self.hook {
            return h.verify(q, dupe).map(|v| v.multi1).unwrap_or_default();
        }
        let c = &self.config;
        if c.prefix_multi != PrefixMulti::Off {
            return self.prefix(&q.callsign);
        }
        let mut s: Vec<char> = q.nr_rcvd.chars().collect();
        if c.cut_tailing_alphabets {
            // Deletes the letters at the end. As in zLog, a number whose only digit is
            // its first character becomes empty (`1H`); one without digits too.
            s = match s.iter().rposition(|ch| ch.is_ascii_digit()) {
                Some(0) | None => Vec::new(),
                Some(i) => s[..=i].to_vec(),
            };
        }
        let (cut, tail) = if self.is_local(&q.nr_rcvd) {
            (c.lcut, c.ltail)
        } else {
            (c.cut, c.tail)
        };
        let len = s.len() as i64;
        if cut > 0 {
            delete(&mut s, len - cut as i64 + 1, cut as i64);
        } else if cut < 0 {
            delete(&mut s, 1, -(cut as i64));
        } else if tail > 0 {
            s = copy(&s, len - tail as i64 + 1, tail as i64);
        } else if tail < 0 {
            s = copy(&s, 1, -(tail as i64));
        }
        s.into_iter().collect()
    }

    /// `IsMultiNeed`: whether a number must be on the list.
    fn multi_needed(&self) -> bool {
        let c = &self.config;
        !(c.undef_multi
            || c.allow_unlisted_multi
            || c.prefix_multi != PrefixMulti::Off
            || c.use_cty
            || c.no_multi)
    }

    /// `MultiCompare`: a code matches one of the comma-separated codes of an entry,
    /// or, with `NRNUMCOMPARE`, the same number.
    fn matches(&self, s: &str, code: &str) -> bool {
        let string = || format!(",{code},").contains(&format!(",{s},"));
        if self.config.numeric_compare {
            // `NumericCompare` (UGeneralMulti2.pas): StrToIntDef(.., -1), so negative
            // and hexadecimal numbers compare by value; only -1 falls back.
            match (delphi_str_to_int(s), delphi_str_to_int(code)) {
                (Some(a), Some(b)) if a != -1 && b != -1 => a == b,
                _ => string(),
            }
        } else {
            string()
        }
    }

    fn index_of(&self, s: &str) -> Option<usize> {
        self.entries.iter().position(|e| self.matches(s, &e.code))
    }

    /// Points of a QSO (`TGeneralScore.CalcPoints`, UGeneralScore.pas:248). The
    /// same-country and same-continent tables need CTY.DAT and are not applied.
    pub fn points(&self, q: &Qso) -> i32 {
        if let Some(h) = &self.hook {
            return h.verify(q, false).map(|v| v.points).unwrap_or(0);
        }
        let c = &self.config;
        let mut p = Config::at(&c.points, q.band, q.mode);
        if self.is_local(&q.nr_rcvd) {
            p = Config::at(&c.local_points, q.band, q.mode);
        }
        if let Some(table) = &c.alphabet_points {
            p = match q.nr_rcvd.chars().last() {
                Some(ch @ '0'..='Z') => table[ch as usize - '0' as usize],
                _ => 0,
            };
        }
        if !c.special_calls.is_empty() && self.special_call(&q.callsign) {
            p = Config::at(&c.special_call_points, q.band, q.mode);
        }
        p
    }

    /// `IsSpecialCallMatch` (UserDefinedContest.pas:1233).
    fn special_call(&self, call: &str) -> bool {
        comma_text(&self.config.special_calls).iter().any(|s| {
            match self.config.special_call_match {
                SpecialCallMatch::Full => s == call,
                SpecialCallMatch::Partial => !s.is_empty() && call.contains(s.as_str()),
            }
        })
    }

    /// `TGeneralMulti2.AddNoUpdate` (UGeneralMulti2.pas:280) without the CTY branch.
    fn add_multi(&mut self, q: &Qso, dupe: bool) -> (String, bool) {
        if self.config.no_multi {
            return (String::new(), false);
        }
        let s = self.extract_with(q, dupe);
        if dupe || !self.mode.accepts(q.mode) {
            return (s, false);
        }
        if let Some(i) = self.index_of(&s) {
            return (s, self.worked[i].insert(q.band));
        }
        if self.config.allow_unlisted_multi {
            return (s, false);
        }
        if self.config.undef_multi || self.config.prefix_multi != PrefixMulti::Off {
            // `AddAndSort`: before the first entry greater than the new one.
            let at = self
                .entries
                .iter()
                .position(|e| str_more(&e.code, &s))
                .unwrap_or(self.entries.len());
            self.entries.insert(
                at,
                DatEntry {
                    code: s.clone(),
                    name: String::new(),
                },
            );
            self.worked.insert(at, BTreeSet::from([q.band]));
            return (s, true);
        }
        (s, false)
    }

    /// Bands with a row in the score window: available bands of the range
    /// (`GetIsAvailableBand`; 10.4G only with `SINGLE10G OFF`).
    fn row_bands(&self) -> impl Iterator<Item = Band> + '_ {
        let (lo, hi) = self.band_range();
        Band::range(lo, hi).filter(|b| self.config.band_available(*b))
    }

    /// The sent number with the QSO's serial number for `$S`.
    pub fn sent_number_with_serial(
        &self,
        band: Band,
        power: Power,
        station: &Station,
        serial: Option<i32>,
    ) -> String {
        let _ = band;
        let prov = if station.prov.is_empty() {
            &self.config.prov
        } else {
            &station.prov
        };
        let qth = if station.city.is_empty() {
            &self.config.city
        } else {
            &station.city
        };
        expand_sent(
            &self.config.sent,
            &SentContext {
                station,
                prov,
                qth,
                power,
                serial,
                handle: if self.mode == ContestMode::Phone {
                    &station.handle_ph
                } else {
                    &station.handle_cw
                },
            },
        )
        .unwrap_or_default()
    }
}

impl Contest for UserContest {
    fn name(&self) -> &str {
        &self.name
    }

    fn band_range(&self) -> (Band, Band) {
        (self.config.band_low(), self.config.band_high())
    }

    fn uses_warc(&self) -> bool {
        self.config.warc
    }

    fn contest_mode(&self) -> ContestMode {
        self.mode
    }

    /// `FUseContestPeriod := FConfig.UseContestPeriod`, `FPeriod := FConfig.Period`
    /// (`TGeneralContest.Create`): CFG `USEPERIOD` and `PERIOD`.
    fn period_hours(&self) -> u32 {
        if self.config.use_period {
            self.config.period.max(0) as u32
        } else {
            0
        }
    }

    /// CFG `USEPERIOD` (`FUseContestPeriod`), also with `PERIOD 0` (or negative): the
    /// period then ends now ([`zlog_core::Log::out_of_open_period`]).
    fn uses_period(&self) -> bool {
        self.config.use_period
    }

    /// CFG `STARTTIME` (`FStartTime`); -1 or out of range is none.
    fn start_hour(&self) -> Option<u32> {
        u32::try_from(self.config.start_time)
            .ok()
            .filter(|h| *h < 24)
    }

    /// CFG `CONTESTID` (`AdifContestId := FConfig.ContestId`).
    fn adif_fields(&self, _q: &Qso) -> Vec<(String, String)> {
        vec![("contest_id".into(), self.config.contest_id.clone())]
    }

    /// `$S` is the next serial (`SERIAL ALL` or `BAND`); without `SERIAL` it is not
    /// known and leaves the number empty.
    fn sent_number(&self, band: Band, power: Power, station: &Station) -> String {
        let serial = self.next_serial_of_tx(band, station.tx_nr);
        self.sent_number_with_serial(band, power, station, serial)
    }

    /// `$D` in CFG `SENDNR` is the QSO's UTC time `hhnn` (`GetInitNrSent`, main.pas);
    /// [`Self::sent_number`] leaves it as written and the logger fills it in place.
    fn sent_with_time(&self) -> bool {
        self.config.sent.to_uppercase().contains("$D")
    }

    /// `FSerialType := FConfig.SerialContestType` (UzLogContest.pas:1490).
    fn next_serial_on(&self, band: Band) -> Option<i32> {
        self.next_serial_of_tx(band, 0)
    }

    /// The last QSO of the own TX (`TLog.GetLastSerial`, UzLogQSO.pas).
    fn next_serial_of_tx(&self, band: Band, tx_nr: u8) -> Option<i32> {
        match self.config.serial {
            SerialType::None => None,
            SerialType::All => Some(self.serials.next(None, tx_nr)),
            SerialType::Band => Some(self.serials.next(Some(band), tx_nr)),
        }
    }

    fn exchange_from(&self, _band: Band, past: &Qso) -> Option<String> {
        (self.config.same_exchange && self.config.serial == SerialType::None)
            .then(|| past.nr_rcvd.clone())
    }

    /// `TGeneralMulti2.ValidMulti` (UGeneralMulti2.pas:187).
    fn valid_number(&self, q: &Qso) -> bool {
        if !is_domestic(&q.callsign) && self.config.allow_dx_no_number {
            return true;
        }
        if !self.multi_needed() {
            return true;
        }
        if self.hook.is_some() {
            return !self.extract_multi(q).is_empty();
        }
        self.index_of(&self.extract_multi(q)).is_some()
    }

    fn score(&mut self, log: &Log) -> Scoring {
        let c = self.config.clone();
        self.entries = self.base.to_vec();
        self.worked = vec![BTreeSet::new(); self.entries.len()];
        self.serials = Serials::scan(log, c.serial_start);
        let dupes = dupe_flags(log, c.accept_different_mode, c.all_phone);
        let range = self.band_range();
        let mut rows = empty_rows(Band::ALL.iter().copied().filter(|b| *b <= Band::HIGHEST));
        let mut qsos: Vec<QsoScore> = Vec::with_capacity(log.qsos.len());
        // Points as integers, as zLog keeps them before they are stored in a byte.
        let mut points: Vec<i32> = Vec::with_capacity(log.qsos.len());

        for (i, (q, &dupe)) in log.qsos.iter().zip(&dupes).enumerate() {
            let mut s = QsoScore {
                dupe,
                ..QsoScore::default()
            };
            let mut p = 0;
            if in_contest(q.band, range, c.warc) && !q.invalid {
                (s.multi1, s.new_multi1) = self.add_multi(q, dupe);
                // A plugin's second multiplier is new on every QSO that has one
                // (UGeneralMulti2.pas:370-372) and counts in the total multipliers.
                if let Some(v) = self.hook.as_ref().and_then(|h| h.verify(q, dupe)) {
                    s.multi2 = v.multi2;
                    s.new_multi2 =
                        !c.no_multi && !dupe && self.mode.accepts(q.mode) && !s.multi2.is_empty();
                }
                let row = rows.entry(q.band).or_default();
                let valid = count_qso(row, self.mode, q);
                if s.new_multi1 {
                    row.multi += multi_count(&s.multi1);
                }
                if s.new_multi2 {
                    row.multi += multi_count(&s.multi2);
                }
                if !dupe && valid {
                    p = self.points(q);
                    if c.count_higher_points && c.accept_different_mode {
                        // `Log.DifferentModePointer`: the last earlier QSO with the same
                        // station on the band in another mode (`TLog.IsDupe`), where
                        // `IsSameBand` counts 10.4G as 10.1G under `SINGLE10G`.
                        let same_band = |b: Band| match b {
                            Band::B10_4G if c.single_10g => Band::B10G,
                            b => b,
                        };
                        let key = |x: &Qso| (x.core_call().to_string(), same_band(x.band));
                        let earlier = log.qsos[..i]
                            .iter()
                            .rposition(|x| !x.invalid && key(x) == key(q));
                        if let Some(j) = earlier {
                            if points[j] < p {
                                let band = log.qsos[j].band;
                                let r = rows.entry(band).or_default();
                                r.points = (r.points as i64 - points[j] as i64).max(0) as u32;
                                points[j] = 0;
                                qsos[j].points = 0;
                            } else {
                                p = 0;
                            }
                        }
                    }
                }
                let row = rows.entry(q.band).or_default();
                row.points = (row.points as i64 + p as i64).max(0) as u32;
            }
            s.points = p.clamp(0, u8::MAX as i32) as u8;
            points.push(p);
            qsos.push(s);
        }

        // Totals count every band, rows only the available ones (`TGeneralScore.UpdateData`).
        let total = sum(rows.values().copied());
        let shown: Vec<Band> = self.row_bands().collect();
        let bands = rows
            .into_iter()
            .filter(|(b, _)| shown.contains(b))
            .collect();
        let score = match &self.hook {
            _ if c.no_multi => total.points as u64,
            Some(h) => h.total(total.points, total.multi, log, &qsos),
            None => total.points as u64 * total.multi as u64,
        };
        // Departure: zLog multiplies by a coefficient of 0 too (score 0); a
        // coefficient that is not positive counts as unset here.
        let with_coeff = c
            .use_coeff
            .then(|| log.score_coeff())
            .filter(|k| *k > 0.0)
            .map(|k| (k, score as f64 * k));
        Scoring {
            qsos,
            sheet: ScoreSheet {
                bands,
                total,
                score,
                with_coeff,
            },
        }
    }

    fn extract_multi(&self, q: &Qso) -> String {
        UserContest::extract_multi(self, q)
    }

    /// `TGeneralMulti2.CheckMulti` (UGeneralMulti2.pas:455).
    ///
    /// Departure: with `UNDEFMULTI ON` zLog shows "<nr> : Not worked on any band"
    /// for a number not on the list; [`MultiCheck`] has no such case, so it is
    /// reported as [`MultiCheck::Invalid`].
    fn check_multi(&self, q: &Qso) -> MultiCheck {
        let s = if self.valid_number(q) {
            self.extract_multi(q)
        } else {
            q.nr_rcvd.clone()
        };
        if s.is_empty() {
            return MultiCheck::Invalid;
        }
        let Some(i) = self.index_of(&s) else {
            return MultiCheck::Invalid;
        };
        let worked: Vec<Band> = self.worked[i].iter().copied().collect();
        let e = &self.entries[i];
        MultiCheck::Known {
            code: e.code.split(',').next().unwrap_or("").to_string(),
            name: e.name.clone(),
            needed: !worked.contains(&q.band),
            worked_on: worked,
        }
    }

    fn multi_table(&self) -> Vec<(String, String, Vec<Band>)> {
        if self.config.no_multi {
            return Vec::new();
        }
        self.entries
            .iter()
            .zip(&self.worked)
            .map(|(e, w)| {
                (
                    e.code.split(',').next().unwrap_or("").to_string(),
                    e.name.clone(),
                    w.iter().copied().collect(),
                )
            })
            .collect()
    }
}
