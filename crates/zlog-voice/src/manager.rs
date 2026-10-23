//! The message manager of the voice memories: the comments and WAV files of F1 to
//! F12 (F1 is the CQ memory) edited in one dialog, applied on OK, dropped on Cancel.

use egui::Context;

use crate::{texts, Settings, VoicePanel};

/// A copy of the comments and files being edited; `None` while the dialog is closed.
#[derive(Debug, Default)]
pub(crate) struct Manager {
    draft: Option<[[String; 12]; 2]>,
}

impl Manager {
    /// Open the dialog on a copy of the memories of `s`.
    pub(crate) fn open(&mut self, s: &Settings) {
        self.draft = Some([s.comments.clone(), s.files.clone()]);
    }

    pub(crate) fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    /// Close the dialog, writing the copy into `s` when `ok`.
    pub(crate) fn close(&mut self, ok: bool, s: &mut Settings) {
        if let Some([comments, files]) = self.draft.take() {
            if ok {
                s.comments = comments;
                s.files = files;
            }
        }
    }
}

impl VoicePanel {
    pub(crate) fn manager_ui(&mut self, ctx: &Context, lang: &str) {
        let t = |k| texts().get(lang, k);
        let Some(draft) = self.manager.draft.as_mut() else {
            return;
        };
        let mut open = true;
        let mut pressed = None;
        zlog_ui::dialog::dialog_with_buttons(
            ctx,
            "voice_messages",
            t("manager.title"),
            [720.0, 420.0],
            &mut open,
            |ui| {
                egui::Grid::new("voice-manager")
                    .num_columns(3)
                    .show(ui, |ui| {
                        let width = (zlog_ui::form_field_width(ui, &["F12"]) / 2.0)
                            .max(zlog_ui::FIELD_MIN / 2.0);
                        ui.label("");
                        ui.label(t("comment"));
                        ui.label(t("file"));
                        ui.end_row();
                        for n in 0..12 {
                            ui.label(format!("F{}", n + 1));
                            for column in draft.iter_mut() {
                                let edit = egui::TextEdit::singleline(&mut column[n]);
                                zlog_ui::sized_field(ui, edit, width);
                            }
                            ui.end_row();
                        }
                    });
            },
            |ui| pressed = zlog_ui::dialog::ok_cancel(ui, lang, true, false),
        );
        if !open {
            pressed = Some(false);
        }
        if let Some(ok) = pressed {
            self.manager.close(ok, &mut self.settings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_applies_comments_and_files() {
        let mut s = Settings::default();
        let mut m = Manager::default();
        assert!(!m.is_open());
        m.open(&s);
        assert!(m.is_open());
        let d = m.draft.as_mut().unwrap();
        d[0][0] = "CQ".into();
        d[1][0] = "/voice/cq.wav".into();
        // Editing the copy leaves the settings alone until OK.
        assert_eq!(s.file(0), None);
        m.close(true, &mut s);
        assert!(!m.is_open());
        assert_eq!(s.comments[0], "CQ");
        assert_eq!(s.file(0), Some(std::path::Path::new("/voice/cq.wav")));
    }

    #[test]
    fn cancel_drops_the_edits() {
        let mut s = Settings::default();
        let mut m = Manager::default();
        m.open(&s);
        m.draft.as_mut().unwrap()[1][11] = "f12.wav".into();
        m.close(false, &mut s);
        assert_eq!(s, Settings::default());
        // Closing a closed dialog changes nothing.
        m.close(true, &mut s);
        assert_eq!(s, Settings::default());
    }
}
