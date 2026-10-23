//! The entry row at the bottom of the main window (`TMainForm`'s edit boxes).
//!
//! Keys follow the zLog manual:
//! Space moves from the callsign to the number (checking dupes and filling the
//! number from earlier QSOs) and back from other fields, Enter logs,
//! Shift+B / Shift+M / Shift+P change band, mode and power, Shift+O toggles CQ and S&P,
//! Shift+X selects the other rig,
//! Alt+C / Alt+N / Alt+M / Alt+R jump to callsign, number, memo and RST,
//! Shift+Up / Shift+Down change the S of an RST, Ctrl+K clears callsign and number,
//! Up moves into the log.

use egui::{Event, Id, Key, Modifiers, RichText};
use zlog_ui::{EntryField, EntryState, EntryStyle, NotLogged};

use crate::texts;

/// Height of the row, as the rows of the log grid.
const ROW_HEIGHT: f32 = 18.0;

/// The fields of the row, left to right.
const FIELDS: [EntryField; 5] = [
    EntryField::Call,
    EntryField::RstSent,
    EntryField::RstRcvd,
    EntryField::Number,
    EntryField::Memo,
];

/// The widget of a field.
fn id(field: EntryField) -> Id {
    Id::unique(("entry", field as u8))
}

/// zLog's entry row; keeps only where the keyboard goes.
#[derive(Default)]
pub struct ZlogRow {
    /// Give the keyboard to this field when next drawn.
    focus: Option<EntryField>,
    /// The field that last had the keyboard, given it back after a click on the
    /// band, mode or power box (zLog `LastFocus.SetFocus`).
    last_field: Option<EntryField>,
    /// Select the whole text of this field when it is next drawn (after an
    /// invalid number).
    select_all: Option<EntryField>,
}

/// Consume a shortcut and the text it would otherwise type into a field.
fn shortcut(ctx: &egui::Context, modifiers: Modifiers, key: Key) -> bool {
    ctx.input_mut(|i| {
        let hit = i.consume_key(modifiers, key);
        if hit {
            i.events.retain(|e| !matches!(e, Event::Text(_)));
        }
        hit
    })
}

/// Whether Shift+B / M / P / O act in a field: not in the memo, where they type
/// capitals (zLog turns `EnableShiftKeyAction` off while the memo has focus).
pub fn shift_keys_act(field: Option<EntryField>) -> bool {
    field != Some(EntryField::Memo)
}

/// The field the keyboard returns to after a click on the band, mode or power box
/// (`LastFocus.SetFocus`; the callsign before any field had it).
pub fn refocus_after_click(last: Option<EntryField>) -> EntryField {
    last.unwrap_or(EntryField::Call)
}

