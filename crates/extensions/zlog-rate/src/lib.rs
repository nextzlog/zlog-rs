pub mod analyze;
pub mod count;
mod tables;

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::sync::{Arc, Mutex};
use zlog_core::{Band, Log};

use zlog_ui::{Panel, Section, View};

use count::{bands, Column, GraphStyle, Hours, StartPosition, MAX_HOURS};

zlog_core::texts!();

pub const SHOW_HOURS: [usize; 7] = [3, 6, 12, 18, 24, 36, 48];

#[derive(Debug, Clone, PartialEq)]
struct Key {
    qsos: usize,
    edits: i64,
    last: u64,
    now_hour: i64,
    position: StartPosition,
    style: GraphStyle,
    show: usize,
    targets: u64,
}

#[derive(Default)]
struct Derived {
    columns: Vec<Column>,
    actual: Hours,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnalysisOptions {
    pub show_cw: bool,
    pub exclude_zero_points: bool,
    pub exclude_zero_hours: bool,
}

pub type Shared = Arc<Mutex<AnalysisOptions>>;

const SPLIT: f32 = 0.6;

pub struct Rate {
    position: StartPosition,
    style: GraphStyle,
    show: usize,
    targets: Hours,
    draft: Option<Hours>,
    analysis: Shared,
    split: f32,
    max10: f64,
    max100: f64,
    key: Option<Key>,
    derived: Derived,
}

impl Default for Rate {
    fn default() -> Self {
        Rate::new(Shared::default())
    }
}

impl Rate {
    pub fn new(analysis: Shared) -> Self {
        Rate {
            position: StartPosition::CurrentTime,
            style: GraphStyle::Original,
            show: 12,
            targets: Hours::default(),
            draft: None,
            analysis,
            split: SPLIT,
            max10: 0.0,
            max100: 0.0,
            key: None,
            derived: Derived::default(),
        }
    }
}

fn band_color(b: Band) -> Color32 {
    let h = (b as usize as f32 * 0.13) % 1.0;
    egui::ecolor::Hsva::new(h, 0.55, 0.85, 1.0).into()
}

fn range_color(r: usize) -> Color32 {
    [Band::B7, Band::B21, Band::B50, Band::B430, Band::B5600]
        .map(band_color)
        .get(r)
        .copied()
        .unwrap_or(Color32::GRAY)
}

impl Rate {
    fn key(&self, log: &Log, now: f64) -> Key {
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
        }
    }

    fn refresh(&mut self, view: &View<'_>, now: f64) {
        let key = self.key(view.log, now);
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let qsos = &view.log.qsos;
        self.derived = Derived {
            columns: count::graph(qsos, &self.targets, now, self.position, self.show),
            actual: count::origin(qsos).map_or_else(Hours::default, |o| count::actual(qsos, o)),
        };
        self.key = Some(key);
    }

    fn summary(&self, view: &View<'_>, now: f64) -> String {
        let qsos = &view.log.qsos;
        let r = |n| {
            count::last_rate(qsos, view.station.tx_nr, n, now)
                .map_or_else(|| "-".to_string(), |r| format!("{r:.1}"))
        };
        let optime = count::hours_minutes(count::operating_minutes(qsos, now));
        texts().format(view.language, "summary", &[&r(10), &r(100), &optime])
    }

    fn rates(&mut self, view: &View<'_>, now: f64) -> [String; 3] {
        let t = |k| texts().get(view.language, k);
        let qsos = &view.log.qsos;
        let r10 = count::last_rate(qsos, view.station.tx_nr, 10, now);
        let r100 = count::last_rate(qsos, view.station.tx_nr, 100, now);
        self.max10 = self.max10.max(r10.unwrap_or(0.0));
        self.max100 = self.max100.max(r100.unwrap_or(0.0));
        let rate = |r: Option<f64>| match r {
            Some(r) => texts().format(view.language, "rate", &[&format!("{r:.2}")]),
            None => "-".into(),
        };
        let max = |m: f64| texts().format(view.language, "max", &[&format!("{m:.2}")]);
        let optime = count::hours_minutes(count::operating_minutes(qsos, now));
        [
            format!("{} {} ({})", t("last10"), rate(r10), max(self.max10)),
            format!("{} {} ({})", t("last100"), rate(r100), max(self.max100)),
            texts().format(view.language, "optime", &[&optime]),
        ]
    }

    fn graph_options(&mut self, ui: &mut Ui, language: &str) {
        use zlog_ui::layout;
        let t = |k| texts().get(language, k);
        let positions = [
            (StartPosition::FirstQso, "show.first"),
            (StartPosition::CurrentTime, "show.current"),
            (StartPosition::LastQso, "show.last"),
        ];
        let at = positions.iter().find(|(p, _)| *p == self.position);
        let range = texts().format(
            language,
            "range",
            &[&at.map_or("", |(_, k)| t(k)), &self.show.to_string()],
        );
        egui::ComboBox::from_id_salt("rate_hours")
            .selected_text(range)
            .show_ui(ui, |ui| {
                for (p, k) in positions {
                    ui.selectable_value(&mut self.position, p, t(k));
                }
                ui.separator();
                for h in SHOW_HOURS {
                    ui.selectable_value(&mut self.show, h, format!("{h} {}", t("hours")));
                }
            })
            .response
            .on_hover_text(t("range.tip"));
        layout::segmented(
            ui,
            &mut self.style,
            &[
                (GraphStyle::Original, t("style.original")),
                (GraphStyle::ByBand, t("style.band")),
                (GraphStyle::ByRange, t("style.range")),
            ],
        );
    }

