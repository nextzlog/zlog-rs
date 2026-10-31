use egui::{DragValue, TextEdit, Ui};
use zlog_ui::device::{AudioDrv, DeviceDriver, DriverChoice};
use zlog_ui::{Section, SettingsPage};

use crate::{texts, Settings, SOUND};

pub const SECTION: &str = "panel.voice";

pub struct VoiceOptions {
    drivers: Vec<Box<dyn AudioDrv>>,
    device: Option<DriverChoice>,
    selected: usize,
}

impl VoiceOptions {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>) -> Self {
        VoiceOptions {
            drivers,
            device: None,
            selected: 0,
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
        memories_ui(ui, &mut s, &mut self.selected, lang);
        s.save(section);
    }
}

fn memories_ui(ui: &mut Ui, s: &mut Settings, selected: &mut usize, lang: &str) {
    let t = |k| texts().get(lang, k);
    let height = ui.spacing().interact_size.y;
    for n in 0..12 {
        let name = std::path::Path::new(s.files[n].trim())
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let label = match s.comments[n].trim() {
            "" => format!("F{}", n + 1),
            c => format!("F{}  {c}", n + 1),
        };
        let row = egui::Button::selectable(*selected == n, label)
            .right_text(egui::RichText::new(name).weak())
            .min_size(egui::vec2(ui.available_width(), height));
        let r = ui.add(row);
        let r = match s.files[n].trim() {
            "" => r,
            f => r.on_hover_text(f),
        };
        if r.clicked() {
            *selected = n;
        }
    }
    ui.add_space(6.0);
    let n = (*selected).min(11);
    ui.strong(format!("F{}", n + 1));
    let labels = [t("comment"), t("file")];
    zlog_ui::form(ui, "voice-memory-detail", &labels, |ui, form| {
        ui.label(labels[0]);
        form.text(ui, TextEdit::singleline(&mut s.comments[n]));
        ui.end_row();
        ui.label(labels[1]);
        form.text(ui, TextEdit::singleline(&mut s.files[n]));
        ui.end_row();
        ui.label("");
        if ui
            .button(format!("{} {}", zlog_ui::icon::FOLDER_OPEN, t("browse")))
            .clicked()
        {
            if let Some(p) = zlog_ui::dialog::import_file("WAV", &["wav"]) {
                s.files[n] = p.display().to_string();
            }
        }
        ui.end_row();
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
