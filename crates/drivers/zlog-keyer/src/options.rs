use egui::{DragValue, TextEdit, Ui};
use zlog_ui::device::{DeviceDriver, DriverChoice, NONE};
use zlog_ui::{Section, SettingsPage};

use crate::{morse, texts, KeyerDrv, Settings};

pub const SECTION: &str = "panel.keyer";

pub struct KeyerOptions {
    drivers: Vec<Box<dyn KeyerDrv>>,
    device: Option<DriverChoice>,
}

impl KeyerOptions {
    pub fn new(drivers: Vec<Box<dyn KeyerDrv>>) -> Self {
        KeyerOptions {
            drivers,
            device: None,
        }
    }
}

impl SettingsPage for KeyerOptions {
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
        zlog_ui::device::page_ui(ui, &mut self.device, section, NONE, labels, &list);
        ui.separator();
        let device = self
            .device
            .as_ref()
            .map(|d| d.section())
            .unwrap_or_default();
        let known = crate::driver::ptt_keys(section, &device);
        ptt_ui(ui, &known, section, lang);
        let mut s = Settings::load(section);
        timing_ui(ui, &mut s, lang);
        ui.separator();
        for (bank, key) in [(0, "bank_a"), (1, "bank_b")] {
            ui.strong(format!("{} ({})", t("messages"), t(key)));
            messages_ui(ui, &mut s.banks[bank], ("keyer-bank", bank));
        }
        ui.label(t("macro_help"));
        ui.separator();
        zlog_ui::form(ui, "keyer-pad", &[t("pad")], |ui, form| {
            ui.label(t("pad"));
            form.multiline(ui, TextEdit::multiline(&mut s.pad).desired_rows(3));
            ui.end_row();
        });
        s.save(section);
    }
}

pub fn ptt_ui(ui: &mut Ui, known: &Section, section: &mut Section, lang: &str) {
    let t = |k| texts().get(lang, k);
    let d = crate::driver::ptt_delays(known);
    let (mut before, mut after) = (d.before.as_millis() as u32, d.after.as_millis() as u32);
    zlog_ui::form(ui, "keyer-ptt", &[t("ptt_delay")], |ui, _| {
        ui.label(t("ptt_delay")).on_hover_text(t("ptt_delay.tip"));
        ui.horizontal(|ui| {
            ui.add(DragValue::new(&mut before).range(1..=255))
                .on_hover_text(t("ptt_before"));
            ui.add(DragValue::new(&mut after).range(0..=255))
                .on_hover_text(t("ptt_after"));
            ui.label("ms");
        });
        ui.end_row();
    });
    section.insert("ptt_before".into(), before.to_string());
    section.insert("ptt_after".into(), after.to_string());
}

fn timing_ui(ui: &mut Ui, s: &mut Settings, lang: &str) {
    let t = |k| texts().get(lang, k);
    let labels = [
        t("speed"),
        t("weight"),
        t("space_factor"),
        t("ei_space_factor"),
        t("cq_repeat"),
        t("cq_max"),
        t("abbrev"),
    ];
    zlog_ui::form(ui, "keyer-timing", &labels, |ui, _| {
        let tm = &mut s.timing;
        ui.label(t("speed"));
        ui.add(
            DragValue::new(&mut tm.wpm)
                .range(morse::MIN_WPM..=morse::MAX_WPM)
                .suffix(" WPM"),
        );
        ui.end_row();
        ui.label(t("weight"));
        ui.add(DragValue::new(&mut tm.weight).range(0..=100));
        ui.end_row();
        ui.label(t("space_factor"));
        ui.add(DragValue::new(&mut tm.space_factor).range(1..=500));
        ui.end_row();
        ui.label(t("ei_space_factor"));
        ui.add(DragValue::new(&mut tm.ei_space_factor).range(1..=500));
        ui.end_row();
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
        ui.label(t("abbrev"));
        ui.horizontal(|ui| {
            for c in [&mut s.abbrev.zero, &mut s.abbrev.one, &mut s.abbrev.nine] {
                let mut text = c.to_string();
                if ui
                    .add(TextEdit::singleline(&mut text).desired_width(16.0))
                    .changed()
                {
                    if let Some(n) = text.chars().last() {
                        *c = n.to_ascii_uppercase();
                    }
                }
            }
        });
        ui.end_row();
    });
    ui.checkbox(&mut s.not_send_leading_zeros, t("no_leading_zeros"));
}

fn messages_ui(ui: &mut Ui, bank: &mut [String; 12], id: (&str, usize)) {
    let keys: Vec<String> = (1..=12).map(|n| format!("F{n}")).collect();
    let labels: Vec<&str> = keys.iter().map(String::as_str).collect();
    zlog_ui::form(ui, id, &labels, |ui, form| {
        for (key, m) in keys.iter().zip(bank.iter_mut()) {
            ui.label(key);
            form.text(ui, TextEdit::singleline(m));
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
        section.insert("F1".into(), "CQ JA1ZLO".into());
        section.insert("F3B".into(), "TU JA1YWX".into());
        section.insert("cq_repeat".into(), "3".into());
        let s = Settings::load(&section);
        assert_eq!(s.banks[0][0], "CQ JA1ZLO");
        assert_eq!(s.banks[1][2], "TU JA1YWX");
        assert_eq!(s.cq_repeat, 3.0);
        let mut page = KeyerOptions::new(Vec::new());
        assert_eq!(page.id(), SECTION);
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| page.ui(ui, &mut section, "en"));
        out.textures_delta.clear();
        assert_eq!(section["F1"], "CQ JA1ZLO");
        assert_eq!(section["F3B"], "TU JA1YWX");
        assert_eq!(section["device"], NONE);
    }
}
