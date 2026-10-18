//! The logbook: the one place where QSOs are inserted, updated and deleted.
//!
//! Every change, whether typed by the operator or requested by an extension,
//! is a [`Command`] passed to [`Book::execute`]. The book applies it, rescores,
//! and delivers one [`LogEvent`] to every extension. Extensions may answer with
//! more commands; these are queued and run afterwards, never inside the handler,
//! so a handler always sees a consistent log and can never re-enter the book.
//!
//! A chain of commands that does not settle within [`MAX_CASCADE`] rounds is stopped
//! and reported with the name of the extension that kept it going.

use std::collections::VecDeque;

use crate::contest::{rescore, Contest, ScoreSheet};
use crate::extension::{Context, LogEvent, Origin, Registry};
use crate::log::Log;
use crate::qso::Qso;

/// How many follow-up rounds one command may cause before the book gives up.
pub const MAX_CASCADE: usize = 32;

/// A requested change to the log. QSOs are named by [`QsoId`], not by position.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Add a QSO at the end. A missing or taken ID is replaced with a fresh one.
    Insert(Qso),
    /// Replace the QSO with this ID; its edit counter is advanced.
    Update(QsoId, Qso),
    Delete(QsoId),
}

/// Identity of a QSO: zLog's `Reserve3` without the edit counter (`TTSSSSRR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QsoId(pub i32);

