//! Settings kept between runs, in one file in the system's configuration folder
//! (`%APPDATA%` on Windows, `~/Library/Application Support` on macOS).
//!
//! The file has sections of `key = value` lines; each part of the application and
//! each extension owns one section and sees no other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zlog_core::qso::QslState;
use zlog_core::{QsyRule, Station};
use zlog_ui::Section;

/// What the toolbar buttons show (the toolbar tab of Options).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarStyle {
    Icons,
    #[default]
    IconsAndLabels,
    Labels,
}

impl ToolbarStyle {
    pub const ALL: [ToolbarStyle; 3] = [
        ToolbarStyle::Icons,
        ToolbarStyle::IconsAndLabels,
        ToolbarStyle::Labels,
    ];

    /// The value in the settings file, also the key of its name in the lang files
    /// (`toolbar.style.<code>`).
    pub fn code(self) -> &'static str {
        match self {
            ToolbarStyle::Icons => "icons",
            ToolbarStyle::IconsAndLabels => "both",
            ToolbarStyle::Labels => "labels",
        }
    }
}

/// Preferences applied to the open log (zlog.ini `[Preferences]` and `[Z-Link]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    /// `CountDown` with `CountDownMinute` (default off, 10) and `QSYCount` with
    /// `CountPerHour` (default off, 8).
    pub qsy_rule: QsyRule,
    /// Minutes of the countdown rule kept while it is off (`CountDownMinute`).
    pub countdown_minutes: u32,
    /// Band changes per hour kept while that rule is off (`CountPerHour`).
    pub qsy_per_hour: u32,
    /// `QslDefault` (default 0, none): the QSL state not written into `.ZLO` memos.
    pub qsl_default: QslState,
    /// `[Z-Link] PCName`: this PC's name, stored in logged QSOs and sent to Z-Server.
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
    /// Turn the countdown rule on or off, keeping its minutes.
    pub fn set_countdown(&mut self, on: bool) {
        self.qsy_rule.countdown_minutes = on.then_some(self.countdown_minutes);
    }

    /// Turn the band-changes-per-hour rule on or off, keeping its count.
    pub fn set_qsy_count(&mut self, on: bool) {
        self.qsy_rule.max_qsy_per_hour = on.then_some(self.qsy_per_hour);
    }
}

/// The value of a line of the old `settings.ini`: as it is, or in double quotes
/// with `\\`, `\n` and `\r` escaped (backslashes outside quotes are literal).
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

#[cfg(test)]
thread_local! {
    /// The settings file of the test running on this thread ([`Settings::path`]).
    pub static TEST_PATH: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Windows whose identifier changed with their crate's name: old, new.
const RENAMED_PANELS: [(&str, &str); 3] =
    [("maplot", "map"), ("country", "qth"), ("zserver", "link")];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    sections: BTreeMap<String, Section>,
}

impl Settings {
    /// The settings file, TOML: one table a section, every value a string.
    pub fn path() -> Option<PathBuf> {
        // Tests keep their settings in a file of their own, or none at all.
        #[cfg(test)]
        return TEST_PATH.with(|p| p.borrow().clone());
        #[cfg(not(test))]
        dirs::config_dir().map(|d| d.join(zlog_ui::APP_DIR).join("settings.toml"))
    }

