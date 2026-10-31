use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zlog_core::qso::QslState;
use zlog_core::{QsyRule, Station};
use zlog_ui::Section;

pub use zlog_ui::toolbar::ToolbarStyle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    pub qsy_rule: QsyRule,
    pub countdown_minutes: u32,
    pub qsy_per_hour: u32,
    pub qsl_default: QslState,
    pub pc_name: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            qsy_rule: QsyRule::default(),
            countdown_minutes: 10,
            qsy_per_hour: 8,
            qsl_default: QslState::None,
            pc_name: String::new(),
        }
    }
}

impl Preferences {
    pub fn set_countdown(&mut self, on: bool) {
        self.qsy_rule.countdown_minutes = on.then_some(self.countdown_minutes);
    }

    pub(crate) fn set_qsy_count(&mut self, on: bool) {
        self.qsy_rule.max_qsy_per_hour = on.then_some(self.qsy_per_hour);
    }
}

fn unescape(v: &str) -> String {
    let Some(inner) = v
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .filter(|_| v.len() >= 2)
    else {
        return v.to_string();
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(c) => out.push(c),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(any(test, feature = "testing"))]
thread_local! {
    pub static TEST_PATH: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

use zlog_ui::migration::Migration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub sections: BTreeMap<String, Section>,
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        #[cfg(any(test, feature = "testing"))]
        return TEST_PATH.with(|p| p.borrow().clone());
        #[cfg(not(any(test, feature = "testing")))]
        dirs::config_dir().map(|d| d.join(zlog_ui::APP_DIR).join("settings.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let mut s = Self::read(&path);
        s.move_old_keys(&crate::registry::migrations());
        s
    }

    fn read(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).unwrap_or_else(|_| {
                set_aside(path);
                Self::default()
            }),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                set_aside(path);
                Self::default()
            }
            Err(_) => {
                let old = std::fs::read_to_string(path.with_extension("ini"))
                    .map(|t| Self::parse_ini(&t));
                match old {
                    Ok(s) => {
                        let _ = s.save();
                        s
                    }
                    Err(_) => Self::default(),
                }
            }
        }
    }

    pub fn move_old_keys(&mut self, migrations: &[Migration]) {
        let renamed: Vec<(&str, &str)> = migrations
            .iter()
            .filter_map(|m| match m {
                Migration::RenamedPanel(old, new) => Some((*old, *new)),
                _ => None,
            })
            .collect();
        let merged: Vec<(&str, &str)> = migrations
            .iter()
            .filter_map(|m| match m {
                Migration::MergedPanel(old, new) => Some((*old, *new)),
                _ => None,
            })
            .collect();
        let moved: Vec<(&str, &str)> = migrations
            .iter()
            .filter_map(|m| match m {
                Migration::MovedCommand(old, new) => Some((*old, *new)),
                _ => None,
            })
            .collect();
        for &(old, new) in &renamed {
            if let Some(from) = self.sections.remove(&format!("panel.{old}")) {
                let to = self.section(&format!("panel.{new}"));
                for (k, v) in from {
                    to.entry(k).or_insert(v);
                }
            }
            for key in ["hidden_toolbars", "shown_toolbars", "toolbar_order"] {
                let Some(text) = self.sections.get("window").and_then(|w| w.get(key)) else {
                    continue;
                };
                let list: Vec<String> = text.split(',').map(|s| s.trim().to_string()).collect();
                if list.iter().any(|id| id == old) {
                    let mut renamed: Vec<&str> = Vec::new();
                    for id in list
                        .iter()
                        .map(|id| if id == old { new } else { id.as_str() })
                    {
                        if !renamed.contains(&id) {
                            renamed.push(id);
                        }
                    }
                    self.section("window").insert(key.into(), renamed.join(","));
                }
            }
        }
        self.move_panel_binds(&renamed);
        self.merge_old_panels(&merged);
        self.move_old_commands(&moved);
        for m in migrations {
            let Migration::CopiedKey { from, to } = m else {
                continue;
            };
            let Some(section) = self.sections.get_mut(from.0) else {
                continue;
            };
            let old = section.remove(from.1).filter(|n| !n.is_empty());
            if section.is_empty() {
                self.sections.remove(from.0);
            }
            let unset = self
                .sections
                .get(to.0)
                .and_then(|s| s.get(to.1))
                .is_none_or(|n| n.is_empty());
            if let (Some(value), true) = (old, unset) {
                self.section(to.0).insert(to.1.into(), value);
            }
        }
    }

    fn move_old_commands(&mut self, moved: &[(&str, &str)]) {
        let Some(bind) = self.sections.get_mut("bind") else {
            return;
        };
        for &(old, new) in moved {
            for prefix in ["", "name:", "short:", "tip:"] {
                if let Some(value) = bind.remove(&format!("{prefix}{old}")) {
                    bind.entry(format!("{prefix}{new}")).or_insert(value);
                }
            }
        }
    }

    fn merge_old_panels(&mut self, merged: &[(&str, &str)]) {
        for &(old, new) in merged {
            if let Some(from) = self.sections.remove(&format!("panel.{old}")) {
                let to = self.section(&format!("panel.{new}"));
                for (k, v) in from {
                    to.entry(k).or_insert(v);
                }
            }
        }
        let olds = |new: &str| -> Vec<&str> {
            merged
                .iter()
                .filter(|(_, n)| *n == new)
                .map(|(o, _)| *o)
                .collect()
        };
        let mut news: Vec<&str> = merged.iter().map(|(_, n)| *n).collect();
        news.dedup();
        let list = |s: &Self, key: &str| -> Option<Vec<String>> {
            s.sections.get("window").and_then(|w| w.get(key)).map(|t| {
                t.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
        };
        let is_old = |id: &str| merged.iter().any(|(o, _)| *o == id);
        let new_of = |id: &str| merged.iter().find(|(o, _)| *o == id).map(|(_, n)| *n);
        let (hidden, shown) = (list(self, "hidden_toolbars"), list(self, "shown_toolbars"));
        let touched = |l: &Option<Vec<String>>| l.iter().flatten().any(|id| is_old(id));
        if touched(&hidden) || touched(&shown) {
            let (mut h, mut sh) = (hidden.unwrap_or_default(), shown.unwrap_or_default());
            for new in &news {
                let all_hidden = olds(new).iter().all(|o| h.iter().any(|x| x == o));
                let any_shown = olds(new).iter().any(|o| sh.iter().any(|x| x == o));
                h.retain(|x| x != new);
                sh.retain(|x| x != new);
                if all_hidden {
                    h.push(new.to_string());
                } else if any_shown {
                    sh.push(new.to_string());
                }
            }
            h.retain(|x| !is_old(x));
            sh.retain(|x| !is_old(x));
            h.sort();
            sh.sort();
            let w = self.section("window");
            w.insert("hidden_toolbars".into(), h.join(","));
            w.insert("shown_toolbars".into(), sh.join(","));
        }
        if let Some(order) = list(self, "toolbar_order").filter(|l| l.iter().any(|x| is_old(x))) {
            let mut out: Vec<String> = Vec::new();
            for id in order {
                let id = new_of(&id).map_or(id, str::to_string);
                if !out.contains(&id) {
                    out.push(id);
                }
            }
            self.section("window")
                .insert("toolbar_order".into(), out.join(","));
        }
        self.move_panel_binds(merged);
    }

    fn move_panel_binds(&mut self, panels: &[(&str, &str)]) {
        let new_of = |id: &str| panels.iter().find(|(o, _)| *o == id).map(|(_, n)| *n);
        if let Some(bind) = self.sections.get_mut("bind") {
            let keys: Vec<String> = bind.keys().cloned().collect();
            for key in keys {
                let (prefix, rest) = ["name:", "short:", "tip:"]
                    .iter()
                    .find_map(|p| key.strip_prefix(p).map(|r| (*p, r)))
                    .unwrap_or(("", key.as_str()));
                let Some((id, command)) = rest.split_once('/') else {
                    continue;
                };
                let Some(new) = new_of(id) else {
                    continue;
                };
                let renamed = format!("{prefix}{new}/{command}");
                let value = bind.remove(&key).unwrap_or_default();
                bind.entry(renamed).or_insert(value);
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = path.with_extension("toml.part");
        std::fs::write(&part, self.format())?;
        std::fs::rename(part, path)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let mut sections = BTreeMap::new();
        for (name, value) in table {
            let toml::Value::Table(t) = value else {
                continue;
            };
            let section: Section = t
                .into_iter()
                .filter_map(|(k, v)| {
                    let v = match v {
                        toml::Value::String(s) => s,
                        toml::Value::Integer(i) => i.to_string(),
                        toml::Value::Float(f) => f.to_string(),
                        toml::Value::Boolean(b) => b.to_string(),
                        _ => return None,
                    };
                    Some((k, v))
                })
                .collect();
            sections.insert(name, section);
        }
        Ok(Settings { sections })
    }

    pub fn parse_ini(text: &str) -> Self {
        let mut sections: BTreeMap<String, Section> = BTreeMap::new();
        let mut current = String::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                current = name.to_string();
            } else if let Some((k, v)) = line.split_once('=') {
                sections
                    .entry(current.clone())
                    .or_default()
                    .insert(k.trim().into(), unescape(v.trim()));
            }
        }
        Settings { sections }
    }

    pub fn format(&self) -> String {
        toml::to_string(&self.sections).unwrap_or_default()
    }

    pub fn only(name: &str, section: &Section) -> Self {
        let mut s = Settings::default();
        s.sections.insert(name.to_string(), section.clone());
        s
    }

    pub fn take(&mut self, name: &str) -> Section {
        self.sections.remove(name).unwrap_or_default()
    }

    pub(crate) fn sections_named(&mut self, names: &[&str]) -> zlog_ui::Sections {
        names
            .iter()
            .map(|n| (n.to_string(), self.section(n).clone()))
            .collect()
    }

    pub(crate) fn set_sections(&mut self, sections: zlog_ui::Sections) {
        self.sections.extend(sections);
    }

    pub fn sections(&self) -> &BTreeMap<String, Section> {
        &self.sections
    }

    pub fn language(&mut self) -> Option<String> {
        self.section("window").get("language").cloned()
    }

    pub fn set_language(&mut self, code: &str) {
        self.section("window")
            .insert("language".into(), code.into());
    }

    pub fn section(&mut self, name: &str) -> &mut Section {
        self.sections.entry(name.to_string()).or_default()
    }

    pub(crate) fn keep_dialogs_on_top(&mut self) -> bool {
        self.section("window")
            .get("keep_on_top")
            .is_some_and(|v| v == "1")
    }

    pub(crate) fn set_keep_dialogs_on_top(&mut self, on: bool) {
        let v = if on { "1" } else { "0" };
        self.section("window")
            .insert("keep_on_top".into(), v.into());
    }

    pub(crate) fn reopen_windows(&mut self) -> bool {
        self.section("window")
            .get("reopen")
            .is_none_or(|v| v != "0")
    }

    pub(crate) fn set_reopen_windows(&mut self, on: bool) {
        let v = if on { "1" } else { "0" };
        self.section("window").insert("reopen".into(), v.into());
    }

    pub(crate) fn toolbar_style(&mut self) -> ToolbarStyle {
        zlog_ui::toolbar::style(self.section(zlog_ui::toolbar::SECTION))
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn set_toolbar_style(&mut self, style: ToolbarStyle) {
        zlog_ui::toolbar::set_style(self.section(zlog_ui::toolbar::SECTION), style);
    }

    pub(crate) fn toolbar_shown(&mut self, id: &str, default: bool) -> bool {
        zlog_ui::toolbar::shown(self.section(zlog_ui::toolbar::SECTION), id, default)
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn set_toolbar_shown(&mut self, id: &str, shown: bool) {
        zlog_ui::toolbar::set_shown(self.section(zlog_ui::toolbar::SECTION), id, shown);
    }

    pub fn toolbar_order(&mut self) -> Vec<String> {
        zlog_ui::toolbar::order(self.section(zlog_ui::toolbar::SECTION))
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn set_toolbar_order(&mut self, keys: &[String]) {
        zlog_ui::toolbar::set_order(self.section(zlog_ui::toolbar::SECTION), keys);
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn reset_toolbar(&mut self) {
        zlog_ui::toolbar::reset(self.section(zlog_ui::toolbar::SECTION));
    }

    pub fn station(&mut self) -> Station {
        zlog_ui::read_station(self.section("station"))
    }

    pub fn use_contest_period(&mut self) -> bool {
        self.section("preferences")
            .get("use_contest_period")
            .is_none_or(|v| v != "false")
    }

    pub(crate) fn set_use_contest_period(&mut self, on: bool) {
        self.section("preferences")
            .insert("use_contest_period".into(), on.to_string());
    }

    pub fn entry_style(&mut self) -> String {
        self.section("preferences")
            .get("entry_style")
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn set_entry_style(&mut self, id: &str) {
        self.section("preferences")
            .insert("entry_style".into(), id.into());
    }

    pub fn preferences(&mut self) -> Preferences {
        let d = Preferences::default();
        let s = self.section("preferences");
        let num = |k: &str, d: u32| {
            s.get(k)
                .and_then(|v| v.parse::<u32>().ok())
                .map_or(d, |n| n.clamp(1, 99))
        };
        let on = |k: &str| s.get(k).is_some_and(|v| v == "true");
        let countdown_minutes = num("countdown_minutes", d.countdown_minutes);
        let qsy_per_hour = num("qsy_per_hour", d.qsy_per_hour);
        let qsy_rule = QsyRule {
            countdown_minutes: on("countdown").then_some(countdown_minutes),
            max_qsy_per_hour: on("qsy_count").then_some(qsy_per_hour),
        };
        let qsl_default = s
            .get("qsl_default")
            .and_then(|v| v.parse::<u8>().ok())
            .map_or(QslState::None, QslState::from_byte);
        let pc_name = s.get("pc_name").cloned().unwrap_or_default();
        Preferences {
            qsy_rule,
            countdown_minutes,
            qsy_per_hour,
            qsl_default,
            pc_name,
        }
    }

    pub fn set_preferences(&mut self, p: &Preferences) {
        let s = self.section("preferences");
        let rule = p.qsy_rule;
        s.insert(
            "countdown".into(),
            rule.countdown_minutes.is_some().to_string(),
        );
        s.insert("countdown_minutes".into(), p.countdown_minutes.to_string());
        s.insert(
            "qsy_count".into(),
            rule.max_qsy_per_hour.is_some().to_string(),
        );
        s.insert("qsy_per_hour".into(), p.qsy_per_hour.to_string());
        s.insert("qsl_default".into(), p.qsl_default.to_byte().to_string());
        s.insert("pc_name".into(), p.pc_name.clone());
    }

    pub(crate) fn set_station(&mut self, st: &Station) {
        zlog_ui::write_station(self.section("station"), st);
    }
}

fn set_aside(path: &Path) {
    let first = path.with_extension("toml.broken");
    let to = std::iter::once(first.clone())
        .chain((1..).map(|n| path.with_extension(format!("toml.broken.{n}"))))
        .find(|p| !p.exists())
        .unwrap_or(first);
    let _ = std::fs::rename(path, to);
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::{Band, Power};

    #[test]
    fn a_renamed_toolbar_listed_twice_is_listed_once() {
        let mut s = Settings::default();
        s.section("window")
            .insert("toolbar_order".into(), "grid,map,old_map".into());
        s.section("window")
            .insert("hidden_toolbars".into(), "old_map".into());
        s.move_old_keys(&[Migration::RenamedPanel("old_map", "map")]);
        assert_eq!(s.section("window")["toolbar_order"], "grid,map");
        assert_eq!(s.section("window")["hidden_toolbars"], "map");
    }

    #[test]
    fn a_renamed_panel_moves_its_binds_and_merges_into_an_existing_section() {
        let mut s = Settings::default();
        let bind = s.section("bind");
        for (k, v) in [
            ("zserver/connect", "Ctrl+J"),
            ("name:zserver/connect", "Join"),
            ("tip:zserver/connect", "tip"),
            ("link/connect", "Ctrl+K"),
            ("grid/sort", "Ctrl+G"),
        ] {
            bind.insert(k.into(), v.into());
        }
        s.section("panel.zserver").insert("host".into(), "a".into());
        s.section("panel.zserver").insert("port".into(), "1".into());
        s.section("panel.link").insert("host".into(), "b".into());
        s.move_old_keys(&[Migration::RenamedPanel("zserver", "link")]);
        let bind = &s.sections["bind"];
        assert_eq!(bind["link/connect"], "Ctrl+K");
        assert_eq!(bind["name:link/connect"], "Join");
        assert_eq!(bind["tip:link/connect"], "tip");
        assert_eq!(bind["grid/sort"], "Ctrl+G");
        assert!(!bind.keys().any(|k| k.contains("zserver")));
        assert!(!s.sections.contains_key("panel.zserver"));
        assert_eq!(s.sections["panel.link"]["host"], "b");
        assert_eq!(s.sections["panel.link"]["port"], "1");
    }

    #[test]
    fn a_copied_key_moves_once_so_a_cleared_value_stays_cleared() {
        let m = [Migration::CopiedKey {
            from: ("old", "k"),
            to: ("new", "k"),
        }];
        let mut s = Settings::default();
        s.section("old").insert("k".into(), "v".into());
        s.move_old_keys(&m);
        assert_eq!(s.section("new")["k"], "v");
        assert!(!s.sections.contains_key("old"));
        s.section("new").insert("k".into(), String::new());
        s.move_old_keys(&m);
        assert_eq!(s.section("new")["k"], "");
    }

    #[test]
    fn a_second_broken_file_keeps_the_first() {
        let dir = std::env::temp_dir().join(format!("zlog-settings-twice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        for text in ["[first\n", "[second\n"] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(Settings::read(&path), Settings::default());
        }
        let read = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap();
        assert_eq!(read("settings.toml.broken"), "[first\n");
        assert_eq!(read("settings.toml.broken.1"), "[second\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn window_settings_round_trip() {
        let mut s = Settings::default();
        assert!(!s.keep_dialogs_on_top());
        assert!(s.toolbar_shown("score", true));
        assert!(!s.toolbar_shown("rate", false));
        s.set_keep_dialogs_on_top(true);
        s.set_toolbar_shown("score", false);
        s.set_toolbar_shown("multipliers", false);
        s.set_toolbar_shown("multipliers", true);
        s.set_toolbar_shown("rate", true);
        let mut back = Settings::parse(&s.format()).unwrap();
        assert!(back.keep_dialogs_on_top());
        assert!(!back.toolbar_shown("score", true));
        assert!(back.toolbar_shown("multipliers", false));
        assert!(back.toolbar_shown("rate", false));
        assert!(!back.toolbar_shown("summary", false));
        back.set_toolbar_shown("rate", false);
        assert!(!back.toolbar_shown("rate", true));
        back.set_keep_dialogs_on_top(false);
        assert!(!Settings::parse(&back.format())
            .unwrap()
            .keep_dialogs_on_top());
    }

    #[test]
    fn toolbar_style_round_trip() {
        let mut s = Settings::default();
        assert_eq!(s.toolbar_style(), ToolbarStyle::IconsAndLabels);
        for style in ToolbarStyle::ALL {
            s.set_toolbar_style(style);
            assert_eq!(Settings::parse(&s.format()).unwrap().toolbar_style(), style);
        }
        s.section("window")
            .insert("toolbar_style".into(), "odd".into());
        assert_eq!(s.toolbar_style(), ToolbarStyle::IconsAndLabels);
    }

    #[test]
    fn toolbar_order_round_trips_and_resets() {
        let mut s = Settings::default();
        assert!(s.toolbar_order().is_empty());
        s.set_toolbar_style(ToolbarStyle::Icons);
        s.set_toolbar_shown("rate", true);
        s.set_toolbar_order(&["score".into(), "file.open".into()]);
        let mut back = Settings::parse(&s.format()).unwrap();
        assert_eq!(back.toolbar_order(), ["score", "file.open"]);
        back.reset_toolbar();
        assert!(back.toolbar_order().is_empty());
        assert!(!back.toolbar_shown("rate", false));
        assert_eq!(back.toolbar_style(), ToolbarStyle::Icons);
    }

    #[test]
    fn old_hidden_toolbars_are_kept() {
        let mut s = Settings::default();
        s.section("window")
            .insert("hidden_toolbars".into(), "multipliers".into());
        let mut back = Settings::parse(&s.format()).unwrap();
        assert!(!back.toolbar_shown("multipliers", true));
        assert!(back.toolbar_shown("score", true));
        assert!(!back.toolbar_shown("rate", false));
    }

    #[test]
    fn multi_line_values_round_trip() {
        let mut s = Settings::default();
        let pad = "CQ TEST\n[x]\nk = v\n back\\slash \"q\" ";
        s.section("keyer").insert("pad".into(), pad.into());
        s.section("keyer")
            .insert("path".into(), "C:\\new\\zlog".into());
        let back = Settings::parse(&s.format()).unwrap();
        assert_eq!(back, s);
        let old = Settings::parse_ini("[a]\np = C:\\new\\x\nq = say \"hi\" now\n");
        assert_eq!(old.sections["a"]["p"], "C:\\new\\x");
        assert_eq!(old.sections["a"]["q"], "say \"hi\" now");
    }

    #[test]
    fn old_ini_settings_become_toml() {
        let dir = std::env::temp_dir().join(format!("zlog-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.ini"),
            "[window]\nlanguage = ja\n[panel.x]\nk.y = \"a\\nb\"\n",
        )
        .unwrap();
        TEST_PATH.with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
        let mut s = Settings::load();
        assert_eq!(s.language().as_deref(), Some("ja"));
        assert_eq!(s.section("panel.x")["k.y"], "a\nb");
        let toml = std::fs::read_to_string(dir.join("settings.toml")).unwrap();
        assert_eq!(Settings::parse(&toml).unwrap(), s);
        std::fs::write(dir.join("settings.toml"), "[window\n").unwrap();
        assert_eq!(Settings::load(), Settings::default());
        assert!(dir.join("settings.toml.broken").exists());
        TEST_PATH.with(|p| *p.borrow_mut() = None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unreadable_settings_file_is_kept_aside() {
        let dir = std::env::temp_dir().join(format!("zlog-settings-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.toml"),
            b"[window]\nlanguage = \"\x93\xfa\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("settings.ini"), "[window]\nlanguage = en\n").unwrap();
        TEST_PATH.with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
        let mut s = Settings::load();
        assert_eq!(s.language(), None, "not taken from the old file");
        assert!(dir.join("settings.toml.broken").exists());
        TEST_PATH.with(|p| *p.borrow_mut() = None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hand_written_numbers_and_booleans_are_text() {
        let s = Settings::parse("[a]\nn = 3\nb = true\nl = [1]\n").unwrap();
        assert_eq!(s.sections["a"]["n"], "3");
        assert_eq!(s.sections["a"]["b"], "true");
        assert!(!s.sections["a"].contains_key("l"));
    }

    #[test]
    fn language_round_trip() {
        let mut s = Settings::default();
        assert_eq!(s.language(), None);
        s.set_language("en");
        assert_eq!(
            Settings::parse(&s.format()).unwrap().language().as_deref(),
            Some("en")
        );
    }

    #[test]
    fn round_trip() {
        let mut s = Settings::default();
        let mut st = Station {
            callsign: "JA1ZLO".into(),
            prov: "10".into(),
            age: "45".into(),
            cq_zone: "25".into(),
            iaru_zone: "45".into(),
            itu_region: "3".into(),
            iota: "AS007".into(),
            tx_nr: 3,
            handle_cw: "TAKA".into(),
            handle_ph: "Taka".into(),
            ..Station::default()
        };
        st.power.insert(Band::B430, Power::P002);
        s.set_station(&st);
        s.section("rig").insert("model".into(), "IC-7300".into());
        assert!(s.use_contest_period());
        s.set_use_contest_period(false);
        let back = Settings::parse(&s.format()).unwrap();
        assert_eq!(back, s);
        let mut back = back;
        assert_eq!(back.station(), st);
        assert!(!back.use_contest_period());
    }

    #[test]
    fn preferences_defaults_and_round_trip() {
        let mut s = Settings::default();
        assert_eq!(s.preferences(), Preferences::default());
        let mut p = Preferences {
            countdown_minutes: 15,
            qsl_default: QslState::NoQsl,
            pc_name: "PC1".into(),
            ..Preferences::default()
        };
        p.set_countdown(true);
        p.set_qsy_count(true);
        assert_eq!(p.qsy_rule.countdown_minutes, Some(15));
        assert_eq!(p.qsy_rule.max_qsy_per_hour, Some(8));
        s.set_preferences(&p);
        let mut back = Settings::parse(&s.format()).unwrap();
        assert_eq!(back.preferences(), p);
        p.set_countdown(false);
        assert_eq!(p.qsy_rule.countdown_minutes, None);
        assert_eq!(p.countdown_minutes, 15, "kept while off");
    }
}
