//! The ZyLO tab of the Options dialog: the plugins of the plugin folder, added
//! from anywhere, removed, enabled and disabled.
//!
//! What the tab does takes effect at once (a plugin starts when enabled and stops
//! when disabled or removed), as file operations cannot wait for OK; the list is
//! saved with the plugin window's own settings.

use std::sync::{Arc, Mutex};

use zlog_ui::{Section, SettingsPage};

use crate::manager::Manager;
use crate::panel::state_text;
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
        let files = rfd::FileDialog::new()
            .add_filter(
                texts().get(lang, "add_filter"),
                &["dll", "dylib", "so", "DLL"],
            )
            .pick_files()
            .unwrap_or_default();
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
            .auto_shrink(false)
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
        if let Some(name) = remove {
            self.message = Some(match m.remove(&name) {
                Ok(()) => (t.format(lang, "removed", &[&name]), false),
                Err(e) => (t.format(lang, "remove_failed", &[&name, &e]), true),
            });
        }
    }
}
