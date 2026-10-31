use zlog_ui::enter::{message_name, EnterSends, KEYS, SECTION};
use zlog_ui::{Section, SettingsPage};

use crate::texts;

#[derive(Default)]
pub struct EnterOptions;

impl SettingsPage for EnterOptions {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "enter.options.title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, language: &str) {
        let t = texts();
        let saved = EnterSends::load(section);
        let mut e = saved.clone();
        ui.checkbox(&mut e.on, t.get(language, "enter.options.enter_sends"))
            .on_hover_text(t.get(language, "enter.options.enter_sends.tip"));
        let mut nav = crate::row::navigator_on(section);
        if ui
            .checkbox(&mut nav, t.get(language, "nav.option"))
            .on_hover_text(t.get(language, "nav.option.tip"))
            .changed()
        {
            section.insert(crate::row::NAVIGATOR.into(), nav.to_string());
        }
        let mut fkeys = zlog_ui::fkeys::shown(section);
        if ui.checkbox(&mut fkeys, t.get(language, "fkeys")).changed() {
            zlog_ui::fkeys::set_shown(section, fkeys);
        }
        {
            let names: Vec<String> = KEYS.iter().map(|k| message_name(language, k)).collect();
            let labels: Vec<&str> = names.iter().map(String::as_str).collect();
            zlog_ui::form(ui, "entry_enter_keys", &labels, |ui, form| {
                for key in KEYS {
                    ui.label(message_name(language, key));
                    let n = e.key_mut(key);
                    form.combo(("entry_enter_key", key))
                        .selected_text(format!("F{n}"))
                        .show_ui(ui, |ui| {
                            for f in 1..=12u8 {
                                ui.selectable_value(n, f, format!("F{f}"));
                            }
                        });
                    ui.end_row();
                }
            });
        }
        ui.add_enabled_ui(e.on, |ui| {
            ui.checkbox(
                &mut e.dupe_sends,
                t.get(language, "enter.options.dupe_sends"),
            );
        });
        if e != saved {
            e.save(section);
        }
    }
}
