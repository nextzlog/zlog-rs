use egui::RichText;
use zlog_ui::chrome::{StatusBar, StatusLine};
use zlog_ui::StatusItem;

#[derive(Default)]
pub struct StatusRow;

fn item(ui: &mut egui::Ui, item: &StatusItem) {
    let text = RichText::new(&item.text);
    let label = ui.label(if item.warning {
        text.color(zlog_ui::palette::error(ui))
    } else {
        text
    });
    if let Some(tip) = &item.tooltip {
        label.on_hover_text(tip);
    }
}

impl StatusBar for StatusRow {
    fn show(&mut self, ui: &mut egui::Ui, line: &StatusLine) {
        ui.horizontal(|ui| {
            item(ui, &line.message);
            for it in &line.items {
                ui.separator();
                item(ui, it);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(&line.count);
                ui.separator();
                let colour = if line.cq {
                    zlog_ui::palette::cq(ui)
                } else {
                    zlog_ui::palette::sp(ui)
                };
                ui.label(RichText::new(&line.cq_text).color(colour));
                ui.separator();
                ui.label(&line.contest);
            });
        });
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.status_bar(|| StatusRow);
}
