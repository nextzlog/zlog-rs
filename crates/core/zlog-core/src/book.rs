use std::collections::{HashMap, HashSet, VecDeque};

use crate::contest::{rescore, Contest, ScoreSheet};
use crate::extension::{Context, LogEvent, Origin, Registry};
use crate::log::{Log, QsyRule};
use crate::qso::Qso;
use crate::step::{Batch, BatchError, Step};

pub const MAX_CASCADE: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Insert(Qso),
    InsertBefore(QsoId, Qso),
    Update(QsoId, Qso),
    Delete(QsoId),
    SortByTime,
    Rescore,
}

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
    #[error("a batch cannot run inside another step")]
    Nested,
    #[error("no free QSO ID is left for TX {0}")]
    NoFreeId(i32),
}

pub struct Book {
    log: Log,
    contest: Box<dyn Contest>,
    registry: Registry,
    sheet: ScoreSheet,
    next_serial: i32,
    changes: u64,
    recording: Option<Step>,
    depth: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Polled {
    pub result: Result<Vec<String>, BookError>,
    pub changed: bool,
}

impl Book {
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
        let (dropped, mut messages) = book.assign_missing_ids();
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
        if let Ok(more) = book.run(Vec::new(), commands) {
            messages.extend(more);
        }
        (book, messages)
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    pub fn set_qsy_rule(&mut self, rule: QsyRule) {
        self.log.qsy_rule = rule;
    }

    pub fn header_mut(&mut self) -> &mut Qso {
        &mut self.log.header
    }

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

    pub fn execute(&mut self, command: Command) -> Result<Vec<String>, BookError> {
        self.begin_step();
        let result = self.run(vec![(Origin::User, command)], Vec::new());
        self.end_step();
        result
    }

    pub fn begin_step(&mut self) {
        self.depth += 1;
        self.recording.get_or_insert_with(Step::default);
    }

    pub fn begin_tagged_step(&mut self, tag: u64) {
        let outermost = self.depth == 0;
        self.begin_step();
        if let Some(step) = self.recording.as_mut().filter(|_| outermost) {
            step.tag = Some(tag);
        }
    }

    pub fn end_step(&mut self) {
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

    pub fn execute_if(&mut self, batch: Batch) -> Result<Vec<String>, BatchError> {
        if self.depth != 0 {
            return Err(BookError::Nested.into());
        }
        batch.check(&self.log.qsos)?;
        self.validate(&batch.commands)?;
        let roots = batch
            .commands
            .into_iter()
            .map(|c| (Origin::User, c))
            .collect();
        self.begin_tagged_step(batch.tag);
        let result = self.run(roots, Vec::new());
        self.end_step();
        Ok(result?)
    }

    fn validate(&self, commands: &[Command]) -> Result<(), BookError> {
        let mut present: HashSet<QsoId> = self.log.qsos.iter().map(QsoId::of).collect();
        for command in commands {
            match command {
                Command::Insert(q) | Command::InsertBefore(_, q) => {
                    if q.reserve3 != 0 {
                        present.insert(QsoId::of(q));
                    }
                }
                Command::Update(id, _) if !present.contains(id) => {
                    return Err(BookError::NotFound(*id))
                }
                Command::Delete(id) if !present.remove(id) => return Err(BookError::NotFound(*id)),
                Command::Update(..)
                | Command::Delete(_)
                | Command::SortByTime
                | Command::Rescore => {}
            }
        }
        Ok(())
    }

    pub fn poll(&mut self) -> Result<Vec<String>, BookError> {
        self.poll_changes().result
    }

    pub fn poll_changes(&mut self) -> Polled {
        let before = self.changes;
        let result = self.poll_inner();
        Polled {
            result,
            changed: self.changes != before,
        }
    }

    pub fn changes(&self) -> u64 {
        self.changes
    }

    fn poll_inner(&mut self) -> Result<Vec<String>, BookError> {
        let mut cx = Context::new(&self.log, Origin::User);
        self.registry.poll(&mut cx);
        let (commands, mut messages, resynced) = (cx.commands, cx.messages, cx.resynced);
        let result = self.run(commands, Vec::new());
        let mut first_error = None;
        match result {
            Ok(said) => messages.extend(said),
            Err(e) => first_error = Some(e),
        }
        if let Some(link) = resynced {
            let (more, said) = self.dispatch(&LogEvent::Resynced, link);
            messages.extend(said);
            match self.run(more, Vec::new()) {
                Ok(said) => messages.extend(said),
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(messages),
        }
    }

    pub fn close(&mut self) -> Vec<String> {
        self.dispatch(&LogEvent::Closed, Origin::User).1
    }

    fn dispatch(
        &mut self,
        event: &LogEvent,
        origin: Origin,
    ) -> (Vec<(Origin, Command)>, Vec<String>) {
        self.dispatch_in(event, origin, origin)
    }

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
                self.insert(index, qso).map(Some)
            }
            Command::InsertBefore(before, qso) => {
                let index = self.find(before).unwrap_or(self.log.qsos.len());
                self.insert(index, qso).map(Some)
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
                self.log.qsos.sort_by(|a, b| a.time.0.total_cmp(&b.time.0));
                Ok(Some(LogEvent::Sorted))
            }
            Command::Rescore => unreachable!("run handles Rescore"),
        }
    }

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

