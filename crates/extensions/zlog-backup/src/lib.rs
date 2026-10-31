use std::path::Path;
use std::time::{Duration, Instant};

use egui::{Key, KeyboardShortcut, Modifiers};
use zlog_ui::migration::Migration;
use zlog_ui::{
    Command, EntryAction, MenuGroup, MenuSection, Panel, Section, SettingsPage, StatusItem, View,
};

pub mod copy;

pub use copy::{backup, BackupError};

zlog_core::texts!();

pub const ID: &str = "backup";
pub const FOLDER: &str = "BackUpPath";

pub const SETTLE: Duration = Duration::from_secs(2);

struct Pending {
    at: Instant,
    path: String,
    log: zlog_core::Log,
}

#[derive(Default)]
pub struct Backup {
    folder: String,
    asked: bool,
    seen: Option<(String, u64)>,
    pending: Option<Pending>,
    actions: Vec<EntryAction>,
}

impl Backup {
    fn due(
        &mut self,
        path: &str,
        changes: u64,
        log: &zlog_core::Log,
        now: Instant,
    ) -> Option<Pending> {
        let seen = self.seen.replace((path.to_string(), changes));
        if seen.as_ref().is_some_and(|(p, _)| p != path) {
            return self.pending.take();
        }
        let changed = seen.is_some_and(|(_, n)| n != changes);
        if changed && !self.folder.trim().is_empty() {
            self.pending = Some(Pending {
                at: now,
                path: path.to_string(),
                log: log.clone(),
            });
        }
        match &self.pending {
            Some(p) if now.duration_since(p.at) >= SETTLE => self.pending.take(),
            _ => None,
        }
    }

    fn write(&self, path: &str, log: &zlog_core::Log) -> Result<std::path::PathBuf, BackupError> {
        backup(Path::new(path), &self.folder, &copy::zlog_folder(), log)
    }

    fn flush(&mut self, p: Pending, lang: &str) {
        if let Err(e) = self.write(&p.path, &p.log) {
            let said = StatusItem::new(e.message(lang)).warning(true);
            self.actions.push(EntryAction::Say(said));
        }
    }

    fn run(&mut self, view: &View<'_>) {
        self.pending = None;
        let lang = view.language;
        let said = match self.write(view.log_path, view.log) {
            Ok(to) => StatusItem::new(texts().format(lang, "done", &[&to.display()])),
            Err(e) => StatusItem::new(e.message(lang)).warning(true),
        };
        self.actions.push(EntryAction::Say(said));
    }

    fn ask(&mut self, view: &View<'_>) {
        if view.log_path.is_empty() {
            let said = StatusItem::new(texts().get(view.language, "no_log")).warning(true);
            self.actions.push(EntryAction::Say(said));
            return;
        }
        if self.folder.trim().is_empty() {
            let start = Path::new(view.log_path)
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            match zlog_ui::dialog::pick_folder(&start) {
                Some(p) => self.folder = p.display().to_string(),
                None => return,
            }
        }
        self.run(view);
    }
}

impl Panel for Backup {
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
        MenuGroup::File
    }

    fn section(&self) -> (MenuSection, i32) {
        (MenuSection::Log, 50)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let t = texts();
        vec![Command::new(
            ID,
            t.get(language, "command"),
            t.get(language, "command.tip"),
        )
        .shortcut(KeyboardShortcut::new(Modifiers::ALT, Key::B))]
    }

    fn run_command(&mut self, id: &str) {
        if id == ID {
            self.asked = true;
        }
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        self.pending
            .as_ref()
            .map(|_| std::time::Duration::from_millis(500))
    }

    fn tick(&mut self, view: &View<'_>) {
        if let Some(p) = self.due(view.log_path, view.changes, view.log, Instant::now()) {
            self.flush(p, view.language);
        }
        if std::mem::take(&mut self.asked) {
            self.ask(view);
        }
    }

    fn on_exit(&mut self) {
        if let Some(p) = self.pending.take() {
            let _ = self.write(&p.path, &p.log);
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &Section) {
        self.folder = section.get(FOLDER).cloned().unwrap_or_default();
    }

    fn save(&self, section: &mut Section) {
        section.insert(FOLDER.into(), self.folder.clone());
    }

    fn migrations(&self) -> Vec<Migration> {
        vec![Migration::CopiedKey {
            from: ("Preferences", FOLDER),
            to: ("panel.backup", FOLDER),
        }]
    }
}

