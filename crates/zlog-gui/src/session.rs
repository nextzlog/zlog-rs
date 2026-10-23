//! One open contest as the window sees it: the book, the station and the entry row.
//!
//! Changes to QSOs go through [`Book::execute`]; this module only turns operator
//! actions into commands, saves after each change and puts messages in the status bar.

use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};

use zlog_core::{
    Band, Batch, BatchError, Book, Command, Contest, ContestMode, DelphiTime, Format, Log,
    LogFormat, Mode, MultiCheck, Power, Qso, QsoId, Registry, ScoreSheet, Station,
};

use crate::entry::Entry;
use crate::settings::Preferences;
use zlog_core::qso::QslState;

/// Text of the status bar and whether it is a warning (shown in red).
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

pub use zlog_ui::{NotLogged, Setup};

/// Whether opening `path` should first ask to convert old Other QSOs: a `.ZLO`
/// file with QSOs in mode 5 (`TMainForm.LoadNewContestFromFile`, `IsOldOtherExist`;
/// zLog asks with the default button No).
pub fn asks_old_other(path: &Path) -> bool {
    format_of(path) == Format::Zlo
        && std::fs::read(path)
            .ok()
            .and_then(|b| Log::from_bytes(&b).ok())
            .is_some_and(|(log, _)| log.has_old_other())
}

/// The panels judge QSOs against the period as logging does.
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
    /// zLog's `UseContestPeriod` option: flag QSOs outside the contest period.
    pub use_contest_period: bool,
    /// zLog's `RecordFreqInMemo` option (`Settings._recrigfreq`, default on): store
    /// the rig frequency in a logged QSO.
    pub record_freq: bool,
    /// zLog's `CountDown` and `QSYCount` options, checked when logging.
    pub qsy_rule: zlog_core::QsyRule,
    /// zLog's `PCName` (zlog.ini `[Z-Link]`), stored in every logged QSO.
    pub pc_name: String,
    /// zLog's `QslDefault` (`Settings._qsl_default`): the QSL state left out of the
    /// memo when saving `.ZLO`.
    pub qsl_default: QslState,
    /// A new log of a contest that uses its period got the suggested start time;
    /// the window asks the operator to confirm or correct it (`InputStartTime(True,
    /// True)` shows `TStartTimeDialog` right after setting the default).
    pub asks_start_time: bool,
    /// [`Book::changes`] when the log was last saved, to tell unsaved changes.
    saved_changes: u64,
}

fn format_of(path: &Path) -> Format {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("zlo") => Format::Zlo,
        _ => Format::Zlox,
    }
}

