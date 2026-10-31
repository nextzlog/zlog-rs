use egui::Ui;
use zlog_ui::SettingsPage;

use crate::control::{Control, Drivers, RigConfig, SIMULATOR};
use crate::sync::{LogMode, RigMode};
use crate::text;

pub struct RigPage {
    number: u8,
    drivers: Drivers,
}

impl RigPage {
    pub fn new(number: u8, drivers: Drivers) -> Self {
        RigPage { number, drivers }
    }
}

impl SettingsPage for RigPage {
    fn id(&self) -> &str {
        crate::pair::SECTIONS[usize::from(self.number == 2)]
    }

    fn section_kind(&self) -> zlog_ui::SectionKind {
        zlog_ui::SectionKind::Personal
    }

    fn title(&self, language: &str) -> String {
        let key = if self.number == 2 {
            "options.rig2"
        } else {
            "options.rig"
        };
        text(language, key)
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut zlog_ui::Section, language: &str) {
        let ports = self.drivers.ports();
        let mut rig = RigConfig::load(section, &self.drivers);
        let before = rig.port.clone();
        rig_page(ui, &mut rig, &ports, &self.drivers, language);
        rig.save(section, &self.drivers);
        if rig.port != before {
            section.insert(crate::control::CHOSEN.into(), "true".into());
        }
    }
}

fn rig_page(ui: &mut Ui, rig: &mut RigConfig, ports: &[String], drivers: &Drivers, lang: &str) {
    let tr = |key: &str| text(lang, key);
    let labels = [
        tr("rig.model"),
        tr("rig.port"),
        tr("rig.baud"),
        tr("rig.transverter"),
    ];
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    zlog_ui::form(ui, "rig", &labels, |ui, form| {
        ui.label(tr("rig.model"));
        form.combo("rig_model")
            .selected_text(drivers.model_name(rig.model))
            .show_ui(ui, |ui| {
                for (i, m) in drivers.models.iter().enumerate() {
                    if ui.selectable_value(&mut rig.model, i, m.name).clicked() {
                        rig.baud = m.baud;
                    }
                }
            });
        ui.end_row();
        ui.label(tr("rig.port"));
        let shown = if rig.port == SIMULATOR {
            tr("rig.simulator")
        } else {
            rig.port.clone()
        };
        form.combo("rig_port")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if drivers.simulator.is_some() {
                    ui.selectable_value(&mut rig.port, SIMULATOR.to_string(), tr("rig.simulator"));
                }
                for p in ports {
                    ui.selectable_value(&mut rig.port, p.clone(), p);
                }
            });
        ui.end_row();
        ui.label(tr("rig.baud"));
        form.combo("rig_baud")
            .selected_text(rig.baud.to_string())
            .show_ui(ui, |ui| {
                for b in [4800, 9600, 19200, 38400, 57600, 115200] {
                    ui.selectable_value(&mut rig.baud, b, b.to_string());
                }
            });
        ui.end_row();
        ui.label(tr("rig.transverter"));
        ui.add(egui::DragValue::new(&mut rig.transverter_khz).range(0..=300_000_000));
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut rig.init_freq_on_mode, tr("rig.init_freq_on_mode"));
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut rig.ignore_rig_mode, tr("rig.ignore_rig_mode"));
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut rig.rtty_keeps_rig_mode, tr("rig.rtty_keeps_rig_mode"));
        ui.end_row();
    });
}

pub fn rig_summary(rig: &Control, config: &RigConfig, lang: &str) -> (String, String) {
    let tr = |key: &str| text(lang, key);
    let info = if config.port == SIMULATOR {
        tr("rig.simulator").to_string()
    } else {
        format!(
            "{}  {}  {} bps",
            rig.drivers.model_name(config.model),
            config.port,
            config.baud
        )
    };
    (info, rig.problem(lang).unwrap_or_default())
}

pub fn sim_ui(ui: &mut Ui, rig: &Control, lang: &str) {
    let tr = |key: &str| text(lang, key);
    if let Some(sim) = rig.sim.as_ref() {
        zlog_ui::layout::caption(ui, &tr("rig.simulator"));
        let mut khz = sim.radio().freq() as f64 / 1000.0;
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("rig.sim.dial"));
            let dial = egui::DragValue::new(&mut khz)
                .speed(0.1)
                .range(1800.0..=10_250_000.0)
                .fixed_decimals(1);
            if ui.add(dial).changed() {
                sim.turn_dial((khz * 1000.0).round() as u64);
            }
            let mut on = sim.radio().power;
            if ui.checkbox(&mut on, tr("rig.sim.power")).changed() {
                sim.with(|r| r.power = on);
            }
        });
        let modes = [
            RigMode::Cw,
            RigMode::Lsb,
            RigMode::Usb,
            RigMode::Fm,
            RigMode::Am,
            RigMode::Rtty,
        ];
        zlog_ui::layout::grid(ui, modes.len(), 52.0, |ui, i| {
            if ui
                .button(modes[i].text())
                .on_hover_text(tr("rig.sim.mode"))
                .clicked()
            {
                sim.press_mode(modes[i]);
            }
        });
        zlog_ui::layout::caption(ui, &tr("rig.sim.received"));
        zlog_ui::layout::rest(ui, "rig_received", |ui| {
            for line in command_lines(&sim.received(), 5) {
                ui.monospace(line);
            }
        });
    }
}

pub fn command_lines(received: &[Vec<u8>], last: usize) -> Vec<String> {
    received[received.len().saturating_sub(last)..]
        .iter()
        .map(|c| match c.iter().all(|b| b.is_ascii_graphic()) {
            true => String::from_utf8_lossy(c).into_owned(),
            false => c
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect()
}

pub fn rig_status(
    model: &str,
    port: &str,
    connected: bool,
    error: Option<String>,
    lang: &str,
) -> String {
    let t = |key: &str, args: &[&dyn std::fmt::Display]| crate::texts().format(lang, key, args);
    let rig = if port == SIMULATOR {
        text(lang, "rig.simulator")
    } else {
        t("menu.device.rig_on", &[&model, &port])
    };
    let state = match (connected, error) {
        (_, Some(e)) => t("menu.device.error", &[&e]),
        (true, None) => text(lang, "menu.device.connected"),
        (false, None) => text(lang, "rig.not_connected"),
    };
    t("menu.device.status", &[&rig, &state])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_commands_show_as_text_or_hex() {
        let got = command_lines(
            &[
                b"FA;".to_vec(),
                b"IF;".to_vec(),
                vec![0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD],
            ],
            2,
        );
        assert_eq!(got, ["IF;", "FE FE 94 E0 03 FD"]);
        assert!(command_lines(&[], 5).is_empty());
    }

    #[test]
    fn rig_status_names_the_simulator_and_the_state() {
        let sim = rig_status("IC-7300", SIMULATOR, true, None, "en");
        assert_eq!(sim, "Rig: Simulator — connected");
        let off = rig_status("IC-7300", "/dev/cu.SLAB", false, None, "en");
        assert_eq!(off, "Rig: IC-7300 on /dev/cu.SLAB — Disconnected");
        let err = rig_status("IC-7300", "COM3", false, Some("busy".into()), "en");
        assert_eq!(err, "Rig: IC-7300 on COM3 — error: busy");
    }
}
