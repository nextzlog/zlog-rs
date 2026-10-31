use egui::{RichText, Ui};
use zlog_core::Band;
use zlog_ui::{Panel, View};

zlog_core::texts!();

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
        .icon(zlog_ui::icon::TROPHY)
        .role("score")]
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
        let gap = ui.spacing().item_spacing.x;
        let room = (ui.available_width() - 3.0 * gap) / 4.0;
        let cell = |ui: &mut Ui, text: RichText| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(text)
            });
        };
        let num = |ui: &mut Ui, n: String| cell(ui, RichText::new(n).monospace());
        egui::Grid::new("score_grid")
            .striped(true)
            .min_col_width(room.max(56.0))
            .show(ui, |ui| {
                ui.label(RichText::new(heads[0]).strong());
                for h in &heads[1..4] {
                    cell(ui, RichText::new(*h).strong());
                }
                ui.end_row();
                for (band, row) in &sheet.bands {
                    ui.monospace(band.label());
                    num(ui, row.qsos.to_string());
                    num(ui, row.points.to_string());
                    num(ui, row.multi.to_string());
                    ui.end_row();
                }
                let t = sheet.total;
                ui.label(RichText::new(heads[4]).strong());
                num(ui, t.qsos.to_string());
                num(ui, t.points.to_string());
                num(ui, t.multi.to_string());
                ui.end_row();
                ui.label(RichText::new(heads[5]).strong());
                ui.label("");
                ui.label("");
                let score = if view.contest.score_unknown() {
                    "?".to_string()
                } else {
                    sheet.score.to_string()
                };
                cell(ui, RichText::new(score).monospace().strong());
                ui.end_row();
                if let Some((coeff, with_coeff)) = sheet.with_coeff {
                    ui.label(format!("x{coeff:.2}"));
                    ui.label("");
                    ui.label("");
                    num(
                        ui,
                        if view.contest.score_unknown() {
                            "?".to_string()
                        } else {
                            format!("{with_coeff:.0}")
                        },
                    );
                    ui.end_row();
                }
            });
    }
}

#[derive(Default)]
pub struct Multipliers {
    filter: String,
}

const CODE: f32 = 60.0;
const NAME: f32 = 140.0;
const MARK: f32 = 38.0;
const ROW: f32 = 18.0;

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
        .icon(zlog_ui::icon::GRID_FOUR)
        .role("multi")]
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
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(texts().get(view.language, "search"))
                .desired_width(f32::INFINITY),
        );
        let weak = zlog_ui::palette::weak(ui);
        let fill = ui.visuals().panel_fill;
        egui::ScrollArea::both()
            .auto_shrink(false)
            .show_viewport(ui, |ui, view_rect| {
                let gap = ui.spacing().item_spacing.x;
                let width = CODE + gap + NAME + bands.len() as f32 * (gap + MARK);
                let height = ROW * (rows.len() + 1) as f32;
                let (all, _) =
                    ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                let line = |ui: &mut Ui, y: f32, add: &mut dyn FnMut(&mut Ui)| {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(all.min.x, all.min.y + y),
                        egui::vec2(width, ROW),
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        add,
                    );
                };
                let first = ((view_rect.min.y / ROW).floor() as usize).saturating_sub(1);
                let last = ((view_rect.max.y / ROW).ceil() as usize).min(rows.len());
                for (i, (code, name, worked)) in rows.iter().enumerate().take(last).skip(first) {
                    let done = worked.contains(&view.band);
                    let paint = |t: RichText| if done { t.color(weak) } else { t };
                    line(ui, ROW * (i + 1) as f32, &mut |ui| {
                        cell(ui, CODE, paint(RichText::new(code.as_str()).monospace()));
                        cell(ui, NAME, paint(RichText::new(name.as_str())));
                        for b in &bands {
                            let mark = if worked.contains(b) { "*" } else { "." };
                            cell(ui, MARK, paint(RichText::new(mark).monospace()));
                        }
                    });
                }
                let top = view_rect.min.y.max(0.0);
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(all.min.x, all.min.y + top),
                        egui::vec2(width, ROW),
                    ),
                    0.0,
                    fill,
                );
                line(ui, top, &mut |ui| {
                    cell(ui, CODE, RichText::new(""));
                    cell(ui, NAME, RichText::new(""));
                    for b in &bands {
                        cell(ui, MARK, RichText::new(b.label()).monospace().strong());
                    }
                });
            });
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panels(130, |_| {
        vec![Box::new(Score), Box::new(Multipliers::default())]
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
