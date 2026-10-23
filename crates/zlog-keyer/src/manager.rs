//! The message manager of the keyer: F1 to F12 of banks A and B (F1 is the CQ
//! message) edited side by side in one dialog, applied on OK, dropped on Cancel.

use egui::Context;

use crate::{texts, KeyerPanel};

/// A copy of both banks being edited; `None` while the dialog is closed.
#[derive(Debug, Default)]
pub(crate) struct Manager {
    draft: Option<[[String; 12]; 2]>,
}

impl Manager {
    /// Open the dialog on a copy of `banks`.
    pub(crate) fn open(&mut self, banks: &[[String; 12]; 2]) {
        self.draft = Some(banks.clone());
    }

    pub(crate) fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    /// Close the dialog, writing the copy into `banks` when `ok`.
    pub(crate) fn close(&mut self, ok: bool, banks: &mut [[String; 12]; 2]) {
        if let Some(d) = self.draft.take() {
            if ok {
                *banks = d;
            }
        }
    }
}

impl KeyerPanel {
    pub(crate) fn manager_ui(&mut self, ctx: &Context, lang: &str) {
        let t = |k| texts().get(lang, k);
        let Some(draft) = self.manager.draft.as_mut() else {
            return;
        };
        let mut open = true;
        let mut pressed = None;
        zlog_ui::dialog::dialog_with_buttons(
            ctx,
            "keyer_messages",
            t("manager.title"),
            [720.0, 420.0],
            &mut open,
            |ui| {
                egui::Grid::new("keyer-manager")
                    .num_columns(3)
                    .show(ui, |ui| {
                        let width = (zlog_ui::form_field_width(ui, &["F12"]) / 2.0)
                            .max(zlog_ui::FIELD_MIN / 2.0);
                        ui.label("");
                        ui.label(t("bank_a"));
                        ui.label(t("bank_b"));
                        ui.end_row();
                        for n in 0..12 {
                            ui.label(format!("F{}", n + 1));
                            for bank in draft.iter_mut() {
                                let edit = egui::TextEdit::singleline(&mut bank[n]);
                                zlog_ui::sized_field(ui, edit, width);
                            }
                            ui.end_row();
                        }
                    });
                ui.label(t("macro_help"));
            },
            |ui| pressed = zlog_ui::dialog::ok_cancel(ui, lang, true, false),
        );
        if !open {
            pressed = Some(false);
        }
        if let Some(ok) = pressed {
            self.manager.close(ok, &mut self.settings.banks);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    #[test]
    fn ok_applies_both_banks() {
        let mut banks = Settings::default().banks;
        let mut m = Manager::default();
        assert!(!m.is_open());
        m.open(&banks);
        assert!(m.is_open());
        let d = m.draft.as_mut().unwrap();
        d[0][0] = "CQ CQ $M".into();
        d[1][11] = "QRL?".into();
        // Editing the copy leaves the settings alone until OK.
        assert_eq!(banks[0][0], "CQ TEST $M TEST");
        m.close(true, &mut banks);
        assert!(!m.is_open());
        assert_eq!(banks[0][0], "CQ CQ $M");
        assert_eq!(banks[1][11], "QRL?");
    }

    #[test]
    fn cancel_drops_the_edits() {
        let mut banks = Settings::default().banks;
        let mut m = Manager::default();
        m.open(&banks);
        m.draft.as_mut().unwrap()[0][1] = "changed".into();
        m.close(false, &mut banks);
        assert_eq!(banks, Settings::default().banks);
        // Closing a closed dialog changes nothing.
        m.close(true, &mut banks);
        assert_eq!(banks, Settings::default().banks);
    }
}
