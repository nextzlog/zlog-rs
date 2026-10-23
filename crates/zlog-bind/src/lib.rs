//! Shortcuts: a tab of Options giving every menu command a key of the
//! operator's choice, or none. The application lists the commands
//! ([`zlog_ui::SettingsPage::set_commands`]) and puts the keys chosen into them
//! ([`zlog_ui::SettingsPage::shortcuts`]); without this crate every command keeps
//! its own.
//!
//! - [`keymap`]: the choices as kept in the settings, and their conflicts.
//! - [`options`]: the tab.

pub mod keymap;
pub mod options;

pub use options::KeysPage;

zlog_core::texts!();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
