use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use zlog_core::{
    Batch, BatchError, Book, BookError, Command, Context, Log, LogEvent, Mismatch, Observer, Qso,
    QsoId, Step,
};

pub const MAX_STEPS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("nothing to undo or redo")]
    Empty,
    #[error(transparent)]
    Changed(#[from] Mismatch),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UndoError {
    #[error(transparent)]
    Refused(#[from] Refusal),
    #[error(transparent)]
    Book(#[from] BookError),
}

impl From<BatchError> for UndoError {
    fn from(e: BatchError) -> Self {
        match e {
            BatchError::Refused(m) => UndoError::Refused(m.into()),
            BatchError::Book(e) => UndoError::Book(e),
        }
    }
}

#[derive(Debug)]
pub struct History {
    undo: VecDeque<Step>,
    redo: VecDeque<Step>,
    pending: VecDeque<(u64, bool, Step)>,
    tags: u64,
    blocked: [Option<Refusal>; 2],
    stale: bool,
}

impl Default for History {
    fn default() -> Self {
        History {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            pending: VecDeque::new(),
            tags: 0,
            blocked: [Some(Refusal::Empty); 2],
            stale: false,
        }
    }
}

pub type Shared = Arc<Mutex<History>>;

impl History {
    fn stack(&self, redo: bool) -> &VecDeque<Step> {
        if redo {
            &self.redo
        } else {
            &self.undo
        }
    }

    fn stack_mut(&mut self, redo: bool) -> &mut VecDeque<Step> {
        if redo {
            &mut self.redo
        } else {
            &mut self.undo
        }
    }

    pub fn steps(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    pub fn next(&self, redo: bool) -> Option<&Step> {
        self.stack(redo).back()
    }

    pub fn check(&self, redo: bool, qsos: &[Qso]) -> Result<(), Refusal> {
        let step = self.next(redo).ok_or(Refusal::Empty)?;
        Ok(expected(step).check(qsos)?)
    }

    pub fn blocked(&self, redo: bool) -> Option<Refusal> {
        self.blocked[redo as usize]
    }

    pub fn skippable(&self) -> Option<bool> {
        [false, true]
            .into_iter()
            .find(|&redo| matches!(self.blocked(redo), Some(Refusal::Changed(_))))
    }

    pub fn batch(&mut self, redo: bool) -> Option<Batch> {
        let step = self.next(redo)?.clone();
        self.tags += 1;
        let batch = Batch {
            commands: inverse(&step),
            tag: self.tags,
            ..expected(&step)
        };
        self.pending.push_back((self.tags, redo, step));
        if self.pending.len() > 8 {
            self.pending.pop_front();
        }
        Some(batch)
    }

    pub fn skip(&mut self, redo: bool) -> bool {
        self.stale = true;
        self.stack_mut(redo).pop_back().is_some()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending.clear();
        self.blocked = [Some(Refusal::Empty); 2];
    }

    fn record(&mut self, step: &Step, qsos: &[Qso]) {
        let mine = self
            .pending
            .iter()
            .position(|(tag, _, _)| step.tag == Some(*tag));
        match mine.and_then(|i| self.pending.remove(i)) {
            Some((_, redo, done)) => {
                let from = self.stack_mut(redo);
                if let Some(i) = from.iter().rposition(|s| *s == done) {
                    from.remove(i);
                }
                let step = Step {
                    tag: done.tag,
                    ..step.clone()
                };
                push(self.stack_mut(!redo), step);
            }
            None => {
                push(&mut self.undo, step.clone());
                self.redo.clear();
            }
        }
        self.refresh(qsos);
    }

    fn concerns(&self, id: QsoId) -> bool {
        [false, true]
            .into_iter()
            .filter_map(|redo| self.next(redo))
            .any(|s| s.changes.iter().any(|c| c.id == id))
    }

    pub fn refresh(&mut self, qsos: &[Qso]) {
        self.blocked = [false, true].map(|redo| self.check(redo, qsos).err());
        self.stale = false;
    }

    pub fn stale(&self) -> bool {
        self.stale
    }
}

fn push(stack: &mut VecDeque<Step>, step: Step) {
    stack.push_back(step);
    if stack.len() > MAX_STEPS {
        stack.pop_front();
    }
}

fn expected(step: &Step) -> Batch {
    Batch {
        expect: step
            .changes
            .iter()
            .map(|c| (c.id, c.after.clone()))
            .collect(),
        ..Batch::default()
    }
}

fn inverse(step: &Step) -> Vec<Command> {
    let mut commands = Vec::new();
    for c in step.changes.iter().rev() {
        match (&c.before, &c.after) {
            (Some(b), Some(_)) => commands.push(Command::Update(c.id, b.clone())),
            (None, Some(_)) => commands.push(Command::Delete(c.id)),
            (Some(b), None) => {
                let mut q = b.clone();
                if q.reserve3 % 100 < 99 {
                    q.reserve3 += 1;
                }
                commands.push(match c.place {
                    Some(p) => Command::InsertBefore(p, q),
                    None => Command::Insert(q),
                });
            }
            (None, None) => {}
        }
    }
    commands
}

pub fn undo(
    history: &Mutex<History>,
    book: &mut Book,
    redo: bool,
) -> Result<Vec<String>, UndoError> {
    let batch = {
        let mut h = history.lock().unwrap();
        h.check(redo, &book.log().qsos)?;
        h.batch(redo).ok_or(Refusal::Empty)?
    };
    Ok(book.execute_if(batch)?)
}

pub struct Recorder(pub Shared);

impl Observer for Recorder {
    fn name(&self) -> &str {
        "undo"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        let qsos = &cx.log().qsos;
        let id_at = |i: usize| qsos.get(i).map(QsoId::of);
        let ids = match event {
            LogEvent::Opened { .. } | LogEvent::Resynced | LogEvent::Closed => {
                self.0.lock().unwrap().clear();
                return;
            }
            LogEvent::Inserted { index } => [id_at(*index), None],
            LogEvent::Updated { index, before } => [id_at(*index), Some(QsoId::of(before))],
            LogEvent::Deleted { qso, .. } => [Some(QsoId::of(qso)), None],
            LogEvent::Sorted => return,
        };
        let mut h = self.0.lock().unwrap();
        if h.stale || ids.into_iter().flatten().any(|id| h.concerns(id)) {
            h.refresh(qsos);
        }
    }

    fn on_step(&mut self, log: &Log, step: &Step) {
        self.0.lock().unwrap().record(step, &log.qsos);
    }
}
