use egui::{RichText, Ui};
use zlog_core::qso::QslState;
use zlog_core::{Band, Power, Station};
use zlog_ui::SettingsPage;

use crate::dialog::{dialog_with_pages, ok_cancel};
use crate::i18n::{self, tr};
use crate::settings::{Preferences, Settings};

pub struct Options {
    pub open: bool,
    page: usize,
    pages: Vec<Box<dyn SettingsPage>>,
    draft: Settings,
    base: Settings,
    drawn: std::collections::BTreeMap<String, zlog_ui::Section>,
    pub(crate) wizard: Option<crate::wizard::Wizard>,
    pub(crate) ask: Option<crate::wizard::Ask>,
    search: String,
}

impl Options {
    pub fn new(pages: Vec<Box<dyn SettingsPage>>) -> Self {
        Options {
            open: false,
            page: 0,
            pages,
            draft: Settings::default(),
            base: Settings::default(),
            drawn: Default::default(),
            wizard: None,
            ask: None,
            search: String::new(),
        }
    }

    pub(crate) fn start_wizard(&mut self, settings: &mut Settings) {
        self.open = false;
        self.show_with(settings);
        self.wizard = Some(crate::wizard::Wizard::default());
    }

    pub(crate) fn apply_now(&mut self, settings: &mut Settings, ids: &[&str]) -> Vec<String> {
        let mut changed = Vec::new();
        for id in ids {
            let edited = self.draft.section(id).clone();
            if *settings.section(id) != edited {
                *settings.section(id) = edited.clone();
                changed.push(id.to_string());
            }
            *self.base.section(id) = edited.clone();
            self.drawn.insert(id.to_string(), edited);
        }
        let _ = settings.save();
        changed
    }

    fn page_body(&mut self, ui: &mut Ui, page: usize, lang: &str) {
        let (pages, draft, drawn) = (&mut self.pages, &mut self.draft, &mut self.drawn);
        if let Some(p) = pages.get_mut(page) {
            let mut section = draft.section(p.id()).clone();
            zlog_ui::form::shared(ui, |ui| p.ui_with(ui, &mut section, draft.sections(), lang));
            drawn
                .entry(p.id().to_string())
                .or_insert_with(|| section.clone());
            *draft.section(p.id()) = section;
        }
    }