impl Session {
    /// Open the file named in the setup, or start a new one if it does not exist.
    pub fn open(setup: Setup, mut registry: Registry) -> Result<Self, String> {
        // A missing DAT does not stop the contest; zLog says so on the status line.
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
        let mut path = setup.path;
        let mut log = if path.exists() {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            dropped = zlog_core::zlo::trailing_bytes(&bytes);
            let (mut log, _, n) =
                Log::from_bytes_counted(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            bad = n;
            // `ConvertModeOther`, then the log continues as .ZLOX.
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
        // `TMainForm` on opening a contest: a contest with a period and a log without
        // a start time gets the suggested one (`InputStartTime(True, True)`; zLog also
        // shows it in a dialog to correct).
        let asks_start_time = rules.uses_period() && log.start_time().0 == 0.0;
        if asks_start_time {
            let start = default_start_time(now_for(&log), rules.start_hour());
            log.set_start_time(start);
        }
        if uses_coeff {
            log.set_score_coeff(setup.coeff);
        }
        // The header carries the current contest setup, not the file's (zLog
        // `LoadNewContestFromFile` assigns the current header before loading).
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

    pub fn log(&self) -> &Log {
        self.book.log()
    }

    pub fn rules(&self) -> &dyn Contest {
        self.book.contest()
    }

    pub fn sheet(&self) -> &ScoreSheet {
        self.book.sheet()
    }

    /// Hours of the contest period to check QSOs against, 0 for none
    /// (`MyContest.UseContestPeriod`, `Period`).
    pub fn period_hours(&self) -> u32 {
        if self.use_contest_period {
            self.rules().period_hours()
        } else {
            0
        }
    }

    /// Whether `q` lies outside the contest period (`TLog.IsOutOfPeriod`): with a
    /// length, from the start time; for a contest that uses its period with a length
    /// of 0, from the start time until now ([`Log::out_of_open_period`]).
    pub fn out_of_period(&self, q: &Qso) -> bool {
        if !self.use_contest_period || !self.rules().uses_period() {
            return false;
        }
        match self.period_hours() {
            0 => self.log().out_of_open_period(q, now_for(self.log())),
            hours => self.log().out_of_period(q, hours),
        }
    }

    /// Apply the preferences of the Options: the QSY rules (to the entry check and
    /// the log's), the default QSL state for saving, and the PC name.
    pub fn apply_preferences(&mut self, p: &Preferences) {
        self.qsy_rule = p.qsy_rule;
        self.book.set_qsy_rule(p.qsy_rule);
        self.qsl_default = p.qsl_default;
        self.pc_name = p.pc_name.clone();
    }

    pub fn formats(&self) -> &[Box<dyn LogFormat>] {
        self.book.registry().formats()
    }

    /// Run a batch of an extension as one edit of the operator (an edit of many at
    /// once, an undo), leaving out updates that change nothing; save once and say
    /// `done`, or how many QSOs changed. A batch refused, as the log is no longer as
    /// it expects, changes nothing and says which QSO differs.
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
                // What the extensions said comes after what was done.
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

    /// Run a command, save, and show what the extensions said.
    fn execute(&mut self, command: Command) -> bool {
        let ok = self.execute_unsaved(command);
        self.save();
        ok
    }

    /// Run a command and show what the extensions said, without saving. Returns
    /// whether the command changed the log: a failure later in its chain (scoring,
    /// a cascade) leaves the change applied, so it counts as done, and the status
    /// says what failed.
    fn execute_unsaved(&mut self, command: Command) -> bool {
        self.execute_reporting(command).0
    }

    /// [`Session::execute_unsaved`], also returning what failed in the chain.
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

    /// Give extensions that listen to the outside (zLink and the like) a turn.
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

    /// Write the log to its file; why not, when it could not be written.
    pub fn try_save(&mut self) -> Result<(), String> {
        self.book
            .log()
            .save_with(&self.path, self.format, self.qsl_default)
            .map_err(|e| e.to_string())?;
        self.saved_changes = self.book.changes();
        Ok(())
    }

    /// Whether the log changed since it was last saved.
    pub fn unsaved(&self) -> bool {
        self.book.changes() != self.saved_changes
    }

    /// Save the log at `path` from now on; when it cannot be written there, it stays
    /// where it was and the status line says why.
    pub fn save_as(&mut self, path: PathBuf) {
        if let Err(e) = self.try_save_as(path) {
            self.status = Status::warning(trf("status.save_failed", &[&e]));
        }
    }

    /// [`Session::save_as`], telling why it failed.
    pub fn try_save_as(&mut self, path: PathBuf) -> Result<(), String> {
        let old = (self.format, std::mem::replace(&mut self.path, path));
        self.format = format_of(&self.path);
        self.try_save().inspect_err(|_| {
            // Still saved where it was saved before.
            (self.format, self.path) = old;
        })
    }

    /// Add the QSOs of a file in a format provided by an extension.
    pub fn import(&mut self, format: usize, path: &Path) {
        let result = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| self.formats()[format].import(&bytes));
        match result {
            Ok(qsos) => {
                // Save once at the end, not once per QSO; count what the book refused.
                // The whole import is one step of the operator, undone at once. A QSO
                // whose chain failed after it was inserted stays in the log: it is
                // counted as added, saved, and the failure is reported.
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
                // What failed stays on the status bar after the count.
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

    /// Next band of the contest (Shift+B).
    pub fn cycle_band(&mut self, back: bool) {
        self.entry
            .cycle_band(self.book.contest(), &self.station, back);
    }

    /// Next mode within what the band carries (Shift+M).
    pub fn cycle_mode(&mut self, back: bool) {
        let rules = self.book.contest();
        let mode = rules.contest_mode();
        // `SetQSOMode`: `if Pos('Pedition', MyContest.Name) > 0 then maxmode := LastMode`.
        let all_modes = rules.name().contains("Pedition");
        self.entry.cycle_mode(mode, all_modes, back);
    }

    /// Fill the number from an earlier QSO with the station (Space; `SpaceBarProc`).
    ///
    /// Unlike zLog, a number already typed is kept, the portable suffix is ignored when
    /// matching the callsign, and invalid QSOs are not used.
    pub fn fill_exchange(&mut self) {
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

    /// Sent number for the band in the entry row.
    pub fn sent_number(&self) -> String {
        self.rules()
            .sent_number(self.entry.band, self.entry.power, &self.station)
    }

    /// Status line after Space in the callsign field: first QSO with the station if dupe.
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

    /// Status line while typing the number (`CheckMulti`).
    pub fn check_number(&mut self) {
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

    /// Log the entry row (Enter). Returns false and explains why when nothing was logged.
    #[cfg(test)]
    pub fn log_entry(&mut self, time: DelphiTime) -> Result<(), NotLogged> {
        self.log_entry_on(time, None)
    }

    /// Log the entry row with the rig frequency in Hz, if a rig reports one; with
    /// `record_freq` the QSO keeps it in kHz (main.pas: `if Hz > 0 ... if
    /// Settings._recrigfreq then Q.Freq := rig.CurrentFreqkHzStr`).
    pub fn log_entry_on(&mut self, time: DelphiTime, hz: Option<u64>) -> Result<(), NotLogged> {
        if self.entry.callsign.trim().is_empty() {
            self.status = Status::warning(tr("status.no_callsign"));
            return Err(NotLogged::NoCallsign);
        }
        let mut qso = self.entry.to_qso(self.rules(), &self.station, time, 0);
        // A QSO outside the contest period is logged as invalid (`FOutOfContestPeriod`).
        qso.invalid = self.out_of_period(&qso);
        // main.pas `LogButtonProc`: the QSY violation and the PC name.
        qso.qsy_violation = self.log().qsy_violation_of(&qso, self.qsy_rule);
        qso.pc_name = self.pc_name.clone();
        if let Some(hz) = hz.filter(|&hz| hz > 0 && self.record_freq) {
            qso.freq = khz_string(hz);
        }
        // `$D` of the sent template: the UTC time as HHMM (BARTG `$S$D`, CFG SENDNR).
        if self.rules().sent_with_time() {
            let hhmm = utc_hhmm(time, self.log().utc_offset_minutes());
            qso.nr_sent = with_time(&qso.nr_sent, &hhmm);
        }
        // main.pas `LogButtonProc` sets `Q.Dupe` before `ValidMulti`: a rule hook
        // gives a dupe no multiplier, so it is refused as an invalid number.
        qso.dupe = self.log().find_dupe(&qso).is_some();
        if !self.rules().valid_number(&qso) {
            self.status = Status::warning(tr("status.invalid_number"));
            return Err(NotLogged::InvalidNumber);
        }
        let call = qso.callsign.clone();
        self.status = Status::default();
        // A QSO inserted stays logged when its chain fails later (the status says
        // why): it is saved and the entry row cleared, so Enter does not log it twice.
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
                // Why the QSO scores nothing (zLog shows `panelOutOfPeriod`).
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
        Ok(())
    }

    /// Where the QSO with this ID is now. Rows move under the dialogs while Z-Link
    /// and observers insert, delete and sort (each [`Session::poll`]), so the dialogs
    /// keep the ID and look the row up when they act, as zLog's `EditQSObyID` does.
    pub fn find(&self, id: QsoId) -> Option<usize> {
        self.book.find(id)
    }

    /// Contest start time of the log (`Log.StartTime`).
    pub fn start_time(&self) -> DelphiTime {
        self.log().start_time()
    }

    /// Set the start time (OK of `TStartTimeDialog`) and save. With `judge`, every
    /// QSO's invalid flag is set again from the period, as `menuCorrectStartTimeClick`
    /// does when the operator answers Yes (`Log.JudgeOutOfPeriod`, then
    /// `SetDupeFlags`, which the book does on each update). Returns how many QSOs
    /// changed their invalid flag.
    ///
    /// Unlike zLog, judging is skipped while `UseContestPeriod` is off, where it
    /// would only clear invalid flags set by hand (zLog disables the menu item then).
    pub fn set_start_time(&mut self, start: DelphiTime, judge: bool) -> usize {
        let b = start.0.to_le_bytes();
        let header = self.book.header_mut();
        header.reserve2 = i32::from_le_bytes(b[..4].try_into().unwrap());
        header.reserve3 = i32::from_le_bytes(b[4..].try_into().unwrap());
        self.asks_start_time = false;
        let mut changed = 0;
        if judge && self.use_contest_period {
            let updates: Vec<(QsoId, Qso)> = self
                .log()
                .qsos
                .iter()
                .filter(|q| q.invalid != self.out_of_period(q))
                .map(|q| {
                    let mut new = q.clone();
                    new.invalid = !q.invalid;
                    (QsoId::of(q), new)
                })
                .collect();
            self.book.begin_step();
            for (id, q) in updates {
                if self.execute_unsaved(Command::Update(id, q)) {
                    changed += 1;
                }
            }
            self.book.end_step();
        }
        self.save();
        changed
    }

    /// Change the QSOs with these IDs with `change` (the grid menu's Change Band,
    /// Mode, Power and the like; `GridBandChangeClick` etc. edit each QSO and send
    /// it with `EditQSObyID`). IDs no longer in the log are skipped. Returns how many
    /// QSOs were changed.
    pub fn change_qsos(&mut self, ids: &[QsoId], change: impl Fn(&mut Qso)) -> usize {
        let mut changed = 0;
        self.book.begin_step();
        for &id in ids {
            let Some(index) = self.find(id) else {
                continue;
            };
            let mut q = self.log().qsos[index].clone();
            change(&mut q);
            if q != self.log().qsos[index] && self.execute_unsaved(Command::Update(id, q)) {
                changed += 1;
            }
        }
        self.book.end_step();
        if changed > 0 {
            self.save();
        }
        changed
    }

    /// Change Band of the grid menu (`GridBandChangeClick`).
    pub fn change_band(&mut self, ids: &[QsoId], band: Band) -> usize {
        self.change_qsos(ids, |q| q.band = band)
    }

    /// Change Mode of the grid menu (`GridModeChangeClick`): between phone and the
    /// other modes the RSTs lose or gain their tone figure (59 and 599).
    pub fn change_mode(&mut self, ids: &[QsoId], mode: Mode) -> usize {
        self.change_qsos(ids, |q| {
            let phone = |m: Mode| matches!(m, Mode::Ssb | Mode::Am | Mode::Fm | Mode::Dv);
            if phone(mode) && !phone(q.mode) {
                q.rst_sent /= 10;
                q.rst_rcvd /= 10;
            } else if !phone(mode) && phone(q.mode) {
                // Values that cannot gain a figure (1000 and more) stay as they are.
                let gain = |r: u16| {
                    r.checked_mul(10)
                        .and_then(|r| r.checked_add(9))
                        .filter(|_| r < 1000)
                        .unwrap_or(r)
                };
                q.rst_sent = gain(q.rst_sent);
                q.rst_rcvd = gain(q.rst_rcvd);
            }
            q.mode = mode;
        })
    }

    /// Change Power of the grid menu (`GridPowerChangeClick`); as in zLog the sent
    /// number is left as it is.
    pub fn change_power(&mut self, ids: &[QsoId], power: Power) -> usize {
        self.change_qsos(ids, |q| q.power = power)
    }

    /// Close the log, saving it first when it has unsaved changes.
    pub fn close(&mut self) {
        if self.unsaved() {
            self.save();
        }
        let messages = self.book.close();
        self.report(Ok(messages));
    }

    /// Close without saving (the operator chose to, after saving failed).
    pub fn close_unsaved(&mut self) {
        let messages = self.book.close();
        self.report(Ok(messages));
    }
}

/// A QSO time as HHMM in UTC (`TQSO.TimeUtc`), from a time in the log's zone
/// (`offset` minutes ahead of UTC, `None` for a UTC log).
/// Put the UTC time into a sent number: every `$D` is replaced in place, as
/// `TMainForm.GetInitNrSent` (main.pas) does; a number without `$D` (BARTG, whose
/// contest builds the serial alone) gets the time appended.
pub fn with_time(sent: &str, hhmm: &str) -> String {
    if sent.contains("$D") {
        sent.replace("$D", hhmm)
    } else {
        format!("{sent}{hhmm}")
    }
}

pub fn utc_hhmm(time: DelphiTime, offset: Option<i32>) -> String {
    let utc = DelphiTime(time.0 - offset.unwrap_or(0) as f64 / 1440.0);
    let (_, _, _, h, m, _) = utc.to_ymd_hms();
    format!("{h:02}{m:02}")
}

/// A frequency in kHz with one decimal, as zLog writes it (`kHzStr`: 7012345 Hz
/// is "7012.3").
pub fn khz_string(hz: u64) -> String {
    format!("{}.{}", hz / 1000, hz % 1000 / 100)
}

fn default_mode(mode: ContestMode) -> zlog_core::Mode {
    match mode {
        ContestMode::Phone => zlog_core::Mode::Ssb,
        _ => zlog_core::Mode::Cw,
    }
}

pub fn local_utc_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

/// The time to stamp a QSO with: UTC when the log records UTC, else local time.
pub fn now_for(log: &Log) -> DelphiTime {
    use chrono::{Datelike, Timelike};
    let t = if log.utc_offset_minutes().is_none() {
        chrono::Utc::now().naive_utc()
    } else {
        chrono::Local::now().naive_local()
    };
    DelphiTime::from_ymd_hms(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
    )
}

/// The start time zLog suggests for a log without one (`TMainForm.InputStartTime`),
/// from `now` in the log's zone: hour `h` of today for `Some(h)`, except midnight
/// (`Some(0)`) of the next day, or the current hour for `None` (zLog -1).
pub fn default_start_time(now: DelphiTime, start_hour: Option<u32>) -> DelphiTime {
    let day = now.0.floor() + if start_hour == Some(0) { 1.0 } else { 0.0 };
    let (_, _, _, hour, _, _) = now.to_ymd_hms();
    let h = start_hour.unwrap_or(hour);
    DelphiTime(day + h as f64 / 24.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Mode;

    fn registry() -> Registry {
        Registry::new()
            .with(zlog_cfg::Cfg)
            .with(zlog_rule_ja::Japan)
    }

    fn setup(dir: &Path) -> Setup {
        Setup {
            contest: 0,
            mode: ContestMode::Mix,
            station: Station {
                callsign: "JA1ZLO".into(),
                prov: "10".into(),
                ..Station::default()
            },
            contest_file: None,
            coeff: 1.0,
            path: dir.join("test.zlox"),
            use_contest_period: true,
            convert_old_other: false,
        }
    }

    #[test]
    fn many_qsos_are_updated_with_one_save() {
        let dir = scratch("update_many");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        for call in ["JA1A", "JA1B"] {
            s.execute(Command::Insert(zlog_core::Qso {
                callsign: call.into(),
                nr_sent: "10".into(),
                ..zlog_core::Qso::default()
            }));
        }
        let list: Vec<(QsoId, Qso)> = s
            .log()
            .qsos
            .iter()
            .map(|q| {
                let mut new = q.clone();
                new.nr_sent = if q.callsign == "JA1A" {
                    "10".into()
                } else {
                    "11".into()
                };
                (QsoId::of(q), new)
            })
            .collect();
        let zlog_ui::EntryAction::Edit { batch, done } = zlog_ui::EntryAction::update_qsos(list)
        else {
            unreachable!()
        };
        s.edit(batch, done);
        assert_eq!(s.status.text, trf("status.qsos_updated", &[&1]));
        assert_eq!(s.log().qsos[1].nr_sent, "11");
        assert!(!s.unsaved());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(feature = "undo")]
    #[test]
    fn a_change_of_many_is_undone_at_once_and_saved() {
        use zlog_undo::Refusal;
        let dir = scratch("undo_many");
        let history = zlog_undo::Shared::default();
        let reg = registry().with(zlog_undo::Undo(history.clone()));
        let mut s = Session::open(setup(&dir), reg).unwrap();
        let blocked = |redo| history.lock().unwrap().blocked(redo);
        assert_eq!(blocked(false), Some(Refusal::Empty));
        for call in ["JA1A", "JA1B", "JA1C"] {
            s.execute(Command::Insert(zlog_core::Qso {
                callsign: call.into(),
                ..zlog_core::Qso::default()
            }));
        }
        let ids: Vec<QsoId> = s.log().qsos.iter().map(QsoId::of).collect();
        assert_eq!(s.change_band(&ids, Band::B21), 3);
        assert_eq!(blocked(false), None);
        let batch = |redo| history.lock().unwrap().batch(redo).unwrap();
        s.edit(batch(false), Some("undone".into()));
        assert!(s.log().qsos.iter().all(|q| q.band != Band::B21));
        assert!(!s.unsaved());
        assert_eq!(s.status.text, "undone");
        let h = history.lock().unwrap().skippable();
        assert_eq!(h, None, "nothing another station changed");
        s.edit(batch(true), None);
        assert!(s.log().qsos.iter().all(|q| q.band == Band::B21));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_log_that_cannot_be_written_stays_unsaved_and_says_why() {
        let dir = scratch("unwritable");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        s.execute(Command::Insert(zlog_core::Qso {
            callsign: "JA1ZLO".into(),
            ..zlog_core::Qso::default()
        }));
        // A folder where the file should be: it cannot be written.
        let blocked = dir.join("blocked.zlox");
        std::fs::create_dir_all(&blocked).unwrap();
        let before = s.path.clone();
        assert!(s.try_save_as(blocked.clone()).is_err());
        assert_eq!(s.path, before, "still saved where it was");
        // Regression G8: the menu's Save As moved the log to the unwritable file.
        s.save_as(blocked.clone());
        assert_eq!(s.path, before, "menu Save As keeps the old file too");
        assert!(s.try_save_as(dir.join("elsewhere.zlox")).is_ok());
        assert!(!s.unsaved());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zlog-gui-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn qsos_from_a_link_are_saved() {
        use std::sync::{Arc, Mutex};
        use zlog_core::{Context, Extension, Link, LogEvent, Observer};
        type Inbox = Arc<Mutex<Vec<zlog_core::Qso>>>;
        struct Feed(Inbox);
        impl Observer for Feed {
            fn name(&self) -> &str {
                "feed"
            }
            fn on_event(&mut self, _: &mut Context<'_>, _: &LogEvent) {}
        }
        impl Link for Feed {
            fn poll(&mut self, cx: &mut Context<'_>) {
                for q in self.0.lock().unwrap().drain(..) {
                    cx.send(Command::Insert(q));
                }
            }
        }
        struct Net(Option<Inbox>);
        impl Extension for Net {
            fn name(&self) -> &str {
                "net"
            }
            fn links(&mut self) -> Vec<Box<dyn Link>> {
                self.0
                    .take()
                    .map(|i| Box::new(Feed(i)) as Box<dyn Link>)
                    .into_iter()
                    .collect()
            }
        }
        let dir = scratch("linked");
        let inbox = Inbox::default();
        let mut s = Session::open(setup(&dir), registry().with(Net(Some(inbox.clone())))).unwrap();
        let t0 = s.log().start_time().0;
        inbox.lock().unwrap().push(zlog_core::Qso {
            callsign: "JA2AAA".into(),
            time: DelphiTime(t0 + 0.1),
            ..zlog_core::Qso::default()
        });
        s.poll();
        assert!(!s.unsaved());
        let s2 = Session::open(setup(&dir), registry()).unwrap();
        assert_eq!(s2.log().qsos.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn close_saves_unsaved_changes() {
        let dir = scratch("close");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0;
        s.execute_unsaved(Command::Insert(zlog_core::Qso {
            callsign: "JA2AAA".into(),
            time: DelphiTime(t0 + 0.1),
            ..zlog_core::Qso::default()
        }));
        assert!(s.unsaved());
        s.close();
        let s2 = Session::open(setup(&dir), registry()).unwrap();
        assert_eq!(s2.log().qsos.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn change_mode_leaves_large_rsts() {
        let dir = scratch("bigrst");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0;
        s.entry.band = Band::B7;
        s.entry.mode = Mode::Ssb;
        s.entry.callsign = "JA2AAA".into();
        s.entry.nr_rcvd = "20H".into();
        assert!(s.log_entry(DelphiTime(t0 + 0.1)).is_ok());
        let id = QsoId::of(&s.log().qsos[0]);
        s.change_qsos(&[id], |q| (q.rst_sent, q.rst_rcvd) = (6553, 59));
        s.change_mode(&[id], Mode::Cw);
        assert_eq!(s.log().qsos[0].rst_sent, 6553);
        assert_eq!(s.log().qsos[0].rst_rcvd, 599);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn log_save_and_reopen() {
        let dir = scratch("reopen");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        // Times inside the contest period from the suggested start time.
        let t0 = s.log().start_time().0;
        s.entry.band = Band::B7;
        s.entry.mode = Mode::Cw;
        s.entry.callsign = "JA2AAA".into();
        s.entry.nr_rcvd = "20H".into();
        assert!(s.log_entry(DelphiTime(t0 + 0.1)).is_ok());
        assert_eq!(s.entry.callsign, "");
        assert_eq!(s.sheet().score, 1);

        s.entry.callsign = "JA2BBB".into();
        s.entry.nr_rcvd = "99H".into();
        assert_eq!(
            s.log_entry(DelphiTime(t0 + 0.2)),
            Err(NotLogged::InvalidNumber)
        );
        assert_eq!(s.status.text, tr("status.invalid_number"));

        let s2 = Session::open(setup(&dir), registry()).unwrap();
        assert_eq!(s2.log().qsos.len(), 1);
        assert_eq!(s2.log().qsos[0].nr_sent, "10H");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn default_start_time_follows_the_contest_start_hour() {
        let now = DelphiTime::from_ymd_hms(2026, 6, 6, 15, 42, 10);
        let at = |t: DelphiTime| t.to_ymd_hms();
        assert_eq!(
            at(default_start_time(now, Some(21))),
            (2026, 6, 6, 21, 0, 0)
        );
        // Start hour 0: midnight of the next day, also across a month end.
        assert_eq!(at(default_start_time(now, Some(0))), (2026, 6, 7, 0, 0, 0));
        let end = DelphiTime::from_ymd_hms(2026, 6, 30, 23, 59, 59);
        assert_eq!(at(default_start_time(end, Some(0))), (2026, 7, 1, 0, 0, 0));
        // None (zLog -1): the current hour.
        assert_eq!(at(default_start_time(now, None)), (2026, 6, 6, 15, 0, 0));
        let midnight = DelphiTime::from_ymd_hms(2026, 6, 6, 0, 0, 0);
        assert_eq!(
            at(default_start_time(midnight, None)),
            (2026, 6, 6, 0, 0, 0)
        );
    }

    #[test]
    fn new_log_gets_a_start_time_and_reopening_keeps_it() {
        let dir = scratch("start");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let start = s.log().start_time();
        assert_ne!(start.0, 0.0);
        // ALL JA starts at 21:00 (`FStartTime := 21`), 24 hours.
        assert_eq!(start.to_ymd_hms().3, 21);
        assert_eq!(s.period_hours(), 24);
        s.save();
        drop(s);
        let s2 = Session::open(setup(&dir), registry()).unwrap();
        assert_eq!(s2.log().start_time(), start);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn qso_outside_the_period_is_logged_invalid() {
        let dir = scratch("period");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0;
        for (call, t) in [("JA2AAA", t0 - 0.01), ("JA2BBB", t0 + 1.01)] {
            s.entry.callsign = call.into();
            s.entry.nr_rcvd = "20H".into();
            s.log_entry(DelphiTime(t)).unwrap();
        }
        assert!(s.log().qsos.iter().all(|q| q.invalid));
        assert_eq!(s.sheet().score, 0);
        s.use_contest_period = false;
        s.entry.callsign = "JA2CCC".into();
        s.entry.nr_rcvd = "20H".into();
        s.log_entry(DelphiTime(t0 - 0.01)).unwrap();
        assert!(!s.log().qsos[2].invalid);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Regression (ALL JA, "12H" scored 0): a new log starts at 21:00 today, so a
    /// QSO logged before then is out of the period and invalid. The operator is
    /// told, and correcting the start time with judging makes it count.
    #[test]
    fn qso_before_the_suggested_start_is_flagged_and_correcting_the_start_fixes_it() {
        let dir = scratch("before_start");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        assert!(s.asks_start_time);
        let start = s.start_time();
        assert_eq!(start.to_ymd_hms().3, 21);
        s.entry.band = Band::B7;
        s.entry.mode = Mode::Cw;
        s.entry.callsign = "JA1ABC".into();
        s.entry.nr_rcvd = "12H".into();
        s.log_entry(DelphiTime(start.0 - 6.0 / 24.0)).unwrap();
        assert!(s.log().qsos[0].invalid);
        assert_eq!(s.sheet().score, 0);
        assert!(s.status.warning);
        assert_eq!(
            s.status.text,
            trf("status.logged_out_of_period", &[&"JA1ABC"])
        );
        // Without judging only the start time changes.
        let earlier = DelphiTime(start.0 - 1.0);
        assert_eq!(s.set_start_time(earlier, false), 0);
        assert!(s.log().qsos[0].invalid);
        assert!(!s.asks_start_time);
        assert_eq!(s.set_start_time(earlier, true), 1);
        assert!(!s.log().qsos[0].invalid);
        assert_eq!(s.sheet().score, 1);
        // Kept in the file.
        let s2 = Session::open(setup(&dir), registry()).unwrap();
        assert_eq!(s2.start_time(), earlier);
        assert!(!s2.asks_start_time);
        assert!(!s2.log().qsos[0].invalid);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// End to end: new ALL JA log, start time in the past, JA1ABC 599 12H on 7 MHz
    /// CW scores 1 point and multiplier 12.
    #[test]
    fn all_ja_12h_scores_one_point_and_one_multiplier() {
        let dir = scratch("allja_12h");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let now = now_for(s.log());
        s.set_start_time(DelphiTime(now.0 - 1.0), true);
        s.entry.band = Band::B7;
        s.entry.mode = Mode::Cw;
        s.entry.rst_sent = "599".into();
        s.entry.rst_rcvd = "599".into();
        s.entry.callsign = "JA1ABC".into();
        s.entry.nr_rcvd = "12H".into();
        s.log_entry(now).unwrap();
        let q = &s.log().qsos[0];
        assert!(!q.invalid && !q.dupe);
        assert_eq!((q.multi1.as_str(), q.points, q.new_multi1), ("12", 1, true));
        assert_eq!(q.nr_sent, "10H");
        assert!(!s.status.warning, "{:?}", s.status);
        assert_eq!(s.sheet().score, 1);
        assert_eq!(s.sheet().total.multi, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn grid_menu_changes_band_mode_power_and_invalid() {
        let dir = scratch("grid_menu");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.start_time().0 + 0.1;
        s.entry.band = Band::B7;
        s.entry.mode = Mode::Cw;
        for (i, call) in ["JA2AAA", "JA2BBB"].into_iter().enumerate() {
            s.entry.callsign = call.into();
            s.entry.nr_rcvd = "20H".into();
            s.log_entry(DelphiTime(t0 + i as f64 * 0.01)).unwrap();
        }
        let ids: Vec<QsoId> = s.log().qsos.iter().map(QsoId::of).collect();
        assert_eq!(s.change_band(&ids, Band::B14), 2);
        assert!(s.log().qsos.iter().all(|q| q.band == Band::B14));
        assert_eq!(s.change_mode(&ids[..1], Mode::Ssb), 1);
        let q = &s.log().qsos[0];
        assert_eq!((q.mode, q.rst_sent, q.rst_rcvd), (Mode::Ssb, 59, 59));
        s.change_mode(&ids[..1], Mode::Cw);
        assert_eq!(s.log().qsos[0].rst_sent, 599);
        assert_eq!(s.change_power(&ids[1..], Power::P001), 1);
        assert_eq!(s.log().qsos[1].power, Power::P001);
        // Same value again: nothing changes.
        assert_eq!(s.change_power(&ids[1..], Power::P001), 0);
        let score = s.sheet().score;
        let toggle = |q: &mut Qso| q.invalid = !q.invalid;
        assert_eq!(s.change_qsos(&ids[..1], toggle), 1);
        assert!(s.log().qsos[0].invalid);
        assert_eq!(s.sheet().score, score - 1);
        s.change_qsos(&ids[..1], toggle);
        assert!(!s.log().qsos[0].invalid);
        // A deleted QSO is skipped.
        assert!(s.execute(Command::Delete(ids[0])));
        assert_eq!(s.change_band(&ids, Band::B21), 1);
        let s2 = Session::open(setup(&dir), registry()).unwrap();
        assert_eq!(s2.log().qsos[0].band, Band::B21);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A format reading one callsign per line, and an observer that refuses
    /// (by a runaway chain) every QSO with the callsign LOOP.
    struct Lines;

    impl zlog_core::LogFormat for Lines {
        fn name(&self) -> &str {
            "lines"
        }
        fn extensions(&self) -> &[&str] {
            &["txt"]
        }
        fn can_import(&self) -> bool {
            true
        }
        fn import(&self, bytes: &[u8]) -> Result<Vec<Qso>, String> {
            let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
            Ok(text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .enumerate()
                .map(|(i, l)| Qso {
                    callsign: l.trim().into(),
                    nr_rcvd: "20H".into(),
                    reserve3: (i as i32 + 1) * 100,
                    ..Qso::default()
                })
                .collect())
        }
    }

    struct Refuser;

    impl zlog_core::Observer for Refuser {
        fn name(&self) -> &str {
            "refuser"
        }
        fn on_event(&mut self, cx: &mut zlog_core::Context<'_>, event: &zlog_core::LogEvent) {
            if let zlog_core::LogEvent::Inserted { index } = event {
                if cx.log().qsos[*index].callsign == "LOOP" {
                    cx.send(Command::Insert(Qso {
                        callsign: "LOOP".into(),
                        ..Qso::default()
                    }));
                }
            }
        }
    }

    struct Testing;

    impl zlog_core::Extension for Testing {
        fn name(&self) -> &str {
            "testing"
        }
        fn formats(&self) -> Vec<Box<dyn LogFormat>> {
            vec![Box::new(Lines)]
        }
        fn observers(&mut self) -> Vec<Box<dyn zlog_core::Observer>> {
            vec![Box::new(Refuser)]
        }
    }

    fn import_registry() -> Registry {
        registry().with(Testing)
    }

    fn lines_format(s: &Session) -> usize {
        s.formats()
            .iter()
            .position(|f| f.name() == "lines")
            .unwrap()
    }

    #[test]
    fn import_keeps_and_saves_qsos_whose_chain_failed() {
        let dir = scratch("import-refused");
        let mut s = Session::open(setup(&dir), import_registry()).unwrap();
        let file = dir.join("in.txt");
        std::fs::write(&file, "JA2AAA\nLOOP\nJA3BBB\n").unwrap();
        s.import(lines_format(&s), &file);
        // LOOP is in the log although its cascade failed: it counts as added.
        assert!(s.status.warning, "{:?}", s.status);
        assert!(s.status.text.starts_with(&trf("status.imported", &[&3])));
        assert!(!s.unsaved());
        let s2 = Session::open(setup(&dir), registry()).unwrap();
        assert_eq!(s2.log().qsos.len(), s.log().qsos.len());
        assert!(s2.log().qsos.iter().any(|q| q.callsign == "LOOP"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn import_status_keeps_a_save_failure() {
        let dir = scratch("import-save-failed");
        let mut s = Session::open(setup(&dir), import_registry()).unwrap();
        let file = dir.join("in.txt");
        std::fs::write(&file, "JA2AAA\nLOOP\n").unwrap();
        s.path = dir.join("missing").join("test.zlox");
        s.import(lines_format(&s), &file);
        assert!(s.status.warning);
        let failed = trf("status.save_failed", &[&""]);
        assert!(s.status.text.contains(failed.trim_end()), "{:?}", s.status);
        assert!(s.unsaved());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn entry_whose_chain_failed_is_logged_once() {
        let dir = scratch("entry-chain-failed");
        let mut s = Session::open(setup(&dir), import_registry()).unwrap();
        let t0 = s.log().start_time().0;
        s.entry.callsign = "LOOP".into();
        s.entry.nr_rcvd = "20H".into();
        assert!(s.log_entry_on(DelphiTime(t0 + 0.1), None).is_ok());
        assert!(s.status.warning, "the failure is reported");
        assert!(
            s.entry.callsign.is_empty(),
            "Enter again would log it twice"
        );
        assert!(!s.unsaved());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(feature = "undo")]
    #[test]
    fn an_import_is_one_step() {
        let dir = scratch("import-step");
        let history = zlog_undo::Shared::default();
        let reg = import_registry().with(zlog_undo::Undo(history.clone()));
        let mut s = Session::open(setup(&dir), reg).unwrap();
        let file = dir.join("in.txt");
        std::fs::write(&file, "JA2AAA\nJA3BBB\nJA4CCC\n").unwrap();
        s.import(lines_format(&s), &file);
        let h = history.lock().unwrap();
        assert_eq!(h.steps(), (1, 0));
        assert_eq!(h.next(false).unwrap().tag, Some(zlog_core::IMPORT));
        assert_eq!(h.next(false).unwrap().changes.len(), 3);
        drop(h);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn import_without_refusals_is_info() {
        let dir = scratch("import-ok");
        let mut s = Session::open(setup(&dir), import_registry()).unwrap();
        let file = dir.join("in.txt");
        std::fs::write(&file, "JA2AAA\n\nJA3BBB\n").unwrap();
        s.import(lines_format(&s), &file);
        assert_eq!(s.status, Status::info(trf("status.imported", &[&2])));
        assert_eq!(s.log().qsos.len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn import_saves_only_when_something_was_added() {
        let dir = scratch("import-none");
        let mut s = Session::open(setup(&dir), import_registry()).unwrap();
        let file = dir.join("in.txt");
        // An empty file adds nothing and saves nothing.
        std::fs::write(&file, "").unwrap();
        s.import(lines_format(&s), &file);
        assert_eq!(s.status, Status::info(trf("status.imported", &[&0])));
        assert!(!s.path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn import_of_a_missing_file_fails() {
        let dir = scratch("import-missing");
        let mut s = Session::open(setup(&dir), import_registry()).unwrap();
        s.import(lines_format(&s), &dir.join("absent.txt"));
        assert!(s.status.warning);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rig_frequency_is_recorded_in_khz() {
        let dir = scratch("freq");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0;
        let log = |s: &mut Session, call: &str, hz: Option<u64>| {
            s.entry.callsign = call.into();
            s.entry.nr_rcvd = "20H".into();
            s.log_entry_on(DelphiTime(t0 + 0.1), hz).unwrap();
            s.log().qsos.last().unwrap().freq.clone()
        };
        assert!(s.record_freq, "RecordFreqInMemo defaults to on");
        assert_eq!(log(&mut s, "JA2AAA", Some(7_012_345)), "7012.3");
        assert_eq!(log(&mut s, "JA2BBB", None), "");
        assert_eq!(
            log(&mut s, "JA2CCC", Some(0)),
            "",
            "no frequency from the rig"
        );
        s.record_freq = false;
        assert_eq!(log(&mut s, "JA2DDD", Some(7_012_345)), "");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn khz_string_as_zlog() {
        assert_eq!(khz_string(7_012_345), "7012.3");
        assert_eq!(khz_string(14_000_000), "14000.0");
        assert_eq!(khz_string(999), "0.9");
        assert_eq!(khz_string(10_489_550_000), "10489550.0");
    }

    #[cfg(feature = "rule-dx")]
    #[test]
    fn pedition_reaches_dv() {
        let dir = scratch("pedi");
        let reg = Registry::new().with(zlog_rule_dx::World);
        let index = reg
            .contests()
            .iter()
            .position(|c| c.build(ContestMode::Mix).name().contains("Pedition"))
            .expect("Pedition contest");
        let mut st = setup(&dir);
        st.contest = index;
        let mut s = Session::open(st, reg).unwrap();
        s.entry.band = Band::B7;
        s.entry.mode = Mode::Ft8;
        s.cycle_mode(false);
        assert_eq!(s.entry.mode, Mode::Dv);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn qsos_outside_the_period_are_told_to_the_panels() {
        use zlog_ui::PeriodCheck;
        let dir = scratch("edit-period");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0;
        let mut q = Qso {
            time: DelphiTime(t0 - 0.5),
            callsign: "JA2AAA".into(),
            band: Band::B7,
            ..Qso::default()
        };
        assert!(s.outside(&q));
        q.time = DelphiTime(t0 + 0.1);
        assert!(!s.outside(&q));
        s.use_contest_period = false;
        q.time = DelphiTime(t0 - 0.5);
        assert!(!s.outside(&q));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn open_takes_the_contest_period_setting() {
        let dir = scratch("period-setting");
        let mut st = setup(&dir);
        st.use_contest_period = false;
        let s = Session::open(st, registry()).unwrap();
        assert!(!s.use_contest_period);
        assert_eq!(s.period_hours(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn one_qso_log(mode: Mode) -> Log {
        let mut log = Log::default();
        log.qsos.push(Qso {
            callsign: "JA2AAA".into(),
            nr_rcvd: "20H".into(),
            band: Band::B7,
            mode,
            ..Qso::default()
        });
        log
    }

    #[test]
    fn out_of_range_bytes_are_reported() {
        let dir = scratch("bad-bytes");
        let st = setup(&dir);
        let mut bytes = one_qso_log(Mode::Cw).to_bytes(Format::Zlox);
        bytes[zlog_core::zlo::ZLOX_RECORD + 92] = 200;
        std::fs::write(&st.path, bytes).unwrap();
        let s = Session::open(st, registry()).unwrap();
        assert_eq!(s.status, Status::warning(trf("status.bad_bytes", &[&1])));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_other_is_asked_for_zlo_only_and_converted_to_zlox() {
        let dir = scratch("old-other");
        let zlo = dir.join("old.zlo");
        one_qso_log(Mode::Ft4).save(&zlo, Format::Zlo).unwrap();
        assert!(asks_old_other(&zlo));
        let zlox = dir.join("new.zlox");
        one_qso_log(Mode::Ft4).save(&zlox, Format::Zlox).unwrap();
        assert!(!asks_old_other(&zlox), "only .ZLO files are asked about");
        one_qso_log(Mode::Cw)
            .save(dir.join("cw.zlo"), Format::Zlo)
            .unwrap();
        assert!(!asks_old_other(&dir.join("cw.zlo")));
        assert!(!asks_old_other(&dir.join("absent.zlo")));

        // No: the file stays as it is.
        let mut st = setup(&dir);
        st.path = zlo.clone();
        let s = Session::open(st.clone(), registry()).unwrap();
        assert_eq!((s.log().qsos[0].mode, s.format), (Mode::Ft4, Format::Zlo));
        drop(s);
        // Yes: Other, continued and saved as .ZLOX.
        st.convert_old_other = true;
        let s = Session::open(st, registry()).unwrap();
        assert_eq!(s.log().qsos[0].mode, Mode::Other);
        assert_eq!(
            (s.path.clone(), s.format),
            (dir.join("old.zlox"), Format::Zlox)
        );
        assert!(s.path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn utc_hhmm_from_the_log_zone() {
        let t = DelphiTime::from_ymd_hms(2026, 6, 6, 9, 5, 30);
        assert_eq!(utc_hhmm(t, None), "0905");
        assert_eq!(utc_hhmm(t, Some(540)), "0005");
        assert_eq!(utc_hhmm(t, Some(600)), "2305");
    }

    #[test]
    fn time_replaces_d_in_place() {
        assert_eq!(with_time("10$D", "1307"), "101307");
        assert_eq!(with_time("$DM", "1307"), "1307M");
        assert_eq!(with_time("001", "1307"), "0011307");
    }

    #[cfg(feature = "rule-dx")]
    #[test]
    fn bartg_sends_serial_and_utc_time() {
        let dir = scratch("bartg");
        let reg = Registry::new().with(zlog_rule_dx::World);
        let index = reg
            .contests()
            .iter()
            .position(|c| c.build(ContestMode::Rtty).name() == "BARTG HF RTTY")
            .expect("BARTG");
        let mut st = setup(&dir);
        st.contest = index;
        st.mode = ContestMode::Rtty;
        let mut s = Session::open(st, reg).unwrap();
        s.entry.band = Band::B14;
        s.entry.mode = Mode::Rtty;
        s.entry.callsign = "G3AAA".into();
        s.entry.nr_rcvd = "001".into();
        let t = DelphiTime::from_ymd_hms(2026, 3, 21, 13, 7, 0);
        s.log_entry(t).unwrap();
        let q = &s.log().qsos[0];
        assert_eq!(q.nr_sent, "0011307");
        assert_eq!(q.serial, 1, "the serial is kept (main.pas Q.Serial)");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Regression: the dialogs kept a row index while Z-Link deleted rows under them,
    /// so OK overwrote or deleted the wrong QSO, or panicked past the end.
    #[test]
    fn dialogs_act_on_the_qso_not_the_row() {
        let dir = scratch("by_id");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0;
        for (i, call) in ["JA1AAA", "JA2BBB"].into_iter().enumerate() {
            s.entry.callsign = call.into();
            s.entry.nr_rcvd = "20H".into();
            s.log_entry(DelphiTime(t0 + 0.1 * (i + 1) as f64)).unwrap();
        }
        // The edit dialog opens on row 1 (JA2BBB), the delete confirmation on row 0.
        let edit = QsoId::of(&s.log().qsos[1]);
        let mut edited = s.log().qsos[1].clone();
        edited.memo = "QSB".into();
        let gone = QsoId::of(&s.log().qsos[0]);
        // Row 0 is deleted elsewhere meanwhile (as Z-Link's poll would).
        assert!(s.execute(Command::Delete(gone)));
        let batch = |c: Command| Batch {
            commands: vec![c],
            ..Batch::default()
        };
        s.edit(batch(Command::Update(edit, edited)), None);
        assert_eq!(s.log().qsos.len(), 1);
        assert_eq!(s.log().qsos[0].callsign, "JA2BBB");
        assert_eq!(s.log().qsos[0].memo, "QSB");
        // OK on the vanished QSO does nothing and says so.
        s.edit(batch(Command::Delete(gone)), None);
        assert!(s.status.warning);
        assert_eq!(s.log().qsos.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dupe_is_reported_on_space() {
        let dir = scratch("dupe");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0;
        s.entry.callsign = "JA2AAA".into();
        s.entry.nr_rcvd = "20H".into();
        s.log_entry(DelphiTime(t0 + 0.1)).unwrap();
        s.entry.callsign = "ja2aaa".into();
        assert!(s.check_call());
        assert!(s.status.warning);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn log_at(s: &mut Session, call: &str, band: Band, t: f64) {
        s.entry.callsign = call.into();
        s.entry.nr_rcvd = "20H".into();
        s.entry.band = band;
        s.log_entry(DelphiTime(t)).unwrap();
    }

    /// Regression C1: logging never set the QSY violation.
    #[test]
    fn logging_sets_the_qsy_violation() {
        let dir = scratch("qsy");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0 + 0.1;
        let min = 1.0 / 1440.0;
        log_at(&mut s, "JA2AAA", Band::B7, t0);
        log_at(&mut s, "JA2BBB", Band::B14, t0 + min);
        assert!(!s.log().qsos[1].qsy_violation, "rules are off by default");
        s.qsy_rule.countdown_minutes = Some(10);
        log_at(&mut s, "JA2CCC", Band::B21, t0 + 2.0 * min);
        assert!(s.log().qsos[2].qsy_violation);
        log_at(&mut s, "JA2DDD", Band::B21, t0 + 30.0 * min);
        assert!(!s.log().qsos[3].qsy_violation);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Regression C6: the PC name was not stored in logged QSOs.
    #[test]
    fn logging_stores_the_pc_name() {
        let dir = scratch("pcname");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let t0 = s.log().start_time().0 + 0.1;
        log_at(&mut s, "JA2AAA", Band::B7, t0);
        assert_eq!(s.log().qsos[0].pc_name, "");
        s.pc_name = "PC1".into();
        log_at(&mut s, "JA2BBB", Band::B7, t0);
        assert_eq!(s.log().qsos[1].pc_name, "PC1");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preferences_reach_the_log_and_the_saved_file() {
        let dir = scratch("prefs");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        let mut p = Preferences {
            qsl_default: QslState::PseQsl,
            pc_name: "PC9".into(),
            ..Preferences::default()
        };
        p.set_countdown(true);
        s.apply_preferences(&p);
        assert_eq!(s.qsy_rule.countdown_minutes, Some(10));
        assert_eq!(s.log().qsy_rule, s.qsy_rule);
        let t0 = s.log().start_time().0 + 0.1;
        log_at(&mut s, "JA2AAA", Band::B7, t0);
        assert_eq!(s.log().qsos[0].pc_name, "PC9");
        // QslDefault goes into the memo of .ZLO files.
        let path = dir.join("prefs.zlo");
        s.save_as(path.clone());
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, s.log().to_bytes_with(Format::Zlo, QslState::PseQsl));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A contest file chosen in the zLog Menu is opened through the registry; a DAT
    /// it names but that is missing is reported, and the contest still opens.
    #[cfg(feature = "rule-ja")]
    #[test]
    fn a_contest_file_opens_with_a_warning_for_a_missing_dat() {
        let dir = scratch("contest-file");
        let cfg = dir.join("MY.CFG");
        std::fs::write(&cfg, "#My Contest\ndat my.dat\n").unwrap();
        let st = Setup {
            contest_file: Some(cfg),
            ..setup(&dir)
        };
        let s = Session::open(
            st,
            Registry::new()
                .with(zlog_cfg::Cfg)
                .with(zlog_rule_ja::Japan),
        )
        .unwrap();
        assert_eq!(s.rules().name(), "My Contest");
        assert!(s.status.text.contains("my.dat"), "{}", s.status.text);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A dupe in a contest with a rule hook (aktest) is refused as an invalid
    /// number, as zLog sets the dupe flag before `ValidMulti`; a plain CFG
    /// contest still logs the dupe, marked.
    #[cfg(feature = "rule-ja")]
    #[test]
    fn a_dupe_is_an_invalid_number_where_the_rule_hook_says_so() {
        let dir = scratch("dupe-hook");
        let plain = dir.join("PLAIN.CFG");
        std::fs::write(&plain, "#Plain Contest\nundefmulti on\n").unwrap();
        let aktest = registry()
            .contests()
            .iter()
            .position(|c| c.id() == zlog_rule_ja::asahikawa::ID)
            .unwrap();
        for (contest, file, refused) in [(aktest, None, true), (0, Some(plain), false)] {
            let _ = std::fs::remove_file(dir.join("test.zlox"));
            let st = Setup {
                contest,
                contest_file: file,
                ..setup(&dir)
            };
            let mut s = Session::open(st, registry()).unwrap();
            let t0 = s.log().start_time().0;
            let log = |s: &mut Session| {
                s.entry.callsign = "JA1AAA".into();
                s.entry.band = Band::B7;
                s.entry.mode = Mode::Cw;
                s.entry.nr_rcvd = "0102".into();
                s.log_entry_on(DelphiTime(t0 + 0.1), None)
            };
            assert_eq!(log(&mut s), Ok(()), "{}", s.rules().name());
            if refused {
                assert_eq!(log(&mut s), Err(NotLogged::InvalidNumber));
                assert_eq!(s.log().qsos.len(), 1);
            } else {
                assert_eq!(log(&mut s), Ok(()));
                assert!(s.log().qsos[1].dupe);
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A CFG contest with `USEPERIOD ON` and `PERIOD 0`: the period runs from the
    /// start time until now (`TLog.GetEndTime`).
    #[cfg(feature = "rule-ja")]
    #[test]
    fn a_used_period_of_length_zero_ends_now() {
        struct OpenPeriod;
        impl zlog_core::Extension for OpenPeriod {
            fn name(&self) -> &str {
                "open-period"
            }
            fn contests(&self) -> Vec<Box<dyn zlog_core::ContestDef>> {
                let cfg = "#Open Period\nuseperiod on\nperiod 0\n";
                vec![Box::new(zlog_cfg::from_cfg(cfg, |_| None))]
            }
        }
        let dir = scratch("open-period");
        let s = Session::open(setup(&dir), Registry::new().with(OpenPeriod)).unwrap();
        assert!(s.rules().uses_period());
        assert_eq!(s.period_hours(), 0);
        let start = s.log().start_time().0;
        assert!(start > 0.0, "a used period suggests a start time");
        let at = |t: f64| Qso {
            time: DelphiTime(t),
            ..Qso::default()
        };
        assert!(!s.out_of_period(&at(start)));
        assert!(s.out_of_period(&at(start - 1.0 / 24.0)), "before the start");
        assert!(s.out_of_period(&at(now_for(s.log()).0 + 1.0)), "after now");
        assert!(zlog_ui::PeriodCheck::outside(&s, &at(start - 1.0)));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
