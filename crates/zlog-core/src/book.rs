//! The logbook: the one place where QSOs are inserted, updated and deleted.
//!
//! Every change, whether typed by the operator or requested by an extension,
//! is a [`Command`] passed to [`Book::execute`]. The book applies it, rescores,
//! and delivers one [`LogEvent`] to every extension. Extensions may answer with
//! more commands; these are queued and run afterwards, never inside the handler,
//! so a handler always sees a consistent log and can never re-enter the book.
//!
//! A chain of commands that does not settle within [`MAX_CASCADE`] rounds is stopped
//! and reported with the name of the extension that kept it going. Only commands sent
//! in reaction to an event count: the operator's command and the commands a link sends
//! from [`Link::poll`](crate::Link::poll) (a whole log downloaded from a server, say)
//! start a chain but are not part of it, however many there are.

use std::collections::VecDeque;

use crate::contest::{rescore, Contest, ScoreSheet};
use crate::extension::{Context, LogEvent, Origin, Registry};
use crate::log::{Log, QsyRule};
use crate::qso::Qso;
use crate::step::{Batch, BatchError, Step};

/// How many commands sent in reaction to events one command from the operator or from
/// [`Link::poll`](crate::Link::poll) may cause before the book gives up.
pub const MAX_CASCADE: usize = 32;

/// A requested change to the log. QSOs are named by [`QsoId`], not by position.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Add a QSO at the end. A missing or taken ID is replaced with a fresh one.
    Insert(Qso),
    /// Add a QSO just before the one with this ID (zLog's `TLog.ProcessInsert` for
    /// `INSQSO`), or at the end when there is none; zLog drops it then. The ID is
    /// handled as by [`Command::Insert`].
    InsertBefore(QsoId, Qso),
    /// Replace the QSO with this ID.
    ///
    /// The stored edit counter is advanced (zLog's `IncEditCounter`), with two
    /// exceptions for a QSO that carries this same ID:
    ///
    /// - sent by a link from [`Link::poll`](crate::Link::poll) (an edit received from
    ///   another station), it keeps its `Reserve3` exactly, even when equal to the
    ///   stored one, as zLog's `TLog.ProcessEdit` (`Assign`) does;
    /// - sent by an observer or link in reaction to an event, it keeps its
    ///   `Reserve3` only when the edit counter differs; an observer that edits a copy
    ///   of the stored QSO (same `Reserve3`) still counts as one edit.
    ///
    /// No QSO with this ID is an error from the operator ([`BookError::NotFound`]);
    /// from an extension the command is ignored, with no event (zLog `ProcessEdit`).
    Update(QsoId, Qso),
    /// Remove the QSO with this ID; an unknown ID is handled as by [`Command::Update`]
    /// (zLog `ProcessDelete`).
    Delete(QsoId),
    /// Sort the log by QSO time, keeping the order of QSOs with the same time, and
    /// rescore (zLog `Log.SortBy(soTime)`). Delivers [`LogEvent::Sorted`]: indices
    /// held from earlier events are void afterwards, so observers that keep them
    /// rebuild from [`Context::log`](crate::Context::log) as on [`LogEvent::Opened`].
    /// IDs do not change.
    SortByTime,
    /// Score the whole log again without changing a QSO (a contest whose answers
    /// came late, such as a plugin's rule). Delivers no event; it counts as a
    /// change in [`Book::changes`] only when a stored score changed.
    Rescore,
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
    /// Commands applied so far (each one that delivered an event), see [`Book::changes`].
    changes: u64,
    /// The step being recorded ([`crate::step`]), and how many [`Book::begin_step`]
    /// are open.
    recording: Option<Step>,
    depth: usize,
}

