use super::*;

impl ContestManager {
    pub(super) fn ui(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        language: &str,
    ) -> Option<ChosenContest> {
        if !self.open {
            return None;
        }
        let kinds = registry.contest_files();
        self.language = language.to_string();
        self.today = today();
        self.builtin = registry
            .contests()
            .iter()
            .map(|c| (c.id().to_string(), c.caption(language), c.group(language)))
            .collect();
        let lang = language;
        self.poll_lists();
        self.poll_downloads(kinds);
        if self.waiting() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.import(&dropped, kinds);
        }
        let mut chosen = None;
        let mut tools = Tools::default();
        let (mut rescan, mut close) = (false, false);
        let mut ok = false;
        let ok_enabled = self.choice().is_some();
        let mut open = self.open;
        zlog_ui::dialog::dialog_with_buttons(
            ctx,
            "cfg_picker",
            tr(lang, "cfg_picker.title"),
            [860.0, 560.0],
            &mut open,
            |ui| self.body(ui, kinds, &mut chosen, &mut tools),
            |ui| {
                ui.horizontal(|ui| {
                    match zlog_ui::dialog::ok_cancel(ui, lang, ok_enabled, false) {
                        Some(true) => ok = true,
                        Some(false) => close = true,
                        None => {}
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            },
        );
        if close {
            open = false;
        }
        if ok {
            chosen = self.choice();
        }
        let Tools {
            import,
            folder,
            reload,
        } = tools;
        if folder {
            if let Some(d) = zlog_ui::dialog::pick_folder(&self.folder) {
                self.change_folder(d.display().to_string(), kinds);
            }
        }
        if reload {
            self.requested = false;
            self.force = true;
            self.selected = None;
            self.cfg = None;
            self.linked = None;
            rescan = true;
        }
        if !open {
            self.open = false;
        }
        if rescan {
            self.rescan(kinds);
        }
        if import {
            let files = zlog_ui::dialog::import_files();
            if !files.is_empty() {
                self.import(&files, kinds);
            }
        }
        if chosen.is_some() {
            self.open = false;
        }
        chosen
    }

    pub(super) fn body(
        &mut self,
        ui: &mut Ui,
        kinds: &[Box<dyn ContestFile>],
        chosen: &mut Option<ChosenContest>,
        tools: &mut Tools,
    ) {
        let language = self.language.clone();
        let lang = language.as_str();
        let show = &mut self.show;
        zlog_ui::field_row(
            ui,
            |_| {},
            |ui| {
                for (s, key) in [
                    (Show::Updates, "cfg_picker.updates"),
                    (Show::Missing, "cfg_picker.missing"),
                    (Show::Installed, "cfg_picker.installed"),
                    (Show::All, "cfg_picker.all"),
                ] {
                    ui.selectable_value(show, s, tr(lang, key));
                }
            },
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text(tr(lang, "cfg_picker.search"))
                        .desired_width(f32::INFINITY),
                )
            },
        );
        ui.horizontal_wrapped(|ui| {
            tools.import = ui.button(tr(lang, "cfg_picker.import")).clicked();
            tools.folder = ui.button(tr(lang, "cfg_picker.choose_folder")).clicked();
            tools.reload = ui
                .button(tr(lang, "cfg_picker.reload"))
                .on_hover_text(tr(lang, "cfg_picker.reload.tip"))
                .clicked();
            ui.separator();
            ui.weak(trf(lang, "cfg_picker.folder_is", &[&self.folder]));
            for (l, listing) in self.lists.iter().zip(&self.listings) {
                ui.separator();
                ui.hyperlink_to(l.title(lang), l.page());
                if let Some((y, m, d)) = l.last_checked() {
                    ui.weak(trf(
                        lang,
                        "cfg_picker.checked",
                        &[&format!("{y}-{m:02}-{d:02}")],
                    ));
                }
                match listing {
                    Listing::Loading => {
                        ui.spinner();
                    }
                    Listing::Failed(e) => {
                        ui.colored_label(
                            zlog_ui::palette::error(ui),
                            trf(lang, "cfg_picker.list_failed", &[e]),
                        );
                    }
                    Listing::Ready(c) => {
                        ui.weak(trf(lang, "cfg_picker.list_count", &[&c.len()]));
                    }
                }
            }
        });
        for l in &self.lists {
            if l.changes().is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(trf(
                lang,
                "cfg_picker.recent",
                &[&l.title(lang), &l.changes().len()],
            ))
            .id_salt(("cfg_changes", l.id()))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("cfg_changes_scroll", l.id()))
                    .max_height(140.0)
                    .show(ui, |ui| changes(ui, l.changes()));
                if let Some(url) = l.history_page() {
                    ui.hyperlink_to(tr(lang, "cfg_picker.older"), url);
                }
            });
        }
        if let Some((m, warning)) = &self.message {
            if *warning {
                ui.colored_label(zlog_ui::palette::warning(ui), m);
            } else {
                ui.label(m);
            }
        }
        ui.separator();
        let rows = self.shown_rows();
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() * 0.4).clamp(240.0, 480.0));
                if rows.is_empty() {
                    ui.weak(tr(lang, "cfg_picker.none"));
                }
                egui::ScrollArea::vertical()
                    .id_salt("cfg_rows")
                    .max_height(height)
                    .show(ui, |ui| {
                        for row in &rows {
                            let on = self.selected.as_ref() == Some(&row.key);
                            let r = ui
                                .horizontal(|ui| {
                                    let r = ui.selectable_label(on, &row.name);
                                    badge(ui, lang, row);
                                    r
                                })
                                .inner;
                            if r.clicked() {
                                self.select(&row.key);
                            }
                            if r.double_clicked() {
                                self.select(&row.key);
                                *chosen = self.choice();
                            }
                        }
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("cfg_details")
                    .max_height(height)
                    .show(ui, |ui| match self.selected.clone() {
                        None => {
                            ui.weak(tr(lang, "cfg_picker.choose"));
                        }
                        Some(key) => self.details(ui, &key, kinds, chosen),
                    });
            });
        });
    }

    pub(super) fn details(
        &mut self,
        ui: &mut Ui,
        key: &Key,
        kinds: &[Box<dyn ContestFile>],
        chosen: &mut Option<ChosenContest>,
    ) {
        let language = self.language.clone();
        let lang = language.as_str();
        let cfgs = self.cfgs_of(key);
        match key {
            Key::Builtin(id) => {
                let Some((_, caption, group)) = self.builtin.iter().find(|b| b.0 == *id) else {
                    return;
                };
                ui.heading(caption);
                ui.label(trf(lang, "cfg_picker.builtin", &[group]));
            }
            Key::Local(p) => {
                let title = self
                    .local
                    .iter()
                    .find(|(l, _)| l == p)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                ui.heading(if title.is_empty() {
                    file_name(p)
                } else {
                    title
                });
                ui.weak(p.display().to_string());
                ui.label(tr(lang, "cfg_picker.local"));
            }
            Key::Listed { list, contest } => {
                let Some(c) = self.listed(*list, *contest).cloned() else {
                    return;
                };
                ui.heading(&c.name);
                if let Some(url) = &c.rules {
                    ui.hyperlink_to(tr(lang, "cfg_picker.rules"), url)
                        .on_hover_text(url);
                }
                ui.horizontal(|ui| {
                    if !c.date.is_empty() {
                        ui.label(trf(lang, "cfg_picker.date", &[&c.date]));
                    }
                    if let Some((y, m, d)) = c.updated {
                        ui.label(trf(
                            lang,
                            "cfg_picker.updated",
                            &[&format!("{y}-{m:02}-{d:02}")],
                        ));
                    }
                    if let Some(n) = updated_days_ago(self.today, &c) {
                        ui.label(days_text(lang, n));
                    }
                    ui.weak(self.lists[*list].title(lang));
                });
                if c.experimental {
                    ui.colored_label(
                        zlog_ui::palette::warning(ui),
                        tr(lang, "cfg_picker.experimental"),
                    );
                }
                if !c.note.is_empty() {
                    ui.add(egui::Label::new(&c.note).wrap());
                }
                if !c.doc.is_empty() {
                    let doc = self.doc_text(&c.doc);
                    egui::ScrollArea::vertical()
                        .id_salt("cfg_listed_doc")
                        .max_height(160.0)
                        .show(ui, |ui| ui.add(egui::Label::new(doc).wrap()));
                }
                if !c.changes.is_empty() {
                    ui.add_space(4.0);
                    ui.strong(tr(lang, "cfg_picker.changes"));
                    changes(ui, &c.changes);
                }
                ui.add_space(4.0);
                let opens = c
                    .files
                    .iter()
                    .any(|f| kinds.iter().any(|k| k.accepts(Path::new(&f.name))));
                if !opens {
                    ui.colored_label(zlog_ui::palette::warning(ui), tr(lang, "cfg_picker.no_cfg"));
                }
                let busy = !self.downloads.is_empty();
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(tr(lang, "cfg_picker.download_all")),
                    )
                    .clicked()
                {
                    self.download(*list, c.files.clone());
                }
                egui::Grid::new("cfg_listed_files")
                    .striped(true)
                    .show(ui, |ui| {
                        for f in &c.files {
                            let state = self.state_of(*list, &c, f);
                            let path = self
                                .files
                                .get(&f.name.to_lowercase())
                                .map(|(p, _)| p.clone())
                                .filter(|p| cfgs.contains(p));
                            match path {
                                Some(p) => {
                                    let on = self.cfg.as_ref() == Some(&p);
                                    let r = ui.selectable_label(on, &f.name);
                                    if r.clicked() {
                                        self.cfg = Some(p.clone());
                                    }
                                    if r.double_clicked() {
                                        *chosen = Some(ChosenContest::File(p));
                                    }
                                }
                                None => {
                                    ui.label(&f.name);
                                }
                            }
                            ui.weak(&f.note);
                            ui.label(state_text(ui, lang, state));
                            if ui
                                .add_enabled(
                                    !busy,
                                    egui::Button::new(tr(lang, "cfg_picker.download")),
                                )
                                .on_hover_text(&f.url)
                                .clicked()
                            {
                                self.download(*list, vec![f.clone()]);
                            }
                            ui.end_row();
                        }
                    });
            }
        }
    }
}

