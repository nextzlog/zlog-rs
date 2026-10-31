use super::*;

impl MenuForm {
    pub(super) fn form(&mut self, ui: &mut egui::Ui, registry: &Registry, lang: &str) {
        let contests = registry.contests();
        let width = (ui.available_width() - MODE_WIDTH).max(340.0);
        ui.horizontal_top(|ui| {
            self.contest_selector(ui, registry, lang, width);
            ui.vertical(|ui| {
                ui.group(|ui| {
                    ui.set_width(SIDE_WIDTH);
                    ui.vertical(|ui| {
                        ui.label(tr(lang, "menu_form.mode"));
                        let modes = self.current(contests).map_or(&[][..], |c| c.modes());
                        let mut mode = self.mode;
                        for &m in modes {
                            ui.radio_value(&mut mode, m, mode_caption(m));
                        }
                        self.mode = mode;
                    });
                });
                ui.group(|ui| {
                    ui.set_width(SIDE_WIDTH);
                    ui.vertical(|ui| {
                        ui.label(tr(lang, "menu_form.power"));
                        ui.radio_value(&mut self.power, None, tr(lang, "menu_form.power_by_band"));
                        for p in ['H', 'M', 'L', 'P'] {
                            ui.radio_value(&mut self.power, Power::from_code(p), p.to_string());
                        }
                    });
                });
            });
        });
        if let Some(f) = &self.file {
            ui.label(format!(
                "{}: {}",
                tr(lang, "menu_form.cfg_file"),
                f.path.display()
            ));
            for w in &f.warnings {
                ui.colored_label(zlog_ui::palette::warning(ui), w);
            }
        }
        ui.add_space(6.0);
        let st = &mut self.station;
        let pairs: [(&'static str, &mut String); 9] = [
            ("menu_form.callsign", &mut st.callsign),
            ("menu_form.operator", &mut st.operator),
            ("menu_form.prov", &mut st.prov),
            ("menu_form.city", &mut st.city),
            ("menu_form.age", &mut st.age),
            ("menu_form.iota", &mut st.iota),
            ("menu_form.cq_zone", &mut st.cq_zone),
            ("menu_form.iaru_zone", &mut st.iaru_zone),
            ("menu_form.itu_region", &mut st.itu_region),
        ];
        let text_width = |ui: &egui::Ui, key: &'static str| {
            egui::WidgetText::from(tr(lang, key))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x
        };
        let label = |col: usize| {
            pairs
                .iter()
                .skip(col)
                .step_by(2)
                .map(|(k, _)| *k)
                .chain((col == 1).then_some("menu_form.tx_nr"))
                .map(|k| text_width(ui, k))
                .fold(0.0, f32::max)
        };
        let labels = [label(0), label(1)];
        let gap = ui.spacing().item_spacing.x;
        let outer = width + GROUP_FRAME;
        let field = ((outer - labels[0] - labels[1] - 3.0 * gap) / 2.0).max(FIELD_WIDTH);
        egui::Grid::new("menu_station")
            .num_columns(4)
            .show(ui, |ui| {
                for (i, (key, value)) in pairs.into_iter().enumerate() {
                    let height = ui.spacing().interact_size.y;
                    ui.allocate_ui_with_layout(
                        egui::vec2(labels[i % 2], height),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(labels[i % 2]);
                            ui.label(tr(lang, key));
                        },
                    );
                    upper_edit(ui, value, field);
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
                ui.label(tr(lang, "menu_form.tx_nr"));
                ui.add(egui::DragValue::new(&mut st.tx_nr).range(0..=15));
                ui.end_row();
            });
        egui::Grid::new("menu_fields")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(tr(lang, "menu_form.coeff"));
                ui.add_enabled(
                    self.current(contests).is_some_and(|c| c.uses_coeff()),
                    egui::TextEdit::singleline(&mut self.coeff).desired_width(60.0),
                );
                ui.end_row();
                ui.end_row();
            });
        let (mut open, mut new) = (false, false);
        ui.scope(|ui| {
            ui.set_max_width(width + GROUP_FRAME);
            zlog_ui::field_row(
                ui,
                |ui| {
                    ui.label(tr(lang, "menu_form.file"));
                },
                // A save panel cannot pick an existing file on macOS, so opening and
                // creating are separate buttons.
                |ui| {
                    new = ui.button(tr(lang, "menu_form.new")).clicked();
                    open = ui.button(tr(lang, "menu_form.open")).clicked();
                },
                |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(f32::INFINITY))
                },
            );
        });
        if open {
            if let Some(p) = zlog_ui::dialog::open_log_file() {
                self.path = p.display().to_string();
                self.replace = None;
            }
        }
        if new {
            match zlog_ui::dialog::choose_log_file("contest.zlox", zlog_core::Format::Zlox)
                .map(new_log)
            {
                Some(Ok(p)) => {
                    self.path = p.display().to_string();
                    self.replace = p.exists().then_some(p);
                    self.error = None;
                }
                Some(Err(p)) => {
                    let name = p.display().to_string();
                    self.error = Some(texts().format(lang, "menu_form.exists", &[&name]));
                }
                None => {}
            }
        }
        if let Some(e) = &self.error {
            ui.colored_label(zlog_ui::palette::error(ui), e);
        }
    }

    pub(super) fn contest_selector(
        &mut self,
        ui: &mut egui::Ui,
        registry: &Registry,
        lang: &str,
        width: f32,
    ) {
        let contests = registry.contests();
        let listing: Vec<usize> = (0..self.entries.len())
            .filter(|i| self.entries[*i].shown(registry))
            .collect();
        if self.entry.is_none() && self.recent.is_empty() && self.pinned.is_empty() {
            self.entry = listing
                .iter()
                .copied()
                .find(|i| self.entries[*i].contests(registry, lang).is_some());
        }
        let needle = self.search.trim().to_lowercase();
        let shown: Vec<Row> = if !needle.is_empty() {
            let files = self.folder_contest_files(registry).to_vec();
            for p in &files {
                self.title_of(p, registry);
            }
            search_rows(&needle, contests, lang, &files, &self.titles)
        } else if let Some(e) = self.entry {
            self.entries
                .get(e)
                .and_then(|e| e.contests(registry, lang))
                .unwrap_or_default()
                .into_iter()
                .map(Row::Builtin)
                .collect()
        } else {
            recent_rows(&self.pinned_first(), contests)
        };
        let in_recent = needle.is_empty() && self.entry.is_none();
        let updates = self.updates(registry, lang);
        if needle.is_empty() {
            self.folder_files = None;
        }
        for row in &shown {
            if let Row::File(p) = row {
                self.title_of(p, registry);
            }
        }
        let mut pick: Option<Row> = None;
        let mut opened: Option<usize> = None;
        ui.group(|ui| {
            ui.set_max_width(width);
            ui.vertical(|ui| {
                ui.label(tr(lang, "menu_form.contest"));
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "menu_form.search"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search).desired_width(f32::INFINITY),
                    );
                });
                let height = (ui.ctx().content_rect().height() - RESERVED_HEIGHT).max(230.0);
                let size = egui::vec2(width - 24.0, height);
                let groups_width = (size.x * 0.3).clamp(110.0, 220.0);
                let layout = egui::Layout::left_to_right(egui::Align::Min);
                ui.allocate_ui_with_layout(size, layout, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("groups")
                        .max_height(size.y)
                        .show(ui, |ui| {
                            ui.set_width(groups_width);
                            ui.vertical(|ui| {
                                if !self.recent.is_empty() || !self.pinned.is_empty() {
                                    ui.selectable_value(
                                        &mut self.entry,
                                        None,
                                        tr(lang, "menu_form.recent"),
                                    );
                                }
                                for &i in &listing {
                                    let e = &self.entries[i];
                                    let mut title = e.title(registry, lang);
                                    if updates.iter().any(|(n, _)| *n == i) {
                                        title =
                                            format!("{title} ({})", tr(lang, "menu_form.update"));
                                    }
                                    if e.contests(registry, lang).is_some() {
                                        ui.selectable_value(&mut self.entry, Some(i), title);
                                    } else if ui.selectable_label(false, title).clicked() {
                                        opened = Some(i);
                                    }
                                }
                            });
                        });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("contests")
                        .max_height(size.y)
                        .show(ui, |ui| {
                            ui.set_width((size.x - groups_width - 20.0).max(210.0));
                            ui.vertical(|ui| {
                                for row in shown {
                                    let (on, caption) = match &row {
                                        Row::Builtin(i) => (
                                            self.file.is_none() && self.contest == *i,
                                            contests[*i].caption(lang),
                                        ),
                                        Row::File(p) => (
                                            self.file.as_ref().is_some_and(|f| f.path == *p),
                                            self.titles.get(p).cloned().unwrap_or_default(),
                                        ),
                                    };
                                    ui.horizontal(|ui| {
                                        if in_recent {
                                            let r = recent_of(&row, contests);
                                            let pinned = self.pinned.contains(&r);
                                            let (glyph, tip) = if pinned {
                                                ("★", "menu_form.unpin")
                                            } else {
                                                ("☆", "menu_form.pin")
                                            };
                                            if ui
                                                .selectable_label(pinned, glyph)
                                                .on_hover_text(tr(lang, tip))
                                                .clicked()
                                            {
                                                self.pin_toggled = Some(r);
                                            }
                                        }
                                        if ui.radio(on, caption).clicked() {
                                            pick = Some(row.clone());
                                        }
                                        if let Row::File(p) = &row {
                                            if updates.iter().any(|(_, f)| f == p) {
                                                badge(ui, lang);
                                            }
                                        }
                                        if !needle.is_empty() {
                                            let tag = match &row {
                                                Row::Builtin(i) => contests[*i].group(lang),
                                                Row::File(_) => {
                                                    tr(lang, "menu_form.tag_file").into()
                                                }
                                            };
                                            ui.label(egui::RichText::new(tag).small().weak());
                                        }
                                    });
                                }
                            });
                        });
                });
            });
        });
        match pick {
            Some(Row::Builtin(i)) => {
                self.contest = i;
                self.file = None;
            }
            Some(Row::File(p)) => self.choose_file(&p, registry, lang),
            None => {}
        }
        if let Some(i) = opened {
            let mut view = EntryView {
                registry,
                language: lang,
                contest_folder: &mut self.cfg_folder,
            };
            self.entries[i].open(&mut view);
        }
        if let Some(c) = self.current(contests) {
            if !c.modes().contains(&self.mode) {
                self.mode = c.modes().first().copied().unwrap_or(ContestMode::Mix);
            }
        }
    }

    pub(super) fn dialogs(&mut self, ctx: &egui::Context, registry: &Registry, lang: &str) {
        let mut chosen = Vec::new();
        for e in &mut self.entries {
            let mut view = EntryView {
                registry,
                language: lang,
                contest_folder: &mut self.cfg_folder,
            };
            chosen.extend(e.ui(ctx, &mut view));
        }
        for c in chosen {
            self.choose(c, registry, lang);
        }
        if let Some(missing) = self.locate.clone() {
            let (_, answer) = zlog_ui::dialog::modal_with_buttons(
                ctx,
                "locate_contest_file",
                |ui| {
                    ui.label(
                        tr(lang, "menu_form.cfg_missing")
                            .replace("{0}", &missing.display().to_string()),
                    );
                },
                |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(tr(lang, "menu_form.locate")).clicked() {
                            Some(true)
                        } else if ui.button(tr(lang, "button.cancel")).clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            Some(false)
                        } else {
                            None
                        }
                    })
                    .inner
                },
            );
            match answer {
                Some(true) => {
                    self.locate = None;
                    if let Some(p) = pick_contest_file(registry.contest_files(), lang) {
                        self.choose_file(&p, registry, lang);
                    }
                }
                Some(false) => self.locate = None,
                None => {}
            }
        }
    }
}

pub(super) fn upper_edit(ui: &mut egui::Ui, text: &mut String, width: f32) {
    zlog_ui::sized_field(ui, egui::TextEdit::singleline(text), width);
    *text = text.to_uppercase();
}

pub(super) fn mode_caption(m: ContestMode) -> &'static str {
    match m {
        ContestMode::Mix => "PH/CW(MIX)",
        ContestMode::Cw => "CW",
        ContestMode::Phone => "PH",
        ContestMode::Rtty => "RTTY",
        ContestMode::All => "ALL",
    }
}

fn new_log(to: zlog_ui::dialog::SaveTo) -> Result<std::path::PathBuf, std::path::PathBuf> {
    if to.ask {
        Err(to.path)
    } else {
        Ok(to.path)
    }
}

#[cfg(test)]
mod tests {
    use super::new_log;
    use zlog_ui::dialog::SaveTo;

    #[test]
    fn a_new_log_never_silently_picks_an_existing_file() {
        let path = std::path::PathBuf::from("foo.zlox");
        let to = |ask| SaveTo {
            path: path.clone(),
            ask,
        };
        assert_eq!(new_log(to(false)), Ok(path.clone()));
        assert_eq!(new_log(to(true)), Err(path.clone()));
    }
}