    fn draw_graph(&self, ui: &mut Ui) {
        let columns = &self.derived.columns;
        let size = Vec2::new(ui.available_width(), ui.available_height().max(40.0));
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
            let show = (c.hour as usize).is_multiple_of(zlog_ui::layout::tick_step(width, 18.0));
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
        let cell_text = |a: u32, g: u32| match (a, g) {
            (0, 0) => String::new(),
            (a, 0) => a.to_string(),
            (a, g) => format!("{a}/{g}"),
        };
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            use zlog_ui::layout::{cell, column_width};
            let num = |ui: &mut Ui, s: String| cell(ui, RichText::new(s).monospace());
            let head = |ui: &mut Ui, s: String| cell(ui, RichText::new(s).strong());
            egui::Grid::new("rate_table")
                .striped(true)
                .min_col_width(column_width(ui, hours + 2, 28.0))
                .show(ui, |ui| {
                    ui.label(RichText::new(t("band")).strong());
                    for h in 0..hours {
                        head(ui, (h + 1).to_string());
                    }
                    head(ui, t("total").to_string());
                    ui.end_row();
                    for b in &rows {
                        ui.monospace(b.label());
                        for h in 0..hours {
                            num(ui, cell_text(actual.get(*b, h), self.targets.get(*b, h)));
                        }
                        num(
                            ui,
                            cell_text(actual.band_total(*b), self.targets.band_total(*b)),
                        );
                        ui.end_row();
                    }
                    ui.label(RichText::new(t("total")).strong());
                    for h in 0..hours {
                        num(
                            ui,
                            cell_text(actual.hour_total(h), self.targets.hour_total(h)),
                        );
                    }
                    num(ui, cell_text(actual.total(), self.targets.total()));
                    ui.end_row();
                });
        });
    }

    fn targets(targets: &mut Hours, ui: &mut Ui, language: &str, period_hours: u32) {
        let t = |k| texts().get(language, k);
        ui.horizontal_wrapped(|ui| {
            for n in [5, 10] {
                if ui
                    .button(texts().format(language, "adjust", &[&n]))
                    .clicked()
                {
                    targets.adjust(n);
                }
            }
            if ui.button(t("from_log")).clicked() {
                let file = zlog_ui::dialog::import_file("zLog", &["zlo", "zlox"]);
                if let Some((log, _)) = file.and_then(|f| Log::load(f).ok()) {
                    *targets = count::targets_from(&log, period_hours);
                }
            }
            if ui.button(t("clear")).clicked() {
                *targets = Hours::default();
            }
        });
        {
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
                            let mut v = targets.get(b, h);
                            if ui
                                .add(egui::DragValue::new(&mut v).range(0..=999))
                                .changed()
                            {
                                targets.set(b, h, v);
                            }
                        }
                        ui.monospace(targets.band_total(b).to_string());
                        ui.end_row();
                    }
                    ui.label(RichText::new(t("total")).strong());
                    for h in 0..MAX_HOURS {
                        ui.monospace(targets.hour_total(h).to_string());
                    }
                    ui.monospace(targets.total().to_string());
                    ui.end_row();
                });
        }
    }

    fn target_dialog(&mut self, ctx: &egui::Context, view: &View<'_>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let lang = view.language;
        let screen = ctx.content_rect().size();
        let size = egui::vec2(
            DIALOG[0].min(screen.x - 40.0).max(200.0),
            DIALOG[1].min(screen.y - 120.0).max(80.0),
        );
        let answer = egui::Modal::new(egui::Id::unique("rate_targets_dialog"))
            .show(ctx, |ui| {
                ui.set_width(size.x);
                ui.heading(texts().get(lang, "targets.title"));
                egui::ScrollArea::both()
                    .id_salt("rate_targets_body")
                    .auto_shrink(false)
                    .max_height(size.y)
                    .min_scrolled_height(size.y)
                    .show(ui, |ui| Self::targets(draft, ui, lang, view.period_hours));
                ui.separator();
                zlog_ui::dialog::ok_cancel(ui, lang, true, false)
            })
            .inner;
        match answer {
            Some(true) => self.targets = self.draft.take().unwrap_or_default(),
            Some(false) => self.draft = None,
            None => {}
        }
    }
}

pub const DIALOG: [f32; 2] = [560.0, 360.0];