pub(super) fn state_text(ui: &Ui, lang: &str, state: FileState) -> RichText {
    match state {
        FileState::Missing => {
            RichText::new(tr(lang, "cfg_picker.state_missing")).color(zlog_ui::palette::weak(ui))
        }
        FileState::Outdated => RichText::new(tr(lang, "cfg_picker.state_outdated"))
            .color(zlog_ui::palette::warning(ui)),
        FileState::Present => RichText::new(tr(lang, "cfg_picker.state_present")),
    }
}

pub(super) fn changes(ui: &mut Ui, list: &[zlog_ui::Change]) {
    for c in list {
        ui.horizontal_wrapped(|ui| {
            if let Some((y, m, d)) = c.date {
                ui.weak(format!("{y}-{m:02}-{d:02}"));
            }
            ui.label(&c.text);
        });
    }
}

pub(super) fn days_text(lang: &str, days: i64) -> String {
    match days {
        0 => tr(lang, "cfg_picker.updated_today").into(),
        n => trf(lang, "cfg_picker.days_ago", &[&n]),
    }
}

pub(super) fn badge_text(lang: &str, row: &Row) -> String {
    match (&row.key, row.state) {
        (Key::Local(_) | Key::Builtin(_), _) => row.source.clone(),
        (_, FileState::Missing) => tr(lang, "cfg_picker.state_missing").into(),
        (_, FileState::Outdated) => tr(lang, "cfg_picker.state_outdated").into(),
        (_, FileState::Present) => tr(lang, "cfg_picker.state_present").into(),
    }
}

pub(super) fn badge(ui: &mut Ui, lang: &str, row: &Row) {
    let color = match (&row.key, row.state) {
        (Key::Builtin(_), _) => Color32::from_rgb(60, 100, 160),
        (Key::Local(_), _) => Color32::from_rgb(110, 110, 130),
        (_, FileState::Missing) => Color32::from_rgb(90, 90, 90),
        (_, FileState::Outdated) => Color32::from_rgb(190, 110, 0),
        (_, FileState::Present) => Color32::from_rgb(40, 130, 70),
    };
    egui::Frame::new()
        .fill(color)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(badge_text(lang, row))
                    .small()
                    .color(Color32::WHITE),
            );
        });
    if let Key::Listed { .. } = row.key {
        ui.label(RichText::new(&row.source).small().weak());
    }
    if let Some(n) = row.days {
        ui.label(
            RichText::new(days_text(lang, n))
                .small()
                .color(zlog_ui::palette::warning(ui)),
        );
    }
}
