//! The plugin window: the enabled plugins, whether each is running, the buttons
//! they hooked, and their recent messages. Plugins are added, removed, enabled
//! and disabled in the ZyLO tab of the Options dialog ([`crate::options`]).
//!
//! zLog's plugin menu holds the menu items plugins create through Win32 handles
//! (`GetUI`); here a plugin's hooked buttons and menu items (`zylo_allow_button`)
//! are buttons in this window. Its hooked edit fields (`zylo_allow_editor`) of the
//! callsign and number fields receive every character typed into them.

use std::sync::{Arc, Mutex};

use zlog_ui::{MenuGroup, Panel, Section, View};

use crate::host::State;
use crate::manager::Manager;
use crate::protocol::{HookKind, ToHelper};
use crate::texts;

pub struct ZyloPanel {
    manager: Arc<Mutex<Manager>>,
    last_call: String,
    last_number: String,
}

impl Default for ZyloPanel {
    fn default() -> Self {
        Self::new(crate::manager::global())
    }
}

impl ZyloPanel {
    pub fn new(manager: Arc<Mutex<Manager>>) -> Self {
        ZyloPanel {
            manager,
            last_call: String::new(),
            last_number: String::new(),
        }
    }
}

/// Characters typed since `old` became `new` (only additions at the end count).
fn typed(old: &str, new: &str) -> Vec<char> {
    new.strip_prefix(old)
        .map(|s| s.chars().collect())
        .unwrap_or_default()
}

/// The state shown for a plugin.
pub fn state_text(lang: &str, state: Option<&State>, enabled: bool) -> String {
    let t = texts();
    match state {
        None if enabled => t.get(lang, "state.waiting").into(),
        None => t.get(lang, "state.off").into(),
        Some(State::Starting) => t.get(lang, "state.starting").into(),
        Some(State::Ready) => t.get(lang, "state.ready").into(),
        Some(State::Stopped) => t.get(lang, "state.off").into(),
        Some(State::Disabled(why)) => t.format(lang, "state.disabled", &[why]),
    }
}

impl Panel for ZyloPanel {
    fn id(&self) -> &str {
        "zylo"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [520.0, 360.0]
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Settings
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Plugins, 0)
    }

    fn tick(&mut self, view: &View<'_>) {
        let mut m = self.manager.lock().unwrap();
        m.language = view.language.to_string();
        {
            let snapshot = m.snapshot();
            let mut s = snapshot.lock().unwrap();
            s.call = view.station.callsign.clone();
            s.band = view.band.label().to_string();
            s.mode = view.mode.label().to_string();
        }
        // Plugins start once the window is up, in the background.
        m.start_enabled();
        m.watch();
        let targets = m.targets();
        drop(m);
        let call = typed(&self.last_call, view.callsign);
        let number = typed(&self.last_number, view.number);
        if !call.is_empty() || !number.is_empty() {
            for h in &targets {
                for hook in h.hooks().iter().filter(|k| k.kind == HookKind::Editor) {
                    let keys = if hook.name.contains("Call") {
                        &call
                    } else if hook.name.contains("Number") {
                        &number
                    } else {
                        continue;
                    };
                    for c in keys {
                        h.send(ToHelper::Editor {
                            id: hook.id,
                            key: *c as i64,
                        });
                    }
                }
            }
        }
        self.last_call = view.callsign.to_string();
        self.last_number = view.number.to_string();
    }

    fn ui(&mut self, ui: &mut egui::Ui, view: &View<'_>) {
        let lang = view.language;
        let t = texts();
        let m = self.manager.lock().unwrap();
        ui.label(t.get(lang, "manage_in_options"));
        ui.separator();
        let names: Vec<String> = m.enabled.iter().cloned().collect();
        if names.is_empty() {
            ui.label(t.get(lang, "none_enabled"));
        }
        // The extensions and then the messages share the window's height.
        egui::ScrollArea::vertical()
            .id_salt("zylo_running")
            .max_height((ui.available_height() / 2.0).max(60.0))
            .show(ui, |ui| {
                for name in names {
                    ui.horizontal(|ui| {
                        ui.label(&name);
                        let state = m.state(&name);
                        ui.label(state_text(lang, state.as_ref(), true));
                        if let Some(h) = m.host(&name) {
                            for hook in h.hooks().iter().filter(|k| k.kind == HookKind::Button) {
                                if ui.button(&hook.name).clicked() {
                                    h.send(ToHelper::Button { id: hook.id });
                                }
                            }
                        }
                    });
                }
            });
        ui.separator();
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

    fn load(&mut self, section: &Section) {
        let mut m = self.manager.lock().unwrap();
        m.load(section);
    }

    fn save(&self, section: &mut Section) {
        self.manager.lock().unwrap().save(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_characters_added_at_the_end_are_typed() {
        assert_eq!(typed("JA", "JA1"), vec!['1']);
        assert_eq!(typed("", "AB"), vec!['A', 'B']);
        assert!(typed("JA1", "JA").is_empty());
        assert!(typed("JA", "JB").is_empty());
    }

    #[test]
    fn states_have_texts() {
        for lang in ["en", "ja"] {
            assert!(state_text(lang, Some(&State::Disabled("x".into())), true).contains('x'));
            assert!(!state_text(lang, None, false).is_empty());
        }
    }
}
