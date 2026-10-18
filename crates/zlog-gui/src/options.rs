//! The Options dialog and the rig window.

use egui::{Color32, RichText, Ui};
use zlog_core::{Band, Power, Station};
use zlog_rig::RigMode;
use zlog_rig_cat::MODELS;
use zlog_ui::SettingsPage;

use crate::dialog::dialog;
use crate::i18n::{self, tr};
use crate::rig::{RigConfig, RigControl, SIMULATOR};
use crate::settings::Settings;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Station,
    Rig,
    Extension(usize),
}

/// The Options dialog, editing a copy of the settings until OK.
pub struct Options {
    pub open: bool,
    page: Page,
    station: Station,
    rig: RigConfig,
    ports: Vec<String>,
    pages: Vec<Box<dyn SettingsPage>>,
}

impl Options {
    pub fn new(pages: Vec<Box<dyn SettingsPage>>) -> Self {
        Options {
            open: false,
            page: Page::Station,
            station: Station::default(),
            rig: RigConfig::from_section(&Default::default()),
            ports: Vec::new(),
            pages,
        }
    }

    /// Open with the current settings.
    pub fn show_with(&mut self, settings: &mut Settings) {
        self.station = settings.station();
        self.rig = RigConfig::from_section(settings.section("rig"));
        self.ports = zlog_rig::serial::available_ports()
            .map(|ps| ps.into_iter().map(|p| p.port_name).collect())
            .unwrap_or_default();
        self.open = true;
    }

    /// Draw the dialog; returns true when OK was pressed (settings were updated and saved).
    pub fn ui(&mut self, ctx: &egui::Context, settings: &mut Settings) -> bool {
        let mut open = self.open;
        let mut ok = false;
        dialog(
            ctx,
            "options",
            tr("options.title"),
            [520.0, 460.0],
            &mut open,
            |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.page, Page::Station, tr("options.station"));
                    ui.selectable_value(&mut self.page, Page::Rig, tr("options.rig"));
                    for (i, p) in self.pages.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.page,
                            Page::Extension(i),
                            p.title(i18n::current()),
                        );
                    }
                });
                ui.separator();
                match self.page {
                    Page::Station => station_page(ui, &mut self.station),
                    Page::Rig => rig_page(ui, &mut self.rig, &self.ports),
                    Page::Extension(i) => {
                        let page = &mut self.pages[i];
                        let section = settings.section(page.id());
                        page.ui(ui, section, i18n::current());
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("button.ok")).clicked() {
                        ok = true;
                    }
                    if ui.button(tr("button.cancel")).clicked() {
                        self.open = false;
                    }
                });
            },
        );
        if !open {
            self.open = false;
        }
        if ok {
            settings.set_station(&self.station);
            self.rig.to_section(settings.section("rig"));
            let _ = settings.save();
            self.open = false;
        }
        ok
    }

    pub fn station(&self) -> &Station {
        &self.station
    }

    pub fn rig(&self) -> &RigConfig {
        &self.rig
    }
}

