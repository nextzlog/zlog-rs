//! Steps: what one action of the operator changed, and batches of commands run only
//! while the log is as their sender expects.
//!
//! A [`Step`] lists the QSOs one action changed, by [`QsoId`] (indices are void after
//! a sort), as they were before and after. Only chains started by the operator are
//! recorded ([`crate::Book::begin_step`]); changes received from a link are not. Each
//! finished step is handed to the extensions ([`crate::Observer::on_step`]), which may
//! keep it, to undo it say.
//!
//! A [`Batch`] is a sender's commands with what it expects of the log: the book runs
//! them as one step of the operator only when every QSO named is as expected, and
//! otherwise refuses them whole with nothing changed ([`crate::Book::execute_if`]).

use crate::book::{BookError, Command, QsoId};
use crate::qso::Qso;

/// The [`Step::tag`] of a step that adds the QSOs of a file (File > Import),
/// opened with [`crate::Book::begin_tagged_step`]; senders of batches choose other
/// tags.
pub const IMPORT: u64 = u64::MAX;

/// One QSO before and after a step; `None` when it was not in the log.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub id: QsoId,
    pub before: Option<Qso>,
    pub after: Option<Qso>,
    /// The QSO that followed it when it was last inserted or deleted, so that a
    /// delete can be taken back to where it was.
    pub place: Option<QsoId>,
}

/// Everything one action of the operator changed, in the order first touched.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub changes: Vec<Change>,
    /// The [`Batch::tag`] of the batch it ran, so its sender knows its own step,
    /// or what kind of step it is ([`IMPORT`]).
    pub tag: Option<u64>,
}

impl Step {
    /// Note that the QSO `id` changes; `before` is its state when first touched.
    pub(crate) fn touch(&mut self, id: QsoId, before: Option<Qso>, place: Option<QsoId>) {
        match self.changes.iter_mut().find(|c| c.id == id) {
            Some(c) => {
                if place.is_some() {
                    c.place = place;
                }
            }
            None => self.changes.push(Change {
                id,
                before,
                after: None,
                place,
            }),
        }
    }

    /// Read the state each QSO was left in; drop the QSOs that ended as they began.
    pub(crate) fn finish(&mut self, qsos: &[Qso]) {
        for c in &mut self.changes {
            c.after = qsos.iter().find(|q| QsoId::of(q) == c.id).cloned();
        }
        self.changes.retain(|c| c.before != c.after);
    }
}

/// Commands to run as one step of the operator, only while the log is as expected.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Batch {
    /// Each QSO as the sender expects it, `None` for one not in the log. Scores and
    /// the edit counter are not compared ([`same`]).
    pub expect: Vec<(QsoId, Option<Qso>)>,
    /// Run in order as the operator's commands, each a chain of its own.
    pub commands: Vec<Command>,
    /// Chosen by the sender and handed back in [`Step::tag`].
    pub tag: u64,
}

impl Batch {
    /// Why the log is not as the batch expects, if it is not.
    pub fn check(&self, qsos: &[Qso]) -> Result<(), Mismatch> {
        for (id, want) in &self.expect {
            let here = qsos.iter().find(|q| QsoId::of(q) == *id);
            match (want, here) {
                (Some(w), Some(h)) if same(w, h) => {}
                (Some(_), Some(_)) => return Err(Mismatch::Edited(*id)),
                (Some(_), None) => return Err(Mismatch::Deleted(*id)),
                (None, Some(_)) => return Err(Mismatch::Taken(*id)),
                (None, None) => {}
            }
        }
        Ok(())
    }
}

/// How the log differs from what a [`Batch`] expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Mismatch {
    #[error("QSO {0:?} was edited since")]
    Edited(QsoId),
    #[error("QSO {0:?} was deleted since")]
    Deleted(QsoId),
    #[error("QSO {0:?} was logged again since")]
    Taken(QsoId),
}

impl Mismatch {
    /// The QSO that differs.
    pub fn id(self) -> QsoId {
        match self {
            Mismatch::Edited(id) | Mismatch::Deleted(id) | Mismatch::Taken(id) => id,
        }
    }
}

/// Why a [`Batch`] did not run, or failed while running.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BatchError {
    /// Refused before anything changed.
    #[error(transparent)]
    Refused(#[from] Mismatch),
    #[error(transparent)]
    Book(#[from] BookError),
}

/// Equal but for the fields the contest scores and the edit counter: later QSOs
/// rescore a QSO, and an edit that puts a QSO back as it was raises its counter.
pub fn same(a: &Qso, b: &Qso) -> bool {
    let plain = |q: &Qso| Qso {
        reserve3: q.reserve3 / 100,
        dupe: false,
        points: 0,
        multi1: String::new(),
        new_multi1: false,
        multi2: String::new(),
        new_multi2: false,
        multi3: String::new(),
        new_multi3: false,
        ..q.clone()
    };
    plain(a) == plain(b)
}
