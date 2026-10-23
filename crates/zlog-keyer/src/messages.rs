//! The message buttons, the message editor, the message pad and the CW keyboard of
//! the keyer panel.

use egui::Ui;
use zlog_ui::View;

use crate::settings::now;
use crate::{expand, texts, KeyerPanel};

impl KeyerPanel {
    pub(crate) fn buttons_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let t = |k| texts().get(lang, k);
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.bank, 0, t("bank_a"));
            ui.radio_value(&mut self.bank, 1, t("bank_b"));
            let running = self.cq_running();
            if ui.selectable_label(running, t("cq_repeat")).clicked() {
                if running {
                    self.stop();
                } else {
                    self.start_cq(view);
                }
            }
            ui.add(
                egui::DragValue::new(&mut self.settings.cq_repeat)
                    .range(0.0..=60.0)
                    .speed(0.1)
                    .suffix(" s"),
            );
            ui.label(t("cq_max"));
            ui.add(egui::DragValue::new(&mut self.settings.cq_max).range(1..=999));
            if ui.button(t("stop")).clicked() {
                self.stop();
            }
            if ui.button(t("manager")).clicked() && !self.manager.is_open() {
                self.manager.open(&self.settings.banks);
            }
        });
        ui.horizontal_wrapped(|ui| {
            for n in 0..12 {
                let preview = self.expanded(self.bank, n, view);
                let b = ui.button(format!("F{}", n + 1)).on_hover_text(preview);
                if b.clicked() {
                    self.play(self.bank, n, view);
                }
            }
        });
    }

    pub(crate) fn editor_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        egui::CollapsingHeader::new(t("messages")).show(ui, |ui| {
            egui::Grid::new("keyer-messages")
                .num_columns(2)
                .show(ui, |ui| {
                    for n in 0..12 {
                        ui.label(format!("F{}", n + 1));
                        let m = &mut self.settings.banks[self.bank][n];
                        ui.add(egui::TextEdit::singleline(m).desired_width(f32::INFINITY));
                        ui.end_row();
                    }
                });
            ui.label(t("macro_help"));
            ui.horizontal(|ui| {
                ui.label(t("abbrev"));
                for c in [
                    &mut self.settings.abbrev.zero,
                    &mut self.settings.abbrev.one,
                    &mut self.settings.abbrev.nine,
                ] {
                    let mut s = c.to_string();
                    if ui
                        .add(egui::TextEdit::singleline(&mut s).desired_width(16.0))
                        .changed()
                    {
                        if let Some(n) = s.chars().last() {
                            *c = n.to_ascii_uppercase();
                        }
                    }
                }
                ui.checkbox(
                    &mut self.settings.not_send_leading_zeros,
                    t("no_leading_zeros"),
                );
            });
        });
    }

    pub(crate) fn pad_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        egui::CollapsingHeader::new(t("pad")).show(ui, |ui| {
            let lines: Vec<String> = self
                .settings
                .pad
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(String::from)
                .collect();
            ui.horizontal_wrapped(|ui| {
                for l in lines {
                    if ui.button(&l).clicked() {
                        let (secs, hour) = now();
                        let f = self.settings.fields(view, secs, hour);
                        let text = expand(&l, &f, &self.settings.abbrev);
                        self.send_text(&text);
                    }
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.settings.pad)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
        });
    }

    /// Type to send (`TCWKeyBoard.ConsoleKeyPress`): each new character is sent as
    /// it is typed, also when typed in the middle of the text; deleting cannot unsend.
    pub(crate) fn keyboard_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.label(t("keyboard"));
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.keyboard)
                    .desired_width(ui.available_width() - 60.0),
            );
            if r.changed() {
                self.keyboard = self.keyboard.to_uppercase();
                let new = inserted(&self.keyboard_last, &self.keyboard);
                self.keyboard_last = self.keyboard.clone();
                if !new.is_empty() {
                    self.send_text(&new);
                }
            }
            if ui.button(t("clear")).clicked() {
                self.keyboard.clear();
                self.keyboard_last.clear();
            }
        });
    }
}

/// What was typed into `old` to make `new`: `new` without the longest common prefix
/// and suffix of the two (a paste or a replaced selection gives the new text only).
pub(crate) fn inserted(old: &str, new: &str) -> String {
    let (o, n): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let room = o.len().min(n.len()) - prefix;
    let suffix = o
        .iter()
        .rev()
        .zip(n.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    n[prefix..n.len() - suffix].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_sends_only_inserted_characters() {
        assert_eq!(inserted("", "CQ"), "CQ");
        assert_eq!(inserted("CQ", "CQ T"), " T");
        // Typed in the middle: only the new character, not the rest of the line.
        assert_eq!(inserted("CQ TEST", "CQ XTEST"), "X");
        assert_eq!(inserted("CQ TEST", "XCQ TEST"), "X");
        assert_eq!(inserted("CQ TEST", "CQ TST"), "");
        assert_eq!(inserted("CQ TEST", "CQ TEXT"), "X");
        assert_eq!(inserted("AAA", "AAAA"), "A");
        assert_eq!(inserted("ABC", ""), "");
    }
}
