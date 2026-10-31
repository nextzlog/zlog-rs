use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};

use zlog_core::{
    Band, Batch, BatchError, Book, Command, Contest, ContestMode, DelphiTime, Format, Log,
    LogFormat, Mode, MultiCheck, Power, Qso, QsoId, Registry, ScoreSheet, Station,
};

use crate::entry::Entry;
use crate::settings::Preferences;
use zlog_core::qso::QslState;

mod period;

pub use period::{
    default_start_time, khz_string, local_utc_offset_minutes, now_for, utc_hhmm, with_time,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub text: String,
    pub warning: bool,
}

impl Status {
    pub fn info(text: impl Into<String>) -> Self {
        Status {
            text: text.into(),
            warning: false,
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Status {
            text: text.into(),
            warning: true,
        }
    }
}

pub use zlog_ui::{NotLogged, Period, Setup};

pub fn asks_old_other(path: &Path) -> bool {
    format_of(path) == Format::Zlo
        && std::fs::read(path)
            .ok()
            .and_then(|b| Log::from_bytes(&b).ok())
            .is_some_and(|(log, _)| log.has_old_other())
}

impl zlog_ui::PeriodCheck for Session {
    fn outside(&self, q: &Qso) -> bool {
        self.out_of_period(q)
    }
}

pub struct Session {
    book: Book,
    pub station: Station,
    pub path: PathBuf,
    pub format: Format,
    pub entry: Entry,
    pub status: Status,
    pub use_contest_period: bool,
    pub record_freq: bool,
    pub qsy_rule: zlog_core::QsyRule,
    pub pc_name: String,
    pub qsl_default: QslState,
    pub asks_start_time: bool,
    pub tab: zlog_ui::TabState,
    saved_changes: u64,
    reopen: Setup,
}

fn format_of(path: &Path) -> Format {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("zlo") => Format::Zlo,
        _ => Format::Zlox,
    }
}

