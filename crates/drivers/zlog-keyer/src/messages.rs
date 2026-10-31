use egui::Ui;
use zlog_ui::{EntryAction, EntryField, View};

use crate::settings::now;
use crate::{expand, texts, KeyerPanel};

impl KeyerPanel {
    pub(crate) fn bank_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        use zlog_ui::{icon, layout};
        layout::segmented(ui, &mut self.bank, &[(0, t("bank_a")), (1, t("bank_b"))]);
        let running = self.cq_running();
        if layout::toggle(ui, icon::MEGAPHONE, t("cq_repeat"), running).clicked() && !running {
            self.start_cq(view);
            self.hold = true;
        }
        if layout::tool(ui, icon::STOP, t("stop")).clicked() {
            self.stop();
        }
    }

    pub(crate) fn buttons_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        if !self.fkeys {
            return;
        }
        let messages: Vec<String> = (0..12).map(|n| self.expanded(self.bank, n, view)).collect();
        if let Some(n) = zlog_ui::fkeys::message_keys(ui, &messages) {
            self.play(self.bank, n as usize - 1, view);
        }
    }

    pub(crate) fn pad_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lines: Vec<String> = self
            .settings
            .pad
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(String::from)
            .collect();
        if lines.is_empty() {
            return;
        }
        zlog_ui::layout::grid(ui, lines.len(), 120.0, |ui, i| {
            let l = &lines[i];
            if ui
                .add(egui::Button::new(l).truncate())
                .on_hover_text(l)
                .clicked()
            {
                let (secs, hour) = now();
                let f = self.settings.fields(view, secs, hour);
                let text = expand(l, &f, &self.settings.abbrev);
                self.send_text(&text);
            }
        });
    }

    pub(crate) fn keyboard_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.add_space(zlog_ui::layout::GAP);
        let mut clear = false;
        let r = zlog_ui::layout::field(
            ui,
            egui::TextEdit::singleline(&mut self.keyboard)
                .hint_text(t("keyboard"))
                .event_filter(egui::EventFilter {
                    escape: true,
                    ..Default::default()
                }),
            |ui| clear = ui.button(t("clear")).clicked(),
        );
        {
            if std::mem::take(&mut self.focus_keyboard) {
                r.request_focus();
            }
            if r.has_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.keyboard.clear();
                self.keyboard_last.clear();
                if !self.escape() {
                    r.surrender_focus();
                    self.actions.push(EntryAction::Focus(EntryField::Call));
                }
            }
            if r.changed() {
                self.keyboard = self.keyboard.to_ascii_uppercase();
                let new = inserted(&self.keyboard_last, &self.keyboard);
                self.keyboard_last = self.keyboard.clone();
                if !new.is_empty() {
                    self.send_text(&new);
                }
            }
            if clear {
                self.keyboard.clear();
                self.keyboard_last.clear();
            }
        }
    }
}

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
        assert_eq!(inserted("CQ TEST", "CQ XTEST"), "X");
        assert_eq!(inserted("CQ TEST", "XCQ TEST"), "X");
        assert_eq!(inserted("CQ TEST", "CQ TST"), "");
        assert_eq!(inserted("CQ TEST", "CQ TEXT"), "X");
        assert_eq!(inserted("AAA", "AAAA"), "A");
        assert_eq!(inserted("ABC", ""), "");
    }
}
