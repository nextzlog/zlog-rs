use egui::{DragValue, Slider, TextEdit, Ui};
use zlog_ui::device::{DeviceDriver, DriverChoice, NONE};
use zlog_ui::{Section, SettingsPage};

use crate::sidetone::{self, Sidetone};
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
        if self.device.as_ref().is_some_and(|d| d.id == SIMULATED) {
            sidetone_ui(ui, section, lang);
        }
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
        let mut fkeys = zlog_ui::fkeys::shown(section);
        ui.checkbox(&mut fkeys, t("fkeys"));
        zlog_ui::fkeys::set_shown(section, fkeys);
        ui.separator();
        ui.strong(t("messages"));
        ui.label(t("macro_help"));
        let [a, b] = &mut s.banks;
        zlog_ui::fkeys::message_table(ui, "keyer-bank", &mut [(t("bank_a"), a), (t("bank_b"), b)]);
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

pub const SIMULATED: &str = "simulated";

fn sidetone_ui(ui: &mut Ui, section: &mut Section, lang: &str) {
    let t = |k| texts().get(lang, k);
    let prefix = format!("{SIMULATED}.");
    let mut own: Section = section
        .iter()
        .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_string(), v.clone())))
        .collect();
    let mut s = Sidetone::load(&own);
    let labels = [t("sidetone"), t("sidetone_pitch"), t("sidetone_volume")];
    zlog_ui::form(ui, "keyer-sidetone", &labels, |ui, _| {
        ui.label(t("sidetone")).on_hover_text(t("sidetone.tip"));
        ui.checkbox(&mut s.on, "");
        ui.end_row();
        ui.label(t("sidetone_pitch"));
        ui.add_enabled(
            s.on,
            DragValue::new(&mut s.pitch)
                .range(sidetone::MIN_PITCH..=sidetone::MAX_PITCH)
                .speed(5)
                .suffix(" Hz"),
        );
        ui.end_row();
        ui.label(t("sidetone_volume"));
        ui.add_enabled_ui(s.on, |ui| {
            zlog_ui::layout::slider(
                ui,
                Slider::new(&mut s.volume, sidetone::MIN_VOLUME..=sidetone::MAX_VOLUME)
                    .suffix(" dBFS"),
            )
        });
        ui.end_row();
    });
    let before = own.clone();
    s.save(&mut own);
    if own != before {
        for (k, v) in own {
            section.insert(format!("{prefix}{k}"), v);
        }
    }
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

    #[test]
    fn the_sidetone_settings_stay_with_the_simulated_keyer() {
        let mut section = Section::new();
        section.insert("device".into(), SIMULATED.into());
        section.insert("simulated.sidetone_pitch".into(), "800".into());
        let mut page = KeyerOptions::new(Vec::new());
        let mut out = egui::Context::default().run_ui(Default::default(), |ui| {
            sidetone_ui(ui, &mut section, "ja");
            page.ui(ui, &mut section, "en")
        });
        out.textures_delta.clear();
        let choice = DriverChoice::load(&section, NONE, &[]);
        assert_eq!(choice.id, SIMULATED);
        let own: Section = section
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("simulated.")?.to_string(), v.clone())))
            .collect();
        let s = Sidetone::load(&own);
        assert_eq!((s.on, s.pitch, s.volume), (true, 800, -20));
    }
}