    fn wizard_ui(&mut self, ctx: &egui::Context, settings: &mut Settings) -> Option<Vec<String>> {
        use crate::wizard::{Move, Step, Wizard};
        let lang = i18n::current();
        let mut w = self.wizard.take()?;
        let steps = w.steps(&self.pages);
        w.at = w.at.min(steps.len() - 1);
        let step = steps[w.at];
        let mut open = true;
        let mut moved = None;
        let mut ask = None;
        let title = format!("{} ({}/{})", tr("wizard.title"), w.at + 1, steps.len());
        let at = w.at;
        let station = matches!(step, Step::Page(i) if self.pages[i].id() == "station");
        let skip = matches!(step, Step::Page(_)) && !station;
        let call_ok = !zlog_ui::read_station(self.draft.section("station"))
            .callsign
            .trim()
            .is_empty();
        crate::dialog::dialog_with_buttons(
            ctx,
            "wizard",
            &title,
            [360.0, 560.0],
            &mut open,
            |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match step {
                    Step::Usage => w.usage_ui(ui),
                    Step::Page(i) => {
                        let id = self.pages[i].id().to_string();
                        ui.heading(self.pages[i].title(lang));
                        if id == "station" {
                            ui.weak(tr("wizard.callsign_needed"));
                        }
                        ask = w.extras_ui(ui, &id);
                        ui.separator();
                        self.page_body(ui, i, lang);
                    }
                    Step::Done => Wizard::done_ui(ui),
                });
            },
            |ui| {
                moved = Wizard::buttons(ui, at, step == Step::Done, !station || call_ok, skip);
            },
        );
        self.ask = ask;
        if !open {
            settings
                .section(crate::wizard::SECTION)
                .insert("done".into(), "true".into());
            let _ = settings.save();
            self.open = false;
            return None;
        }
        match moved {
            Some(Move::Back) => w.at = w.at.saturating_sub(1),
            Some(Move::Next) => {
                if step == Step::Usage {
                    w.try_without_rig(&mut self.draft);
                }
                w.status = None;
                w.at += 1;
            }
            Some(Move::Finish) => {
                self.draft
                    .section(crate::wizard::SECTION)
                    .insert("done".into(), "true".into());
                let mut changed = self.merge(settings);
                if !changed.iter().any(|c| c == crate::wizard::SECTION) {
                    *settings.section(crate::wizard::SECTION) =
                        self.draft.section(crate::wizard::SECTION).clone();
                    let _ = settings.save();
                    changed.push(crate::wizard::SECTION.into());
                }
                self.open = false;
                return Some(changed);
            }
            None => {}
        }
        self.wizard = Some(w);
        None
    }

    fn merge(&mut self, settings: &mut Settings) -> Vec<String> {
        let mut changed = Vec::new();
        let mut ids: Vec<String> = self.pages.iter().map(|p| p.id().to_string()).collect();
        ids.push(crate::wizard::SECTION.into());
        ids.dedup();
        for id in ids {
            let edited = self.draft.section(&id).clone();
            let base = match self.drawn.get(&id) {
                Some(drawn) => drawn.clone(),
                None => self.base.section(&id).clone(),
            };
            if merge_edits(&base, &edited, settings.section(&id)) && !changed.contains(&id) {
                changed.push(id);
            }
        }
        let _ = settings.save();
        changed
    }

    pub(crate) fn show_with(&mut self, settings: &mut Settings) {
        if self.open {
            return;
        }
        self.draft = settings.clone();
        self.base = settings.clone();
        self.drawn.clear();
        self.search.clear();
        self.open = true;
    }

    pub(crate) fn section_infos(&self, lang: &str) -> Vec<zlog_ui::SectionInfo> {
        self.pages
            .iter()
            .map(|p| zlog_ui::SectionInfo {
                name: p.id().to_string(),
                title: p.title(lang),
                kind: p.section_kind(),
            })
            .collect()
    }

    pub(crate) fn show_page(&mut self, id: &str) {
        if let Some(i) = self.pages.iter().position(|p| p.id() == id) {
            self.page = i;
        }
    }

    pub(crate) fn set_commands(
        &mut self,
        commands: &[zlog_ui::keys::CommandInfo],
        reserved: &[egui::KeyboardShortcut],
    ) {
        for p in self.pages.iter_mut().filter_map(|p| p.shortcuts_mut()) {
            p.set_commands(commands, reserved);
        }
    }

    pub(crate) fn shortcuts(&self, settings: &mut Settings) -> zlog_ui::keys::CommandOverrides {
        let mut all = zlog_ui::keys::CommandOverrides::default();
        for p in &self.pages {
            if let Some(keys) = p.shortcuts() {
                all.extend(keys.overrides(settings.section(p.id())));
            }
        }
        all
    }

    pub(crate) fn wants_keys(&self) -> bool {
        self.open
            && self
                .pages
                .iter()
                .any(|p| p.shortcuts().is_some_and(|k| k.wants_keys()))
    }

    pub fn ui(&mut self, ctx: &egui::Context, settings: &mut Settings) -> Option<Vec<String>> {
        if !self.open {
            return None;
        }
        if self.wizard.is_some() {
            return self.wizard_ui(ctx, settings);
        }
        let mut open = self.open;
        let lang = i18n::current();
        let titles: Vec<String> = self.pages.iter().map(|p| p.title(lang)).collect();
        let search = &mut self.search;
        let page = &mut self.page;
        let (pages, draft, drawn) = (&mut self.pages, &mut self.draft, &mut self.drawn);
        let shown = dialog_with_pages(
            ctx,
            "options",
            tr("options.title"),
            [640.0, 560.0],
            &mut open,
            |ui| {
                let r = ui
                    .add(
                        egui::TextEdit::singleline(search)
                            .hint_text(tr("options.search"))
                            .desired_width(f32::INFINITY),
                    )
                    .on_hover_text(tr("options.search.tip"));
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::SearchInput, true, tr("options.search"))
                });
                let needle = search.trim().to_lowercase();
                for (i, title) in titles.iter().enumerate() {
                    if needle.is_empty() || title.to_lowercase().contains(&needle) {
                        ui.selectable_value(page, i, title);
                    }
                }
                *page
            },
            |ui, page| {
                if let Some(p) = pages.get_mut(page) {
                    let mut section = draft.section(p.id()).clone();
                    zlog_ui::form::shared(ui, |ui| {
                        p.ui_with(ui, &mut section, draft.sections(), lang)
                    });
                    drawn
                        .entry(p.id().to_string())
                        .or_insert_with(|| section.clone());
                    *draft.section(p.id()) = section;
                }
            },
            |ui| ok_cancel(ui, crate::i18n::current(), true, false),
        );
        let pressed = shown.and_then(|(_, b)| b);
        let ok = pressed == Some(true);
        if pressed == Some(false) || !open {
            self.open = false;
        }
        if !ok {
            return None;
        }
        let changed = self.merge(settings);
        self.open = false;
        Some(changed)
    }
}

pub(crate) struct StationPage;

impl SettingsPage for StationPage {
    fn id(&self) -> &str {
        "station"
    }

    fn section_kind(&self) -> zlog_ui::SectionKind {
        zlog_ui::SectionKind::Personal
    }

