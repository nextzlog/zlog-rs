use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers};

use crate::{MenuGroup, MenuSection};

#[derive(Debug, Clone, PartialEq)]
pub struct CommandInfo {
    pub key: String,
    pub label: String,
    pub group: MenuGroup,
    pub group_title: String,
    pub section: MenuSection,
    pub weight: i32,
    pub default: Option<KeyboardShortcut>,
    pub short_label: Option<String>,
    pub description: String,
}

pub type Overrides = BTreeMap<String, Option<KeyboardShortcut>>;

pub type Names = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommandOverrides {
    pub shortcuts: Overrides,
    pub names: Names,
    pub short_names: Names,
    pub descriptions: Names,
}

impl CommandOverrides {
    pub fn is_empty(&self) -> bool {
        self.shortcuts.is_empty()
            && self.names.is_empty()
            && self.short_names.is_empty()
            && self.descriptions.is_empty()
    }

    pub fn extend(&mut self, other: CommandOverrides) {
        self.shortcuts.extend(other.shortcuts);
        self.names.extend(other.names);
        self.short_names.extend(other.short_names);
        self.descriptions.extend(other.descriptions);
    }
}

pub fn text_focused(ctx: &egui::Context) -> bool {
    let key = egui::Id::unique("zlog_ui::keys::text_focused");
    let pass = ctx.cumulative_pass_nr();
    let now = ctx
        .memory(|m| m.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
    let seen: Option<(u64, bool, bool)> = ctx.data(|d| d.get_temp(key));
    let before = match seen {
        Some((p, _, before)) if p == pass => before,
        Some((p, was, _)) => was && p + 1 == pass,
        None => false,
    };
    ctx.data_mut(|d| d.insert_temp(key, (pass, now, before)));
    let escape = ctx.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::Key { key: Key::Escape, pressed: true, modifiers: m, .. }
                if !m.any())
        })
    });
    now || before && escape
}

pub fn consume_exact(input: &mut egui::InputState, modifiers: Modifiers, key: Key) -> bool {
    let before = input.events.len();
    input.events.retain(|e| {
        !matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
            if *k == key && m.matches_exact(modifiers))
    });
    input.events.len() != before
}

pub fn consume_shortcut(input: &mut egui::InputState, modifiers: Modifiers, key: Key) -> bool {
    let Some(at) = input.events.iter().position(|e| {
        matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
            if *k == key && m.matches_exact(modifiers))
    }) else {
        return false;
    };
    consume_exact(input, modifiers, key);
    if modifiers.command || modifiers.ctrl || modifiers.mac_cmd {
        return true;
    }
    if let Some(t) = input.events[at..]
        .iter()
        .position(|e| matches!(e, egui::Event::Text(_)))
    {
        input.events.remove(at + t);
    }
    true
}

pub fn consume_typed(input: &mut egui::InputState, text: &str, key: Key) -> bool {
    let typed = input
        .events
        .iter()
        .any(|e| matches!(e, egui::Event::Text(t) if t == text));
    if !typed {
        return consume_exact(input, Modifiers::NONE, key);
    }
    input.events.retain(|e| match e {
        egui::Event::Text(t) => t != text,
        egui::Event::Key {
            key: k,
            pressed: true,
            modifiers: m,
            ..
        } => *k != key || m.command || m.ctrl || m.mac_cmd,
        _ => true,
    });
    true
}

pub fn command_key(provider: &str, command: &str) -> String {
    format!("{provider}/{command}")
}

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

pub fn same(a: &KeyboardShortcut, b: &KeyboardShortcut) -> bool {
    format(a) == format(b)
}

pub fn overlap(a: &KeyboardShortcut, b: &KeyboardShortcut) -> bool {
    overlap_on(cfg!(target_os = "macos"), a, b)
}

pub fn overlap_on(mac: bool, a: &KeyboardShortcut, b: &KeyboardShortcut) -> bool {
    let same_key = a.logical_key == b.logical_key;
    let (a, b) = (logical(mac, a.modifiers), logical(mac, b.modifiers));
    same_key && (a.matches_logically(b) || b.matches_logically(a))
}

fn logical(mac: bool, mut m: Modifiers) -> Modifiers {
    if !mac && (m.ctrl || m.command) {
        m.ctrl = true;
        m.command = true;
    }
    m
}

pub fn specificity(k: &KeyboardShortcut) -> usize {
    let m = k.modifiers;
    [m.alt, m.shift, m.ctrl || m.command, m.mac_cmd]
        .into_iter()
        .filter(|b| *b)
        .count()
}

pub fn assignable(k: &KeyboardShortcut) -> bool {
    let m = k.modifiers;
    let function = k
        .logical_key
        .name()
        .strip_prefix('F')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    m.ctrl || m.command || m.mac_cmd || m.alt || function
}

pub fn default_clashes(
    commands: &[CommandInfo],
    reserved: &[KeyboardShortcut],
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (i, c) in commands.iter().enumerate() {
        let Some(k) = c.default else {
            continue;
        };
        if let Some(r) = reserved.iter().find(|r| overlap(r, &k)) {
            out.push((c.key.clone(), format(r)));
        }
        for d in &commands[i + 1..] {
            if d.default.is_some_and(|d| overlap(&d, &k)) {
                out.push((c.key.clone(), d.key.clone()));
            }
        }
    }
    out
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
        let pressed = if cfg!(target_os = "macos") {
            Modifiers::CTRL
        } else {
            Modifiers::CTRL | Modifiers::COMMAND
        };
        assert!(pressed.matches_logically(back.modifiers));
        assert_eq!(parse("MacCtrl+S").unwrap().modifiers, Modifiers::CTRL);
        assert!(!Modifiers::MAC_CMD.matches_logically(Modifiers::CTRL));
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

    #[test]
    fn control_and_command_are_one_key_except_on_mac() {
        let ctrl_k = KeyboardShortcut::new(Modifiers::CTRL, Key::K);
        let cmd_k = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
        let cmd_l = KeyboardShortcut::new(Modifiers::COMMAND, Key::L);
        assert!(overlap_on(false, &ctrl_k, &cmd_k));
        assert!(!overlap_on(true, &ctrl_k, &cmd_k));
        assert!(!overlap_on(false, &ctrl_k, &cmd_l));
        assert!(overlap_on(true, &ctrl_k, &ctrl_k));
    }

    #[test]
    fn clashing_defaults_are_found() {
        let info = |key: &str, default| CommandInfo {
            key: key.into(),
            label: key.into(),
            group: MenuGroup::File,
            group_title: String::new(),
            section: MenuSection::Log,
            weight: 0,
            default,
            short_label: None,
            description: String::new(),
        };
        let alt = |k| KeyboardShortcut::new(Modifiers::ALT, k);
        let backup = info("backup/backup", Some(alt(Key::B)));
        let other = info("other/b", Some(alt(Key::B)));
        let free = info("free/x", Some(alt(Key::X)));
        let back = KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::B);
        assert!(default_clashes(&[backup.clone(), free.clone()], &[]).is_empty());
        assert_eq!(
            default_clashes(&[backup.clone(), other, free.clone()], &[]),
            [("backup/backup".to_string(), "other/b".to_string())]
        );
        assert_eq!(
            default_clashes(&[free, backup], &[back]),
            [("backup/backup".to_string(), "Alt+Shift+B".to_string())]
        );
    }
}
