use std::path::{Path, PathBuf};

use egui::Ui;

use crate::{audio, texts, VoicePanel};

impl VoicePanel {
    pub(crate) fn toggle_record(&mut self, n: usize) {
        if let Some((m, path, mut got)) = self.recording.take() {
            let r = self.audio().and_then(|io| {
                io.record(&mut got);
                audio::save_recording(&path, &got, io.sample_rate())
            });
            if r.is_ok() {
                self.settings.files[m] = path.display().to_string();
            }
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
        match self.audio().map(|io| io.record(&mut Vec::new())) {
            Ok(()) => {
                self.recording = Some((n, path, Vec::new()));
                self.status.clear();
            }
            Err(e) => self.status = e,
        }
    }

    pub(crate) fn memories_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        let current = self.voice.current();
        let recording = self.recording.as_ref().map(|(n, _, _)| *n);
        for n in 0..12 {
            ui.horizontal(|ui| {
                let key = format!("F{}", n + 1);
                let size = egui::vec2(zlog_ui::layout::KEY_WIDTH, ui.spacing().interact_size.y);
                if ui
                    .add_sized(size, egui::Button::selectable(current == Some(n), &key))
                    .clicked()
                {
                    self.play(n);
                }
                let comment = self.settings.comments[n].trim();
                let shown = match (comment, self.settings.file(n)) {
                    ("", Some(f)) => f.file_name().map(|f| f.to_string_lossy().to_string()),
                    ("", None) => None,
                    (c, _) => Some(c.to_string()),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if recording == Some(n) {
                        t("stop_recording")
                    } else {
                        t("record")
                    };
                    let free = recording.is_none() || recording == Some(n);
                    let glyph =
                        egui::RichText::new(zlog_ui::icon::RECORD).family(zlog_ui::icon::family());
                    if ui
                        .add_enabled(free, egui::Button::new((glyph, label)))
                        .clicked()
                    {
                        self.toggle_record(n);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(shown.unwrap_or_default()).truncate());
                    });
                });
            });
        }
    }
}

pub fn new_memory_path(n: usize) -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("voice")
        .join(format!("F{}.wav", n + 1))
}