    fn title(&self, _language: &str) -> String {
        tr("options.station").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut zlog_ui::Section, _language: &str) {
        let mut s = Settings::only("station", section);
        let mut st = s.station();
        station_page(ui, &mut st);
        s.set_station(&st);
        *section = s.take("station");
    }
}

pub(crate) struct PreferencesPage {
    styles: Vec<Box<dyn zlog_ui::EntryStyle>>,
}

impl PreferencesPage {
    pub fn new(styles: Vec<Box<dyn zlog_ui::EntryStyle>>) -> Self {
        PreferencesPage { styles }
    }
}

impl SettingsPage for PreferencesPage {
    fn id(&self) -> &str {
        "preferences"
    }

    fn title(&self, _language: &str) -> String {
        tr("options.preferences").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut zlog_ui::Section, language: &str) {
        let mut s = Settings::only("preferences", section);
        let mut period = s.use_contest_period();
        ui.checkbox(&mut period, tr("options.use_contest_period"));
        ui.add_space(6.0);
        let mut p = s.preferences();
        preferences_page(ui, &mut p);
        s.set_use_contest_period(period);
        s.set_preferences(&p);
        if self.styles.len() > 1 {
            ui.add_space(6.0);
            let saved = s.entry_style();
            let mut style = saved.clone();
            let first = self.styles[0].id().to_string();
            let chosen = self
                .styles
                .iter()
                .find(|e| e.id() == style)
                .map_or(first, |e| e.id().to_string());
            let label = tr("options.entry_style");
            zlog_ui::form(ui, "entry_style_form", &[label], |ui, form| {
                ui.label(label);
                let name = |id: &str| {
                    let e = self.styles.iter().find(|e| e.id() == id);
                    e.map(|e| e.title(language)).unwrap_or_default()
                };
                form.fit(ui, |ui| {
                    form.combo("entry_style")
                        .selected_text(name(&chosen))
                        .show_ui(ui, |ui| {
                            for e in &self.styles {
                                let title = e.title(language);
                                ui.selectable_value(&mut style, e.id().to_string(), title);
                            }
                        });
                });
                ui.end_row();
            });
            if style != saved {
                s.set_entry_style(&style);
            }
        }
        *section = s.take("preferences");
    }
}

fn labels(keys: &[&str]) -> Vec<&'static str> {
    keys.iter().map(|k| tr(k)).collect()
}

fn station_page(ui: &mut Ui, st: &mut Station) {
    let keys = [
        "menu_form.callsign",
        "menu_form.operator",
        "menu_form.prov",
        "menu_form.city",
        "menu_form.age",
        "menu_form.cq_zone",
        "menu_form.iaru_zone",
        "menu_form.itu_region",
        "menu_form.iota",
        "menu_form.handle_cw",
        "menu_form.handle_ph",
        "menu_form.tx_nr",
    ];
    zlog_ui::form(ui, "station", &labels(&keys), |ui, form| {
        for (key, value) in [
            ("menu_form.callsign", &mut st.callsign),
            ("menu_form.operator", &mut st.operator),
            ("menu_form.prov", &mut st.prov),
            ("menu_form.city", &mut st.city),
            ("menu_form.age", &mut st.age),
            ("menu_form.cq_zone", &mut st.cq_zone),
            ("menu_form.iaru_zone", &mut st.iaru_zone),
            ("menu_form.itu_region", &mut st.itu_region),
            ("menu_form.iota", &mut st.iota),
        ] {
            ui.label(tr(key));
            form.text(ui, egui::TextEdit::singleline(value));
            *value = value.to_uppercase();
            ui.end_row();
        }
        for (key, value) in [
            ("menu_form.handle_cw", &mut st.handle_cw),
            ("menu_form.handle_ph", &mut st.handle_ph),
        ] {
            ui.label(tr(key));
            form.text(ui, egui::TextEdit::singleline(value));
            ui.end_row();
        }
        ui.label(tr("menu_form.tx_nr"));
        ui.add(egui::DragValue::new(&mut st.tx_nr).range(0..=15));
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label(RichText::new(tr("options.power_per_band")).strong());
    egui::ScrollArea::vertical()
        .max_height(ui.available_height().max(120.0))
        .show(ui, |ui| {
            egui::Grid::new("power").striped(true).show(ui, |ui| {
                ui.label("");
                for code in ['H', 'M', 'L', 'P'] {
                    ui.strong(code.to_string());
                }
                ui.end_row();
                for band in Band::range(Band::B1_9, Band::HIGHEST) {
                    ui.monospace(band.label());
                    let mut power = st.power_for(band);
                    for code in ['H', 'M', 'L', 'P'] {
                        let p = Power::from_code(code).unwrap();
                        ui.radio_value(&mut power, p, "")
                            .on_hover_text(format!("{} {code}", band.label()));
                    }
                    if power != st.power_for(band) {
                        st.power.insert(band, power);
                    }
                    ui.end_row();
                }
            });
        });
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("options.power_all"));
        for code in ['H', 'M', 'L', 'P'] {
            if ui.button(code.to_string()).clicked() {
                let p = Power::from_code(code).unwrap();
                for band in Band::range(Band::B1_9, Band::HIGHEST) {
                    st.power.insert(band, p);
                }
            }
        }
    });
}

