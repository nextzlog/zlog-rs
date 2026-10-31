use std::sync::{Arc, Mutex};

use zlog_ui::{Section, SettingsPage};

use crate::manager::Manager;
use crate::panel::state_text;
use crate::protocol::{HookKind, ToHelper};
use crate::texts;

pub struct ZyloOptions {
    manager: Arc<Mutex<Manager>>,
    folder: Option<String>,
    message: Option<(String, bool)>,
}

impl Default for ZyloOptions {
    fn default() -> Self {
        Self::new(crate::manager::global())
    }
}

impl ZyloOptions {
    pub fn new(manager: Arc<Mutex<Manager>>) -> Self {
        ZyloOptions {
            manager,
            folder: None,
            message: None,
        }
    }

    fn add(&mut self, lang: &str) {
        let files = zlog_ui::dialog::import_files_of(
            texts().get(lang, "add_filter"),
            &["dll", "dylib", "so"],
        );
        let mut m = self.manager.lock().unwrap();
        for f in files {
            self.message = Some(match m.add(&f) {
                Ok(name) => (texts().format(lang, "added", &[&name]), false),
                Err(e) => (texts().format(lang, "add_failed", &[&e]), true),
            });
        }
    }
}

impl SettingsPage for ZyloOptions {
    fn id(&self) -> &str {
        "zylo"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "options.title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _section: &mut Section, lang: &str) {
        let t = texts();
        let folder = self
            .folder
            .get_or_insert_with(|| self.manager.lock().unwrap().dir.display().to_string());
        let changed = zlog_ui::field_row(
            ui,
            |ui| {
                ui.label(t.get(lang, "folder"));
            },
            |_| {},
            |ui| ui.add(egui::TextEdit::singleline(folder).desired_width(f32::INFINITY)),
        )
        .lost_focus();
        if changed {
            self.manager.lock().unwrap().dir = std::path::PathBuf::from(folder.trim());
        }
        ui.add(egui::Label::new(t.get(lang, "note")).wrap());
        if ui.button(t.get(lang, "add")).clicked() {
            self.add(lang);
        }
        ui.add(egui::Label::new(egui::RichText::new(t.get(lang, "add_help")).weak()).wrap());
        if let Some((m, warning)) = &self.message {
            let text = egui::RichText::new(m.as_str());
            let text = if *warning {
                text.color(zlog_ui::palette::error(ui))
            } else {
                text
            };
            ui.add(egui::Label::new(text).wrap());
        }
        ui.separator();
        let mut m = self.manager.lock().unwrap();
        let files = m.files();
        if files.is_empty() {
            ui.label(t.get(lang, "none"));
        }
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("zylo_list")
            .max_height((ui.available_height() / 2.0).max(80.0))
            .show(ui, |ui| {
                egui::Grid::new("zylo_plugins")
                    .striped(true)
                    .show(ui, |ui| {
                        for (name, here) in files {
                            let mut on = m.enabled.contains(&name);
                            if ui
                                .add_enabled(here || on, egui::Checkbox::new(&mut on, &name))
                                .on_hover_text(t.get(lang, "enable_help"))
                                .changed()
                            {
                                m.set_enabled(&name, on);
                            }
                            if here {
                                let mut rules = m.rules_only.contains(&name);
                                if ui.checkbox(&mut rules, t.get(lang, "rules_only")).changed() {
                                    m.set_rules_only(&name, rules);
                                }
                                let state = m.state(&name);
                                ui.label(state_text(lang, state.as_ref(), on));
                            } else {
                                ui.weak(t.get(lang, "other_platform"));
                                ui.label("");
                            }
                            if ui
                                .button(t.get(lang, "remove"))
                                .on_hover_text(t.get(lang, "remove_help"))
                                .clicked()
                            {
                                remove = Some(name.clone());
                            }
                            ui.end_row();
                        }
                    });
            });
        drop(m);
        running_ui(ui, &self.manager.lock().unwrap(), lang);
        if let Some(name) = remove {
            let mut m = self.manager.lock().unwrap();
            self.message = Some(match m.remove(&name) {
                Ok(()) => (t.format(lang, "removed", &[&name]), false),
                Err(e) => (t.format(lang, "remove_failed", &[&name, &e]), true),
            });
        }
    }
}

fn running_ui(ui: &mut egui::Ui, m: &Manager, lang: &str) {
    let t = texts();
    ui.separator();
    if m.enabled.is_empty() {
        ui.label(t.get(lang, "none_enabled"));
    }
    for name in &m.enabled {
        let Some(h) = m.host(name) else {
            continue;
        };
        let buttons: Vec<_> = h
            .hooks()
            .iter()
            .filter(|k| k.kind == HookKind::Button)
            .cloned()
            .collect();
        if buttons.is_empty() {
            continue;
        }
        ui.horizontal(|ui| {
            ui.label(name);
            for hook in buttons {
                if ui.button(&hook.name).clicked() {
                    h.send(ToHelper::Button { id: hook.id });
                }
            }
        });
    }
    ui.label(t.get(lang, "messages"));
    egui::ScrollArea::vertical()
        .id_salt("zylo_messages")
        .auto_shrink(false)
        .show(ui, |ui| {
            for msg in m.history.iter().rev() {
                ui.label(msg);
            }
        });
}
