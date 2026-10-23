//! The QSO rate window (`TRateDialogEx`): last-10 and last-100 rates, the graph of
//! QSOs per hour against the targets, the per-band per-hour table (ZAQ2), the
//! target editor (`TTargetEditor`) and the analysis text (`TZAnalyze`).
//!
//! Everything is derived from [`View::log`]; the counting is in [`count`] and
//! [`analyze`], and is redone only when the log or a setting changes.

pub mod analyze;
pub mod count;
mod tables;

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use zlog_core::{Band, Log};
use zlog_ui::{Panel, Section, View};

use count::{bands, Column, GraphStyle, Hours, StartPosition, MAX_HOURS};

zlog_core::texts!();

/// Hours the graph can show (`ShowLastCombo`).
pub const SHOW_HOURS: [usize; 7] = [3, 6, 12, 18, 24, 36, 48];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Graph,
    Table,
    Targets,
    Analyze,
}

/// What the derived numbers were computed from.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    qsos: usize,
    /// Sum of the QSO IDs, whose last digits count the edits of each QSO.
    edits: i64,
    last: u64,
    now_hour: i64,
    position: StartPosition,
    style: GraphStyle,
    show: usize,
    targets: u64,
    show_cw: bool,
    exclude_zero_points: bool,
    exclude_zero_hours: bool,
    language: String,
}

#[derive(Default)]
struct Derived {
    columns: Vec<Column>,
    actual: Hours,
    chart: analyze::TimeChart,
    /// The analysis as zLog's text (ZAF and ZAQ), for Copy and Save.
    analysis: String,
}

/// The rate window.
pub struct Rate {
    tab: Tab,
    position: StartPosition,
    style: GraphStyle,
    show: usize,
    targets: Hours,
    show_cw: bool,
    exclude_zero_points: bool,
    exclude_zero_hours: bool,
    /// Highest rates seen while the window was open (`Last10QsoRateMax`).
    max10: f64,
    max100: f64,
    key: Option<Key>,
    derived: Derived,
}

impl Default for Rate {
    fn default() -> Self {
        Rate {
            tab: Tab::Graph,
            position: StartPosition::CurrentTime,
            style: GraphStyle::Original,
            show: 12,
            targets: Hours::default(),
            show_cw: false,
            exclude_zero_points: false,
            exclude_zero_hours: false,
            max10: 0.0,
            max100: 0.0,
            key: None,
            derived: Derived::default(),
        }
    }
}

/// Bar colour of a band.
fn band_color(b: Band) -> Color32 {
    let h = (b as usize as f32 * 0.13) % 1.0;
    egui::ecolor::Hsva::new(h, 0.55, 0.85, 1.0).into()
}

/// Colour of a frequency range of [`GraphStyle::ByRange`].
fn range_color(r: usize) -> Color32 {
    [Band::B7, Band::B21, Band::B50, Band::B430, Band::B5600]
        .map(band_color)
        .get(r)
        .copied()
        .unwrap_or(Color32::GRAY)
}

impl Rate {
    fn key(&self, log: &Log, now: f64, language: &str) -> Key {
        let targets = self
            .targets
            .0
            .iter()
            .flatten()
            .enumerate()
            .fold(0u64, |a, (i, t)| {
                a.wrapping_mul(31).wrapping_add(*t as u64 ^ i as u64)
            });
        Key {
            qsos: log.qsos.len(),
            edits: log.qsos.iter().map(|q| q.reserve3 as i64).sum(),
            last: log.qsos.last().map_or(0, |q| q.time.0.to_bits()),
            now_hour: (now * 24.0).floor() as i64,
            position: self.position,
            style: self.style,
            show: self.show,
            targets,
            show_cw: self.show_cw,
            exclude_zero_points: self.exclude_zero_points,
            exclude_zero_hours: self.exclude_zero_hours,
            language: language.into(),
        }
    }