fn preferences_page(ui: &mut Ui, p: &mut Preferences) {
    let keys = [
        "options.countdown",
        "options.qsy_count",
        "options.qsl_default",
        "options.pc_name",
    ];
    zlog_ui::form(ui, "preferences", &labels(&keys), |ui, form| {
        let mut on = p.qsy_rule.countdown_minutes.is_some();
        ui.checkbox(&mut on, tr("options.countdown"));
        ui.add_enabled(
            on,
            egui::DragValue::new(&mut p.countdown_minutes).range(1..=99),
        );
        p.set_countdown(on);
        ui.end_row();
        let mut on = p.qsy_rule.max_qsy_per_hour.is_some();
        ui.checkbox(&mut on, tr("options.qsy_count"));
        ui.add_enabled(on, egui::DragValue::new(&mut p.qsy_per_hour).range(1..=99));
        p.set_qsy_count(on);
        ui.end_row();
        ui.label(tr("options.qsl_default"));
        ui.horizontal_wrapped(|ui| {
            for (state, key) in [
                (QslState::None, "options.qsl_none"),
                (QslState::PseQsl, "options.qsl_pse"),
                (QslState::NoQsl, "options.qsl_no"),
            ] {
                ui.radio_value(&mut p.qsl_default, state, tr(key));
            }
        });
        ui.end_row();
        ui.label(tr("options.pc_name"));
        form.text(ui, egui::TextEdit::singleline(&mut p.pc_name));
        ui.end_row();
    });
}

fn merge_edits(
    base: &zlog_ui::Section,
    edited: &zlog_ui::Section,
    live: &mut zlog_ui::Section,
) -> bool {
    let before = live.clone();
    for k in base.keys().chain(edited.keys()) {
        match (base.get(k), edited.get(k)) {
            (b, e) if b == e => {}
            (_, Some(e)) => {
                live.insert(k.clone(), e.clone());
            }
            (_, None) => {
                live.remove(k);
            }
        }
    }
    *live != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_keeps_values_changed_elsewhere_while_open() {
        let mut o = Options::new(vec![Box::new(StationPage)]);
        let mut settings = Settings::default();
        settings
            .section("station")
            .insert("operator".into(), "JA1AAA".into());
        o.show_with(&mut settings);
        o.draft
            .section("station")
            .insert("callsign".into(), "JA1ZLO".into());
        settings
            .section("station")
            .insert("operator".into(), "JA1BBB".into());
        let mut live = settings.section("station").clone();
        let base = o.base.section("station").clone();
        assert!(merge_edits(&base, o.draft.section("station"), &mut live));
        assert_eq!(live["callsign"], "JA1ZLO");
        assert_eq!(live["operator"], "JA1BBB");
        assert!(!merge_edits(&base, &base, &mut live));
    }

    #[test]
    fn drawing_a_tab_is_not_an_edit() {
        let mut o = Options::new(vec![Box::new(StationPage)]);
        let mut settings = Settings::default();
        o.show_with(&mut settings);
        let ctx = egui::Context::default();
        let frame = |o: &mut Options, settings: &mut Settings, events| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut ok = None;
            let mut out = ctx.run_ui(input, |ui| ok = o.ui(ui.ctx(), settings));
            out.textures_delta.clear();
            ok
        };
        assert!(frame(&mut o, &mut settings, vec![]).is_none());
        let changed: Vec<(String, String)> = o
            .draft
            .section("station")
            .keys()
            .map(|k| (k.clone(), "elsewhere".to_string()))
            .collect();
        assert!(!changed.is_empty(), "the tab wrote its values");
        for (k, v) in &changed {
            settings.section("station").insert(k.clone(), v.clone());
        }
        let mut live = settings.section("station").clone();
        let drawn = o.drawn["station"].clone();
        assert!(!merge_edits(&drawn, o.draft.section("station"), &mut live));
        for (k, v) in &changed {
            assert_eq!(&live[k], v);
        }
    }

    #[test]
    fn opening_again_keeps_the_tabs_edits() {
        let mut o = Options::new(vec![Box::new(StationPage)]);
        let mut settings = Settings::default();
        o.show_with(&mut settings);
        o.draft
            .section("station")
            .insert("callsign".into(), "JA1ZLO".into());
        o.show_with(&mut settings);
        assert_eq!(o.draft.section("station")["callsign"], "JA1ZLO");
    }
}