    /// The saved settings, or empty ones when there are none yet. Without a
    /// `settings.toml`, the `settings.ini` of earlier versions next to it is read
    /// and saved as TOML (the old file is left as it is). A file that cannot be
    /// read as TOML is kept aside as `settings.toml.broken` (`.broken.1` and on when
    /// that is taken, so an earlier one is never lost) and empty settings
    /// are used, rather than overwriting it.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let mut s = Self::read(&path);
        s.move_old_keys();
        s
    }

    fn read(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).unwrap_or_else(|_| {
                set_aside(path);
                Self::default()
            }),
            // There but unreadable (not UTF-8, say): kept aside as well.
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

    /// Keys of earlier versions moved to where they are read now: the windows of
    /// [`RENAMED_PANELS`] under their new names, and, so every tab of the Options
    /// dialog sees them in its own section, the PC name of `[zserver] pc-name`
    /// (kept there too).
    fn move_old_keys(&mut self) {
        // Windows renamed with their crates: their settings and toolbar choices.
        for (old, new) in RENAMED_PANELS {
            let (from, to) = (format!("panel.{old}"), format!("panel.{new}"));
            if !self.sections.contains_key(&to) {
                if let Some(s) = self.sections.remove(&from) {
                    self.sections.insert(to, s);
                }
            }
            for key in ["hidden_toolbars", "shown_toolbars", "toolbar_order"] {
                // Read without making the section, which only a change writes.
                let Some(text) = self.sections.get("window").and_then(|w| w.get(key)) else {
                    continue;
                };
                let list: Vec<String> = text.split(',').map(|s| s.trim().to_string()).collect();
                if list.iter().any(|id| id == old) {
                    let renamed: Vec<&str> = list
                        .iter()
                        .map(|id| if id == old { new } else { id.as_str() })
                        .collect();
                    self.section("window").insert(key.into(), renamed.join(","));
                }
            }
        }
        let old = self
            .sections
            .get("zserver")
            .and_then(|s| s.get("pc-name"))
            .filter(|n| !n.is_empty())
            .cloned();
        let unset = self
            .sections
            .get("preferences")
            .and_then(|s| s.get("pc_name"))
            .is_none_or(|n| n.is_empty());
        if let (Some(name), true) = (old, unset) {
            self.section("preferences").insert("pc_name".into(), name);
        }
    }

    /// Write the settings, through a temporary file so a crash never leaves half
    /// a file.
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

    /// Settings from TOML. Numbers and booleans written by hand are taken as
    /// their text; arrays and nested tables are ignored.
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

    /// Settings from the `settings.ini` of earlier versions.
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

    /// Settings holding only `section` under `name` (a tab of the Options dialog
    /// reads and writes its section through the typed accessors).
    pub fn only(name: &str, section: &Section) -> Self {
        let mut s = Settings::default();
        s.sections.insert(name.to_string(), section.clone());
        s
    }

    /// The section `name`, taken out.
    pub fn take(&mut self, name: &str) -> Section {
        self.sections.remove(name).unwrap_or_default()
    }

    /// Copies of the sections `names` (those of the start screen), made when missing.
    pub fn sections_named(&mut self, names: &[&str]) -> zlog_ui::Sections {
        names
            .iter()
            .map(|n| (n.to_string(), self.section(n).clone()))
            .collect()
    }

    /// Put back sections taken with [`Settings::sections_named`].
    pub fn set_sections(&mut self, sections: zlog_ui::Sections) {
        self.sections.extend(sections);
    }

    /// Every section, by name (for converting the settings as a whole).
    #[cfg(feature = "ini")]
    pub fn sections(&self) -> &BTreeMap<String, Section> {
        &self.sections
    }

    /// The language chosen in the Language menu, if any (`window` section).
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

    /// Whether dialogs float above other windows (Window menu, `window` section).
    pub fn keep_dialogs_on_top(&mut self) -> bool {
        self.section("window")
            .get("keep_on_top")
            .is_some_and(|v| v == "1")
    }

    pub fn set_keep_dialogs_on_top(&mut self, on: bool) {
        let v = if on { "1" } else { "0" };
        self.section("window")
            .insert("keep_on_top".into(), v.into());
    }

    /// What the toolbar buttons show (`window` section, `toolbar_style`).
    pub fn toolbar_style(&mut self) -> ToolbarStyle {
        let v = self.section("window").get("toolbar_style").cloned();
        ToolbarStyle::ALL
            .into_iter()
            .find(|s| v.as_deref() == Some(s.code()))
            .unwrap_or_default()
    }

    pub fn set_toolbar_style(&mut self, style: ToolbarStyle) {
        self.section("window")
            .insert("toolbar_style".into(), style.code().into());
    }

    /// Whether the toolbar buttons of the panel `id` are shown (the toolbar tab):
    /// as the operator chose, else `default` ([`zlog_ui::Panel::toolbar_default_shown`]).
    /// Choices are kept in two lists so a changed default never overrides one.
    pub fn toolbar_shown(&mut self, id: &str, default: bool) -> bool {
        if self.toolbar_list("hidden_toolbars").iter().any(|h| h == id) {
            false
        } else if self.toolbar_list("shown_toolbars").iter().any(|h| h == id) {
            true
        } else {
            default
        }
    }

    pub fn set_toolbar_shown(&mut self, id: &str, shown: bool) {
        for (key, add) in [("hidden_toolbars", !shown), ("shown_toolbars", shown)] {
            let mut list = self.toolbar_list(key);
            list.retain(|h| h != id);
            if add {
                list.push(id.to_string());
            }
            list.sort();
            self.section("window").insert(key.into(), list.join(","));
        }
    }

    /// The order the operator gave the toolbar buttons, by their keys
    /// (`toolbar_order`); empty for the order of the menus. Keys it does not name
    /// (an extension added since) follow in the order of the menus.
    pub fn toolbar_order(&mut self) -> Vec<String> {
        self.toolbar_list("toolbar_order")
    }

    pub fn set_toolbar_order(&mut self, keys: &[String]) {
        self.section("window")
            .insert("toolbar_order".into(), keys.join(","));
    }

    /// The toolbar's buttons as at first: shown by default, in the order of the
    /// menus (the style is kept).
    pub fn reset_toolbar(&mut self) {
        let w = self.section("window");
        for key in ["hidden_toolbars", "shown_toolbars", "toolbar_order"] {
            w.remove(key);
        }
    }

    fn toolbar_list(&mut self, key: &str) -> Vec<String> {
        self.section("window")
            .get(key)
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The station, from the `station` section.
    pub fn station(&mut self) -> Station {
        zlog_ui::read_station(self.section("station"))
    }

    /// zLog's `UseContestPeriod` (`[Preferences]`), on by default.
    pub fn use_contest_period(&mut self) -> bool {
        self.section("preferences")
            .get("use_contest_period")
            .is_none_or(|v| v != "false")
    }

    pub fn set_use_contest_period(&mut self, on: bool) {
        self.section("preferences")
            .insert("use_contest_period".into(), on.to_string());
    }

    /// The entry style chosen in Options ([`zlog_ui::EntryStyle::id`]), empty for
    /// the first linked in.
    pub fn entry_style(&mut self) -> String {
        self.section("preferences")
            .get("entry_style")
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_entry_style(&mut self, id: &str) {
        self.section("preferences")
            .insert("entry_style".into(), id.into());
    }

    /// The preferences, from the `preferences` section. A PC name missing there is
    /// read from the old key of the Z-Link panel (`zserver`, `pc-name`).
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

    pub fn set_station(&mut self, st: &Station) {
        zlog_ui::write_station(self.section("station"), st);
    }
}

/// Move the settings file at `path` aside to the first free `.broken` name.
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

    /// Regression G16: a second broken file replaced the first one kept aside.
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
        // The operator's choices win over either default.
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

    /// A settings file from before default-hidden toolbars (only `hidden_toolbars`)
    /// keeps its meaning.
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
        // Old files: backslashes and quotes inside values are taken as they are.
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
        // A broken file is kept aside, not overwritten.
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
        // Not UTF-8 (Shift_JIS from a hand edit), and an old file beside it.
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
    fn renamed_windows_keep_their_settings_and_toolbars() {
        let mut s = Settings::parse_ini(
            "[panel.zserver]\nhost = h\n[panel.maplot]\nk = v\n[window]\nhidden_toolbars = score,maplot\nshown_toolbars = country\n",
        );
        s.move_old_keys();
        assert_eq!(s.section("panel.link")["host"], "h");
        assert_eq!(s.section("panel.map")["k"], "v");
        assert!(!s.sections.contains_key("panel.zserver"));
        assert_eq!(s.section("window")["hidden_toolbars"], "score,map");
        assert_eq!(s.section("window")["shown_toolbars"], "qth");
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
        // zLog: CountDown off/10, QSYCount off/8, QslDefault 0, PCName empty.
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

    #[test]
    fn pc_name_falls_back_to_the_old_zserver_key() {
        let mut s = Settings::parse_ini("[zserver]\npc-name = OLD\n");
        s.move_old_keys();
        assert_eq!(s.preferences().pc_name, "OLD");
        // The preferences tab, which sees its own section only, shows it too.
        let mut tab = Settings::only("preferences", s.section("preferences"));
        assert_eq!(tab.preferences().pc_name, "OLD");
        let mut p = s.preferences();
        p.pc_name = "NEW".into();
        s.set_preferences(&p);
        assert_eq!(s.preferences().pc_name, "NEW");
    }
}
