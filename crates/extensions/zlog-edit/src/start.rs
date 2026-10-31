use zlog_ui::dialog::{modal_with_buttons, ok_cancel};
use zlog_ui::time::StartTimeForm;

use crate::texts;

pub use zlog_ui::time::StartTimeDone as Done;

pub fn dialog(ctx: &egui::Context, form: &mut StartTimeForm, lang: &str) -> Option<Done> {
    let tr = |key: &'static str| texts().get(lang, key);
    let valid = form.parse().is_some();
    let new_log = form.new_log;
    let (_, done) = modal_with_buttons(
        ctx,
        "start_time",
        |ui| {
            ui.heading(tr("start.title"));
            let labels = [tr("start.date"), tr("start.time")];
            zlog_ui::form(ui, "start_grid", &labels, |ui, fields| {
                let err = (!valid).then_some(ui.visuals().error_fg_color);
                ui.label(tr("start.date"));
                let edit = egui::TextEdit::singleline(&mut form.date).text_color_opt(err);
                fields.text(ui, edit);
                ui.end_row();
                ui.label(tr("start.time"));
                let edit = egui::TextEdit::singleline(&mut form.time).text_color_opt(err);
                fields.text(ui, edit);
                ui.end_row();
            });
            if !new_log {
                ui.checkbox(&mut form.judge, tr("start.judge"));
            }
        },
        |ui| {
            ok_cancel(ui, lang, valid, false).or_else(|| {
                ui.input(|i| i.key_pressed(egui::Key::Escape))
                    .then_some(false)
            })
        },
    );
    match done? {
        true => form.parse().map(|start| Done::Set {
            start,
            judge: form.judge && !form.new_log,
        }),
        false => Some(Done::Cancelled),
    }
}
