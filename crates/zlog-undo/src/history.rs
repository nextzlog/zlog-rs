//! The steps to undo and to redo, and how one is undone.
//!
//! The book hands every finished step of the operator to the [`Recorder`]
//! ([`zlog_core::Observer::on_step`]). A step is undone as a [`Batch`] that expects
//! every QSO it touched exactly as the step left it, so the book refuses it whole,
//! the log unchanged, when another station edited one since; the operator may skip
//! it and go on with older steps. Undo and redo run as the operator's commands, so
//! links forward them as any edit.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use zlog_core::{
    Batch, BatchError, Book, BookError, Command, Context, Log, LogEvent, Mismatch, Observer, Qso,
    QsoId, Step,
};

/// How many steps are kept for undo (and for redo).
pub const MAX_STEPS: usize = 100;

/// Why a step cannot be undone or redone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("nothing to undo or redo")]
    Empty,
    /// A QSO of the step is no longer as the step left it (another station changed it).
    #[error(transparent)]
    Changed(#[from] Mismatch),
}

/// Why an undo or redo did not run, or failed while running.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UndoError {
    /// Refused before anything changed.
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

/// The steps to undo and to redo, newest last; in memory only.
#[derive(Debug)]
pub struct History {
    undo: VecDeque<Step>,
    redo: VecDeque<Step>,
    /// The batches sent for steps not yet seen done, oldest first: tag, whether it
    /// redoes, and the step it takes back. Several when the operator pressed Undo
    /// again before the first ran.
    pending: VecDeque<(u64, bool, Step)>,
    /// The tag of the last batch sent.
    tags: u64,
    /// Why the next undo and the next redo cannot run, as of the last change.
    blocked: [Option<Refusal>; 2],
    /// Whether `blocked` waits for [`History::refresh`] (after a skip).
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

/// The history shared by the recorder and the menu.
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

    /// How many steps there are to undo and to redo.
    pub fn steps(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    /// The next step to undo (or redo with `redo`).
    pub fn next(&self, redo: bool) -> Option<&Step> {
        self.stack(redo).back()
    }

    /// Why the next undo (or redo) cannot run on `qsos`, if it cannot.
    pub fn check(&self, redo: bool, qsos: &[Qso]) -> Result<(), Refusal> {
        let step = self.next(redo).ok_or(Refusal::Empty)?;
        Ok(expected(step).check(qsos)?)
    }

    /// [`History::check`] as of the last change of the log the recorder saw.
    pub fn blocked(&self, redo: bool) -> Option<Refusal> {
        self.blocked[redo as usize]
    }

    /// The step Skip would drop: the one to undo when another station changed it,
    /// else such a one to redo.
    pub fn skippable(&self) -> Option<bool> {
        [false, true]
            .into_iter()
            .find(|&redo| matches!(self.blocked(redo), Some(Refusal::Changed(_))))
    }

    /// The batch that undoes (or redoes) the next step; the step moves to the other
    /// side once the book reports it done. `None` when there is none.
    pub fn batch(&mut self, redo: bool) -> Option<Batch> {
        let step = self.next(redo)?.clone();
        self.tags += 1;
        let batch = Batch {
            commands: inverse(&step),
            tag: self.tags,
            ..expected(&step)
        };
        self.pending.push_back((self.tags, redo, step));
        // Batches refused or never run are not seen again; keep a few.
        if self.pending.len() > 8 {
            self.pending.pop_front();
        }
        Some(batch)
    }

    /// Drop the next step to undo (or redo) without changing the log, so the operator
    /// can go on with older steps. Returns whether there was one. What blocks the
    /// steps below waits for [`History::refresh`].
    pub fn skip(&mut self, redo: bool) -> bool {
        self.stale = true;
        self.stack_mut(redo).pop_back().is_some()
    }

    /// Forget every step (the log was closed or replaced).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending.clear();
        self.blocked = [Some(Refusal::Empty); 2];
    }

    /// Keep `step`: the result of the batch sent for it moves the step it took back
    /// to the other side; any other step is new and clears the redo steps.
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
                // What the step was (an import) stays with it on the other side.
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

    /// Whether the next step to undo or redo touches the QSO `id`.
    fn concerns(&self, id: QsoId) -> bool {
        [false, true]
            .into_iter()
            .filter_map(|redo| self.next(redo))
            .any(|s| s.changes.iter().any(|c| c.id == id))
    }

    /// Check the next steps against `qsos` again ([`History::blocked`]).
    pub fn refresh(&mut self, qsos: &[Qso]) {
        self.blocked = [false, true].map(|redo| self.check(redo, qsos).err());
        self.stale = false;
    }

    /// Whether [`History::blocked`] waits for [`History::refresh`].
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

/// The QSOs as `step` left them.
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

/// The commands that take the log back from `after` to `before`, last change first.
/// An update raises the edit counter as any edit; a QSO put back keeps its ID with
/// one edit more than it had, so merges prefer it to an older copy. It goes back
/// before the QSO that followed it, or at the end when that one is gone.
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

/// Undo (or redo with `redo`) the next step on `book` at once. When a QSO it touched
/// is no longer as the step left it, nothing changes and the step stays.
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

/// The observer that keeps the operator's steps; it never changes the log.
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
