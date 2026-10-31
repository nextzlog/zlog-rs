use std::path::{Path, PathBuf};

use zlog_net::Fetch;
use zlog_rule_dx::cty::Cty;
use zlog_ui::{Command, EntryAction, MenuGroup, MenuSection, Panel, Section, SettingsPage};
use zlog_ui::{StatusItem, View};

zlog_core::texts!();

pub const ID: &str = "cty";
pub const PAGE: &str = "panel.cty";
pub const URL: &str = "https://www.country-files.com/cty/cty.dat";
pub const AUTO: &str = "CheckAtStart";
pub const LAST: &str = "LastCheck";
pub const MIN_COUNTRIES: usize = 200;

pub fn version(text: &str) -> Option<String> {
    text.match_indices("=VER").find_map(|(at, _)| {
        let digits: String = text[at + 4..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        (digits.len() == 8).then_some(digits)
    })
}

pub fn validate(text: &str) -> Result<String, &'static str> {
    let head = text
        .trim_start()
        .get(..64)
        .unwrap_or(text)
        .to_ascii_lowercase();
    if head.starts_with('<') || head.contains("html") {
        return Err("invalid");
    }
    let countries = Cty::parse(text).countries().len() - 1;
    match version(text) {
        Some(v) if countries >= MIN_COUNTRIES => Ok(v),
        _ => Err("invalid"),
    }
}

pub fn installed_version(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    version(&String::from_utf8_lossy(&bytes))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Updated(String),
    Current(String),
}

pub fn install(path: &Path, text: &str) -> Result<Outcome, &'static str> {
    let new = validate(text)?;
    if installed_version(path).is_some_and(|old| old >= new) {
        return Ok(Outcome::Current(new));
    }
    let dir = path.parent().ok_or("no_folder")?;
    std::fs::create_dir_all(dir).map_err(|_| "no_folder")?;
    zlog_core::file::write_atomic(path, text.as_bytes()).map_err(|_| "no_folder")?;
    Ok(Outcome::Updated(new))
}

pub struct Updater {
    auto: bool,
    last: String,
    started: bool,
    asked: bool,
    job: Option<Fetch>,
    path: Option<PathBuf>,
    get: fn(&str) -> Fetch,
    reload: fn(),
    actions: Vec<EntryAction>,
}

impl Default for Updater {
    fn default() -> Self {
        Updater {
            auto: false,
            last: String::new(),
            started: false,
            asked: false,
            job: None,
            path: Cty::installed_path(),
            get: |url| Fetch::get(url),
            reload: Cty::reload,
            actions: Vec::new(),
        }
    }
}

impl Updater {
    fn say(&mut self, text: String, warning: bool) {
        let item = StatusItem::new(text).warning(warning);
        self.actions.push(EntryAction::Say(item));
    }

    fn step(&mut self, lang: &str) {
        let t = texts();
        let start = std::mem::take(&mut self.asked) || (self.auto && !self.started);
        self.started = true;
        if start && self.job.is_none() {
            self.job = Some((self.get)(URL));
            self.say(t.get(lang, "checking").into(), false);
        }
        let Some(result) = self.job.as_mut().and_then(Fetch::take) else {
            return;
        };
        self.job = None;
        self.last = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let outcome = result.map_err(|e| e.to_string()).and_then(|text| {
            let path = self.path.clone().ok_or("no_folder")?;
            install(&path, &text).map_err(|k| t.get(lang, k).to_string())
        });
        match outcome {
            Ok(Outcome::Updated(v)) => {
                (self.reload)();
                self.say(t.format(lang, "updated", &[&v]), false);
            }
            Ok(Outcome::Current(v)) => self.say(t.format(lang, "current", &[&v]), false),
            Err(e) => self.say(t.format(lang, "failed", &[&e]), true),
        }
    }
}

impl Panel for Updater {
    fn id(&self) -> &str {
        ID
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "command").into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Settings
    }

    fn section(&self) -> (MenuSection, i32) {
        (MenuSection::Other, 50)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let t = texts();
        vec![Command::new(
            ID,
            t.get(language, "command"),
            t.get(language, "command.tip"),
        )]
    }

    fn run_command(&mut self, id: &str) {
        if id == ID {
            self.asked = true;
        }
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        let pending = self.job.is_some() || self.asked || (self.auto && !self.started);
        pending.then(|| std::time::Duration::from_millis(250))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.step(view.language);
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &Section) {
        self.auto = section.get(AUTO).is_some_and(|v| v == "1");
        self.last = section.get(LAST).cloned().unwrap_or_default();
    }

    fn save(&self, section: &mut Section) {
        section.insert(AUTO.into(), if self.auto { "1" } else { "0" }.into());
        section.insert(LAST.into(), self.last.clone());
    }
}

#[derive(Default)]
pub struct CtyOptions;

impl SettingsPage for CtyOptions {
    fn id(&self) -> &str {
        PAGE
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "options.title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, lang: &str) {
        let t = texts();
        let version = Cty::installed_path()
            .and_then(|p| installed_version(&p))
            .unwrap_or_else(|| t.get(lang, "options.none").into());
        ui.label(t.format(lang, "options.version", &[&version]));
        let last = section
            .get(LAST)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| t.get(lang, "options.never").into());
        ui.label(t.format(lang, "options.last", &[&last]));
        let mut auto = section.get(AUTO).is_some_and(|v| v == "1");
        let box_ = egui::Checkbox::new(&mut auto, t.get(lang, "options.auto"));
        if ui
            .add(box_)
            .on_hover_text(t.get(lang, "options.auto.tip"))
            .changed()
        {
            section.insert(AUTO.into(), if auto { "1" } else { "0" }.into());
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(125, |_| Updater::default());
    b.page(430, |_| CtyOptions);
}

#[cfg(test)]
mod tests;
