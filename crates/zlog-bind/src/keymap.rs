//! The shortcuts chosen, as kept in the section of the tab (`bind`): one key per
//! command ([`zlog_ui::keys::command_key`]) whose value is the shortcut
//! (`Ctrl+Shift+S`) or `none` for no shortcut. Commands left out keep their own.
//!
//! A choice is refused when it has no Ctrl, Alt or Cmd and is no function key
//! (such keys are typed into the fields), when the entry row or the log grid
//! keeps the keys (also with Shift or Alt added, as egui matches them), or when
//! another command has them already. Ctrl+S and Ctrl+Shift+S may both be given:
//! the application tries the shortcuts with more modifiers first.

use egui::KeyboardShortcut;
use zlog_ui::keys::{self, CommandInfo, Overrides};
use zlog_ui::Section;

/// The value of a command whose shortcut was taken away.
const NONE: &str = "none";

/// The choices of `section`; values that are no shortcut are left out.
pub fn load(section: &Section) -> Overrides {
    section
        .iter()
        .filter_map(|(k, v)| match v.as_str() {
            NONE => Some((k.clone(), None)),
            text => keys::parse(text).map(|s| (k.clone(), Some(s))),
        })
        .collect()
}

/// Choose `shortcut` for `command` (`None` for none), or its own again when it is
/// that already.
pub fn set(section: &mut Section, command: &CommandInfo, shortcut: Option<KeyboardShortcut>) {
    let own = match (&command.default, &shortcut) {
        (Some(a), Some(b)) => keys::same(a, b),
        (None, None) => true,
        _ => false,
    };
    if own {
        section.remove(&command.key);
    } else {
        let text = shortcut.map_or(NONE.to_string(), |s| keys::format(&s));
        section.insert(command.key.clone(), text);
    }
}

/// The shortcut `command` has now: the one chosen, else its own.
pub fn current(section: &Section, command: &CommandInfo) -> Option<KeyboardShortcut> {
    match section.get(&command.key).map(String::as_str) {
        Some(NONE) => None,
        Some(text) => keys::parse(text).or(command.default),
        None => command.default,
    }
}

/// Why `shortcut` cannot go to `command`.
#[derive(Debug, Clone, PartialEq)]
pub enum Conflict {
    /// No Ctrl, Alt or Cmd, and no function key.
    Plain,
    /// The entry row or the log grid keeps it.
    Reserved,
    /// The command with this label has it.
    Taken(String),
}

/// Whether `shortcut` can go to `command`, given every command's present one.
pub fn conflict(
    section: &Section,
    commands: &[CommandInfo],
    reserved: &[KeyboardShortcut],
    command: &CommandInfo,
    shortcut: &KeyboardShortcut,
) -> Option<Conflict> {
    if !keys::assignable(shortcut) {
        return Some(Conflict::Plain);
    }
    if reserved.iter().any(|r| keys::overlap(r, shortcut)) {
        return Some(Conflict::Reserved);
    }
    commands
        .iter()
        .filter(|c| c.key != command.key)
        .find(|c| current(section, c).is_some_and(|k| keys::same(&k, shortcut)))
        .map(|c| Conflict::Taken(c.label.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};
    use zlog_ui::{MenuGroup, MenuSection};

    fn info(key: &str, default: Option<KeyboardShortcut>) -> CommandInfo {
        CommandInfo {
            key: key.into(),
            label: key.into(),
            group: MenuGroup::File,
            group_title: "File".into(),
            section: MenuSection::Log,
            weight: 0,
            default,
        }
    }

    fn ctrl(k: Key) -> KeyboardShortcut {
        KeyboardShortcut::new(Modifiers::COMMAND, k)
    }

    #[test]
    fn choices_are_kept_and_given_back() {
        let save = info("file/file.save", Some(ctrl(Key::S)));
        let open = info("file/file.open", None);
        let mut s = Section::new();
        set(&mut s, &open, Some(ctrl(Key::O)));
        set(&mut s, &save, None);
        assert_eq!(s["file/file.open"], "Ctrl+O");
        assert_eq!(s["file/file.save"], "none");
        let o = load(&s);
        assert_eq!(
            o["file/file.open"].map(|k| keys::format(&k)).as_deref(),
            Some("Ctrl+O")
        );
        assert_eq!(o["file/file.save"], None);
        assert_eq!(current(&s, &save), None);
        // Its own again is no choice.
        set(&mut s, &save, Some(ctrl(Key::S)));
        assert!(!s.contains_key("file/file.save"));
        assert_eq!(current(&s, &save), Some(ctrl(Key::S)));
    }

    #[test]
    fn conflicts_are_found() {
        let save = info("file/file.save", Some(ctrl(Key::S)));
        let open = info("file/file.open", None);
        let commands = vec![save.clone(), open.clone()];
        let enter = KeyboardShortcut::new(Modifiers::NONE, Key::Enter);
        let shift_enter = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter);
        let mut s = Section::new();
        let ctrl_enter = ctrl(Key::Enter);
        let check = |s: &Section, k| conflict(s, &commands, &[enter, ctrl_enter], &open, &k);
        assert_eq!(check(&s, enter), Some(Conflict::Plain));
        // Ctrl+Shift+Enter would also run the entry row's Ctrl+Enter.
        assert_eq!(check(&s, shift_enter), Some(Conflict::Reserved));
        let b = KeyboardShortcut::new(Modifiers::SHIFT, Key::B);
        assert_eq!(check(&s, b), Some(Conflict::Plain));
        let f5 = KeyboardShortcut::new(Modifiers::NONE, Key::F5);
        assert_eq!(check(&s, f5), None);
        // Ctrl+Shift+S beside Ctrl+S is fine: the more specific is tried first.
        let save_as = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        assert_eq!(check(&s, save_as), None);
        assert_eq!(
            check(&s, ctrl(Key::S)),
            Some(Conflict::Taken("file/file.save".into()))
        );
        assert_eq!(check(&s, ctrl(Key::O)), None);
        // Once Save gives its keys up, Open may have them.
        set(&mut s, &save, None);
        assert_eq!(check(&s, ctrl(Key::S)), None);
        // A command's own keys are no conflict.
        assert_eq!(conflict(&s, &commands, &[], &save, &ctrl(Key::S)), None);
    }
}