impl Session {
    pub fn open(setup: Setup, mut registry: Registry) -> Result<Self, String> {
        let mut warnings = Vec::new();
        let index = match &setup.contest_file {
            Some(file) => {
                let opened = registry.open_contest_file(file, crate::i18n::current())?;
                warnings = opened.warnings;
                registry.add_contest(opened.def)
            }
            None => setup.contest,
        };
        let def = registry
            .contests()
            .get(index)
            .ok_or(tr("error.no_contest"))?;
        let (id, uses_coeff) = (def.id().to_string(), def.uses_coeff());
        let (mut dropped, mut bad) = (0, 0);
        let reopen = Setup {
            convert_old_other: false,
            ..setup.clone()
        };
        let mut path = setup.path;
        let mut log = if path.exists() {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            dropped = zlog_core::zlo::trailing_bytes(&bytes);
            let (mut log, _, n) =
                Log::from_bytes_counted(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            bad = n;
            if setup.convert_old_other && format_of(&path) == Format::Zlo {
                log.convert_old_other();
                path.set_extension("zlox");
            }
            log
        } else {
            let mut log = Log::default();
            // zLog stores UTC as 32767, else the negated offset of local time (`SetUseUTC`).
            log.header.rst_sent = if def.uses_utc() {
                zlog_core::log::USE_UTC
            } else {
                (-local_utc_offset_minutes()) as i16 as u16
            };
            log
        };
        let rules = def.build(setup.mode);
        let asks_start_time = rules.uses_period() && log.start_time().0 == 0.0;
        if asks_start_time {
            let start = default_start_time(now_for(&log), rules.start_hour());
            log.set_start_time(start);
        }
        if uses_coeff {
            log.set_score_coeff(setup.coeff);
        }
        log.header.callsign = setup.station.callsign.clone();
        let (low, high) = rules.band_range();
        let first = if (low..=high).contains(&Band::B7) {
            Band::B7
        } else {
            low
        };
        let band = log.qsos.last().map_or(first, |q| q.band);
        let mode = log.qsos.last().map_or(default_mode(setup.mode), |q| q.mode);
        let (book, mut messages) = Book::open(log, rules, &id, registry);
        warnings.append(&mut messages);
        let messages = warnings;
        let power = setup.station.power_for(band);
        let mut session = Session {
            reopen,
            book,
            station: setup.station,
            format: format_of(&path),
            path,
            entry: Entry::new(band, mode, power),
            status: Status::default(),
            use_contest_period: setup.use_contest_period,
            record_freq: true,
            qsy_rule: Default::default(),
            pc_name: String::new(),
            qsl_default: QslState::None,
            asks_start_time,
            tab: zlog_ui::TabState::Normal,
            saved_changes: 0,
        };
        session.saved_changes = session.book.changes();
        session.report(Ok(messages));
        if bad > 0 {
            session.status = Status::warning(trf("status.bad_bytes", &[&bad]));
        }
        if dropped > 0 {
            session.status = Status::warning(trf("status.truncated", &[&dropped]));
        }
        if setup.convert_old_other && !session.path.exists() {
            session.save();
        }
        Ok(session)
    }

    pub fn reopen_setup(&self) -> Setup {
        Setup {
            path: self.path.clone(),
            station: self.station.clone(),
            ..self.reopen.clone()
        }
    }

    pub fn log(&self) -> &Log {
        self.book.log()
    }

    pub fn rules(&self) -> &dyn Contest {
        self.book.contest()
    }

    pub fn sheet(&self) -> &ScoreSheet {
        self.book.sheet()
    }

    pub fn period_hours(&self) -> u32 {
        if self.use_contest_period {
            self.rules().period_hours()
        } else {
            0
        }
    }

    pub fn out_of_period(&self, q: &Qso) -> bool {
        if !self.use_contest_period || !self.rules().uses_period() {
            return false;
        }
        match self.period_hours() {
            0 => self.log().out_of_open_period(q, now_for(self.log())),
            hours => self.log().out_of_period(q, hours),
        }
    }

    pub fn apply_preferences(&mut self, p: &Preferences) {
        self.qsy_rule = p.qsy_rule;
        self.book.set_qsy_rule(p.qsy_rule);
        self.qsl_default = p.qsl_default;
        self.pc_name = p.pc_name.clone();
    }

    pub fn formats(&self) -> &[Box<dyn LogFormat>] {
        self.book.registry().formats()
    }

    pub fn edit(&mut self, mut batch: Batch, done: Option<String>) {
        let log = self.log();
        batch.commands.retain(|c| match c {
            Command::Update(id, q) => self.find(*id).is_some_and(|i| log.qsos[i] != *q),
            _ => true,
        });
        let (count, before) = (batch.commands.len(), self.book.changes());
        let callsign = |id: QsoId| {
            let expected = batch.expect.iter().find(|(e, _)| *e == id);
            let q = expected.and_then(|(_, q)| q.as_ref());
            q.or_else(|| self.find(id).map(|i| &log.qsos[i]))
                .map_or(String::new(), |q| q.callsign.clone())
        };
        let refused = batch.check(&log.qsos).err().map(|m| callsign(m.id()));
        let result = self.book.execute_if(batch);
        if self.book.changes() != before {
            self.save();
        }
        match result {
            Ok(messages) => {
                let mut text = done.unwrap_or_else(|| trf("status.qsos_updated", &[&count]));
                for m in messages {
                    text.push_str("  ");
                    text.push_str(&m);
                }
                self.status = Status::info(text)
            }
            Err(BatchError::Refused(_)) => {
                let call = refused.unwrap_or_default();
                self.status = Status::warning(trf("status.edit_refused", &[&call]));
            }
            Err(BatchError::Book(e)) => self.status = Status::warning(e.to_string()),
        }
    }

    pub fn execute(&mut self, command: Command) -> bool {
        let ok = self.execute_unsaved(command);
        self.save();
        ok
    }

    pub fn execute_unsaved(&mut self, command: Command) -> bool {
        self.execute_reporting(command).0
    }

    fn execute_reporting(&mut self, command: Command) -> (bool, Option<String>) {
        let before = self.book.changes();
        let result = self.book.execute(command).map_err(|e| e.to_string());
        let error = result.as_ref().err().cloned();
        self.report(result);
        (self.book.changes() != before, error)
    }

    fn report(&mut self, result: Result<Vec<String>, String>) {
        match result {
            Ok(messages) if !messages.is_empty() => self.status = Status::info(messages.join("  ")),
            Ok(_) => {}
            Err(e) => self.status = Status::warning(e),
        }
    }

    pub fn poll(&mut self) {
        // zLog saves after every QSO received over Z-Link; save whenever a command
        // from a link changed the log, also when a later one failed.
        let polled = self.book.poll_changes();
        if polled.changed {
            self.save();
        }
        self.report(polled.result.map_err(|e| e.to_string()));
    }

    pub fn save(&mut self) {
        if let Err(e) = self.try_save() {
            self.status = Status::warning(trf("status.save_failed", &[&e]));
        }
    }

    pub(crate) fn try_save(&mut self) -> Result<(), String> {
        self.book
            .log()
            .save_with(&self.path, self.format, self.qsl_default)
            .map_err(|e| e.to_string())?;
        self.saved_changes = self.book.changes();
        Ok(())
    }

    pub fn changes(&self) -> u64 {
        self.book.changes()
    }

    pub fn unsaved(&self) -> bool {
        self.book.changes() != self.saved_changes
    }

    pub fn save_as(&mut self, path: PathBuf) {
        if let Err(e) = self.try_save_as(path) {
            self.status = Status::warning(trf("status.save_failed", &[&e]));
        }
    }

    pub fn try_save_as(&mut self, path: PathBuf) -> Result<(), String> {
        let old = (self.format, std::mem::replace(&mut self.path, path));
        self.format = format_of(&self.path);
        self.try_save().inspect_err(|_| {
            (self.format, self.path) = old;
        })
    }

    pub fn import(&mut self, format: usize, path: &Path) {
        let result = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| self.formats()[format].import(&bytes));
        match result {
            Ok(qsos) => {
                let (mut added, mut refused, mut failure) = (0, 0, None);
                self.status = Status::default();
                self.book.begin_tagged_step(zlog_core::IMPORT);
                for q in qsos {
                    let (applied, error) = self.execute_reporting(Command::Insert(q));
                    if applied {
                        added += 1;
                    } else {
                        refused += 1;
                    }
                    if failure.is_none() {
                        failure = error;
                    }
                }
                self.book.end_step();
                let saved = if self.unsaved() {
                    self.try_save().err()
                } else {
                    None
                };
                let mut status = if refused == 0 {
                    Status::info(trf("status.imported", &[&added]))
                } else {
                    Status::warning(trf("status.imported_refused", &[&added, &refused]))
                };
                for e in failure
                    .into_iter()
                    .chain(saved.map(|e| trf("status.save_failed", &[&e])))
                {
                    status.text.push_str("  ");
                    status.text.push_str(&e);
                    status.warning = true;
                }
                self.status = status;
            }
            Err(e) => self.status = Status::warning(trf("status.import_failed", &[&e])),
        }
    }

