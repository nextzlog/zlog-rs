//! Editing QSOs after they were logged: the edit dialog, inserting a QSO before
//! another, deleting one after asking, and the invalid flag, from the Edit menu,
//! the keys of the log grid and its right-click menu.
//!
//! - [`change`]: what each of them does to the log, as a batch of commands.
//! - [`editor`]: the commands and the dialogs.

pub mod change;
pub mod editor;

pub use editor::Editor;

zlog_core::texts!();

/// The text of `key` in `language`, for tests that look for the dialogs.
pub fn text(language: &str, key: &str) -> String {
    texts().get(language, key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
