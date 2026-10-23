//! The memories of the voice panel: the memory grid, recording a memory and where a
//! new recording is saved.

use std::path::{Path, PathBuf};

use egui::Ui;

use crate::{audio, texts, VoicePanel};

impl VoicePanel {
    /// Start recording memory `n` into its file, or into [`new_memory_path`] when it
    /// has none; stop the recording running.
    pub(crate) fn toggle_record(&mut self, n: usize) {
        if let Some((m, rec)) = self.recording.take() {
            let r = rec.finish().map(|p| {
                self.settings.files[m] = p.display().to_string();
            });
            self.report(r);
            return;
        }
        self.voice.abort();
        let path = self
            .settings
            .file(n)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| new_memory_path(n));
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match audio::Recording::start(&self.settings.input, path) {
            Ok(rec) => {
                self.recording = Some((n, rec));
                self.status.clear();
            }
            Err(e) => self.status = e,
        }
    }

    pub(crate) fn memories_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        let current = self.voice.current();
        let recording = self.recording.as_ref().map(|(n, _)| *n);
        egui::Grid::new("voice-memories")
            .num_columns(5)
            .show(ui, |ui| {
                ui.label("");
                ui.label(t("comment"));
                ui.label(t("file"));
                ui.end_row();
                let buttons = t("browse").len().max(t("record").len()) as f32 * 16.0;
                let room = zlog_ui::form_field_width(ui, &["F12"]) - buttons;
                let width = (room / 2.0).max(zlog_ui::FIELD_MIN / 2.0);
                for n in 0..12 {
                    let key = format!("F{}", n + 1);
                    if ui.selectable_label(current == Some(n), &key).clicked() {
                        self.play(n);
                    }
                    let comment = egui::TextEdit::singleline(&mut self.settings.comments[n]);
                    zlog_ui::sized_field(ui, comment, width);
                    let file = egui::TextEdit::singleline(&mut self.settings.files[n]);
                    zlog_ui::sized_field(ui, file, width);
                    if ui.button(t("browse")).clicked() {
                        let picked = rfd::FileDialog::new()
                            .add_filter("WAV", &["wav"])
                            .pick_file();
                        if let Some(p) = picked {
                            self.settings.files[n] = p.display().to_string();
                        }
                    }
                    let label = if recording == Some(n) {
                        t("stop_recording")
                    } else {
                        t("record")
                    };
                    if ui
                        .add_enabled(
                            recording.is_none() || recording == Some(n),
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        self.toggle_record(n);
                    }
                    ui.end_row();
                }
            });
    }
}

/// Where a memory recorded without a file of its own is saved: `F<n>.wav` in
/// `zlog/voice` ([`zlog_ui::APP_DIR`]) of the documents folder (the working directory of an app is
/// not a place the operator knows).
pub fn new_memory_path(n: usize) -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("voice")
        .join(format!("F{}.wav", n + 1))
}