/// What [`Book::poll_changes`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Polled {
    /// The extensions' status messages, or the first error (as [`Book::poll`]).
    pub result: Result<Vec<String>, BookError>,
    /// Whether any command changed the log (also when `result` is an error: the
    /// commands before the failing one are kept). The GUI saves the log when this is
    /// set, as zLog saves after a QSO received over Z-Link.
    pub changed: bool,
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
            changes: 0,
            recording: None,
            depth: 0,
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
        if let Ok(more) = book.run(Vec::new(), commands) {
            messages.extend(more);
        }
        (book, messages)
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    /// Set the QSY rules of the log ([`Log::qsy_rule`], used by
    /// [`Log::set_dupe_flags`]; zLog reads `Settings._countdown` and `_qsycount`
    /// each time). Flags already stored in QSOs are left as they are.
    pub fn set_qsy_rule(&mut self, rule: QsyRule) {
        self.log.qsy_rule = rule;
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
    ///
    /// What it changes, with the reactions to it, is one step, or part of the step
    /// opened by [`Book::begin_step`].
    pub fn execute(&mut self, command: Command) -> Result<Vec<String>, BookError> {
        self.begin_step();
        let result = self.run(vec![(Origin::User, command)], Vec::new());
        self.end_step();
        result
    }

    /// Make what the commands executed until the matching [`Book::end_step`] change
    /// one step of the operator (an edit of many QSOs at once). Steps may nest; the
    /// outermost counts.
    pub fn begin_step(&mut self) {
        self.depth += 1;
        self.recording.get_or_insert_with(Step::default);
    }

    /// [`Book::begin_step`], with the step tagged `tag` (such as [`crate::IMPORT`])
    /// when this level is the outermost.
    pub fn begin_tagged_step(&mut self, tag: u64) {
        let outermost = self.depth == 0;
        self.begin_step();
        if let Some(step) = self.recording.as_mut().filter(|_| outermost) {
            step.tag = Some(tag);
        }
    }

    /// Close one level; the outermost hands the finished step, when it changed
    /// anything, to every extension ([`crate::Observer::on_step`]).
    pub fn end_step(&mut self) {
        // An end without a begin is ignored.
        let Some(depth) = self.depth.checked_sub(1) else {
            return;
        };
        self.depth = depth;
        if self.depth > 0 {
            return;
        }
        let Some(mut step) = self.recording.take() else {
            return;
        };
        step.finish(&self.log.qsos);
        if !step.changes.is_empty() {
            self.registry.step(&self.log, &step);
        }
    }

    /// Run the commands of `batch` as one step of the operator when the log is as it
    /// expects, checked and run with nothing in between; otherwise nothing changes.
    /// The step carries the batch's tag. Not inside [`Book::begin_step`].
    pub fn execute_if(&mut self, batch: Batch) -> Result<Vec<String>, BatchError> {
        debug_assert_eq!(self.depth, 0, "execute_if inside begin_step");
        batch.check(&self.log.qsos)?;
        let roots = batch
            .commands
            .into_iter()
            .map(|c| (Origin::User, c))
            .collect();
        // Inside a step (a misuse) the outer step keeps its own tag.
        self.begin_tagged_step(batch.tag);
        let result = self.run(roots, Vec::new());
        self.end_step();
        Ok(result?)
    }

    /// Let extensions that wait on something outside (a network, a timer) send commands.
    ///
    /// Use [`Book::poll_changes`] to learn whether the log changed.
    pub fn poll(&mut self) -> Result<Vec<String>, BookError> {
        self.poll_changes().result
    }

    /// [`Book::poll`], also reporting whether any command changed the log, so the
    /// caller can save after changes received from a link (Z-Link).
    pub fn poll_changes(&mut self) -> Polled {
        let before = self.changes;
        let result = self.poll_inner();
        Polled {
            result,
            changed: self.changes != before,
        }
    }

    /// How many commands have changed the log since it was opened; a caller that
    /// remembers this number can tell whether anything changed since.
    pub fn changes(&self) -> u64 {
        self.changes
    }

    fn poll_inner(&mut self) -> Result<Vec<String>, BookError> {
        let mut cx = Context::new(&self.log, Origin::User);
        self.registry.poll(&mut cx);
        let (commands, mut messages, resynced) = (cx.commands, cx.messages, cx.resynced);
        let result = self.run(commands, Vec::new());
        // Told after the commands of the poll, whether they all ran or not; what the
        // extensions send back runs as if from the poll, in the link's chain.
        if let Some(link) = resynced {
            let (more, said) = self.dispatch(&LogEvent::Resynced, link);
            messages.extend(said);
            messages.extend(self.run(more, Vec::new())?);
        }
        messages.extend(result?);
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
        self.dispatch_in(event, origin, origin)
    }

    /// [`Self::dispatch`] within the chain started by `root` ([`Context::root_origin`]).
    fn dispatch_in(
        &mut self,
        event: &LogEvent,
        origin: Origin,
        root: Origin,
    ) -> (Vec<(Origin, Command)>, Vec<String>) {
        let mut cx = Context::new(&self.log, origin).with_root(root);
        self.registry.dispatch(&mut cx, event);
        (cx.commands, cx.messages)
    }

    /// Run the `roots` (from the operator or [`Link::poll`](crate::Link::poll)), not
    /// counted, and the `reactions` sent to events, counted against [`MAX_CASCADE`].
    ///
    /// The reactions to each command run before the next root, in the order sent;
    /// the limit applies to the chain of each root separately.
    ///
    /// A command that fails (an unknown ID, scoring, a runaway chain) ends the chain
    /// of its root; the remaining roots still run (a link's poll may send many
    /// independent commands), and the first error is returned at the end.
    fn run(
        &mut self,
        roots: Vec<(Origin, Command)>,
        reactions: Vec<(Origin, Command)>,
    ) -> Result<Vec<String>, BookError> {
        let mut roots: VecDeque<(Origin, Command)> = roots.into();
        let mut queue: VecDeque<(Origin, Command)> = reactions.into();
        let mut messages = Vec::new();
        let mut rounds = 0;
        let mut first_error = None;
        // The origin of the root of the running chain; reactions queued before any
        // root (while opening) belong to the operator.
        let mut chain_root = Origin::User;
        loop {
            let (origin, command, root) = if let Some(next) = queue.pop_front() {
                rounds += 1;
                if rounds > MAX_CASCADE {
                    let e = BookError::Cascade(self.registry.name_of(next.0).to_string());
                    first_error.get_or_insert(e);
                    queue.clear();
                    continue;
                }
                (next.0, next.1, false)
            } else if let Some(next) = roots.pop_front() {
                // Each root starts a chain of its own.
                rounds = 0;
                chain_root = next.0;
                (next.0, next.1, true)
            } else {
                break;
            };
            if matches!(command, Command::Rescore) {
                let before = self.log.qsos.clone();
                match rescore(&mut self.log, self.contest.as_mut()) {
                    Ok(sheet) => self.sheet = sheet,
                    Err(e) => {
                        first_error.get_or_insert(BookError::Scoring(e));
                        queue.clear();
                        continue;
                    }
                }
                if self.log.qsos != before {
                    self.changes += 1;
                }
                continue;
            }
            let event = match self.apply(origin, command, root, chain_root) {
                Ok(Some(event)) => {
                    self.changes += 1;
                    if chain_root == Origin::User {
                        self.record(&event);
                    }
                    event
                }
                // Nothing changed: no rescore, no event, the queue goes on.
                Ok(None) => continue,
                Err(e) => {
                    first_error.get_or_insert(e);
                    queue.clear();
                    continue;
                }
            };
            match rescore(&mut self.log, self.contest.as_mut()) {
                Ok(sheet) => self.sheet = sheet,
                Err(e) => {
                    first_error.get_or_insert(BookError::Scoring(e));
                    queue.clear();
                    continue;
                }
            }
            let (commands, said) = self.dispatch_in(&event, origin, chain_root);
            messages.extend(said);
            queue.extend(commands);
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(messages),
        }
    }

    /// Apply one command; `root` tells a command from the operator or a link's poll
    /// from one sent in reaction to an event, and `chain_root` is the origin of the
    /// root of the chain.
    ///
    /// Edit counter (the last two digits of `reserve3`, zLog `IncEditCounter`): the
    /// operator's edits bump it; a link's own update keeps the counter it brings; a
    /// reaction bumps it unless it sets a counter itself, or its chain started from a
    /// link's poll. The last rule keeps a QSO received from the network at the
    /// sender's counter when a local observer (say, one filling `entity`) touches
    /// it: the reaction is not sent back (every peer runs its own observers), so a
    /// bump would make the counters differ and every merge resend the QSO.
    fn apply(
        &mut self,
        origin: Origin,
        command: Command,
        root: bool,
        chain_root: Origin,
    ) -> Result<Option<LogEvent>, BookError> {
        match command {
            Command::Insert(qso) => {
                let index = self.log.qsos.len();
                Ok(Some(self.insert(index, qso)))
            }
            Command::InsertBefore(before, qso) => {
                let index = self.find(before).unwrap_or(self.log.qsos.len());
                Ok(Some(self.insert(index, qso)))
            }
            Command::Update(id, mut qso) => {
                let Some(index) = self.find_from(origin, id)? else {
                    return Ok(None);
                };
                let old = self.log.qsos[index].reserve3;
                let keep = origin != Origin::User
                    && QsoId::of(&qso) == id
                    && (root || qso.reserve3 != old || chain_root != Origin::User);
                if !keep {
                    // The edit counter stops at 99, as in zLog (`IncEditCounter`).
                    qso.reserve3 = if old % 100 < 99 { old + 1 } else { old };
                }
                let before = std::mem::replace(&mut self.log.qsos[index], qso);
                Ok(Some(LogEvent::Updated { index, before }))
            }
            Command::Delete(id) => {
                let Some(index) = self.find_from(origin, id)? else {
                    return Ok(None);
                };
                let qso = self.log.qsos.remove(index);
                Ok(Some(LogEvent::Deleted { index, qso }))
            }
            Command::SortByTime => {
                // `sort_by` is stable; times are compared as `TDateTime` values.
                self.log.qsos.sort_by(|a, b| a.time.0.total_cmp(&b.time.0));
                Ok(Some(LogEvent::Sorted))
            }
            Command::Rescore => unreachable!("run handles Rescore"),
        }
    }

    /// Note in the step being recorded which QSO `event` changed, by ID.
    fn record(&mut self, event: &LogEvent) {
        let Some(step) = &mut self.recording else {
            return;
        };
        let qsos = &self.log.qsos;
        let id_at = |i: usize| qsos.get(i).map(QsoId::of);
        match event {
            LogEvent::Inserted { index } => {
                step.touch(id_at(*index).unwrap(), None, id_at(index + 1))
            }
            LogEvent::Updated { index, before } => {
                step.touch(id_at(*index).unwrap(), Some(before.clone()), None)
            }
            LogEvent::Deleted { index, qso } => {
                step.touch(QsoId::of(qso), Some(qso.clone()), id_at(*index))
            }
            LogEvent::Opened { .. } | LogEvent::Sorted | LogEvent::Resynced | LogEvent::Closed => {}
        }
    }

    /// The index of the QSO an update or delete names. An unknown ID is an error for
    /// the operator; from an extension it is ignored (`Ok(None)`), as zLog's
    /// `TLog.ProcessEdit` and `ProcessDelete` do nothing when no QSO has the ID.
    ///
    /// Departure: zLog's `actEditOrAdd` (add when not found) has no command here.
    fn find_from(&self, origin: Origin, id: QsoId) -> Result<Option<usize>, BookError> {
        match self.find(id) {
            Some(index) => Ok(Some(index)),
            None if origin == Origin::User => Err(BookError::NotFound(id)),
            None => Ok(None),
        }
    }

    fn insert(&mut self, index: usize, mut qso: Qso) -> LogEvent {
        let taken = qso.reserve3 == 0 || self.find(QsoId::of(&qso)).is_some();
        if taken {
            qso.reserve3 = self.fresh_id(&qso);
        }
        self.log.qsos.insert(index, qso);
        LogEvent::Inserted { index }
    }

    /// Give an ID to every QSO without one (files from old versions store 0
    /// everywhere), as zLog does on load (`ImportSetQsoId`).
    ///
    /// A record with the ID and callsign of an earlier one is the same QSO saved twice
    /// and is dropped, keeping the first, as zLog does (`IsContainsSameQSO`). A record
    /// with the ID of an earlier one but another callsign is kept with its ID, as in
    /// zLog; [`Book::find`] then names the first of them. Returns how many were dropped.
    fn assign_missing_ids(&mut self) -> usize {
        // The callsigns seen under each ID (`FQsoIdDic` holds the first only).
        let mut seen: std::collections::HashMap<QsoId, String> = std::collections::HashMap::new();
        let qsos = std::mem::take(&mut self.log.qsos);
        let mut dropped = 0;
        for mut q in qsos {
            if q.reserve3 == 0 {
                q.reserve3 = self.fresh_id(&q);
            }
            let id = QsoId::of(&q);
            match seen.get(&id) {
                Some(call) if *call == q.callsign => {
                    dropped += 1;
                    continue;
                }
                Some(_) => {}
                None => {
                    seen.insert(id, q.callsign.clone());
                }
            }
            self.log.qsos.push(q);
        }
        dropped
    }

    /// `TTSSSSRR00`: TX number (at most 15, `MAX_TX - 1`), serial counter, a two-digit
    /// salt, edit counter 0.
    ///
    /// When every serial is taken under that salt, the next salts are tried.
    fn fresh_id(&mut self, q: &Qso) -> i32 {
        let salt = (q.time.0.fract() * 1e6) as i32 % 100;
        let tx = (q.tx as i32).min(15);
        let mut id = 0;
        for offset in 0..100 {
            let salt = (salt + offset) % 100;
            for _ in 0..10000 {
                let serial = self.next_serial % 10000;
                self.next_serial += 1;
                id = tx * 100_000_000 + serial * 10_000 + salt * 100;
                if self.find(QsoId(id / 100)).is_none() {
                    return id;
                }
            }
        }
        // A million QSOs of one TX: no ID is free.
        id
    }
}
