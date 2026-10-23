//! Shortcuts the operator chose: what the application hands a tab that assigns
//! them ([`crate::SettingsPage::set_commands`]) and what it takes back
//! ([`crate::SettingsPage::shortcuts`]) to put into [`crate::Command::shortcut`]
//! before drawing the menus and the toolbar and running keys. Without such a tab
//! every command keeps its own shortcut.
//!
//! A command is named by its provider and its id ([`command_key`]), so the
//! choices for an extension that is no longer linked in are simply not used.

use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers};

use crate::{MenuGroup, MenuSection};

/// A command that may be given a shortcut, as a tab lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandInfo {
    /// [`command_key`] of its provider and its id.
    pub key: String,
    pub label: String,
    pub group: MenuGroup,
    /// The title of its group's menu.
    pub group_title: String,
    pub section: MenuSection,
    pub weight: i32,
    /// Its own shortcut, before any choice.
    pub default: Option<KeyboardShortcut>,
}

/// Shortcuts chosen by command key; `None` takes a command's shortcut away.
pub type Overrides = BTreeMap<String, Option<KeyboardShortcut>>;

/// The name of the command `command` of the provider `provider`.
pub fn command_key(provider: &str, command: &str) -> String {
    format!("{provider}/{command}")
}

/// `Ctrl+Shift+S`, as kept in the settings. `Ctrl` is the platform's command key
/// (Ctrl, Cmd on macOS); the Control key of macOS, pressed without Cmd, is
/// `MacCtrl`, so a shortcut recorded with it is not run by Cmd.
pub fn format(k: &KeyboardShortcut) -> String {
    let m = k.modifiers;
    let mut parts = Vec::new();
    if m.command || m.mac_cmd || (m.ctrl && !cfg!(target_os = "macos")) {
        parts.push("Ctrl");
    }
    if m.ctrl && cfg!(target_os = "macos") {
        parts.push("MacCtrl");
    }
    if m.alt {
        parts.push("Alt");
    }
    if m.shift {
        parts.push("Shift");
    }
    parts.push(k.logical_key.name());
    parts.join("+")
}

/// The shortcut written by [`format`].
pub fn parse(text: &str) -> Option<KeyboardShortcut> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part {
            "Ctrl" => modifiers |= Modifiers::COMMAND,
            "MacCtrl" => modifiers |= Modifiers::CTRL,
            "Alt" => modifiers |= Modifiers::ALT,
            "Shift" => modifiers |= Modifiers::SHIFT,
            name => key = Some(Key::from_name(name)?),
        }
    }
    Some(KeyboardShortcut::new(modifiers, key?))
}

/// Whether two shortcuts are the same keys (Ctrl and Cmd alike).
pub fn same(a: &KeyboardShortcut, b: &KeyboardShortcut) -> bool {
    format(a) == format(b)
}

/// Whether pressing `b` also runs `a` or the other way round, as egui matches
/// shortcuts (extra Shift or Alt pressed are ignored).
pub fn overlap(a: &KeyboardShortcut, b: &KeyboardShortcut) -> bool {
    a.logical_key == b.logical_key
        && (a.modifiers.matches_logically(b.modifiers)
            || b.modifiers.matches_logically(a.modifiers))
}

/// How many modifiers `k` asks for; the shortcuts asking for more are tried
/// first, so Ctrl+Shift+S is not taken by Ctrl+S.
pub fn specificity(k: &KeyboardShortcut) -> usize {
    let m = k.modifiers;
    [m.alt, m.shift, m.ctrl || m.command, m.mac_cmd]
        .into_iter()
        .filter(|b| *b)
        .count()
}

/// Whether `k` may be a command's shortcut: with Ctrl, Alt or Cmd, or a function
/// key, as other keys (B, Shift+B) are typed into the fields.
pub fn assignable(k: &KeyboardShortcut) -> bool {
    let m = k.modifiers;
    let function = k
        .logical_key
        .name()
        .strip_prefix('F')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    m.ctrl || m.command || m.mac_cmd || m.alt || function
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_round_trip_as_text() {
        let k = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        assert_eq!(format(&k), "Ctrl+Shift+S");
        assert!(same(&parse("Ctrl+Shift+S").unwrap(), &k));
        assert_eq!(format(&parse("F5").unwrap()), "F5");
        assert!(parse("Ctrl+Nothing").is_none());
        assert!(parse("Ctrl").is_none());
        assert_eq!(command_key("file", "file.save"), "file/file.save");
    }

    /// The Control key of macOS is kept apart from Cmd; elsewhere it is Ctrl.
    #[test]
    fn mac_control_is_not_cmd() {
        let k = KeyboardShortcut::new(Modifiers::CTRL, Key::S);
        let text = format(&k);
        if cfg!(target_os = "macos") {
            assert_eq!(text, "MacCtrl+S");
        } else {
            assert_eq!(text, "Ctrl+S");
        }
        let back = parse(&text).unwrap();
        // A Control press as egui reports it: off macOS it is also the command key.
        let pressed = if cfg!(target_os = "macos") {
            Modifiers::CTRL
        } else {
            Modifiers::CTRL | Modifiers::COMMAND
        };
        assert!(pressed.matches_logically(back.modifiers));
        assert_eq!(parse("MacCtrl+S").unwrap().modifiers, Modifiers::CTRL);
        assert!(!Modifiers::MAC_CMD.matches_logically(Modifiers::CTRL));
        // A Cmd press as egui reports it on macOS.
        let cmd = KeyboardShortcut::new(Modifiers::MAC_CMD | Modifiers::COMMAND, Key::S);
        assert_eq!(format(&cmd), "Ctrl+S");
    }

    #[test]
    fn overlaps_and_assignable_keys() {
        let ctrl_s = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let ctrl_shift_s = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        assert!(overlap(&ctrl_s, &ctrl_shift_s));
        assert!(!overlap(
            &ctrl_s,
            &KeyboardShortcut::new(Modifiers::NONE, Key::S)
        ));
        assert!(specificity(&ctrl_shift_s) > specificity(&ctrl_s));
        assert!(assignable(&ctrl_s));
        assert!(assignable(&KeyboardShortcut::new(Modifiers::ALT, Key::B)));
        assert!(assignable(&KeyboardShortcut::new(Modifiers::NONE, Key::F5)));
        assert!(assignable(&KeyboardShortcut::new(
            Modifiers::SHIFT,
            Key::F12
        )));
        assert!(!assignable(&KeyboardShortcut::new(Modifiers::NONE, Key::B)));
        assert!(!assignable(&KeyboardShortcut::new(
            Modifiers::SHIFT,
            Key::B
        )));
        assert!(!assignable(&KeyboardShortcut::new(
            Modifiers::NONE,
            Key::Enter
        )));
    }
}
