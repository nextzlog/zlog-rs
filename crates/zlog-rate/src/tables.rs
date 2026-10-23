//! The analysis as tables: the result per band, the QSOs per hour and the time
//! chart, from the same counts as zLog's text (which Copy and Save still give).

use egui::{RichText, Ui};

use crate::analyze::{band_string, Count, TimeChart, ALL, SUM};
use crate::texts;

/// Draw the three tables.
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

/// QSOs, with the CW QSOs among them in brackets when asked.
fn qsos(ui: &mut Ui, c: Count, show_cw: bool) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if show_cw && c.cw > 0 {
            ui.label(format!("{} ({})", c.qso, c.cw));
        } else {
            ui.label(c.qso.to_string());
        }
    });
}

fn result(ui: &mut Ui, chart: &TimeChart, language: &str) {
    let t = |k| texts().get(language, k);
    let all = chart.data[SUM][ALL];
    let with2 = all.multi2 > 0;
    egui::Grid::new("rate_result").striped(true).show(ui, |ui| {
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
    egui::Grid::new("rate_per_hour")
        .striped(true)
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

/// Bands down, hours across, with the totals and the running total (zLog's ZAQ).
fn time_chart(ui: &mut Ui, chart: &TimeChart, language: &str) {
    let t = |k| texts().get(language, k);
    let last = chart.last_hour();
    egui::Grid::new("rate_time_chart")
        .striped(true)
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
                            ui.weak("-");
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
