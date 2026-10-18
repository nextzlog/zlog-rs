//! One open contest as the window sees it: the book, the station and the entry row.
//!
//! Changes to QSOs go through [`Book::execute`]; this module only turns operator
//! actions into commands, saves after each change and puts messages in the status bar.

use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};

use zlog_core::{
    Band, Book, Command, Contest, ContestMode, DelphiTime, Format, Log, LogFormat, MultiCheck, Qso,
    QsoId, Registry, ScoreSheet, Station,
};

use crate::entry::Entry;

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

/// What the operator chose in the zLog Menu.
#[derive(Debug, Clone)]
pub struct Setup {
    /// Index into `Registry::contests()`.
    pub contest: usize,
    pub mode: ContestMode,
    pub station: Station,
    pub coeff: f64,
    pub path: PathBuf,
}

/// Why Enter did not log the entry row; the window moves the cursor accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotLogged {
    NoCallsign,
    InvalidNumber,
    /// The book refused the change (the status bar says why).
    Refused,
}

pub struct Session {
    book: Book,
    pub station: Station,
    pub path: PathBuf,
    pub format: Format,
    pub entry: Entry,
    pub status: Status,
}

fn format_of(path: &Path) -> Format {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("zlo") => Format::Zlo,
        _ => Format::Zlox,
    }
}

impl Session {
    /// Open the file named in the setup, or start a new one if it does not exist.
    pub fn open(setup: Setup, registry: Registry) -> Result<Self, String> {
        let def = registry
            .contests()
            .get(setup.contest)
            .ok_or(tr("error.no_contest"))?;
        let (id, uses_coeff) = (def.id().to_string(), def.uses_coeff());
        let mut dropped = 0;
        let mut log = if setup.path.exists() {
            let bytes =
                std::fs::read(&setup.path).map_err(|e| format!("{}: {e}", setup.path.display()))?;
            dropped = zlog_core::zlo::trailing_bytes(&bytes);
            Log::from_bytes(&bytes)
                .map_err(|e| format!("{}: {e}", setup.path.display()))?
                .0
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
        if uses_coeff {
            log.set_score_coeff(setup.coeff);
        }
        let (low, high) = rules.band_range();
        let first = if (low..=high).contains(&Band::B7) {
            Band::B7
        } else {
            low
        };
        let band = log.qsos.last().map_or(first, |q| q.band);
        let mode = log.qsos.last().map_or(default_mode(setup.mode), |q| q.mode);
        let (book, messages) = Book::open(log, rules, &id, registry);
        let power = setup.station.power_for(band);
        let mut session = Session {
            book,
            station: setup.station,
            format: format_of(&setup.path),
            path: setup.path,
            entry: Entry::new(band, mode, power),
            status: Status::default(),
        };
        session.report(Ok(messages));
        if dropped > 0 {
            session.status = Status::warning(trf("status.truncated", &[&dropped]));
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

    pub fn formats(&self) -> &[Box<dyn LogFormat>] {
        self.book.registry().formats()
    }

    /// Run a command, save, and show what the extensions said.
    fn execute(&mut self, command: Command) -> bool {
        let result = self.book.execute(command);
        let ok = result.is_ok();
        self.save();
        self.report(result.map_err(|e| e.to_string()));
        ok
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
        let result = self.book.poll();
        if matches!(&result, Ok(m) if !m.is_empty()) || result.is_err() {
            self.save();
        }
        self.report(result.map_err(|e| e.to_string()));
    }

    pub fn save(&mut self) {
        if let Err(e) = self.book.log().save(&self.path, self.format) {
            self.status = Status::warning(trf("status.save_failed", &[&e]));
        }
    }

    pub fn save_as(&mut self, path: PathBuf) {
        self.format = format_of(&path);
        self.path = path;
        self.save();
    }

    /// Add the QSOs of a file in a format provided by an extension.
    pub fn import(&mut self, format: usize, path: &Path) {
        let result = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| self.formats()[format].import(&bytes));
        match result {
            Ok(qsos) => {
                let n = qsos.len();
                for q in qsos {
                    self.execute(Command::Insert(q));
                }
                self.status = Status::info(trf("status.imported", &[&n]));
            }
            Err(e) => self.status = Status::warning(trf("status.import_failed", &[&e])),
        }
    }

    pub fn export(&mut self, format: usize, path: &Path) {
        let result = self.formats()[format]
            .export(self.book.log())
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
        let mode = self.book.contest().contest_mode();
        self.entry.cycle_mode(mode, back);
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
    pub fn log_entry(&mut self, time: DelphiTime) -> Result<(), NotLogged> {
        if self.entry.callsign.trim().is_empty() {
            self.status = Status::warning(tr("status.no_callsign"));
            return Err(NotLogged::NoCallsign);
        }
        let qso = self.entry.to_qso(self.rules(), &self.station, time, 0);
        if !self.rules().valid_number(&qso) {
            self.status = Status::warning(tr("status.invalid_number"));
            return Err(NotLogged::InvalidNumber);
        }
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
            self.status = if q.dupe {
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

    pub fn replace(&mut self, index: usize, qso: Qso) {
        let id = QsoId::of(&self.log().qsos[index]);
        self.execute(Command::Update(id, qso));
    }

    pub fn delete(&mut self, index: usize) {
        let call = self.log().qsos[index].callsign.clone();
        let id = QsoId::of(&self.log().qsos[index]);
        self.status = Status::info(trf("status.deleted", &[&call]));
        self.execute(Command::Delete(id));
    }

    pub fn close(&mut self) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Mode;

    fn registry() -> Registry {
        Registry::new()
            .with(zlog_jarl::Jarl)
            .with(zlog_prefix::PrefixExtension)
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
            coeff: 1.0,
            path: dir.join("test.zlox"),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zlog-gui-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn log_save_and_reopen() {
        let dir = scratch("reopen");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        s.entry.band = Band::B7;
        s.entry.mode = Mode::Cw;
        s.entry.callsign = "JA2AAA".into();
        s.entry.nr_rcvd = "20H".into();
        assert!(s.log_entry(DelphiTime(46000.5)).is_ok());
        assert_eq!(s.entry.callsign, "");
        assert_eq!(s.sheet().score, 1);

        s.entry.callsign = "JA2BBB".into();
        s.entry.nr_rcvd = "99H".into();
        assert_eq!(
            s.log_entry(DelphiTime(46000.6)),
            Err(NotLogged::InvalidNumber)
        );
        assert_eq!(s.status.text, tr("status.invalid_number"));

        let s2 = Session::open(setup(&dir), registry()).unwrap();
        assert_eq!(s2.log().qsos.len(), 1);
        assert_eq!(s2.log().qsos[0].nr_sent, "10H");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dupe_is_reported_on_space() {
        let dir = scratch("dupe");
        let mut s = Session::open(setup(&dir), registry()).unwrap();
        s.entry.callsign = "JA2AAA".into();
        s.entry.nr_rcvd = "20H".into();
        s.log_entry(DelphiTime(46000.5)).unwrap();
        s.entry.callsign = "ja2aaa".into();
        assert!(s.check_call());
        assert!(s.status.warning);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