    fn refresh(&mut self, view: &View<'_>, now: f64) {
        let key = self.key(view.log, now, view.language);
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let qsos = &view.log.qsos;
        let chart = analyze::TimeChart::new(qsos, self.exclude_zero_points);
        let mut lines = analyze::zaf(&chart, self.show_cw, self.exclude_zero_hours, view.language);
        lines.extend(analyze::zaq(&chart, view.language));
        self.derived = Derived {
            columns: count::graph(qsos, &self.targets, now, self.position, self.show),
            actual: count::origin(qsos).map_or_else(Hours::default, |o| count::actual(qsos, o)),
            analysis: lines.join("\n"),
            chart,
        };
        self.key = Some(key);
    }

    fn rates(&mut self, ui: &mut Ui, view: &View<'_>, now: f64) {
        let t = |k| texts().get(view.language, k);
        let qsos = &view.log.qsos;
        let r10 = count::last_rate(qsos, view.station.tx_nr, 10, now);
        let r100 = count::last_rate(qsos, view.station.tx_nr, 100, now);
        self.max10 = self.max10.max(r10.unwrap_or(0.0));
        self.max100 = self.max100.max(r100.unwrap_or(0.0));
        let rate = |r: Option<f64>| match r {
            Some(r) => texts().format(view.language, "rate", &[&format!("{r:3.2}")]),
            None => "-".into(),
        };
        let max = |m: f64| texts().format(view.language, "max", &[&format!("{m:3.2}")]);
        egui::Grid::new("rate_last").show(ui, |ui| {
            ui.label(t("last10"));
            ui.monospace(rate(r10));
            ui.monospace(max(self.max10));
            ui.end_row();
            ui.label(t("last100"));
            ui.monospace(rate(r100));
            ui.monospace(max(self.max100));
            ui.end_row();
        });
        let optime = count::hours_minutes(count::operating_minutes(qsos, now));
        ui.label(texts().format(view.language, "optime", &[&optime]));
    }

    fn graph_options(&mut self, ui: &mut Ui, language: &str) {
        let t = |k| texts().get(language, k);
        ui.horizontal(|ui| {
            for (p, k) in [
                (StartPosition::FirstQso, "show.first"),
                (StartPosition::CurrentTime, "show.current"),
                (StartPosition::LastQso, "show.last"),
            ] {
                ui.radio_value(&mut self.position, p, t(k));
            }
            egui::ComboBox::from_id_salt("rate_hours")
                .selected_text(self.show.to_string())
                .width(48.0)
                .show_ui(ui, |ui| {
                    for h in SHOW_HOURS {
                        ui.selectable_value(&mut self.show, h, h.to_string());
                    }
                });
            ui.label(t("hours"));
        });
        ui.horizontal(|ui| {
            for (s, k) in [
                (GraphStyle::Original, "style.original"),
                (GraphStyle::ByBand, "style.band"),
                (GraphStyle::ByRange, "style.range"),
            ] {
                ui.radio_value(&mut self.style, s, t(k));
            }
        });
    }

