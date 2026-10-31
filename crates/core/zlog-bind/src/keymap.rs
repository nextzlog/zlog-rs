use egui::KeyboardShortcut;
use zlog_ui::keys::{self, CommandInfo, CommandOverrides, Names, Overrides};
use zlog_ui::Section;

const NONE: &str = "none";

pub const NAME: &str = "name:";

pub const SHORT: &str = "short:";

pub const TIP: &str = "tip:";

pub fn names(section: &Section, prefix: &str) -> Names {
    section
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(prefix)?.to_string(), v.trim())))
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k, v.to_string()))
        .collect()
}

pub fn name<'a>(section: &'a Section, prefix: &str, command: &CommandInfo) -> Option<&'a str> {
    section
        .get(&format!("{prefix}{}", command.key))
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
}

pub fn set_name(section: &mut Section, prefix: &str, command: &CommandInfo, own: &str, name: &str) {
    let key = format!("{prefix}{}", command.key);
    let name = name.trim();
    if name.is_empty() || name == own {
        section.remove(&key);
    } else {
        section.insert(key, name.to_string());
    }
}

pub fn overrides(section: &Section) -> CommandOverrides {
    CommandOverrides {
        shortcuts: load(section),
        names: names(section, NAME),
        short_names: names(section, SHORT),
        descriptions: names(section, TIP),
    }
}

pub fn load(section: &Section) -> Overrides {
    section
        .iter()
        .filter(|(k, _)| ![NAME, SHORT, TIP].iter().any(|p| k.starts_with(p)))
        .filter_map(|(k, v)| match v.as_str() {
            NONE => Some((k.clone(), None)),
            text => keys::parse(text).map(|s| (k.clone(), Some(s))),
        })
        .collect()
}

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

pub fn current(section: &Section, command: &CommandInfo) -> Option<KeyboardShortcut> {
    match section.get(&command.key).map(String::as_str) {
        Some(NONE) => None,
        Some(text) => keys::parse(text).or(command.default),
        None => command.default,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Conflict {
    Plain,
    Reserved,
    Taken(String),
}

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
        .map(|c| Conflict::Taken(name(section, NAME, c).unwrap_or(&c.label).to_string()))
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
            short_label: None,
            description: String::new(),
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
        set(&mut s, &save, Some(ctrl(Key::S)));
        assert!(!s.contains_key("file/file.save"));
        assert_eq!(current(&s, &save), Some(ctrl(Key::S)));
    }

    #[test]
    fn names_are_kept_apart_from_shortcuts() {
        let save = info("file/file.save", Some(ctrl(Key::S)));
        let mut s = Section::new();
        let own = save.label.clone();
        set_name(&mut s, NAME, &save, &own, " Write ");
        set_name(&mut s, SHORT, &save, "Save", "Wr");
        set_name(&mut s, TIP, &save, "", "Mine");
        set(&mut s, &save, None);
        s.insert(format!("{NAME}gone/window"), "Gone".into());
        assert_eq!(name(&s, NAME, &save), Some("Write"));
        let o = overrides(&s);
        assert_eq!(o.names["file/file.save"], "Write");
        assert_eq!(o.short_names["file/file.save"], "Wr");
        assert_eq!(o.descriptions["file/file.save"], "Mine");
        assert_eq!(o.names["gone/window"], "Gone");
        assert_eq!(o.shortcuts.len(), 1, "names are no shortcuts");
        set_name(&mut s, NAME, &save, &own, &own);
        assert_eq!(name(&s, NAME, &save), None, "its own label");
        set_name(&mut s, SHORT, &save, "Save", "");
        assert_eq!(name(&s, SHORT, &save), None);
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
        assert_eq!(check(&s, shift_enter), Some(Conflict::Reserved));
        let b = KeyboardShortcut::new(Modifiers::SHIFT, Key::B);
        assert_eq!(check(&s, b), Some(Conflict::Plain));
        let f5 = KeyboardShortcut::new(Modifiers::NONE, Key::F5);
        assert_eq!(check(&s, f5), None);
        let save_as = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        assert_eq!(check(&s, save_as), None);
        assert_eq!(
            check(&s, ctrl(Key::S)),
            Some(Conflict::Taken("file/file.save".into()))
        );
        assert_eq!(check(&s, ctrl(Key::O)), None);
        set(&mut s, &save, None);
        assert_eq!(check(&s, ctrl(Key::S)), None);
        assert_eq!(conflict(&s, &commands, &[], &save, &ctrl(Key::S)), None);
    }
}
