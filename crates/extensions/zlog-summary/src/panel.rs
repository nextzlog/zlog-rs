use egui::{RichText, Ui};
use zlog_ui::{Panel, Section, View};

use rxsl::sheets::summary::{self as jarl, Version};

use crate::form::{self, Form, Order, Score};
use crate::{cabrillo, float_to_str, text_fields, texts, today};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Elog,
    Cabrillo,
}

pub struct Summary {
    tab: Tab,
    form: Form,
    header: cabrillo::Header,
    claimed: Option<u64>,
    message: String,
    upload: Option<Box<dyn SubmitForm>>,
    upload_asked: bool,
    web: Option<Box<dyn WebSubmit>>,
}

pub trait WebSubmit {
    fn accepts(&self, name: &str, first_mode: Option<zlog_core::Mode>) -> bool;
    fn open(
        &self,
        name: &str,
        first_mode: Option<zlog_core::Mode>,
        elog: &[u8],
    ) -> Option<Box<dyn SubmitForm>>;
}

pub trait SubmitForm {
    fn ui(&mut self, ui: &mut Ui, language: &str);
    fn buttons(&mut self, ui: &mut Ui, language: &str) -> bool;
}

impl Default for Summary {
    fn default() -> Self {
        Summary {
            tab: Tab::Elog,
            form: crate::default_form(),
            header: cabrillo::Header::default(),
            claimed: None,
            message: String::new(),
            upload: None,
            upload_asked: false,
            web: None,
        }
    }
}

fn shown(key: &str, version: Version) -> bool {
    match key {
        "CategoryName" | "License" | "Equipment" | "ClubName" => version == Version::R1,
        "Age" => version == Version::R2,
        _ => true,
    }
}

fn save_as(name: &str, bytes: Result<Vec<u8>, String>) -> Result<bool, String> {
    let bytes = bytes?;
    let Some(path) = zlog_ui::dialog::save_file_as(name) else {
        return Ok(false);
    };
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    Ok(true)
}

impl Summary {
    fn save(&mut self, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        let operators = operators(view);
        self.form
            .fill_defaults(view.contest.name(), &view.station.callsign, &operators);
        let input = crate::Input {
            log: view.log,
            sheet: view.sheet,
            contest: view.contest,
            callsign: &view.station.callsign,
            operator: view.operator,
            period_hours: view.period_hours,
        };
        let (name, bytes) = match self.tab {
            Tab::Elog => match crate::elog_file(&self.form, &input) {
                Err(crate::Failure::Missing(k)) => {
                    self.message = t(k).into();
                    return;
                }
                Err(crate::Failure::Failed(e)) => ("log.em", Err(e)),
                Ok(b) => ("log.em", Ok(b)),
            },
            Tab::Cabrillo => ("log.cbr", crate::cabrillo_file(&self.header, &input)),
        };
        self.message = match save_as(name, bytes) {
            Ok(true) => t("saved").into(),
            Ok(false) => String::new(),
            Err(e) => e,
        };
    }

    fn upload_offered(&self, view: &View<'_>) -> bool {
        let first = view.log.qsos.first().map(|q| q.mode);
        self.web
            .as_ref()
            .is_some_and(|w| w.accepts(view.contest.name(), first))
    }

    pub fn with_web(mut self, web: Box<dyn WebSubmit>) -> Self {
        self.web = Some(web);
        self
    }

    fn upload(&mut self, view: &View<'_>) {
        let score = Score::new(view.sheet, view.log, view.contest.band_range());
        let operators = operators(view);
        self.form
            .fill_defaults(view.contest.name(), &view.station.callsign, &operators);
        if let Some(k) = self.form.missing().first() {
            self.message = texts().get(view.language, k).into();
            return;
        }
        match form::elog(&self.form, &score, view.log, view.period_hours) {
            Ok(bytes) => {
                let first = view.log.qsos.first().map(|q| q.mode);
                let web = self.web.as_ref();
                self.upload = web.and_then(|w| w.open(view.contest.name(), first, &bytes));
                self.message.clear();
            }
            Err(e) => self.message = e,
        }
    }

