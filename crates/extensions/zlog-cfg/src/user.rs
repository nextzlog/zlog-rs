use std::collections::BTreeSet;
use std::sync::Arc;

use zlog_core::{
    Band, Contest, ContestDef, ContestMode, Log, MultiCheck, Power, Qso, QsoScore, ScoreSheet,
    Scoring, Station,
};

use zlog_rule_dx::cty::Cty;

use crate::cfg::{comma_text, Config, PrefixMulti, SerialType, SpecialCallMatch};
use crate::common::{
    count_qso, dupe_flags, empty_rows, expand_sent, in_contest, is_domestic, multi_count, sum,
    SentContext, Serials,
};
use crate::script::{Inputs, Judged, ScoreScript, ScriptProgram, ScriptQso, Sums};
use crate::table::{in_list, Index, Table};
use crate::texts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatEntry {
    pub code: String,
    pub name: String,
}

use crate::cfg::delphi_trim;

pub fn parse_dat(text: &str) -> Vec<DatEntry> {
    text.lines()
        .skip(1)
        .take_while(|l| !l.to_ascii_lowercase().contains("end of file"))
        .map(|l| match l.find(' ') {
            Some(i) if i > 0 => DatEntry {
                code: l[..i].to_string(),
                name: delphi_trim(&l[i + 1..]).to_string(),
            },
            Some(i) => DatEntry {
                code: String::new(),
                name: delphi_trim(&l[i + 1..]).to_string(),
            },
            None => DatEntry {
                code: String::new(),
                name: delphi_trim(l).to_string(),
            },
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    pub points: i32,
    pub multi1: String,
    pub multi2: String,
}

pub trait RuleHook: Send + Sync {
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict>;

    fn total(&self, points: u32, multis: u32, log: &Log, scored: &[QsoScore]) -> u64 {
        let _ = (log, scored);
        points as u64 * multis as u64
    }
}

#[derive(Clone)]
pub struct UserDefinedDef {
    id: String,
    hook: Option<Arc<dyn RuleHook>>,
    script: Option<Arc<dyn ScriptProgram>>,
    script_error: Option<String>,
    config: Arc<Config>,
    entries: Arc<Index>,
    dat_loaded: bool,
    cty: Option<Arc<Cty>>,
}

fn verdict_of(j: &Judged) -> Option<Verdict> {
    if j.invalid.is_some() {
        return None;
    }
    let multi = |i: usize| j.multis.get(i).cloned().unwrap_or_default();
    Some(Verdict {
        points: j.points,
        multi1: multi(0),
        multi2: multi(1),
    })
}

fn load_script(cfg: &str, name: &str) -> (Option<Arc<dyn ScriptProgram>>, Option<String>) {
    let Some(section) = crate::script::split(cfg).1 else {
        return (None, None);
    };
    let language = section.language.to_ascii_uppercase();
    let Some(engine) = crate::script::engine(&section.language) else {
        return (
            None,
            Some(format!(
                "SCRIPT {language}: no engine for this language in this zLog; scored as a plain CFG"
            )),
        );
    };
    let version = match section.version.as_deref() {
        None => Some(1),
        Some(v) => v.parse().ok().filter(|v| engine.versions().contains(v)),
    };
    match version {
        Some(version) => (
            Some(crate::script::host::program(
                engine,
                name,
                section.source,
                version,
            )),
            None,
        ),
        None => (
            None,
            Some(format!(
                "SCRIPT {language} {}: this zLog does not know this API version; scored as a plain CFG",
                section.version.as_deref().unwrap_or("")
            )),
        ),
    }
}

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
        let (script, script_error) = load_script(cfg, &config.contest_name);
        UserDefinedDef {
            id,
            hook: None,
            script,
            script_error,
            config: Arc::new(config),
            entries: Arc::new(Index::new(Arc::new(entries))),
            dat_loaded,
            cty: None,
        }
    }

    pub fn with_cty(mut self, cty: Arc<Cty>) -> Self {
        self.cty = Some(cty);
        self
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn with_entries(mut self, entries: Vec<DatEntry>) -> Self {
        self.dat_loaded = !entries.is_empty();
        self.entries = Arc::new(Index::new(Arc::new(entries)));
        self
    }

    pub fn with_hook(mut self, id: &str, hook: Arc<dyn RuleHook>) -> Self {
        self.id = id.to_string();
        self.hook = Some(hook);
        self.script = None;
        self.script_error = None;
        self
    }

    pub fn dat_loaded(&self) -> bool {
        self.dat_loaded
    }

    pub fn has_script(&self) -> bool {
        self.script.is_some()
    }

    pub fn script_error(&self) -> Option<&str> {
        self.script_error.as_deref()
    }

    pub fn build_rules(&self, mode: ContestMode) -> UserContest {
        let cty = if self.config.use_cty {
            self.cty.clone().unwrap_or_else(Cty::global)
        } else {
            Arc::new(Cty::default())
        };
        UserContest {
            cty_worked: vec![BTreeSet::new(); cty.countries().len()],
            cty,
            my_country: "JA".into(),
            my_continent: "AS".into(),
            name: self.config.contest_name.clone(),
            config: self.config.clone(),
            hook: self.hook.clone(),
            script: self.script.as_ref().map(|p| p.start()),
            inputs: Inputs::default(),
            unknown: false,
            mode,
            table: Table::new(self.entries.clone(), self.config.numeric_compare),
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

pub struct UserContest {
    name: String,
    hook: Option<Arc<dyn RuleHook>>,
    script: Option<Box<dyn ScoreScript>>,
    inputs: Inputs,
    unknown: bool,
    config: Arc<Config>,
    mode: ContestMode,
    table: Table,
    serials: Serials,
    cty: Arc<Cty>,
    cty_worked: Vec<BTreeSet<Band>>,
    my_country: String,
    my_continent: String,
}

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

fn delete(s: &mut Vec<char>, index: i64, count: i64) {
    if index < 1 || index as usize > s.len() || count <= 0 {
        return;
    }
    let from = index as usize - 1;
    let to = (from + count as usize).min(s.len());
    s.drain(from..to);
}

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

    fn country(&self, q: &Qso) -> Option<usize> {
        self.config
            .use_cty
            .then(|| self.cty.lookup(&q.callsign, true).index)
    }

    pub fn is_local(&self, q: &Qso) -> bool {
        let c = &self.config;
        if let Some(i) = self.country(q) {
            let country = &self.cty.countries()[i];
            if !c.local_country.is_empty() && in_list(&country.code, &c.local_country) {
                return true;
            }
            if !c.local_continent.is_empty() && in_list(&country.continent, &c.local_continent) {
                return true;
            }
        }
        let nr = q.nr_rcvd.as_str();
        let s: String = if c.min_local_len > 0 {
            nr.chars().take(c.min_local_len as usize).collect()
        } else {
            nr.to_string()
        };
        match c.local.as_slice() {
            [] => false,
            [(key, _)] => !key.is_empty() && s.starts_with(key.as_str()),
            many => many.iter().any(|(k, _)| *k == s),
        }
    }

    fn ruling(&self, q: &Qso, dupe: bool, judged: Option<&Judged>) -> Option<Option<Verdict>> {
        if let Some(s) = &self.script {
            return Some(match judged {
                Some(j) => verdict_of(j),
                None => {
                    let counted =
                        in_contest(q.band, self.band_range(), self.config.warc) && !q.invalid;
                    s.check(&ScriptQso::of(q, dupe, counted))
                        .as_ref()
                        .and_then(verdict_of)
                }
            });
        }
        self.hook.as_ref().map(|h| h.verify(q, dupe))
    }

    fn typed_unknown(&self, q: &Qso) -> bool {
        let Some(s) = &self.script else {
            return false;
        };
        let counted = in_contest(q.band, self.band_range(), self.config.warc) && !q.invalid;
        s.check(&ScriptQso::of(q, q.dupe, counted)).is_none()
    }

    pub fn extract_multi(&self, q: &Qso) -> String {
        self.extract_with(q, q.dupe, None)
    }

    fn extract_with(&self, q: &Qso, dupe: bool, judged: Option<&Judged>) -> String {
        if let Some(r) = self.ruling(q, dupe, judged) {
            return r.map(|v| v.multi1).unwrap_or_default();
        }
        let c = &self.config;
        if c.prefix_multi != PrefixMulti::Off {
            return self.prefix(&q.callsign);
        }
        let mut s: Vec<char> = q.nr_rcvd.chars().collect();
        if c.cut_tailing_alphabets {
            s = match s.iter().rposition(|ch| ch.is_ascii_digit()) {
                Some(0) | None => Vec::new(),
                Some(i) => s[..=i].to_vec(),
            };
        }
        let (cut, tail) = if self.is_local(q) {
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

    fn multi_needed(&self) -> bool {
        let c = &self.config;
        !(c.undef_multi
            || c.allow_unlisted_multi
            || c.prefix_multi != PrefixMulti::Off
            || c.use_cty
            || c.no_multi)
    }

    fn index_of(&self, s: &str) -> Option<usize> {
        self.table.find(s)
    }

    pub fn points(&self, q: &Qso) -> i32 {
        self.points_with(q, None)
    }

    fn points_with(&self, q: &Qso, judged: Option<&Judged>) -> i32 {
        if let Some(r) = self.ruling(q, false, judged) {
            return r.map(|v| v.points).unwrap_or(0);
        }
        let c = &self.config;
        let mut p = Config::at(&c.points, q.band, q.mode);
        if let Some(i) = self.country(q) {
            let country = &self.cty.countries()[i];
            match (&c.same_cty_points, &c.same_cont_points) {
                (Some(t), _) if country.code == self.my_country => {
                    p = Config::at(t, q.band, q.mode);
                }
                (_, Some(t)) if country.continent == self.my_continent => {
                    p = Config::at(t, q.band, q.mode);
                }
                _ => {}
            }
        }
        if self.is_local(q) {
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

    fn special_call(&self, call: &str) -> bool {
        comma_text(&self.config.special_calls).iter().any(|s| {
            match self.config.special_call_match {
                SpecialCallMatch::Full => s == call,
                SpecialCallMatch::Partial => !s.is_empty() && call.contains(s.as_str()),
            }
        })
    }

    fn add_multi(&mut self, q: &Qso, dupe: bool, judged: Option<&Judged>) -> (String, bool) {
        if self.config.no_multi {
            return (String::new(), false);
        }
        if let Some(i) = self.country(q) {
            let c = &self.config;
            let country = &self.cty.countries()[i];
            if c.no_country_multi != "*" && !in_list(&country.code, &c.no_country_multi) {
                if dupe || (c.cty_entity_only && country.is_waedc()) {
                    return (String::new(), false);
                }
                let s = country.code.clone();
                if !self.mode.accepts(q.mode) {
                    return (s, false);
                }
                let worked = &mut self.cty_worked[i];
                let new = if c.count_multi_once {
                    worked.is_empty() && worked.insert(q.band)
                } else {
                    worked.insert(q.band)
                };
                return (s, new);
            }
        }
        let s = self.extract_with(q, dupe, judged);
        if dupe || !self.mode.accepts(q.mode) {
            return (s, false);
        }
        if let Some(i) = self.index_of(&s) {
            return (s, self.table.work(i, q.band));
        }
        if self.config.allow_unlisted_multi {
            return (s, false);
        }
        if self.config.undef_multi || self.config.prefix_multi != PrefixMulti::Off {
            self.table.insert(&s, q.band);
            return (s, true);
        }
        (s, false)
    }

    fn my_place(&self, log: &Log) -> (String, String) {
        let call = log.header.callsign.trim().to_ascii_uppercase();
        if !self.cty.is_loaded() || call.is_empty() {
            return ("JA".into(), "AS".into());
        }
        let f = self.cty.lookup(&call, true);
        (f.country.code.clone(), f.continent().to_string())
    }

    fn row_bands(&self) -> impl Iterator<Item = Band> + '_ {
        let (lo, hi) = self.band_range();
        Band::range(lo, hi).filter(|b| self.config.band_available(*b))
    }

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

    fn period_hours(&self) -> u32 {
        if self.config.use_period {
            self.config.period.max(0) as u32
        } else {
            0
        }
    }

    fn uses_period(&self) -> bool {
        self.config.use_period
    }

    fn start_hour(&self) -> Option<u32> {
        u32::try_from(self.config.start_time)
            .ok()
            .filter(|h| *h < 24)
    }

    fn adif_fields(&self, _q: &Qso) -> Vec<(String, String)> {
        vec![("contest_id".into(), self.config.contest_id.clone())]
    }

    fn sent_template(&self, _station: &Station) -> Option<String> {
        Some(self.config.sent.clone())
    }

    fn sent_number(&self, band: Band, power: Power, station: &Station) -> String {
        let serial = self.next_serial_of_tx(band, station.tx_nr);
        self.sent_number_with_serial(band, power, station, serial)
    }

    fn sent_with_time(&self) -> bool {
        self.config.sent.to_ascii_uppercase().contains("$D")
    }

    fn next_serial_on(&self, band: Band) -> Option<i32> {
        self.next_serial_of_tx(band, 0)
    }

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

    fn valid_number(&self, q: &Qso) -> bool {
        if !is_domestic(&q.callsign) && self.config.allow_dx_no_number {
            return true;
        }
        if !self.multi_needed() {
            return true;
        }
        if self.typed_unknown(q) {
            return true;
        }
        if self.hook.is_some() || self.script.is_some() {
            return !self.extract_multi(q).is_empty();
        }
        self.index_of(&self.extract_multi(q)).is_some()
    }

    fn score(&mut self, log: &Log) -> Scoring {
        let c = self.config.clone();
        self.table.reset();
        self.cty_worked = vec![BTreeSet::new(); self.cty.countries().len()];
        (self.my_country, self.my_continent) = self.my_place(log);
        self.serials = Serials::scan(log, c.serial_start);
        let dupes = dupe_flags(log, c.accept_different_mode, c.all_phone);
        let range = self.band_range();
        if self.script.is_some() {
            let old = &self.inputs;
            let fresh: Vec<Arc<ScriptQso>> = log
                .qsos
                .iter()
                .zip(&dupes)
                .enumerate()
                .map(|(i, (q, &d))| {
                    let counted = in_contest(q.band, range, c.warc) && !q.invalid;
                    match old.get(i) {
                        Some(o) if o.is(q, d, counted) => o.clone(),
                        _ => Arc::new(ScriptQso::of(q, d, counted)),
                    }
                })
                .collect();
            let unchanged = fresh.len() == old.len()
                && fresh.iter().zip(old.iter()).all(|(a, b)| Arc::ptr_eq(a, b));
            if !unchanged {
                self.inputs = Arc::new(fresh);
            }
        }
        let inputs = self.inputs.clone();
        let judged = self.script.as_ref().and_then(|s| s.judge(&inputs));
        self.unknown = self.script.is_some() && judged.is_none();
        let empty = Judged {
            invalid: Some(String::new()),
            ..Judged::default()
        };
        let scripted = self.script.is_some();
        let judged_at = |i: usize| match &judged {
            Some(all) => all.get(i),
            None => scripted.then_some(&empty),
        };
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
                let j = judged_at(i);
                (s.multi1, s.new_multi1) = self.add_multi(q, dupe, j);
                if let Some(v) = self.ruling(q, dupe, j).flatten() {
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
                    p = self.points_with(q, j);
                    if c.count_higher_points && c.accept_different_mode {
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

        let total = sum(rows.values().copied());
        let shown: Vec<Band> = self.row_bands().collect();
        let bands = rows
            .into_iter()
            .filter(|(b, _)| shown.contains(b))
            .collect();
        let score = match &self.hook {
            _ if c.no_multi => total.points as u64,
            Some(h) => h.total(total.points, total.multi, log, &qsos),
            None => match &self.script {
                Some(s) => {
                    let sums = Sums {
                        qsos: total.qsos,
                        points: total.points,
                        multis: total.multi,
                    };
                    let known = if self.unknown {
                        None
                    } else {
                        s.total(&inputs, sums)
                    };
                    self.unknown = known.is_none();
                    known.unwrap_or(0)
                }
                None => total.points as u64 * total.multi as u64,
            },
        };
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

    fn score_unknown(&self) -> bool {
        self.unknown
    }

    fn extract_multi(&self, q: &Qso) -> String {
        UserContest::extract_multi(self, q)
    }

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
        let worked: Vec<Band> = self.table.worked(i).iter().copied().collect();
        let e = self.table.entry(i);
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
        let cities = self.table.ordered().map(|(e, w)| {
            (
                e.code.split(',').next().unwrap_or("").to_string(),
                e.name.clone(),
                w.iter().copied().collect(),
            )
        });
        let countries = (self.config.use_cty && self.config.no_country_multi != "*")
            .then(|| self.cty.countries().iter().zip(&self.cty_worked))
            .into_iter()
            .flatten()
            .filter(|(c, _)| !c.code.is_empty())
            .map(|(c, w)| (c.code.clone(), c.name.clone(), w.iter().copied().collect()));
        cities.chain(countries).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dat_lines_follow_delphi_trim_and_lower_case() {
        let text =
            "header\n01 \t札幌市\u{3000}\x1f\r\n 02\u{a0}\nX\u{a0}\n03 Ünd\nEND OF FILE\n04 x\n";
        let e = |c: &str, n: &str| DatEntry {
            code: c.into(),
            name: n.into(),
        };
        assert_eq!(
            parse_dat(text),
            vec![
                e("01", "札幌市\u{3000}"),
                e("", "02\u{a0}"),
                e("", "X\u{a0}"),
                e("03", "Ünd"),
            ]
        );
    }

    #[test]
    fn the_sent_template_is_sendnr() {
        let def = UserDefinedDef::from_cfg("#T\nSENDNR $Q$P;\n", |_| None);
        let c = def.build_rules(ContestMode::Mix);
        assert_eq!(
            c.sent_template(&Station::default()).as_deref(),
            Some("$Q$P")
        );
    }

    #[test]
    fn an_empty_single_local_key_matches_nothing_like_zlog_pos() {
        let mut def = UserDefinedDef::from_cfg("#T\nLOCAL 10;\n", |_| None);
        let q = Qso {
            callsign: "JA1ZLO".into(),
            nr_rcvd: "10".into(),
            ..Qso::default()
        };
        assert!(def.build_rules(ContestMode::Mix).is_local(&q));
        Arc::make_mut(&mut def.config).local = vec![(String::new(), String::new())];
        assert!(!def.build_rules(ContestMode::Mix).is_local(&q));
    }
}
