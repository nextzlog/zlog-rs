//! The QSO edit dialog and the delete confirmation.

use crate::dialog::dialog;
use crate::i18n::{tr, trf};
use zlog_core::{Band, Mode, Qso};

use crate::main_window::View;
use crate::session::Session;

/// QSO edit dialog (`TEditDialog`).
pub fn edit_dialog(ctx: &egui::Context, s: &mut Session, v: &mut View) {
    let Some((index, qso)) = v.editing.as_mut() else {
        return;
    };
    let (lo, hi) = s.rules().band_range();
    let mut result: Option<Option<Qso>> = None;
    // Closing the dialog from its title bar is the same as Cancel.
    let mut open = true;
    dialog(
        ctx,
        "edit",
        tr("edit.title"),
        [320.0, 320.0],
        &mut open,
        |ui| {
            egui::Grid::new("edit_grid").num_columns(2).show(ui, |ui| {
                ui.label(tr("column.time"));
                ui.label(qso.time.to_string());
                ui.end_row();
                ui.label(tr("column.call"));
                ui.text_edit_singleline(&mut qso.callsign);
                ui.end_row();
                ui.label(tr("edit.rst"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut qso.rst_sent));
                    ui.add(egui::DragValue::new(&mut qso.rst_rcvd));
                });
                ui.end_row();
                ui.label(tr("column.sent"));
                ui.text_edit_singleline(&mut qso.nr_sent);
                ui.end_row();
                ui.label(tr("column.rcvd"));
                ui.text_edit_singleline(&mut qso.nr_rcvd);
                ui.end_row();
                ui.label(tr("column.band"));
                egui::ComboBox::from_id_salt("edit_band")
                    .selected_text(qso.band.label())
                    .show_ui(ui, |ui| {
                        for b in Band::range(lo, hi) {
                            ui.selectable_value(&mut qso.band, b, b.label());
                        }
                    });
                ui.end_row();
                ui.label(tr("column.mode"));
                egui::ComboBox::from_id_salt("edit_mode")
                    .selected_text(qso.mode.label())
                    .show_ui(ui, |ui| {
                        for m in Mode::ALL {
                            ui.selectable_value(&mut qso.mode, *m, m.label());
                        }
                    });
                ui.end_row();
                ui.label(tr("column.memo"));
                ui.text_edit_singleline(&mut qso.memo);
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut qso.invalid, tr("edit.invalid"));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button(tr("button.ok")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    qso.callsign = qso.callsign.to_uppercase();
                    qso.nr_rcvd = qso.nr_rcvd.to_uppercase();
                    result = Some(Some(qso.clone()));
                }
                if ui.button(tr("button.cancel")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    result = Some(None);
                }
            });
        },
    );
    let index = *index;
    if !open {
        result = Some(None);
    }
    match result {
        Some(Some(q)) => {
            s.replace(index, q);
            v.editing = None;
        }
        Some(None) => v.editing = None,
        None => {}
    }
}

pub fn confirm_delete(ctx: &egui::Context, s: &mut Session, v: &mut View) {
    let Some(index) = v.confirm_delete else {
        return;
    };
    let call = s.log().qsos[index].callsign.clone();
    let response = egui::Modal::new(egui::Id::unique("confirm_delete")).show(ctx, |ui| {
        ui.label(trf("delete.confirm", &[&call]));
        ui.horizontal(|ui| {
            if ui.button(tr("button.ok")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                Some(true)
            } else if ui.button(tr("button.cancel")).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
                Some(false)
            } else {
                None
            }
        })
        .inner
    });
    match response.inner {
        Some(true) => {
            s.delete(index);
            v.confirm_delete = None;
            v.selected = if s.log().qsos.is_empty() {
                None
            } else {
                Some(index.min(s.log().qsos.len() - 1))
            };
        }
        Some(false) => v.confirm_delete = None,
        None => {}
    }
}