    fn claim(&mut self, score: u64) {
        if self.claimed != Some(score) {
            self.claimed = Some(score);
            self.header
                .values
                .insert("CLAIMED-SCORE".into(), score.to_string());
        }
    }

    fn score(&self, ui: &mut Ui, score: &Score, language: &str) {
        let t = |k| texts().get(language, k);
        use zlog_ui::layout::{cell, column_width};
        let num = |ui: &mut Ui, n: u32| cell(ui, RichText::new(n.to_string()).monospace());
        egui::Grid::new("summary_score")
            .striped(true)
            .min_col_width(column_width(ui, 4, 48.0))
            .show(ui, |ui| {
                ui.label(RichText::new(t("head.band")).strong());
                for k in ["head.qsos", "head.points", "head.multi"] {
                    cell(ui, RichText::new(t(k)).strong());
                }
                ui.end_row();
                let row = |ui: &mut Ui, name: &str, s: &zlog_core::BandScore| {
                    ui.monospace(name);
                    num(ui, s.qsos);
                    num(ui, s.points);
                    num(ui, s.multi);
                    ui.end_row();
                };
                for (b, s) in &score.bands {
                    row(ui, b.label(), s);
                }
                row(ui, t("head.total"), &score.total);
            });
        let total = float_to_str(score.total_score);
        match score.coeff {
            Some(c) => {
                ui.label(texts().format(language, "score.coeff", &[&total, &float_to_str(c)]))
            }
            None => ui.label(texts().format(language, "score.total", &[&total])),
        };
    }

    fn elog_form(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        let f = &mut self.form;
        f.fill_defaults(
            view.contest.name(),
            &view.station.callsign,
            &operators(view),
        );
        if f.summary.date.is_empty() {
            f.summary.date = jarl::date_text(today(view.log.utc_offset_minutes()));
        }
        ui.horizontal(|ui| {
            for v in [Version::R1, Version::R2] {
                ui.radio_value(&mut f.summary.version, v, v.label());
            }
        });
        let version = f.summary.version;
        let newcomer = f.is_newcomer();
        let labels: Vec<String> = std::iter::once(t("field.ContestName").to_string())
            .chain(
                text_fields(f)
                    .into_iter()
                    .filter(|(key, _, _)| shown(key, version))
                    .map(|(key, _, _)| {
                        texts()
                            .get(view.language, &format!("field.{key}"))
                            .to_string()
                    }),
            )
            .collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        zlog_ui::form(ui, "summary_elog", &labels, |ui, form| {
            ui.label(t("field.ContestName"));
            form.text(ui, egui::TextEdit::singleline(&mut f.summary.contest));
            ui.end_row();
            for (key, field, multi) in text_fields(f) {
                if !shown(key, version) {
                    continue;
                }
                ui.label(texts().get(view.language, &format!("field.{key}")));
                if multi {
                    form.multiline(ui, egui::TextEdit::multiline(field).desired_rows(2));
                } else {
                    form.text(ui, egui::TextEdit::singleline(field));
                }
                ui.end_row();
            }
        });
        let f = &mut self.form;
        if version == Version::R1 {
            ui.horizontal(|ui| {
                ui.radio_value(&mut f.power_measured, false, t("power.rated"));
                ui.radio_value(&mut f.power_measured, true, t("power.measured"));
            });
        }
        if version == Version::R2 && newcomer {
            ui.horizontal(|ui| {
                ui.label(t("field.LicenseDate"));
                let (mut y, mut m, mut d) = f.license_date.unwrap_or((2000, 1, 1));
                ui.add(egui::DragValue::new(&mut y).range(1950..=2100));
                ui.add(egui::DragValue::new(&mut m).range(1..=12));
                ui.add(egui::DragValue::new(&mut d).range(1..=31));
                f.license_date = Some((y, m, d));
            });
        }
        zlog_ui::form::buttons(ui, |ui| {
            if ui.button(t("oath.jarl")).clicked() {
                f.summary.oath = jarl::oath(true).into();
            }
            if ui.button(t("oath.other")).clicked() {
                f.summary.oath = jarl::oath(false).into();
            }
        });
        zlog_ui::field_row(
            ui,
            |ui| {
                ui.label(t("field.Date"));
            },
            |_| {},
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut f.summary.date).desired_width(f32::INFINITY))
            },
        );
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("summary_order")
                .selected_text(t(match f.order {
                    Order::AsEntered => "order.entered",
                    Order::Time => "order.time",
                    Order::BandThenTime => "order.band",
                }))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.order, Order::AsEntered, t("order.entered"));
                    ui.selectable_value(&mut f.order, Order::Time, t("order.time"));
                    ui.selectable_value(&mut f.order, Order::BandThenTime, t("order.band"));
                });
            ui.checkbox(&mut f.field_extend, t("field_extend"));
        });
    }

    fn cabrillo_form(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        let h = &mut self.header;
        ui.horizontal(|ui| {
            ui.label(t("zone"));
            ui.radio_value(&mut h.zone_hours, 0, "UTC");
            ui.radio_value(&mut h.zone_hours, 9, "JST");
        });
        ui.checkbox(&mut h.utf8, t("utf8"))
            .on_hover_text(t("utf8.tip"));
        zlog_ui::form(ui, "summary_cabrillo", &cabrillo::TAGS, |ui, form| {
            for tag in cabrillo::TAGS {
                ui.label(tag);
                let v = h.values.entry(tag.into()).or_default();
                let choices = cabrillo::choices(tag);
                if choices.is_empty() {
                    form.text(ui, egui::TextEdit::singleline(v));
                } else {
                    form.combo(tag).selected_text(v.clone()).show_ui(ui, |ui| {
                        ui.selectable_value(v, String::new(), "");
                        for c in choices {
                            ui.selectable_value(v, c.to_string(), *c);
                        }
                    });
                }
                ui.end_row();
            }
        });
    }
}