impl EntryStyle for ZlogRow {
    fn id(&self) -> &str {
        "zlog"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn keys(&mut self, ctx: &egui::Context, entry: &mut dyn EntryState) {
        let field = self.focused(ctx);
        if field.is_some() {
            self.last_field = field;
        }
        let shift = shift_keys_act(field);
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::B) {
            entry.cycle_band(false);
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::M) {
            entry.cycle_mode(false);
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::P) {
            entry.cycle_power();
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::O) {
            entry.toggle_cq();
        }
        // ShortCut 8280 of `actionToggleRig`.
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::X) {
            entry.toggle_rig();
        }
        for (key, up) in [(Key::ArrowUp, true), (Key::ArrowDown, false)] {
            match field {
                Some(f @ (EntryField::RstSent | EntryField::RstRcvd))
                    if shortcut(ctx, Modifiers::SHIFT, key) =>
                {
                    entry.bump_s(f, up)
                }
                _ => {}
            }
        }
        for (key, target) in [
            (Key::C, EntryField::Call),
            (Key::N, EntryField::Number),
            (Key::M, EntryField::Memo),
            (Key::R, EntryField::RstRcvd),
        ] {
            if shortcut(ctx, Modifiers::ALT, key) {
                self.focus = Some(target);
            }
        }
        if shortcut(ctx, Modifiers::COMMAND, Key::K) {
            entry.clear();
            self.focus = Some(EntryField::Call);
        }
        let space = |ctx: &egui::Context| shortcut(ctx, Modifiers::NONE, Key::Space);
        match field {
            // As zLog with dupes allowed: a dupe is reported, and the cursor still moves on.
            Some(EntryField::Call) if space(ctx) => {
                entry.complete_call();
                self.focus = Some(EntryField::Number);
            }
            Some(EntryField::Memo) | None => {}
            Some(_) if space(ctx) => self.focus = Some(EntryField::Call),
            Some(_) => {}
        }
        if field.is_some() && shortcut(ctx, Modifiers::NONE, Key::Enter) {
            match entry.log() {
                Ok(()) | Err(NotLogged::NoCallsign) => self.focus = Some(EntryField::Call),
                Err(NotLogged::InvalidNumber) => {
                    self.focus = Some(EntryField::Number);
                    self.select_all = Some(EntryField::Number);
                }
                Err(NotLogged::Refused) => self.focus = field,
            }
        }
        if let Some(f) = field {
            if entry.qso_count() > 0 && shortcut(ctx, Modifiers::NONE, Key::ArrowUp) {
                entry.enter_log();
                ctx.memory_mut(|m| m.surrender_focus(id(f)));
            }
        }
    }

    /// The fields lined up with the columns of the log grid.
    fn ui(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState) {
        let widths = entry.columns().to_vec();
        let width = |col: usize| widths.get(col).copied().unwrap_or(60.0);
        let (_, mo, d, h, mi, _) = entry.now().to_ymd_hms();
        let sent = entry.sent();
        let operator = entry.operator().to_string();
        ui.horizontal(|ui| {
            fixed_label(ui, width(0), "");
            fixed_label(ui, width(1), &format!("{mo:02}/{d:02}"));
            fixed_label(ui, width(2), &format!("{h:02}:{mi:02}"));
            self.edit(ui, width(3), entry, EntryField::Call);
            self.edit(ui, width(4), entry, EntryField::RstSent);
            fixed_label(ui, width(5), &sent);
            self.edit(ui, width(6), entry, EntryField::RstRcvd);
            self.edit(ui, width(7), entry, EntryField::Number);
            fixed_label(ui, width(8), "");
            let mut clicked = false;
            if button(ui, width(9), entry.band().label()) {
                entry.cycle_band(false);
                clicked = true;
            }
            if button(ui, width(10), entry.mode().label()) {
                entry.cycle_mode(false);
                clicked = true;
            }
            let code = entry.power().code().map(String::from).unwrap_or_default();
            if button(ui, width(11), &code) {
                entry.cycle_power();
                clicked = true;
            }
            if clicked {
                self.focus = Some(refocus_after_click(self.last_field));
            }
            fixed_label(ui, width(12), &operator);
            self.edit(ui, width(13), entry, EntryField::Memo);
        });
    }

    fn focused(&self, ctx: &egui::Context) -> Option<EntryField> {
        FIELDS
            .into_iter()
            .find(|f| ctx.memory(|m| m.has_focus(id(*f))))
    }

    fn focus(&mut self, field: EntryField) {
        self.focus = Some(field);
    }
}

impl ZlogRow {
    fn edit(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        entry: &mut dyn EntryState,
        field: EntryField,
    ) {
        let mut text = entry.text(field).to_string();
        if self.select_all == Some(field) {
            let mut state =
                egui::text_edit::TextEditState::load(ui.ctx(), id(field)).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all_str(&text)));
            state.store(ui.ctx(), id(field));
            self.select_all = None;
        }
        let response = ui.add_sized(
            [width, ROW_HEIGHT + 2.0],
            egui::TextEdit::singleline(&mut text)
                .id(id(field))
                .font(egui::TextStyle::Monospace),
        );
        if text != entry.text(field) {
            entry.set_text(field, text);
        }
        if self.focus == Some(field) {
            response.request_focus();
            self.focus = None;
        }
        if response.gained_focus() {
            entry.leave_log();
        }
    }
}

fn fixed_label(ui: &mut egui::Ui, width: f32, text: &str) {
    ui.add_sized(
        [width, ROW_HEIGHT + 2.0],
        egui::Label::new(RichText::new(text).monospace()),
    );
}

fn button(ui: &mut egui::Ui, width: f32, text: &str) -> bool {
    ui.add_sized(
        [width, ROW_HEIGHT + 2.0],
        egui::Button::new(RichText::new(text).monospace()),
    )
    .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_keys_type_capitals_in_the_memo() {
        assert!(!shift_keys_act(Some(EntryField::Memo)));
        for f in [
            None,
            Some(EntryField::Call),
            Some(EntryField::Number),
            Some(EntryField::RstSent),
        ] {
            assert!(shift_keys_act(f), "{f:?}");
        }
    }

    #[test]
    fn click_on_a_box_returns_to_the_last_field() {
        assert_eq!(
            refocus_after_click(Some(EntryField::Number)),
            EntryField::Number
        );
        assert_eq!(
            refocus_after_click(Some(EntryField::Memo)),
            EntryField::Memo
        );
        assert_eq!(refocus_after_click(None), EntryField::Call);
    }
}
