//! Undo and redo of the operator's edits (zLog has none).
//!
//! - [`history`]: the steps to undo and redo, kept by an observer, and how one is
//!   undone as a batch the book runs only on the log as the step left it.
//! - [`menu`]: Undo, Redo and Skip in the Edit menu, and their keys on the log grid.
//!
//! The observer is recreated with each log, while the menu lives for the whole
//! program, so both reach the one [`History`] through [`shared`]. The steps are
//! forgotten when a log is opened or closed, and when a link replaced or merged the
//! log ([`zlog_core::LogEvent::Resynced`]).

use std::sync::{Arc, Mutex, OnceLock};

use zlog_core::{Extension, Observer};

pub mod history;
pub mod menu;

pub use history::{undo, History, Recorder, Refusal, Shared, UndoError, MAX_STEPS};
pub use menu::UndoMenu;

zlog_core::texts!();

/// The process-wide history.
pub fn shared() -> Shared {
    static HISTORY: OnceLock<Shared> = OnceLock::new();
    HISTORY
        .get_or_init(|| Arc::new(Mutex::new(History::default())))
        .clone()
}

/// The package: one recorder on a history, the shared one by default.
pub struct Undo(pub Shared);

impl Default for Undo {
    fn default() -> Self {
        Undo(shared())
    }
}

impl Extension for Undo {
    fn name(&self) -> &str {
        "Undo"
    }

    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        vec![Box::new(Recorder(self.0.clone()))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