impl Panel for Rate {
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
        *self.analysis.lock().unwrap() = AnalysisOptions {
            show_cw: get("show_cw") == Some("true"),
            exclude_zero_points: get("exclude_zero_points") == Some("true"),
            exclude_zero_hours: get("exclude_zero_hours") == Some("true"),
        };
        self.split = zlog_ui::layout::load_ratio(section, "split", SPLIT);
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
        let a = *self.analysis.lock().unwrap();
        section.insert("show_cw".into(), a.show_cw.to_string());
        let z = a.exclude_zero_points.to_string();
        section.insert("exclude_zero_points".into(), z);
        let z = a.exclude_zero_hours.to_string();
        section.insert("exclude_zero_hours".into(), z);
        zlog_ui::layout::save_ratio(section, "split", self.split);
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
        vec![
            zlog_ui::Command::window(
                texts().get(language, "toolbar"),
                texts().get(language, "toolbar.tip"),
            )
            .icon(zlog_ui::icon::CHART_LINE),
            zlog_ui::Command::new(
                "targets",
                texts().get(language, "targets.title"),
                texts().get(language, "targets.tip"),
            )
            .icon(zlog_ui::icon::FLAG)
            .dialog(),
        ]
    }

    fn run_command(&mut self, id: &str) {
        if id == "targets" {
            self.draft = Some(self.targets.clone());
        }
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 420.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let now = count::now(view.log.utc_offset_minutes());
        let rates = self.rates(view, now);
        zlog_ui::layout::toolbar(ui, &[], |ui| self.graph_options(ui, view.language));
        zlog_ui::layout::line(ui, &self.summary(view, now), &rates.join("\n"));
        self.refresh(view, now);
        let this = &*self;
        let mut split = self.split;
        zlog_ui::layout::split(
            ui,
            &mut split,
            |ui| this.draw_graph(ui),
            |ui| this.table(ui, view.language),
        );
        self.split = split;
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(5));
    }

    fn dialogs(&mut self, ctx: &egui::Context, view: &View<'_>) {
        self.target_dialog(ctx, view);
    }

    fn modal(&self) -> bool {
        self.draft.is_some()
    }
}

pub struct Analysis {
    options: Shared,
    key: Option<(usize, i64, u64, AnalysisOptions, String)>,
    chart: analyze::TimeChart,
    text: String,
}

impl Analysis {
    pub fn new(options: Shared) -> Self {
        Analysis {
            options,
            key: None,
            chart: analyze::TimeChart::default(),
            text: String::new(),
        }
    }

    fn refresh(&mut self, view: &View<'_>, o: AnalysisOptions) {
        let qsos = &view.log.qsos;
        let key = (
            qsos.len(),
            qsos.iter().map(|q| q.reserve3 as i64).sum(),
            qsos.last().map_or(0, |q| q.time.0.to_bits()),
            o,
            view.language.to_string(),
        );
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.chart = analyze::TimeChart::new(qsos, o.exclude_zero_points);
        let mut lines = analyze::zaf(&self.chart, o.show_cw, o.exclude_zero_hours, view.language);
        lines.extend(analyze::zaq(&self.chart, view.language));
        self.text = lines.join("\n");
        self.key = Some(key);
    }
}

impl Panel for Analysis {
    fn id(&self) -> &str {
        "rate-analysis"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Report
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Progress, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "analysis.title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "analysis.toolbar"),
            texts().get(language, "analysis.tip"),
        )
        .icon(zlog_ui::icon::FILE_TEXT)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 420.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let t = |k| texts().get(lang, k);
        let mut o = *self.options.lock().unwrap();
        zlog_ui::layout::toolbar(ui, &[], |ui| {
            use zlog_ui::{icon, layout};
            for (on, k) in [
                (&mut o.show_cw, "show_cw"),
                (&mut o.exclude_zero_points, "exclude_zero_points"),
                (&mut o.exclude_zero_hours, "exclude_zero_hours"),
            ] {
                let tip = texts().get(lang, &format!("{k}.tip")).to_string();
                if layout::toggle(ui, "", t(k), *on)
                    .on_hover_text(tip)
                    .clicked()
                {
                    *on = !*on;
                }
            }
            ui.separator();
            if layout::tool(ui, icon::STACK, t("copy")).clicked() {
                ui.ctx().copy_text(self.text.clone());
            }
            if layout::tool(ui, icon::FLOPPY_DISK, t("save")).clicked() {
                // zLog writes the text in Shift_JIS next to the log (.ZAF, .ZAQ);
                // here one UTF-8 file of both parts.
                if let Some(path) = zlog_ui::dialog::save_file_as("analysis.txt") {
                    let _ = std::fs::write(path, self.text.replace('\n', "\r\n"));
                }
            }
        });
        *self.options.lock().unwrap() = o;
        self.refresh(view, o);
        zlog_ui::layout::rest(ui, "rate_analysis", |ui| {
            tables::show(ui, &self.chart, o.show_cw, o.exclude_zero_hours, lang)
        });
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panels(540, |_| {
        let shared = Shared::default();
        vec![
            Box::new(Rate::new(shared.clone())) as Box<dyn Panel>,
            Box::new(Analysis::new(shared)),
        ]
    });
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
