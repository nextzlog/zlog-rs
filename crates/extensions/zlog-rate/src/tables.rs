use egui::{RichText, Ui};

use crate::analyze::{band_string, Count, TimeChart, ALL, SUM};
use crate::texts;

pub fn show(
    ui: &mut Ui,
    chart: &TimeChart,
    show_cw: bool,
    exclude_zero_hours: bool,
    language: &str,
) {
    let t = |k| texts().get(language, k);
    ui.strong(t("table.result"));
    result(ui, chart, language);
    ui.add_space(8.0);
    ui.strong(t(if show_cw {
        "table.per_hour_cw"
    } else {
        "table.per_hour"
    }));
    per_hour(ui, chart, show_cw, exclude_zero_hours, language);
    ui.add_space(8.0);
    ui.strong(t("table.chart"));
    time_chart(ui, chart, language);
}

fn head(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).strong());
}

fn number(ui: &mut Ui, n: u32) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(n.to_string());
    });
}

fn qsos(ui: &mut Ui, c: Count, show_cw: bool) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let text = if show_cw && c.cw > 0 {
            format!("{} ({})", c.qso, c.cw)
        } else {
            c.qso.to_string()
        };
        ui.label(text);
    });
}

fn result(ui: &mut Ui, chart: &TimeChart, language: &str) {
    let t = |k| texts().get(language, k);
    let all = chart.data[SUM][ALL];
    let with2 = all.multi2 > 0;
    let cols = if with2 { 5.0 } else { 4.0 };
    let gap = ui.spacing().item_spacing.x;
    let room = (ui.available_width() - gap * (cols - 1.0)) / cols;
    egui::Grid::new("rate_result")
        .striped(true)
        .min_col_width(room.max(48.0))
        .show(ui, |ui| {
            for k in ["band", "table.qsos", "table.points", "table.multi"] {
                head(ui, t(k));
            }
            if with2 {
                head(ui, t("table.multi2"));
            }
            ui.end_row();
            let row = |ui: &mut Ui, c: Count| {
                number(ui, c.qso);
                number(ui, c.points);
                number(ui, c.multi);
                if with2 {
                    number(ui, c.multi2);
                }
                ui.end_row();
            };
            for b in chart.worked() {
                ui.label(band_string(b));
                row(ui, chart.data[SUM][b as usize]);
            }
            head(ui, t("total"));
            row(ui, all);
        });
    let score = all.points * (all.multi + all.multi2);
    ui.label(texts().format(
        language,
        "table.score",
        &[&all.points, &(all.multi + all.multi2), &score],
    ));
}

fn per_hour(
    ui: &mut Ui,
    chart: &TimeChart,
    show_cw: bool,
    exclude_zero_hours: bool,
    language: &str,
) {
    let t = |k| texts().get(language, k);
    let cols = chart.worked().count() + 2;
    egui::Grid::new("rate_per_hour")
        .striped(true)
        .min_col_width(zlog_ui::layout::column_width(ui, cols, 24.0))
        .show(ui, |ui| {
            head(ui, t("table.hour"));
            for b in chart.worked() {
                head(ui, b.label());
            }
            head(ui, t("total"));
            ui.end_row();
            for h in 1..=chart.last_hour() {
                if exclude_zero_hours && chart.data[h][ALL].qso == 0 {
                    continue;
                }
                ui.label(chart.hour_text(h));
                for b in chart.worked() {
                    qsos(ui, chart.data[h][b as usize], show_cw);
                }
                qsos(ui, chart.data[h][ALL], show_cw);
                ui.end_row();
            }
            head(ui, t("total"));
            for b in chart.worked() {
                qsos(ui, chart.data[SUM][b as usize], show_cw);
            }
            qsos(ui, chart.data[SUM][ALL], show_cw);
            ui.end_row();
        });
}

fn time_chart(ui: &mut Ui, chart: &TimeChart, language: &str) {
    let t = |k| texts().get(language, k);
    let last = chart.last_hour();
    egui::Grid::new("rate_time_chart")
        .striped(true)
        .min_col_width(zlog_ui::layout::column_width(ui, last + 3, 16.0))
        .show(ui, |ui| {
            head(ui, t("band"));
            for h in 1..=last {
                head(ui, &chart.hour_text(h));
            }
            head(ui, t("total"));
            ui.end_row();
            for b in chart.worked() {
                ui.label(b.label());
                for h in 1..=last {
                    match chart.data[h][b as usize].qso {
                        0 => {
                            zlog_ui::layout::cell(ui, RichText::new("-").weak());
                        }
                        n => number(ui, n),
                    }
                }
                number(ui, chart.data[SUM][b as usize].qso);
                ui.end_row();
            }
            head(ui, t("total"));
            for h in 1..=last {
                number(ui, chart.data[h][ALL].qso);
            }
            number(ui, chart.data[SUM][ALL].qso);
            ui.end_row();
            head(ui, t("table.cumulative"));
            let mut cumulative = 0;
            for h in 1..=last {
                cumulative += chart.data[h][ALL].qso;
                number(ui, cumulative);
            }
            ui.end_row();
        });
}
