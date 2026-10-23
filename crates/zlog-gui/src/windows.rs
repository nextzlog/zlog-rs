//! The start-time dialog.

use crate::dialog::{modal_with_buttons, ok_cancel};
use crate::i18n::{tr, trf};

use crate::main_window::View;
use crate::session::{Session, Status};

/// The start-time dialog (`TStartTimeDialog`, opened by `TMainForm.InputStartTime`):
/// OK sets the start time (judging every QSO when asked), Cancel keeps it.
pub fn start_time_dialog(ctx: &egui::Context, s: &mut Session, v: &mut View) {
    let Some(form) = v.start_time.as_mut() else {
        return;
    };
    let valid = form.parse().is_some();
    let new_log = form.new_log;
    let (_, done) = modal_with_buttons(
        ctx,
        "start_time",
        |ui| {
            ui.heading(tr("start.title"));
            let width = zlog_ui::form_field_width(ui, &[tr("start.date"), tr("start.time")]);
            egui::Grid::new("start_grid").num_columns(2).show(ui, |ui| {
                let err = (!valid).then_some(ui.visuals().error_fg_color);
                ui.label(tr("start.date"));
                let edit = egui::TextEdit::singleline(&mut form.date).text_color_opt(err);
                zlog_ui::sized_field(ui, edit, width);
                ui.end_row();
                ui.label(tr("start.time"));
                let edit = egui::TextEdit::singleline(&mut form.time).text_color_opt(err);
                zlog_ui::sized_field(ui, edit, width);
                ui.end_row();
            });
            if !new_log {
                ui.checkbox(&mut form.judge, tr("start.judge"));
            }
        },
        // Enter is not OK here: it ends typing the date or the time.
        |ui| {
            ok_cancel(ui, crate::i18n::current(), valid, false).or_else(|| {
                ui.input(|i| i.key_pressed(egui::Key::Escape))
                    .then_some(false)
            })
        },
    );
    match done {
        Some(true) => {
            if let Some(start) = form.parse() {
                let judge = form.judge && !form.new_log;
                let changed = s.set_start_time(start, judge);
                s.status = Status::info(trf("status.start_time_set", &[&changed]));
            }
            v.start_time = None;
        }
        Some(false) => {
            s.asks_start_time = false;
            v.start_time = None;
        }
        None => {}
    }
}