    fn find_from(&self, origin: Origin, id: QsoId) -> Result<Option<usize>, BookError> {
        match self.find(id) {
            Some(index) => Ok(Some(index)),
            None if origin == Origin::User => Err(BookError::NotFound(id)),
            None => Ok(None),
        }
    }

    fn insert(&mut self, index: usize, mut qso: Qso) -> Result<LogEvent, BookError> {
        let taken = qso.reserve3 == 0 || self.find(QsoId::of(&qso)).is_some();
        if taken {
            let ids = self.log.qsos.iter().map(QsoId::of).collect();
            qso.reserve3 = self.fresh_id(&qso, &ids)?;
        }
        self.log.qsos.insert(index, qso);
        Ok(LogEvent::Inserted { index })
    }

    fn assign_missing_ids(&mut self) -> (usize, Vec<String>) {
        let mut seen: HashMap<QsoId, String> = HashMap::new();
        let qsos = std::mem::take(&mut self.log.qsos);
        let mut dropped = 0;
        let mut renew = Vec::new();
        for q in qsos {
            if q.reserve3 != 0 {
                let id = QsoId::of(&q);
                match seen.get(&id) {
                    Some(call) if *call == q.callsign => {
                        dropped += 1;
                        continue;
                    }
                    Some(_) => renew.push(self.log.qsos.len()),
                    None => {
                        seen.insert(id, q.callsign.clone());
                    }
                }
            } else {
                renew.push(self.log.qsos.len());
            }
            self.log.qsos.push(q);
        }
        let mut ids: HashSet<QsoId> = seen.into_keys().collect();
        let mut errors = Vec::new();
        for index in renew {
            let q = self.log.qsos[index].clone();
            match self.fresh_id(&q, &ids) {
                Ok(id) => {
                    self.log.qsos[index].reserve3 = id;
                    ids.insert(QsoId::of(&self.log.qsos[index]));
                }
                Err(e) => errors.push(e.to_string()),
            }
        }
        (dropped, errors)
    }

    fn fresh_id(&mut self, q: &Qso, taken: &HashSet<QsoId>) -> Result<i32, BookError> {
        let salt = (q.time.0.fract() * 1e6) as i32 % 100;
        let tx = (q.tx as i32).min(15);
        for offset in 0..100 {
            let salt = (salt + offset) % 100;
            for _ in 0..10000 {
                let serial = self.next_serial % 10000;
                self.next_serial += 1;
                let id = tx * 100_000_000 + serial * 10_000 + salt * 100;
                if !taken.contains(&QsoId(id / 100)) {
                    return Ok(id);
                }
            }
        }
        Err(BookError::NoFreeId(tx))
    }
}