    /// The bar chart with the cumulative lines (`Chart1`), drawn with the painter.
    fn draw_graph(&self, ui: &mut Ui) {
        let columns = &self.derived.columns;
        let size = Vec2::new(ui.available_width(), ui.available_height().max(160.0));
        let (response, painter) = ui.allocate_painter(size, Sense::hover());
        let visuals = ui.visuals();
        let text = visuals.text_color();
        let font = FontId::monospace(10.0);
        let frame = Rect::from_min_max(
            response.rect.min + Vec2::new(32.0, 8.0),
            response.rect.max - Vec2::new(40.0, 18.0),
        );
        painter.rect_stroke(
            frame,
            0.0,
            Stroke::new(1.0, visuals.weak_text_color()),
            egui::StrokeKind::Inside,
        );
        if columns.is_empty() {
            return;
        }
        // Scales as zLog's: hours in steps of 10, totals in steps of 50.
        let peak = columns
            .iter()
            .map(|c| c.actual_sum().max(c.target_sum()))
            .max()
            .unwrap_or(0);
        let left = ((peak / 10) + 1) * 10;
        let total = columns
            .iter()
            .map(|c| c.actual_total.max(c.target_total))
            .max()
            .unwrap_or(0);
        let right = ((total / 50) + 1) * 50;
        let y = |v: f32, max: u32| frame.bottom() - frame.height() * v / max as f32;
        for (v, max, x, align) in [
            (left, left, frame.left() - 4.0, Align2::RIGHT_CENTER),
            (right, right, frame.right() + 4.0, Align2::LEFT_CENTER),
        ] {
            painter.text(
                Pos2::new(x, y(v as f32, max)),
                align,
                v.to_string(),
                font.clone(),
                text,
            );
            painter.text(Pos2::new(x, frame.bottom()), align, "0", font.clone(), text);
        }
        let n = columns.len() as f32;
        let width = frame.width() / n;
        let mut actual_line = Vec::new();
        let mut target_line = Vec::new();
        for (i, c) in columns.iter().enumerate() {
            let x0 = frame.left() + width * i as f32;
            let bar = width * 0.4;
            // Actual QSOs, stacked by the style.
            let parts: Vec<(u32, Color32)> = match self.style {
                GraphStyle::Original => vec![(c.actual_sum(), band_color(Band::B7))],
                GraphStyle::ByBand => bands()
                    .map(|b| (c.actual[b as usize], band_color(b)))
                    .collect(),
                GraphStyle::ByRange => (0..5)
                    .map(|r| {
                        let n = bands()
                            .filter(|b| count::range_of(*b) == r)
                            .map(|b| c.actual[b as usize])
                            .sum();
                        (n, range_color(r))
                    })
                    .collect(),
            };
            let mut base = 0;
            for (n, color) in parts.into_iter().filter(|(n, _)| *n > 0) {
                let r = Rect::from_x_y_ranges(
                    x0 + width * 0.1..=x0 + width * 0.1 + bar,
                    y((base + n) as f32, left)..=y(base as f32, left),
                );
                painter.rect_filled(r, 0.0, color);
                base += n;
            }
            if base > 0 {
                let p = Pos2::new(x0 + width * 0.1 + bar / 2.0, y(base as f32, left) - 6.0);
                painter.text(
                    p,
                    Align2::CENTER_CENTER,
                    base.to_string(),
                    font.clone(),
                    text,
                );
            }
            // Target QSOs beside them.
            let t = c.target_sum();
            if t > 0 {
                let r = Rect::from_x_y_ranges(
                    x0 + width * 0.5..=x0 + width * 0.5 + bar,
                    y(t as f32, left)..=frame.bottom(),
                );
                painter.rect_filled(r, 0.0, visuals.weak_text_color().gamma_multiply(0.5));
            }
            let mid = x0 + width / 2.0;
            actual_line.push(Pos2::new(mid, y(c.actual_total as f32, right)));
            target_line.push(Pos2::new(mid, y(c.target_total as f32, right)));
            // Hour labels, thinned as zLog does for long graphs.
            let show = (columns.len() <= 12 || c.hour % 2 == 0)
                && (columns.len() <= 24 || c.hour % 4 == 0);
            if show {
                let p = Pos2::new(mid, frame.bottom() + 8.0);
                painter.text(
                    p,
                    Align2::CENTER_CENTER,
                    c.hour.to_string(),
                    font.clone(),
                    text,
                );
            }
        }
        painter.add(egui::Shape::line(
            actual_line,
            Stroke::new(2.0, Color32::from_rgb(220, 60, 60)),
        ));
        if self.targets.total() > 0 {
            let s = Stroke::new(1.5, visuals.weak_text_color());
            painter.add(egui::Shape::line(target_line, s));
        }
    }