impl QsoId {
    pub fn of(q: &Qso) -> Self {
        QsoId(q.reserve3 / 100)
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BookError {
    #[error("no QSO with ID {0:?}")]
    NotFound(QsoId),
    #[error("scoring failed: {0}")]
    Scoring(String),
    #[error("changes did not settle after {MAX_CASCADE} rounds; last command came from {0}")]
    Cascade(String),
}

pub struct Book {
    log: Log,
    contest: Box<dyn Contest>,
    registry: Registry,
    sheet: ScoreSheet,
    next_serial: i32,
}

impl Book {
    /// Open a log under a contest and tell the extensions.
    pub fn open(
        log: Log,
        contest: Box<dyn Contest>,
        contest_id: &str,
        registry: Registry,
    ) -> (Self, Vec<String>) {
        let next_serial = log
            .qsos
            .iter()
            .map(|q| q.reserve3 / 10000 % 10000)
            .max()
            .unwrap_or(0)
            + 1;
        let mut book = Book {
            log,
            contest,
            registry,
            sheet: ScoreSheet::default(),
            next_serial,
        };
        let dropped = book.assign_missing_ids();
        // Scores stored in the file are never trusted: score the whole log first.
        let mut messages = Vec::new();
        if dropped > 0 {
            messages.push(format!("{dropped} QSOs saved twice were read once"));
        }
        match rescore(&mut book.log, book.contest.as_mut()) {
            Ok(sheet) => book.sheet = sheet,
            Err(e) => messages.push(BookError::Scoring(e).to_string()),
        }
        let (commands, said) = book.dispatch(
            &LogEvent::Opened {
                contest: contest_id.into(),
            },
            Origin::User,
        );
        messages.extend(said);
        // Commands sent while opening are run like any other.
        if let Ok(more) = book.run(commands) {
            messages.extend(more);
        }
        (book, messages)
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    /// Header fields (UTC offset, coefficient) are settings, not QSOs, so they change directly.
    pub fn header_mut(&mut self) -> &mut Qso {
        &mut self.log.header
    }

    /// The extensions this book delivers events to.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn contest(&self) -> &dyn Contest {
        self.contest.as_ref()
    }

    pub fn sheet(&self) -> &ScoreSheet {
        &self.sheet
    }

    pub fn find(&self, id: QsoId) -> Option<usize> {
        self.log.qsos.iter().position(|q| QsoId::of(q) == id)
    }

    /// Run a command from the operator; returns the extensions' status messages.
    pub fn execute(&mut self, command: Command) -> Result<Vec<String>, BookError> {
        self.run(vec![(Origin::User, command)])
    }

    /// Let extensions that wait on something outside (a network, a timer) send commands.
    pub fn poll(&mut self) -> Result<Vec<String>, BookError> {
        let mut cx = Context::new(&self.log, Origin::User);
        self.registry.poll(&mut cx);
        let (commands, mut messages) = (cx.commands, cx.messages);
        messages.extend(self.run(commands)?);
        Ok(messages)
    }

    pub fn close(&mut self) -> Vec<String> {
        self.dispatch(&LogEvent::Closed, Origin::User).1
    }

    /// Deliver one event to every extension; returns what they sent back.
    fn dispatch(
        &mut self,
        event: &LogEvent,
        origin: Origin,
    ) -> (Vec<(Origin, Command)>, Vec<String>) {
        let mut cx = Context::new(&self.log, origin);
        self.registry.dispatch(&mut cx, event);
        (cx.commands, cx.messages)
    }

    fn run(&mut self, first: Vec<(Origin, Command)>) -> Result<Vec<String>, BookError> {
        let mut queue: VecDeque<(Origin, Command)> = first.into();
        let mut messages = Vec::new();
        let mut rounds = 0;
        while let Some((origin, command)) = queue.pop_front() {
            if origin != Origin::User {
                rounds += 1;
                if rounds > MAX_CASCADE {
                    return Err(BookError::Cascade(
                        self.registry.name_of(origin).to_string(),
                    ));
                }
            }
            let event = self.apply(command)?;
            self.sheet =
                rescore(&mut self.log, self.contest.as_mut()).map_err(BookError::Scoring)?;
            let (commands, said) = self.dispatch(&event, origin);
            messages.extend(said);
            queue.extend(commands);
        }
        Ok(messages)
    }

    fn apply(&mut self, command: Command) -> Result<LogEvent, BookError> {
        match command {
            Command::Insert(mut qso) => {
                let taken = qso.reserve3 == 0 || self.find(QsoId::of(&qso)).is_some();
                if taken {
                    qso.reserve3 = self.fresh_id(&qso);
                }
                self.log.qsos.push(qso);
                let index = self.log.qsos.len() - 1;
                Ok(LogEvent::Inserted { index })
            }
            Command::Update(id, mut qso) => {
                let index = self.find(id).ok_or(BookError::NotFound(id))?;
                let old = self.log.qsos[index].reserve3;
                // The edit counter stops at 99, as in zLog (`IncEditCounter`).
                qso.reserve3 = if old % 100 < 99 { old + 1 } else { old };
                let before = std::mem::replace(&mut self.log.qsos[index], qso);
                Ok(LogEvent::Updated { index, before })
            }
            Command::Delete(id) => {
                let index = self.find(id).ok_or(BookError::NotFound(id))?;
                let qso = self.log.qsos.remove(index);
                Ok(LogEvent::Deleted { index, qso })
            }
        }
    }

    /// Give an ID to every QSO without one, or whose ID an earlier QSO already has
    /// (files from old versions store 0 everywhere), as zLog does on load.
    ///
    /// A record with the ID and callsign of an earlier one is the same QSO saved twice
    /// and is dropped, keeping the first, as zLog does (`IsContainsSameQSO`). Returns
    /// how many were dropped.
    fn assign_missing_ids(&mut self) -> usize {
        let mut seen: std::collections::HashMap<QsoId, String> = std::collections::HashMap::new();
        let qsos = std::mem::take(&mut self.log.qsos);
        let mut dropped = 0;
        for mut q in qsos {
            let id = QsoId::of(&q);
            if q.reserve3 != 0 {
                match seen.get(&id) {
                    Some(call) if *call == q.callsign => {
                        dropped += 1;
                        continue;
                    }
                    Some(_) => {}
                    None => {
                        seen.insert(id, q.callsign.clone());
                        self.log.qsos.push(q);
                        continue;
                    }
                }
            }
            // No ID, or the ID of a different QSO: give it a new one.
            q.reserve3 = self.fresh_id(&q);
            seen.insert(QsoId::of(&q), q.callsign.clone());
            self.log.qsos.push(q);
        }
        dropped
    }

    /// `TTSSSSRR00`: TX number (at most 15, `MAX_TX - 1`), serial counter, a two-digit
    /// salt, edit counter 0.
    fn fresh_id(&mut self, q: &Qso) -> i32 {
        loop {
            let serial = self.next_serial % 10000;
            self.next_serial += 1;
            let salt = (q.time.0.fract() * 1e6) as i32 % 100;
            let id = (q.tx as i32).min(15) * 100_000_000 + serial * 10_000 + salt * 100;
            if self.find(QsoId(id / 100)).is_none() {
                return id;
            }
        }
    }
}
