//! Search of the log (zLog's QSO Search, `USearch.pas` and
//! `TMainForm.QsoFindNext`): the QSOs whose callsign, received number or remarks
//! contain the text typed, in a list; Next and Previous step through them and
//! select the QSO in the log.
//!
//! Departures from zLog: the text is matched ignoring case, Previous matches the
//! same fields as Next (zLog's matches the callsign only), and every match is
//! listed at once.

use egui::{RichText, Ui};
use zlog_core::{Qso, QsoId};
use zlog_ui::{Command, EntryAction, Panel, View};

zlog_core::texts!();

/// The QSOs of `qsos` that `text` matches, by index; none for empty text.
pub fn matches(qsos: &[Qso], text: &str) -> Vec<usize> {
    let needle = text.trim().to_uppercase();
    if needle.is_empty() {
        return Vec::new();
    }
    qsos.iter()
        .enumerate()
        .filter(|(_, q)| {
            [&q.callsign, &q.nr_rcvd, &q.memo]
                .iter()
                .any(|f| f.to_uppercase().contains(&needle))
        })
        .map(|(i, _)| i)
        .collect()
}

/// The match after (`forward`) or before `current` among `found`, wrapping
/// around; the first (or last) when there is no current one.
pub fn step(found: &[usize], current: Option<usize>, forward: bool) -> Option<usize> {
    let (first, last) = (*found.first()?, *found.last()?);
    Some(match (current, forward) {
        (None, true) => first,
        (None, false) => last,
        (Some(c), true) => found.iter().copied().find(|&i| i > c).unwrap_or(first),
        (Some(c), false) => found.iter().rev().copied().find(|&i| i < c).unwrap_or(last),
    })
}

#[derive(Default)]
pub struct Search {
    text: String,
    /// The match shown, by index into the log.
    current: Option<usize>,
    actions: Vec<EntryAction>,
}

impl Search {
    fn go(&mut self, qsos: &[Qso], index: usize) {
        self.current = Some(index);
        if let Some(q) = qsos.get(index) {
            self.actions.push(EntryAction::SelectQso(QsoId::of(q)));
        }
    }
}

impl Panel for Search {
    fn id(&self) -> &str {
        "search"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 420.0]
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Find, 0)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        vec![Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::MAGNIFYING_GLASS)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = texts();
        let lang = view.language;
        let qsos = &view.log.qsos;
        let mut step_by = None;
        let edit = zlog_ui::field_row(
            ui,
            |ui| {
                ui.label(t.get(lang, "text"));
            },
            // Right to left: Previous, then Next at the end of the row.
            |ui| {
                if ui.button(t.get(lang, "next")).clicked() {
                    step_by = Some(true);
                }
                if ui.button(t.get(lang, "previous")).clicked() {
                    step_by = Some(false);
                }
            },
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .hint_text(t.get(lang, "hint"))
                        .desired_width(f32::INFINITY),
                )
            },
        );
        if edit.changed() {
            // zLog `QsoFindInit`: a new text starts from the top again.
            self.current = None;
        }
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            step_by = Some(!ui.input(|i| i.modifiers.shift));
            edit.request_focus();
        }
        let found = matches(qsos, &self.text);
        if let Some(forward) = step_by {
            match step(&found, self.current, forward) {
                Some(i) => self.go(qsos, i),
                None => self.current = None,
            }
        }
        ui.label(if self.text.trim().is_empty() {
            t.get(lang, "empty").to_string()
        } else {
            t.format(lang, "count", &[&found.len()])
        });
        ui.separator();
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for &i in &found {
                    let q = &qsos[i];
                    let (_, _, _, h, m, _) = q.time.to_ymd_hms();
                    let line = format!(
                        "{:>5}  {h:02}:{m:02}  {:<12} {:<10} {:>5} {:<4}  {}",
                        i + 1,
                        q.callsign,
                        q.nr_rcvd,
                        q.band.label(),
                        q.mode.label(),
                        q.memo
                    );
                    let on = self.current == Some(i);
                    let r = ui.selectable_label(on, RichText::new(line).monospace());
                    if on && step_by.is_some() {
                        r.scroll_to_me(Some(egui::Align::Center));
                    }
                    if r.clicked() {
                        chosen = Some(i);
                    }
                }
            });
        if let Some(i) = chosen {
            self.go(qsos, i);
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qso(call: &str, nr: &str, memo: &str) -> Qso {
        Qso {
            callsign: call.into(),
            nr_rcvd: nr.into(),
            memo: memo.into(),
            ..Qso::default()
        }
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn matches_callsign_number_and_remarks_ignoring_case() {
        let log = [
            qso("JA1ZLO", "10M", ""),
            qso("JR8PPG", "01H", "zlo friend"),
            qso("JG1VPP", "110M", ""),
        ];
        assert_eq!(matches(&log, "zlo"), [0, 1]);
        assert_eq!(matches(&log, "10"), [0, 2]);
        assert!(matches(&log, "  ").is_empty());
    }

    #[test]
    fn steps_wrap_around_both_ways() {
        let found = [2, 5, 9];
        assert_eq!(step(&found, None, true), Some(2));
        assert_eq!(step(&found, None, false), Some(9));
        assert_eq!(step(&found, Some(5), true), Some(9));
        assert_eq!(step(&found, Some(9), true), Some(2));
        assert_eq!(step(&found, Some(2), false), Some(9));
        assert_eq!(step(&[], None, true), None);
    }
}