    /// QSOs and targets per band and hour (ZAQ2, `TargetToGrid2`).
    fn table(&self, ui: &mut Ui, language: &str) {
        let t = |k| texts().get(language, k);
        let actual = &self.derived.actual;
        let rows: Vec<Band> = bands()
            .filter(|b| actual.band_total(*b) + self.targets.band_total(*b) > 0)
            .collect();
        let hours = (0..MAX_HOURS)
            .rev()
            .find(|h| actual.hour_total(*h) + self.targets.hour_total(*h) > 0)
            .map_or(0, |h| h + 1);
        let cell = |a: u32, g: u32| match (a, g) {
            (0, 0) => String::new(),
            (a, 0) => a.to_string(),
            (a, g) => format!("{a}/{g}"),
        };
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("rate_table").striped(true).show(ui, |ui| {
                ui.label(RichText::new(t("band")).strong());
                for h in 0..hours {
                    ui.label(RichText::new((h + 1).to_string()).strong());
                }
                ui.label(RichText::new(t("total")).strong());
                ui.end_row();
                for b in &rows {
                    ui.monospace(b.label());
                    for h in 0..hours {
                        ui.monospace(cell(actual.get(*b, h), self.targets.get(*b, h)));
                    }
                    ui.monospace(cell(actual.band_total(*b), self.targets.band_total(*b)));
                    ui.end_row();
                }
                ui.label(RichText::new(t("total")).strong());
                for h in 0..hours {
                    ui.monospace(cell(actual.hour_total(h), self.targets.hour_total(h)));
                }
                ui.monospace(cell(actual.total(), self.targets.total()));
                ui.end_row();
            });
        });
    }

    /// The target editor (`TTargetEditor`).
    fn targets(&mut self, ui: &mut Ui, language: &str, period_hours: u32) {
        let t = |k| texts().get(language, k);
        ui.horizontal(|ui| {
            for n in [5, 10] {
                if ui
                    .button(texts().format(language, "adjust", &[&n]))
                    .clicked()
                {
                    self.targets.adjust(n);
                }
            }
            if ui.button(t("from_log")).clicked() {
                // A modal OS dialog, as zLog's.
                let file = rfd::FileDialog::new()
                    .add_filter("zLog", &["zlo", "zlox", "ZLO", "ZLOX"])
                    .pick_file();
                if let Some((log, _)) = file.and_then(|f| Log::load(f).ok()) {
                    self.targets = count::targets_from(&log, period_hours);
                }
            }
            if ui.button(t("clear")).clicked() {
                self.targets = Hours::default();
            }
        });
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("rate_targets")
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new(t("band")).strong());
                    for h in 0..MAX_HOURS {
                        ui.label(RichText::new((h + 1).to_string()).strong());
                    }
                    ui.label(RichText::new(t("total")).strong());
                    ui.end_row();
                    for b in bands().filter(|b| !b.is_warc()) {
                        ui.monospace(b.label());
                        for h in 0..MAX_HOURS {
                            let mut v = self.targets.get(b, h);
                            if ui
                                .add(egui::DragValue::new(&mut v).range(0..=999))
                                .changed()
                            {
                                self.targets.set(b, h, v);
                            }
                        }
                        ui.monospace(self.targets.band_total(b).to_string());
                        ui.end_row();
                    }
                    ui.label(RichText::new(t("total")).strong());
                    for h in 0..MAX_HOURS {
                        ui.monospace(self.targets.hour_total(h).to_string());
                    }
                    ui.monospace(self.targets.total().to_string());
                    ui.end_row();
                });
        });
    }

    fn analysis(&mut self, ui: &mut Ui, language: &str) {
        let t = |k| texts().get(language, k);
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_cw, t("show_cw"));
            ui.checkbox(&mut self.exclude_zero_points, t("exclude_zero_points"));
            ui.checkbox(&mut self.exclude_zero_hours, t("exclude_zero_hours"));
            if ui.button(t("copy")).clicked() {
                ui.ctx().copy_text(self.derived.analysis.clone());
            }
            if ui.button(t("save")).clicked() {
                // zLog writes the text in Shift_JIS next to the log (.ZAF, .ZAQ);
                // here one UTF-8 file of both parts.
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("analysis.txt")
                    .save_file()
                {
                    let _ = std::fs::write(path, self.derived.analysis.replace('\n', "\r\n"));
                }
            }
        });
        egui::ScrollArea::both().show(ui, |ui| {
            tables::show(
                ui,
                &self.derived.chart,
                self.show_cw,
                self.exclude_zero_hours,
                language,
            )
        });
    }
}

