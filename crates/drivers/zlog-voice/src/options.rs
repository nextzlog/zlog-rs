use egui::{DragValue, TextEdit, Ui};
use zlog_ui::device::{AudioDrv, DeviceDriver, DriverChoice};
use zlog_ui::{Section, SettingsPage};

use crate::{texts, Settings, SOUND};

pub const SECTION: &str = "panel.voice";

pub struct VoiceOptions {
    drivers: Vec<Box<dyn AudioDrv>>,
    device: Option<DriverChoice>,
}

impl VoiceOptions {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>) -> Self {
        VoiceOptions {
            drivers,
            device: None,
        }
    }
}

impl SettingsPage for VoiceOptions {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut Section, lang: &str) {
        let t = |k| texts().get(lang, k);
        let list: Vec<&dyn DeviceDriver> = self.drivers.iter().map(|d| &**d as _).collect();
        let labels = (lang, t("device"), t("none"));
        let device = &mut self.device;
        zlog_ui::device::own_sound_ui(ui, section, lang, |ui, section| {
            zlog_ui::device::page_ui(ui, device, section, SOUND, labels, &list);
        });
        ui.separator();
        let mut s = Settings::load(section);
        ui.checkbox(&mut s.ptt, t("ptt"));
        ui.weak(t("ptt_delay.shared"));
        let labels = [t("cq_repeat"), t("cq_max")];
        zlog_ui::form(ui, "voice-timing", &labels, |ui, _| {
            ui.label(t("cq_repeat"));
            ui.add(
                DragValue::new(&mut s.cq_repeat)
                    .range(0.0..=60.0)
                    .speed(0.1)
                    .suffix(" s"),
            );
            ui.end_row();
            ui.label(t("cq_max"));
            ui.add(DragValue::new(&mut s.cq_max).range(1..=999));
            ui.end_row();
        });
        ui.separator();
        memories_ui(ui, &mut s, lang);
        s.save(section);
    }
}

fn memories_ui(ui: &mut Ui, s: &mut Settings, lang: &str) {
    let t = |k| texts().get(lang, k);
    let keys: Vec<String> = (1..=12).map(|n| format!("F{n}")).collect();
    let mut labels: Vec<&str> = keys.iter().map(String::as_str).collect();
    labels.push(t("browse"));
    zlog_ui::form(ui, "voice-memories-options", &labels, |ui, form| {
        for (n, key) in keys.iter().enumerate() {
            ui.label(key);
            form.text(
                ui,
                TextEdit::singleline(&mut s.comments[n]).hint_text(t("comment")),
            );
            ui.end_row();
            if ui.button(t("browse")).clicked() {
                if let Some(p) = zlog_ui::dialog::import_file("WAV", &["wav"]) {
                    s.files[n] = p.display().to_string();
                }
            }
            form.text(
                ui,
                TextEdit::singleline(&mut s.files[n]).hint_text(t("file")),
            );
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_keeps_the_panel_keys() {
        let mut section = Section::new();
        section.insert("file1".into(), "/voice/JA1ZLO.wav".into());
        section.insert("comment1".into(), "CQ".into());
        let mut page = VoiceOptions::new(Vec::new());
        assert_eq!(page.id(), SECTION);
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| page.ui(ui, &mut section, "en"));
        out.textures_delta.clear();
        assert_eq!(section["file1"], "/voice/JA1ZLO.wav");
        assert_eq!(section["comment1"], "CQ");
        assert_eq!(section["own_sound"], "false");
    }
}
