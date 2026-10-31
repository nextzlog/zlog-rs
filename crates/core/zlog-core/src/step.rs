use crate::book::{BookError, Command, QsoId};
use crate::qso::Qso;

pub const IMPORT: u64 = u64::MAX;

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub id: QsoId,
    pub before: Option<Qso>,
    pub after: Option<Qso>,
    pub place: Option<QsoId>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub changes: Vec<Change>,
    pub tag: Option<u64>,
}

impl Step {
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

    pub(crate) fn finish(&mut self, qsos: &[Qso]) {
        for c in &mut self.changes {
            c.after = qsos.iter().find(|q| QsoId::of(q) == c.id).cloned();
        }
        self.changes.retain(|c| c.before != c.after);
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Batch {
    pub expect: Vec<(QsoId, Option<Qso>)>,
    pub commands: Vec<Command>,
    pub tag: u64,
}

impl Batch {
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
    pub fn id(self) -> QsoId {
        match self {
            Mismatch::Edited(id) | Mismatch::Deleted(id) | Mismatch::Taken(id) => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BatchError {
    #[error(transparent)]
    Refused(#[from] Mismatch),
    #[error(transparent)]
    Book(#[from] BookError),
}

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