#[derive(Default)]
pub struct BackupOptions;

impl SettingsPage for BackupOptions {
    fn id(&self) -> &str {
        "panel.backup"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "options.title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, lang: &str) {
        let t = texts();
        let mut folder = section.get(FOLDER).cloned().unwrap_or_default();
        let before = folder.clone();
        let mut browse = false;
        zlog_ui::field_row(
            ui,
            |ui| {
                ui.label(t.get(lang, "options.folder"));
            },
            |ui| browse = ui.button(t.get(lang, "browse")).clicked(),
            |ui| {
                zlog_ui::form::text(ui, egui::TextEdit::singleline(&mut folder))
                    .on_hover_text(t.get(lang, "options.folder.tip"));
            },
        );
        if browse {
            if let Some(p) = zlog_ui::dialog::pick_folder(&folder) {
                folder = p.display().to_string();
            }
        }
        if folder != before {
            section.insert(FOLDER.into(), folder);
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(120, |_| Backup::default());
    b.page(600, |_| BackupOptions);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    fn log(n: usize) -> zlog_core::Log {
        zlog_core::Log {
            qsos: vec![zlog_core::Qso::default(); n],
            ..zlog_core::Log::default()
        }
    }

    fn watched(folder: &str) -> Backup {
        Backup {
            folder: folder.into(),
            ..Backup::default()
        }
    }

    #[test]
    fn every_change_is_backed_up_once_it_settles() {
        let mut b = watched("bak");
        let t = Instant::now();
        let l = log(1);
        let due = |b: &mut Backup, path, n, at| b.due(path, n, &l, at).is_some();
        assert!(!due(&mut b, "a.zlox", 0, t));
        assert!(!due(&mut b, "a.zlox", 1, t));
        assert!(!due(&mut b, "a.zlox", 2, t + SETTLE / 2));
        assert!(!due(&mut b, "a.zlox", 2, t + SETTLE));
        assert!(due(&mut b, "a.zlox", 2, t + SETTLE / 2 + SETTLE));
        assert!(!due(&mut b, "a.zlox", 2, t + SETTLE * 4));
        assert!(!due(&mut b, "a.zlox", 3, t + SETTLE * 4));
        let old = b.due("b.zlox", 9, &log(5), t + SETTLE * 4).unwrap();
        assert_eq!((old.path.as_str(), old.log.qsos.len()), ("a.zlox", 1));
        assert!(!due(&mut b, "b.zlox", 9, t + SETTLE * 9));
        let mut off = watched(" ");
        off.due("a.zlox", 0, &l, t);
        off.due("a.zlox", 1, &l, t);
        assert!(off.pending.is_none());
    }

    #[test]
    fn a_change_just_before_quitting_is_backed_up() {
        let dir = std::env::temp_dir().join(format!("zlog-backup-exit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("log")).unwrap();
        let path = dir.join("log/a.zlox").display().to_string();
        let mut b = watched(&dir.join("bak").display().to_string());
        let t = Instant::now();
        b.due(&path, 0, &log(0), t);
        b.due(&path, 1, &log(2), t);
        b.on_exit();
        let saved = std::fs::read(dir.join("bak/a.zlox.BAK")).unwrap();
        assert_eq!(saved, log(2).to_bytes(zlog_core::Format::Zlox));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_folder_is_kept_under_the_zlog_ini_key() {
        let mut s = Section::new();
        s.insert(FOLDER.into(), "D:\\bak".into());
        let mut b = Backup::default();
        b.load(&s);
        let mut out = Section::new();
        b.save(&mut out);
        assert_eq!(out[FOLDER], "D:\\bak");
    }
}