fn station_page(ui: &mut Ui, st: &mut Station) {
    egui::Grid::new("station").num_columns(2).show(ui, |ui| {
        for (key, value) in [
            ("menu_form.callsign", &mut st.callsign),
            ("menu_form.operator", &mut st.operator),
            ("menu_form.prov", &mut st.prov),
            ("menu_form.city", &mut st.city),
        ] {
            ui.label(tr(key));
            ui.text_edit_singleline(value);
            *value = value.to_uppercase();
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new(tr("options.power_per_band")).strong());
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .show(ui, |ui| {
            egui::Grid::new("power").striped(true).show(ui, |ui| {
                for band in Band::range(Band::B1_9, Band::HIGHEST) {
                    ui.monospace(band.label());
                    let mut power = st.power_for(band);
                    for code in ['H', 'M', 'L', 'P'] {
                        let p = Power::from_code(code).unwrap();
                        ui.radio_value(&mut power, p, code.to_string());
                    }
                    if power != st.power_for(band) {
                        st.power.insert(band, power);
                    }
                    ui.end_row();
                }
            });
        });
}

fn rig_page(ui: &mut Ui, rig: &mut RigConfig, ports: &[String]) {
    egui::Grid::new("rig").num_columns(2).show(ui, |ui| {
        ui.label(tr("rig.model"));
        egui::ComboBox::from_id_salt("rig_model")
            .selected_text(MODELS[rig.model].name)
            .show_ui(ui, |ui| {
                for (i, m) in MODELS.iter().enumerate() {
                    if ui.selectable_value(&mut rig.model, i, m.name).clicked() {
                        rig.baud = m.baud;
                    }
                }
            });
        ui.end_row();
        ui.label(tr("rig.port"));
        let shown = if rig.port == SIMULATOR {
            tr("rig.simulator").to_string()
        } else {
            rig.port.clone()
        };
        egui::ComboBox::from_id_salt("rig_port")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut rig.port, SIMULATOR.to_string(), tr("rig.simulator"));
                for p in ports {
                    ui.selectable_value(&mut rig.port, p.clone(), p);
                }
            });
        ui.end_row();
        ui.label(tr("rig.baud"));
        egui::ComboBox::from_id_salt("rig_baud")
            .selected_text(rig.baud.to_string())
            .show_ui(ui, |ui| {
                for b in [4800, 9600, 19200, 38400, 57600, 115200] {
                    ui.selectable_value(&mut rig.baud, b, b.to_string());
                }
            });
        ui.end_row();
    });
}

fn khz(hz: Option<u64>) -> String {
    hz.map(|h| format!("{:.1}", h as f64 / 1000.0))
        .unwrap_or_else(|| "-".into())
}

/// The rig window: what the rig reports, connect and disconnect, and the controls of
/// the simulated radio when it is in use.
pub fn rig_window(ctx: &egui::Context, rig: &mut RigControl, open: &mut bool, config: &RigConfig) {
    dialog(ctx, "rig", tr("window.rig"), [340.0, 260.0], open, |ui| {
        ui.label(RichText::new(MODELS[config.model].name).strong());
        if rig.connected() {
            let st = rig.state().unwrap_or_default();
            egui::Grid::new("rig_state").num_columns(2).show(ui, |ui| {
                ui.label(tr("rig.vfo_a"));
                ui.monospace(khz(st.freq_a));
                ui.end_row();
                ui.label(tr("rig.vfo_b"));
                ui.monospace(khz(st.freq_b));
                ui.end_row();
                ui.label(tr("rig.mode"));
                ui.monospace(
                    st.mode
                        .map(|m| format!("{m:?}"))
                        .unwrap_or_else(|| "-".into()),
                );
                ui.end_row();
            });
            if let Some(e) = rig.error() {
                ui.colored_label(Color32::RED, e);
            }
            if ui.button(tr("rig.disconnect")).clicked() {
                rig.disconnect();
            }
        } else {
            ui.weak(tr("rig.not_connected"));
            if let Some(m) = &rig.message {
                ui.colored_label(Color32::RED, m);
            }
            if ui.button(tr("rig.connect")).clicked() {
                rig.connect(config.clone());
            }
        }
        if let Some(sim) = rig.sim.clone() {
            ui.separator();
            ui.label(RichText::new(tr("rig.simulator")).strong());
            let mut khz = sim.with(|r| match r.vfo {
                zlog_rig::Vfo::A => r.freq_a,
                zlog_rig::Vfo::B => r.freq_b,
            }) as f64
                / 1000.0;
            ui.horizontal(|ui| {
                ui.label(tr("rig.sim.dial"));
                let dial = egui::DragValue::new(&mut khz)
                    .speed(0.1)
                    .range(1800.0..=10_250_000.0)
                    .fixed_decimals(1);
                if ui.add(dial).changed() {
                    sim.tune((khz * 1000.0).round() as u64);
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("rig.sim.mode"));
                for m in [
                    RigMode::Cw,
                    RigMode::Lsb,
                    RigMode::Usb,
                    RigMode::Fm,
                    RigMode::Am,
                    RigMode::Rtty,
                ] {
                    if ui.button(format!("{m:?}")).clicked() {
                        sim.press_mode(m);
                    }
                }
            });
            let mut on = sim.with(|r| r.power_on);
            if ui.checkbox(&mut on, tr("rig.sim.power")).changed() {
                sim.with(|r| r.power_on = on);
            }
        }
    });
}