    pub fn export(&mut self, format: usize, path: &Path) {
        let result = self.formats()[format]
            .export(self.book.log(), self.book.contest())
            .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()));
        self.status = match result {
            Ok(()) => Status::info(trf("status.exported", &[&path.display()])),
            Err(e) => Status::warning(trf("status.export_failed", &[&e])),
        };
    }

    pub fn cycle_band(&mut self, back: bool) {
        self.entry
            .cycle_band(self.book.contest(), &self.station, back);
    }

    pub fn bands(&self) -> Vec<Band> {
        Entry::bands(self.book.contest())
    }

    pub fn modes(&self) -> Vec<Mode> {
        let rules = self.book.contest();
        let all_modes = rules.name().contains("Pedition");
        self.entry.modes(rules.contest_mode(), all_modes)
    }

    pub fn cycle_mode(&mut self, back: bool) {
        let rules = self.book.contest();
        let mode = rules.contest_mode();
        let all_modes = rules.name().contains("Pedition");
        self.entry.cycle_mode(mode, all_modes, back);
    }

    pub(crate) fn fill_exchange(&mut self) {
        if !self.entry.nr_rcvd.is_empty() {
            return;
        }
        let call = self.entry.callsign.trim().to_uppercase();
        let probe = Qso {
            callsign: call,
            ..Qso::default()
        };
        let filled = self
            .log()
            .qsos
            .iter()
            .filter(|q| !q.invalid && q.core_call() == probe.core_call())
            .find_map(|q| self.rules().exchange_from(self.entry.band, q));
        if let Some(nr) = filled {
            self.entry.nr_rcvd = nr;
        }
    }

    pub fn sent_number(&self) -> String {
        self.rules()
            .sent_number(self.entry.band, self.entry.power, &self.station)
    }

    pub fn check_call(&mut self) -> bool {
        let probe = Qso {
            callsign: self.entry.callsign.trim().to_uppercase(),
            band: self.entry.band,
            mode: self.entry.mode,
            ..Qso::default()
        };
        match self.log().find_dupe(&probe) {
            Some(i) => {
                let q = &self.log().qsos[i];
                self.status = Status::warning(trf(
                    "status.dupe",
                    &[&q.time, &q.band, &q.mode, &q.callsign, &q.nr_rcvd],
                ));
                true
            }
            None => {
                self.status = Status::default();
                false
            }
        }
    }

    pub(crate) fn check_number(&mut self) {
        if self.entry.nr_rcvd.is_empty() {
            return;
        }
        let probe = Qso {
            nr_rcvd: self.entry.nr_rcvd.to_uppercase(),
            band: self.entry.band,
            ..Qso::default()
        };
        self.status = match self.rules().check_multi(&probe) {
            MultiCheck::Invalid => Status::warning(tr("status.invalid_number")),
            MultiCheck::Known {
                code,
                name,
                needed,
                worked_on,
            } => {
                let state = if needed {
                    tr("status.needed")
                } else {
                    tr("status.worked")
                };
                let bands: Vec<&str> = worked_on.iter().map(|b| b.label()).collect();
                let worked = if bands.is_empty() {
                    String::new()
                } else {
                    trf("status.worked_on", &[&bands.join(" ")])
                };
                Status::info(format!("[{code}: {name}] {state}{worked}"))
            }
        };
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn log_entry(&mut self, time: DelphiTime) -> Result<(), NotLogged> {
        self.log_entry_with(time, None, Period::Warn)
    }

    pub fn log_entry_with(
        &mut self,
        time: DelphiTime,
        hz: Option<u64>,
        period: Period,
    ) -> Result<(), NotLogged> {
        let qso = self.entry_qso_with(time, hz, period)?;
        self.insert_entry(qso)
    }

    pub fn log_entry_forced(&mut self, time: DelphiTime, hz: Option<u64>) -> Result<(), NotLogged> {
        let qso = self.entry_qso_checked(time, hz, Period::Warn, true)?;
        self.insert_entry(qso)
    }

    fn insert_entry(&mut self, qso: Qso) -> Result<(), NotLogged> {
        let call = qso.callsign.clone();
        self.status = Status::default();
        if !self.execute(Command::Insert(qso)) {
            return Err(NotLogged::Refused);
        }
        if self.status.text.is_empty() {
            let q = self
                .log()
                .qsos
                .iter()
                .rev()
                .find(|q| q.callsign == call)
                .cloned()
                .unwrap_or_default();
            self.status = if q.invalid && self.out_of_period(&q) {
                Status::warning(trf("status.logged_out_of_period", &[&call]))
            } else if q.dupe {
                Status::warning(trf("status.logged_dupe", &[&call]))
            } else if q.new_multi1 {
                Status::info(trf("status.logged_new_multi", &[&call, &q.multi1]))
            } else {
                Status::info(trf("status.logged", &[&call]))
            };
        }
        self.entry.wipe();
        self.tab = zlog_ui::TabState::Normal;
        Ok(())
    }

    pub fn entry_qso(&mut self, time: DelphiTime, hz: Option<u64>) -> Result<Qso, NotLogged> {
        self.entry_qso_with(time, hz, Period::Warn)
    }

    pub(crate) fn entry_qso_with(
        &mut self,
        time: DelphiTime,
        hz: Option<u64>,
        period: Period,
    ) -> Result<Qso, NotLogged> {
        self.entry_qso_checked(time, hz, period, false)
    }

    fn entry_qso_checked(
        &mut self,
        time: DelphiTime,
        hz: Option<u64>,
        period: Period,
        force: bool,
    ) -> Result<Qso, NotLogged> {
        if self.entry.callsign.trim().is_empty() {
            self.status = Status::warning(tr("status.no_callsign"));
            return Err(NotLogged::NoCallsign);
        }
        let mut qso = self.entry.to_qso(self.rules(), &self.station, time, 0);
        qso.invalid = self.out_of_period(&qso);
        if qso.invalid && period == Period::Refuse {
            self.status = Status::warning(tr("status.out_of_period"));
            return Err(NotLogged::OutOfPeriod);
        }
        qso.qsy_violation = self.log().qsy_violation_of(&qso, self.qsy_rule);
        qso.pc_name = self.pc_name.clone();
        if let Some(hz) = hz.filter(|&hz| hz > 0 && self.record_freq) {
            qso.freq = khz_string(hz);
        }
        if self.rules().sent_with_time() {
            let hhmm = utc_hhmm(time, self.log().utc_offset_minutes());
            qso.nr_sent = with_time(&qso.nr_sent, &hhmm);
        }
        qso.dupe = self.log().find_dupe(&qso).is_some();
        qso.forced = force;
        if !force && !self.rules().valid_number(&qso) {
            self.status = Status::warning(tr("status.invalid_number"));
            return Err(NotLogged::InvalidNumber);
        }
        Ok(qso)
    }

    pub fn log_external(&mut self, mut qso: Qso) {
        qso.invalid = self.out_of_period(&qso);
        qso.qsy_violation = self.log().qsy_violation_of(&qso, self.qsy_rule);
        qso.pc_name = self.pc_name.clone();
        qso.dupe = self.log().find_dupe(&qso).is_some();
        let bad_number = !self.rules().valid_number(&qso);
        let call = qso.callsign.clone();
        self.status = Status::default();
        if !self.execute(Command::Insert(qso)) || !self.status.text.is_empty() {
            return;
        }
        let q = self.log().qsos.last().cloned().unwrap_or_default();
        self.status = if q.invalid && self.out_of_period(&q) {
            Status::warning(trf("status.logged_out_of_period", &[&call]))
        } else if bad_number {
            Status::warning(trf("status.logged_bad_number", &[&call]))
        } else if q.dupe {
            Status::warning(trf("status.logged_dupe", &[&call]))
        } else if q.new_multi1 {
            Status::info(trf("status.logged_new_multi", &[&call, &q.multi1]))
        } else {
            Status::info(trf("status.logged", &[&call]))
        };
    }

    pub fn find(&self, id: QsoId) -> Option<usize> {
        self.book.find(id)
    }

    pub fn close(&mut self) {
        if self.unsaved() {
            self.save();
        }
        let messages = self.book.close();
        self.report(Ok(messages));
    }

    pub(crate) fn close_unsaved(&mut self) {
        let messages = self.book.close();
        self.report(Ok(messages));
    }
}

fn default_mode(mode: ContestMode) -> zlog_core::Mode {
    match mode {
        ContestMode::Phone => zlog_core::Mode::Ssb,
        _ => zlog_core::Mode::Cw,
    }
}