fn operators<'a>(view: &View<'a>) -> Vec<&'a str> {
    let mut out: Vec<&str> = view.log.qsos.iter().map(|q| q.operator.as_str()).collect();
    out.push(view.operator);
    out
}

impl Panel for Summary {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

    fn load(&mut self, section: &Section) {
        crate::load(&mut self.form, &mut self.header, section);
    }

    fn save(&self, section: &mut Section) {
        crate::save(&self.form, &self.header, section);
    }

    fn id(&self) -> &str {
        "summary"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Report
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Submit, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = |k| texts().get(language, k);
        let mut list = vec![zlog_ui::Command::window(t("toolbar"), t("toolbar.tip"))
            .icon(zlog_ui::icon::FILE_TEXT)
            .role("elog")];
        if self.web.is_some() {
            let mut upload =
                zlog_ui::Command::new("upload", t("toolbar.upload"), t("toolbar.upload.tip"))
                    .menu_label(t("upload"))
                    .icon(zlog_ui::icon::CLOUD_ARROW_UP)
                    .opens_window()
                    .dialog()
                    .role("jarl_upload");
            upload.menu = false;
            list.push(upload);
        }
        list
    }

    fn run_command(&mut self, id: &str) {
        if id == "upload" && self.web.is_some() {
            self.tab = Tab::Elog;
            self.upload_asked = true;
        }
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [520.0, 640.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        if let Some(u) = self.upload.as_mut() {
            u.ui(ui, view.language);
            return;
        }
        if std::mem::take(&mut self.upload_asked) {
            match self.upload_offered(view) {
                true if self.form.summary.version == Version::R2 => {
                    self.upload(view);
                    return;
                }
                true => self.message = t("upload.r2").into(),
                false => self.message = t("upload.none").into(),
            }
        }
        zlog_ui::layout::toolbar(ui, &[], |ui| {
            let options = [
                (Tab::Elog, t("tab.elog")),
                (Tab::Cabrillo, t("tab.cabrillo")),
            ];
            zlog_ui::layout::segmented(ui, &mut self.tab, &options);
        });
        self.claim(view.sheet.score);
        let score = Score::new(view.sheet, view.log, view.contest.band_range());
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
            Tab::Elog => {
                self.score(ui, &score, view.language);
                ui.separator();
                self.elog_form(ui, view);
            }
            Tab::Cabrillo => self.cabrillo_form(ui, view.language),
        });
    }

    fn capability_mut(&mut self, want: zlog_ui::WantMut) -> Option<zlog_ui::CapabilityMut<'_>> {
        match want {
            zlog_ui::WantMut::Buttons => Some(zlog_ui::CapabilityMut::Buttons(self)),
            _ => None,
        }
    }
}

