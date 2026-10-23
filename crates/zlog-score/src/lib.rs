//! The score window and the multiplier window.
//!
//! Both only draw: the numbers come from `Contest::renew` (the score sheet) and the
//! table from `Contest::multi_table`. Nothing is counted here, unlike zLog, where
//! these windows did the scoring themselves.

use egui::{RichText, Ui};
use zlog_core::Band;
use zlog_ui::{Panel, View};

zlog_core::texts!();

/// Score window (`TALLJAScore`): QSOs, points and multipliers per band.
#[derive(Default)]
pub struct Score;

impl Panel for Score {
    fn id(&self) -> &str {
        "score"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "score.title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [300.0, 260.0]
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Report
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Score, 0)
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "score.title"),
            texts().get(language, "score.tooltip"),
        )
        .icon(zlog_ui::icon::TROPHY)]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let heads = [
            "head.mhz",
            "head.qsos",
            "head.points",
            "head.multi",
            "head.total",
            "head.score",
        ]
        .map(|k| texts().get(view.language, k));
        let sheet = view.sheet;
        egui::Grid::new("score_grid")
            .striped(true)
            .min_col_width(56.0)
            .show(ui, |ui| {
                for h in &heads[..4] {
                    ui.label(RichText::new(*h).strong());
                }
                ui.end_row();
                for (band, row) in &sheet.bands {
                    ui.monospace(band.label());
                    ui.monospace(row.qsos.to_string());
                    ui.monospace(row.points.to_string());
                    ui.monospace(row.multi.to_string());
                    ui.end_row();
                }
                let t = sheet.total;
                ui.label(RichText::new(heads[4]).strong());
                ui.monospace(t.qsos.to_string());
                ui.monospace(t.points.to_string());
                ui.monospace(t.multi.to_string());
                ui.end_row();
                ui.label(RichText::new(heads[5]).strong());
                ui.label("");
                ui.label("");
                ui.monospace(RichText::new(sheet.score.to_string()).strong());
                ui.end_row();
                if let Some((coeff, with_coeff)) = sheet.with_coeff {
                    ui.label(format!("x{coeff:.2}"));
                    ui.label("");
                    ui.label("");
                    ui.monospace(format!("{with_coeff:.0}"));
                    ui.end_row();
                }
            });
    }
}

/// Multiplier window: every multiplier with a mark for each band worked.
#[derive(Default)]
pub struct Multipliers {
    filter: String,
}

const CODE: f32 = 60.0;
const NAME: f32 = 140.0;
const MARK: f32 = 38.0;

fn cell(ui: &mut Ui, width: f32, text: RichText) {
    ui.add_sized([width, 16.0], egui::Label::new(text).truncate());
}

impl Panel for Multipliers {
    fn id(&self) -> &str {
        "multipliers"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "multi.title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [520.0, 400.0]
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Report
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Score, 1)
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "multi.title"),
            texts().get(language, "multi.tooltip"),
        )
        .icon(zlog_ui::icon::GRID_FOUR)]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let (lo, hi) = view.contest.band_range();
        let bands: Vec<Band> = Band::range(lo, hi)
            .filter(|b| (view.contest.uses_warc() || !b.is_warc()) && *b != Band::B10_4G)
            .collect();
        let table = view.contest.multi_table();
        let rows: Vec<_> = table
            .iter()
            .filter(|(code, name, _)| {
                self.filter.is_empty()
                    || code.starts_with(&self.filter)
                    || name.contains(&self.filter)
            })
            .collect();
        ui.horizontal(|ui| {
            ui.label(texts().get(view.language, "search"));
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            cell(ui, CODE + NAME, RichText::new(""));
            for b in &bands {
                cell(ui, MARK, RichText::new(b.label()).monospace().strong());
            }
        });
        let weak = zlog_ui::palette::weak(ui);
        egui::ScrollArea::both().show_rows(ui, 18.0, rows.len(), |ui, range| {
            for (code, name, worked) in &rows[range] {
                // Grey rows are already worked on the band of the entry row, as in zLog.
                let done = worked.contains(&view.band);
                let paint = |t: RichText| if done { t.color(weak) } else { t };
                ui.horizontal(|ui| {
                    cell(ui, CODE, paint(RichText::new(code).monospace()));
                    cell(ui, NAME, paint(RichText::new(name)));
                    for b in &bands {
                        let mark = if worked.contains(b) { "*" } else { "." };
                        cell(ui, MARK, paint(RichText::new(mark).monospace()));
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
