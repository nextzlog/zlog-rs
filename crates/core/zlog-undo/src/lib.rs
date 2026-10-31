use std::sync::{Arc, Mutex, OnceLock};

use zlog_core::{Extension, Observer};

pub mod history;
pub mod menu;

pub use history::{undo, History, Recorder, Refusal, Shared, UndoError, MAX_STEPS};
pub use menu::UndoMenu;

zlog_core::texts!();

pub fn shared() -> Shared {
    static HISTORY: OnceLock<Shared> = OnceLock::new();
    HISTORY
        .get_or_init(|| Arc::new(Mutex::new(History::default())))
        .clone()
}

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

pub fn register(b: &mut zlog_ui::Builder) {
    b.extension(zlog_ui::builder::order::OBSERVERS + 1, Undo::default);
    b.panel(120, |ctx| {
        UndoMenu::default().linked((ctx.registry)().has_links())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