impl zlog_ui::ButtonRow for Summary {
    fn buttons(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let t = |k| texts().get(view.language, k);
        if let Some(u) = self.upload.as_mut() {
            if u.buttons(ui, view.language) {
                self.upload = None;
            }
            return false;
        }
        zlog_ui::form::buttons(ui, |ui| {
            if ui.button(t("save")).clicked() {
                self.save(view);
            }
            if self.tab == Tab::Elog && self.upload_offered(view) {
                let r2 = self.form.summary.version == Version::R2;
                if ui
                    .add_enabled(r2, egui::Button::new(t("upload")))
                    .on_hover_text(t("upload.tip"))
                    .clicked()
                {
                    self.upload(view);
                }
            }
            ui.button(t("close")).clicked()
        })
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(550, |ctx| {
        let web = ctx.drivers.take::<dyn WebSubmit>().into_iter().next();
        match web {
            Some(web) => Summary::default().with_web(web),
            None => Summary::default(),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{escape, unescape};

    #[test]
    fn settings_round_trip() {
        let mut s = Summary::default();
        s.form.summary.address = "Tokyo\nBunkyo \\ 1".into();
        s.form.summary.version = Version::R1;
        s.form.license_date = Some((2024, 5, 6));
        s.header.utf8 = true;
        s.header.values.insert("CONTEST".into(), "CQ-WW-CW".into());
        let mut section = Section::new();
        s.save(&mut section);
        assert!(!section["elog.Address"].contains('\n'));
        let mut r = Summary::default();
        r.load(&section);
        assert_eq!(r.form.summary.address, s.form.summary.address);
        assert_eq!(r.form.summary.version, Version::R1);
        assert!(r.header.utf8);
        assert_eq!(r.form.license_date, Some((2024, 5, 6)));
        assert_eq!(r.header.get("CONTEST"), "CQ-WW-CW");
    }

    #[test]
    fn claimed_score_is_set_once_per_score() {
        let mut s = Summary::default();
        s.claim(120);
        assert_eq!(s.header.get("CLAIMED-SCORE"), "120");
        s.header.values.insert("CLAIMED-SCORE".into(), "100".into());
        s.claim(120);
        assert_eq!(s.header.get("CLAIMED-SCORE"), "100");
        s.claim(130);
        assert_eq!(s.header.get("CLAIMED-SCORE"), "130");
        s.claim(0);
        assert_eq!(s.header.get("CLAIMED-SCORE"), "0");
    }

    #[test]
    fn claimed_score_is_not_saved() {
        let mut s = Summary::default();
        s.claim(5);
        let mut section = Section::new();
        s.save(&mut section);
        assert!(!section.contains_key("cabrillo.CLAIMED-SCORE"));
    }

    #[test]
    fn escape_round_trip_edge_cases() {
        for v in ["", "\\", "a\\n", "\n\n", "end\\"] {
            assert_eq!(unescape(&escape(v)), v);
        }
        assert_eq!(unescape("a\\"), "a\\");
    }

    #[test]
    fn field_labels_exist() {
        let mut f = Form::default();
        for (key, _, _) in text_fields(&mut f) {
            let k = format!("field.{key}");
            assert_ne!(texts().get("en", &k), k);
        }
    }

    #[test]
    fn zlog_defaults() {
        let s = Summary::default();
        assert_eq!(s.form.summary.version, Version::R1);
        assert_eq!(s.form.summary.address, "\u{3012}");
        assert!(!s.header.utf8);
        let mut r = Summary::default();
        r.form.summary.version = Version::R2;
        r.load(&Section::new());
        assert_eq!(r.form.summary.version, Version::R1);
        let mut section = Section::new();
        section.insert("elog.Version".into(), "1".into());
        r.load(&section);
        assert_eq!(r.form.summary.version, Version::R2);
        section.insert("elog.Version".into(), "junk".into());
        r.load(&section);
        assert_eq!(r.form.summary.version, Version::R1);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
