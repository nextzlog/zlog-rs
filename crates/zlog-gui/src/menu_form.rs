//! The zLog Menu dialog shown at start (`TMenuForm`): contest, mode, station, file.

use crate::i18n::tr;
use std::path::PathBuf;

use zlog_core::{ContestDef, ContestMode, Power, Station};

use crate::session::Setup;

pub struct MenuForm {
    /// Index into the registry's contest list.
    contest: usize,
    /// Text typed in the search box; filters across all groups.
    search: String,
    /// Group shown in the list; `None` is the recently used contests.
    group: Option<String>,
    /// Identifiers of recently used contests, newest first.
    pub recent: Vec<String>,
    mode: ContestMode,
    station: Station,
    /// One power on every band, or `None` for zLog's default per band.
    power: Option<Power>,
    coeff: String,
    path: String,
    pub error: Option<String>,
}

impl MenuForm {
    /// The form filled with the station from the settings.
    pub fn with_station(station: Station, recent: Vec<String>) -> Self {
        MenuForm {
            station,
            recent,
            ..MenuForm::default()
        }
    }
}

impl Default for MenuForm {
    fn default() -> Self {
        MenuForm {
            contest: 0,
            search: String::new(),
            group: None,
            recent: Vec::new(),
            mode: ContestMode::Mix,
            station: Station::default(),
            power: None,
            coeff: "1".into(),
            path: String::new(),
            error: None,
        }
    }
}

impl MenuForm {
    /// Draw the dialog; returns the setup when OK is pressed with valid input.
    pub fn show(&mut self, ui: &mut egui::Ui, contests: &[Box<dyn ContestDef>]) -> Option<Setup> {
        let mut result = None;
        ui.heading(tr("menu_form.title"));
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            self.contest_selector(ui, contests);
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.label(tr("menu_form.mode"));
                    let modes = contests.get(self.contest).map_or(&[][..], |c| c.modes());
                    for &m in modes {
                        ui.radio_value(&mut self.mode, m, mode_caption(m));
                    }
                });
            });
        });
        ui.add_space(6.0);
        egui::Grid::new("menu_fields")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(tr("menu_form.callsign"));
                upper_edit(ui, &mut self.station.callsign, 120.0);
                ui.end_row();
                ui.label(tr("menu_form.operator"));
                upper_edit(ui, &mut self.station.operator, 120.0);
                ui.end_row();
                ui.label(tr("menu_form.prov"));
                upper_edit(ui, &mut self.station.prov, 60.0);
                ui.end_row();
                ui.label(tr("menu_form.city"));
                upper_edit(ui, &mut self.station.city, 80.0);
                ui.end_row();
                ui.label(tr("menu_form.power"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.power, None, tr("menu_form.power_by_band"));
                    for p in ['H', 'M', 'L', 'P'] {
                        ui.radio_value(&mut self.power, Power::from_code(p), p.to_string());
                    }
                });
                ui.end_row();
                ui.label(tr("menu_form.coeff"));
                ui.add_enabled(
                    contests.get(self.contest).is_some_and(|c| c.uses_coeff()),
                    egui::TextEdit::singleline(&mut self.coeff).desired_width(60.0),
                );
                ui.end_row();
                ui.label(tr("menu_form.file"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(260.0));
                    if ui.button("...").clicked() {
                        let picked = rfd::FileDialog::new()
                            .add_filter("zLog", &["zlox", "ZLOX", "zlo", "ZLO"])
                            .set_file_name("contest.zlox")
                            .save_file();
                        if let Some(p) = picked {
                            self.path = p.display().to_string();
                        }
                    }
                });
                ui.end_row();
            });
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::RED, e);
        }
        ui.add_space(6.0);
        if ui.button(tr("button.ok")).clicked() {
            match self.validate() {
                Ok(setup) => result = Some(setup),
                Err(e) => self.error = Some(e),
            }
        }
        result
    }

    /// Groups on the left, the contests of the chosen group (or the search results)
    /// on the right; the layout grows with the number of contests linked in.
    fn contest_selector(&mut self, ui: &mut egui::Ui, contests: &[Box<dyn ContestDef>]) {
        let lang = crate::i18n::current();
        let mut groups: Vec<String> = Vec::new();
        for c in contests {
            let g = c.group(lang);
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        if self.group.is_none() && self.recent.is_empty() {
            self.group = groups.first().cloned();
        }
        let needle = self.search.trim().to_lowercase();
        let shown: Vec<usize> = if !needle.is_empty() {
            (0..contests.len())
                .filter(|i| {
                    contests[*i].caption(lang).to_lowercase().contains(&needle)
                        || contests[*i].id().contains(&needle)
                })
                .collect()
        } else if let Some(g) = &self.group {
            (0..contests.len())
                .filter(|i| contests[*i].group(lang) == *g)
                .collect()
        } else {
            self.recent
                .iter()
                .filter_map(|id| contests.iter().position(|c| c.id() == id))
                .collect()
        };
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(tr("menu_form.contest"));
                ui.horizontal(|ui| {
                    ui.label(tr("menu_form.search"));
                    ui.text_edit_singleline(&mut self.search);
                });
                // Two columns of fixed height: groups, and the contests of the group.
                let size = egui::vec2(340.0, 170.0);
                let layout = egui::Layout::left_to_right(egui::Align::Min);
                ui.allocate_ui_with_layout(size, layout, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("groups")
                        .max_height(size.y)
                        .show(ui, |ui| {
                            ui.set_width(110.0);
                            ui.vertical(|ui| {
                                if !self.recent.is_empty() {
                                    ui.selectable_value(
                                        &mut self.group,
                                        None,
                                        tr("menu_form.recent"),
                                    );
                                }
                                for g in &groups {
                                    ui.selectable_value(&mut self.group, Some(g.clone()), g);
                                }
                            });
                        });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("contests")
                        .max_height(size.y)
                        .show(ui, |ui| {
                            ui.set_width(210.0);
                            ui.vertical(|ui| {
                                for i in shown {
                                    ui.radio_value(&mut self.contest, i, contests[i].caption(lang));
                                }
                            });
                        });
                });
            });
        });
        // Keep the mode one the chosen contest offers.
        if let Some(c) = contests.get(self.contest) {
            if !c.modes().contains(&self.mode) {
                self.mode = c.modes().first().copied().unwrap_or(ContestMode::Mix);
            }
        }
    }

    fn validate(&self) -> Result<Setup, String> {
        if self.path.trim().is_empty() {
            return Err(tr("menu_form.need_file").into());
        }
        let coeff: f64 = self
            .coeff
            .trim()
            .parse()
            .map_err(|_| tr("menu_form.bad_coeff"))?;
        Ok(Setup {
            contest: self.contest,
            mode: self.mode,
            station: {
                let mut st = self.station.clone();
                if let Some(p) = self.power {
                    st.set_power_everywhere(p);
                }
                st
            },
            coeff,
            path: PathBuf::from(self.path.trim()),
        })
    }
}

fn upper_edit(ui: &mut egui::Ui, text: &mut String, width: f32) {
    ui.add(egui::TextEdit::singleline(text).desired_width(width));
    *text = text.to_uppercase();
}

fn mode_caption(m: ContestMode) -> &'static str {
    match m {
        ContestMode::Mix => "PH/CW(MIX)",
        ContestMode::Cw => "CW",
        ContestMode::Phone => "PH",
        ContestMode::Rtty => "RTTY",
        ContestMode::All => "ALL",
    }
}