impl Panel for Rate {
    /// Settings: graph options and the targets, one key per band of 48 hours
    /// (departure: zLog keeps the targets in its own file `zlog_target.txt`, one
    /// line per band led by the band's ordinal; see [`count::Hours::band_text`]).
    fn load(&mut self, section: &Section) {
        let get = |k: &str| section.get(k).map(String::as_str);
        self.position = match get("start") {
            Some("first") => StartPosition::FirstQso,
            Some("last") => StartPosition::LastQso,
            _ => StartPosition::CurrentTime,
        };
        self.style = match get("style") {
            Some("band") => GraphStyle::ByBand,
            Some("range") => GraphStyle::ByRange,
            _ => GraphStyle::Original,
        };
        self.show = get("hours")
            .and_then(|h| h.parse().ok())
            .filter(|h| SHOW_HOURS.contains(h))
            .unwrap_or(12);
        self.show_cw = get("show_cw") == Some("true");
        self.exclude_zero_points = get("exclude_zero_points") == Some("true");
        self.exclude_zero_hours = get("exclude_zero_hours") == Some("true");
        for b in bands() {
            if let Some(text) = get(&format!("target.{}", b.label())) {
                self.targets.set_band_text(b, text);
            }
        }
    }

    fn save(&self, section: &mut Section) {
        let start = match self.position {
            StartPosition::FirstQso => "first",
            StartPosition::CurrentTime => "current",
            StartPosition::LastQso => "last",
        };
        let style = match self.style {
            GraphStyle::Original => "total",
            GraphStyle::ByBand => "band",
            GraphStyle::ByRange => "range",
        };
        section.insert("start".into(), start.into());
        section.insert("style".into(), style.into());
        section.insert("hours".into(), self.show.to_string());
        section.insert("show_cw".into(), self.show_cw.to_string());
        let z = self.exclude_zero_points.to_string();
        section.insert("exclude_zero_points".into(), z);
        let z = self.exclude_zero_hours.to_string();
        section.insert("exclude_zero_hours".into(), z);
        for b in bands() {
            let key = format!("target.{}", b.label());
            if self.targets.band_total(b) > 0 {
                section.insert(key, self.targets.band_text(b));
            } else {
                section.remove(&key);
            }
        }
    }

    fn id(&self) -> &str {
        "rate"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Report
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Progress, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::CHART_LINE)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 420.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let now = count::now(view.log.utc_offset_minutes());
        let t = |k| texts().get(view.language, k);
        self.rates(ui, view, now);
        ui.horizontal(|ui| {
            for (tab, k) in [
                (Tab::Graph, "tab.graph"),
                (Tab::Table, "tab.table"),
                (Tab::Targets, "tab.targets"),
                (Tab::Analyze, "tab.analyze"),
            ] {
                ui.selectable_value(&mut self.tab, tab, t(k));
            }
        });
        ui.separator();
        match self.tab {
            Tab::Graph => self.graph_options(ui, view.language),
            Tab::Targets => self.targets(ui, view.language, view.period_hours),
            Tab::Analyze => self.analysis(ui, view.language),
            Tab::Table => {}
        }
        // Settings changed above take effect in the same frame.
        self.refresh(view, now);
        match self.tab {
            Tab::Graph => self.draw_graph(ui),
            Tab::Table => self.table(ui, view.language),
            _ => {}
        }
        // Rates move with the clock; redraw now and then.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let mut r = Rate {
            position: StartPosition::FirstQso,
            style: GraphStyle::ByRange,
            show: 24,
            ..Rate::default()
        };
        r.targets.set(Band::B7, 3, 42);
        let mut s = Section::new();
        r.save(&mut s);
        assert_eq!(s.get("target.7").unwrap().split(',').nth(3), Some("42"));
        let mut q = Rate::default();
        q.load(&s);
        assert_eq!(q.position, StartPosition::FirstQso);
        assert_eq!(q.style, GraphStyle::ByRange);
        assert_eq!(q.show, 24);
        assert_eq!(q.targets, r.targets);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
