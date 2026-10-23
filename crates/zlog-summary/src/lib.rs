//! Summary sheets: the JARL E-Log summary sheet (R1.0 or R2.1, `UELogJarlEx`) and
//! the Cabrillo 3.0 header (`UELogCabrillo`), with their form, checks and score.
//!
//! The QSOs of each document are written by zlog-qxsl, the only log file writer
//! ([`zlog_qxsl::encode`]: QxSL `jarl` for the E-Log log sheet, `cqww` for the
//! Cabrillo QSO lines). The documents are made by pure functions ([`elog`],
//! [`cabrillo`]); the panel keeps the form, shows the score of [`View::sheet`] and
//! saves the file. The summary parts are written in Shift_JIS with CRLF, as zLog
//! writes them. With the `jarl` feature the E-Log R2.1 of the JARL contests can
//! be submitted on JARL's Web form (`zlog_jarl::upload`).

pub mod cabrillo;
pub mod delphi;
pub mod elog;

use egui::{RichText, Ui};
use zlog_core::Log;
use zlog_ui::{Panel, Section, View};

use elog::{Form, Order, Score, Version};

zlog_core::texts!();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Elog,
    Cabrillo,
}

/// The submission window.
pub struct Summary {
    tab: Tab,
    form: Form,
    header: cabrillo::Header,
    /// The score last put into `CLAIMED-SCORE`, so that the field is filled once
    /// and again only when the score changes, keeping what the user typed
    /// (`TformELogCabrillo.InitializeFields` fills it when the form opens).
    claimed: Option<u64>,
    message: String,
    /// The Web submission shown in place of the form, while there is one.
    #[cfg(feature = "jarl")]
    upload: Option<zlog_jarl::upload::Upload>,
}

impl Default for Summary {
    fn default() -> Self {
        Summary {
            tab: Tab::Elog,
            form: Form {
                // zLog's default address (`Address1`, the postal mark), in ja.lang.
                address: texts().get("ja", "doc.address").into(),
                oath: elog::oath(true).into(),
                newcomer_categories: elog::NEWCOMER.into(),
                senior_junior_categories: elog::SENIOR_JUNIOR.into(),
                ..Form::default()
            },
            header: cabrillo::Header::default(),
            claimed: None,
            message: String::new(),
            #[cfg(feature = "jarl")]
            upload: None,
        }
    }
}

/// Multi-line values are kept on one line: `\n` for a line break, `\\` for `\`.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            },
            (c, false) => out.push(c),
        }
    }
    out
}

/// The text fields of the form with their settings keys (`SummaryInfo` of
/// `zlog.ini`) and label keys.
fn text_fields(f: &mut Form) -> [(&'static str, &mut String, bool); 20] {
    [
        ("CategoryCode", &mut f.category_code, false),
        ("CategoryName", &mut f.category_name, false),
        ("Callsign", &mut f.callsign, false),
        ("OperatorCallsign", &mut f.op_callsign, false),
        ("Address", &mut f.address, true),
        ("Telephone", &mut f.tel, false),
        ("OperatorName", &mut f.name, false),
        ("EMail", &mut f.email, false),
        ("License", &mut f.license, false),
        ("Power", &mut f.power, false),
        ("QTH", &mut f.qth, false),
        ("PowerSupply", &mut f.power_supply, false),
        ("Equipment", &mut f.equipment, true),
        ("Age", &mut f.age, false),
        ("Comment", &mut f.comments, true),
        ("MultiOpList", &mut f.multi_op_list, true),
        ("ClubID", &mut f.club_id, false),
        ("ClubName", &mut f.club_name, false),
        ("Oath", &mut f.oath, true),
        ("Signature", &mut f.signature, false),
    ]
}

/// Fields shown only for one version (`TabControl1Change`).
fn shown(key: &str, version: Version) -> bool {
    match key {
        "CategoryName" | "License" | "Equipment" | "ClubName" => version == Version::R1,
        "Age" => version == Version::R2,
        _ => true,
    }
}

/// Today's date: in the zone of the log, and in JST (UTC+9) for a log kept in UTC
/// (`offset_minutes` `None`), as zLog takes the date from the PC clock, which is
/// Japanese time on the PCs it is used on.
fn today(offset_minutes: Option<i32>) -> (i32, u32, u32) {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let t = 25569.0 + (unix + offset_minutes.unwrap_or(540) as f64 * 60.0) / 86400.0;
    let (y, m, d, ..) = zlog_core::DelphiTime(t).to_ymd_hms();
    (y, m, d)
}

/// The log with only its QSOs in the contest period of `period_hours`
/// ([`Log::out_of_period`]), for the encoders.
pub(crate) fn without_out_of_period(log: &Log, period_hours: u32) -> Log {
    let mut out = log.clone();
    out.qsos.retain(|q| !log.out_of_period(q, period_hours));
    out
}

/// Write `bytes` to a file the user picks.
fn save_as(name: &str, bytes: Result<Vec<u8>, String>) -> Result<bool, String> {
    let bytes = bytes?;
    let Some(path) = rfd::FileDialog::new().set_file_name(name).save_file() else {
        return Ok(false);
    };
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    Ok(true)
}

impl Summary {
    /// Write the document of the tab shown, asking where.
    fn save(&mut self, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        let score = Score::new(view.sheet, view.log, view.contest.band_range());
        let operators = operators(view);
        self.form
            .fill_defaults(view.contest.name(), &view.station.callsign, &operators);
        self.message = match self.tab {
            Tab::Elog => match self.form.missing().first() {
                Some(k) => t(k).into(),
                None => {
                    let bytes = elog::elog(
                        &self.form,
                        &score,
                        view.log,
                        view.contest,
                        view.period_hours,
                    );
                    match save_as("log.em", bytes) {
                        Ok(true) => t("saved").into(),
                        Ok(false) => String::new(),
                        Err(e) => e,
                    }
                }
            },
            Tab::Cabrillo => {
                // The own callsign defaults to the application's station.
                let mut header = self.header.clone();
                if header.get("CALLSIGN").is_empty() {
                    header
                        .values
                        .insert("CALLSIGN".into(), view.station.callsign.clone());
                }
                let bytes = cabrillo::document(
                    &header,
                    view.log,
                    view.contest,
                    &view.station.callsign,
                    view.period_hours,
                );
                match save_as("log.cbr", bytes) {
                    Ok(true) => t("saved").into(),
                    Ok(false) => String::new(),
                    Err(e) => e,
                }
            }
        };
    }

    /// The contest of JARL's upload page for the open log, `None` when zLog hides
    /// the Web upload button (`TformELogJarlEx.FormShow`).
    #[cfg(feature = "jarl")]
    fn upload_contest(view: &View<'_>) -> Option<zlog_jarl::upload::Contest> {
        let first = view.log.qsos.first().map(|q| q.mode);
        zlog_jarl::upload::contest_of(view.contest.name(), first)
    }

    /// Open the Web submission of the E-Log (`buttonWebUploadClick`): the same
    /// document as [`Summary::save`] writes, as text.
    #[cfg(feature = "jarl")]
    fn upload(&mut self, view: &View<'_>, contest: zlog_jarl::upload::Contest) {
        let score = Score::new(view.sheet, view.log, view.contest.band_range());
        let operators = operators(view);
        self.form
            .fill_defaults(view.contest.name(), &view.station.callsign, &operators);
        if let Some(k) = self.form.missing().first() {
            self.message = texts().get(view.language, k).into();
            return;
        }
        match elog::elog(
            &self.form,
            &score,
            view.log,
            view.contest,
            view.period_hours,
        ) {
            Ok(bytes) => {
                let text = encoding_rs::SHIFT_JIS.decode(&bytes).0.into_owned();
                self.message.clear();
                self.upload = Some(zlog_jarl::upload::Upload::new(contest, text));
            }
            Err(e) => self.message = e,
        }
    }

    /// Put `score` into `CLAIMED-SCORE` when it differs from the score put there last.
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
        egui::Grid::new("summary_score")
            .striped(true)
            .show(ui, |ui| {
                for k in ["head.band", "head.qsos", "head.points", "head.multi"] {
                    ui.label(RichText::new(t(k)).strong());
                }
                ui.end_row();
                let row = |ui: &mut Ui, name: &str, s: &zlog_core::BandScore| {
                    ui.monospace(name);
                    ui.monospace(s.qsos.to_string());
                    ui.monospace(s.points.to_string());
                    ui.monospace(s.multi.to_string());
                    ui.end_row();
                };
                for (b, s) in &score.bands {
                    row(ui, b.label(), s);
                }
                row(ui, t("head.total"), &score.total);
            });
        let total = delphi::float_to_str(score.total_score);
        match score.coeff {
            Some(c) => ui.label(texts().format(
                language,
                "score.coeff",
                &[&total, &delphi::float_to_str(c)],
            )),
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
        if f.date.is_empty() {
            let (y, m, d) = today(view.log.utc_offset_minutes());
            f.date = elog::date_text(y, m, d);
        }
        ui.horizontal(|ui| {
            ui.radio_value(&mut f.version, Version::R1, "R1.0");
            ui.radio_value(&mut f.version, Version::R2, "R2.1");
        });
        let version = f.version;
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
        let width = zlog_ui::form_field_width(ui, &labels);
        egui::Grid::new("summary_elog")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(t("field.ContestName"));
                zlog_ui::sized_field(ui, egui::TextEdit::singleline(&mut f.contest_name), width);
                ui.end_row();
                for (key, field, multi) in text_fields(f) {
                    if !shown(key, version) {
                        continue;
                    }
                    ui.label(texts().get(view.language, &format!("field.{key}")));
                    if multi {
                        ui.add(
                            egui::TextEdit::multiline(field)
                                .desired_rows(2)
                                .desired_width(width),
                        );
                    } else {
                        zlog_ui::sized_field(ui, egui::TextEdit::singleline(field), width);
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
        ui.horizontal(|ui| {
            if ui.button(t("oath.jarl")).clicked() {
                f.oath = elog::oath(true).into();
            }
            if ui.button(t("oath.other")).clicked() {
                f.oath = elog::oath(false).into();
            }
        });
        zlog_ui::field_row(
            ui,
            |ui| {
                ui.label(t("field.Date"));
            },
            |_| {},
            |ui| ui.add(egui::TextEdit::singleline(&mut f.date).desired_width(f32::INFINITY)),
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
        });
    }

    fn cabrillo_form(&mut self, ui: &mut Ui) {
        let h = &mut self.header;
        let width = zlog_ui::form_field_width(ui, &cabrillo::TAGS);
        egui::Grid::new("summary_cabrillo")
            .num_columns(2)
            .show(ui, |ui| {
                for tag in cabrillo::TAGS {
                    ui.label(tag);
                    let v = h.values.entry(tag.into()).or_default();
                    let choices = cabrillo::choices(tag);
                    if choices.is_empty() {
                        zlog_ui::sized_field(ui, egui::TextEdit::singleline(v), width);
                    } else {
                        egui::ComboBox::from_id_salt(tag)
                            .selected_text(v.clone())
                            .width(width)
                            .show_ui(ui, |ui| {
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

/// The operators of the log's QSOs, then the current one (for the multi-operator
/// list).
fn operators<'a>(view: &View<'a>) -> Vec<&'a str> {
    let mut out: Vec<&str> = view.log.qsos.iter().map(|q| q.operator.as_str()).collect();
    out.push(view.operator);
    out
}

impl Panel for Summary {
    /// Settings: the form fields and the Cabrillo header (`SummaryInfo` of
    /// `zlog.ini` and of `zlog_cbr.ini`).
    fn load(&mut self, section: &Section) {
        let get = |k: &str| section.get(k).map(|v| unescape(v));
        for (key, field, _) in text_fields(&mut self.form) {
            if let Some(v) = get(&format!("elog.{key}")) {
                *field = v;
            }
        }
        self.form.version = match get("elog.Version").as_deref() {
            Some("1") => Version::R2,
            _ => Version::R1,
        };
        self.form.power_measured = get("elog.PowerType").as_deref() == Some("1");
        self.form.license_date = get("elog.LicenseDate").and_then(|d| {
            let v: Vec<&str> = d.split('-').collect();
            match v[..] {
                [y, m, d] => Some((y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)),
                _ => None,
            }
        });
        if let Some(v) = get("elog.NewComer") {
            self.form.newcomer_categories = v;
        }
        if let Some(v) = get("elog.SeniorJunior") {
            self.form.senior_junior_categories = v;
        }
        for tag in cabrillo::TAGS {
            if let Some(v) = get(&format!("cabrillo.{tag}")) {
                self.header.values.insert(tag.into(), v);
            }
        }
    }

    fn save(&self, section: &mut Section) {
        let mut form = self.form.clone();
        for (key, field, _) in text_fields(&mut form) {
            section.insert(format!("elog.{key}"), escape(field));
        }
        let version = if form.version == Version::R1 {
            "0"
        } else {
            "1"
        };
        section.insert("elog.Version".into(), version.into());
        section.insert(
            "elog.PowerType".into(),
            (form.power_measured as u8).to_string(),
        );
        if let Some((y, m, d)) = form.license_date {
            section.insert("elog.LicenseDate".into(), format!("{y:04}-{m:02}-{d:02}"));
        }
        section.insert("elog.NewComer".into(), form.newcomer_categories);
        section.insert("elog.SeniorJunior".into(), form.senior_junior_categories);
        for (tag, v) in &self.header.values {
            // The claimed score comes from the log each time.
            if tag != "CLAIMED-SCORE" {
                section.insert(format!("cabrillo.{tag}"), escape(v));
            }
        }
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
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::FILE_TEXT)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [520.0, 640.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        #[cfg(feature = "jarl")]
        if let Some(u) = self.upload.as_mut() {
            u.ui(ui, view.language);
            return;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Elog, t("tab.elog"));
            ui.selectable_value(&mut self.tab, Tab::Cabrillo, t("tab.cabrillo"));
        });
        ui.separator();
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
            Tab::Cabrillo => self.cabrillo_form(ui),
        });
    }

    fn has_buttons(&self) -> bool {
        true
    }

    fn buttons(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let t = |k| texts().get(view.language, k);
        #[cfg(feature = "jarl")]
        if let Some(u) = self.upload.as_mut() {
            // Closing the submission goes back to the form.
            if u.buttons(ui, view.language) {
                self.upload = None;
            }
            return false;
        }
        ui.horizontal(|ui| {
            if ui.button(t("save")).clicked() {
                self.save(view);
            }
            // Shown for the JARL contests, enabled for R2.1 (`TabControl1Change`).
            #[cfg(feature = "jarl")]
            if let Some(contest) = Self::upload_contest(view).filter(|_| self.tab == Tab::Elog) {
                let r2 = self.form.version == Version::R2;
                if ui
                    .add_enabled(r2, egui::Button::new(t("upload")))
                    .on_hover_text(t("upload.tip"))
                    .clicked()
                {
                    self.upload(view, contest);
                }
            }
            ui.button(t("close")).clicked()
        })
        .inner
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use zlog_core::{Band, Contest, ContestMode, Power, Qso, Scoring, Station};

    /// A contest without contest fields, for the encoders.
    pub(crate) struct Plain;

    impl Contest for Plain {
        fn name(&self) -> &str {
            "Plain"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::HIGHEST)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Cw
        }
        fn sent_number(&self, _: Band, _: Power, _: &Station) -> String {
            String::new()
        }
        fn valid_number(&self, _: &Qso) -> bool {
            true
        }
        fn score(&mut self, _: &Log) -> Scoring {
            Scoring::default()
        }
    }

    #[test]
    fn settings_round_trip() {
        let mut s = Summary::default();
        s.form = Form {
            address: "Tokyo\nBunkyo \\ 1".into(),
            version: Version::R1,
            license_date: Some((2024, 5, 6)),
            ..s.form
        };
        s.header.values.insert("CONTEST".into(), "CQ-WW-CW".into());
        let mut section = Section::new();
        s.save(&mut section);
        assert!(!section["elog.Address"].contains('\n'));
        let mut r = Summary::default();
        r.load(&section);
        assert_eq!(r.form.address, s.form.address);
        assert_eq!(r.form.version, Version::R1);
        assert_eq!(r.form.license_date, Some((2024, 5, 6)));
        assert_eq!(r.header.get("CONTEST"), "CQ-WW-CW");
    }

    #[test]
    fn claimed_score_is_set_once_per_score() {
        // Before the fix the field was overwritten with the sheet's score every frame.
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
        // A lone backslash at the end is kept.
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
        assert_eq!(s.form.version, Version::R1);
        assert_eq!(s.form.address, "\u{3012}");
        // Settings without a version open at R1.0, as zLog's default 0.
        let mut r = Summary::default();
        r.form.version = Version::R2;
        r.load(&Section::new());
        assert_eq!(r.form.version, Version::R1);
        let mut section = Section::new();
        section.insert("elog.Version".into(), "1".into());
        r.load(&section);
        assert_eq!(r.form.version, Version::R2);
        section.insert("elog.Version".into(), "junk".into());
        r.load(&section);
        assert_eq!(r.form.version, Version::R1);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
