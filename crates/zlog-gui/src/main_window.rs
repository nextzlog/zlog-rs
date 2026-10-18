//! The main window (`TMainForm`): menu, toolbar, log grid, entry row and status bar.
//!
//! Keys follow the zLog manual:
//! Space moves from the callsign to the number (checking dupes and filling the
//! number from earlier QSOs) and back from other fields, Enter logs,
//! Shift+B / Shift+M / Shift+P change band, mode and power,
//! Alt+C / Alt+N / Alt+M / Alt+R jump to callsign, number, memo and RST,
//! Shift+Up / Shift+Down change the S of an RST, Ctrl+K clears callsign and number,
//! Up moves into the log, where Enter edits and Delete deletes.

use crate::i18n::{tr, trf};
use egui::{Color32, Event, Id, Key, Modifiers, RichText, Sense};
use egui_extras::{Column, TableBuilder};

use crate::entry::Entry;
use crate::session::{self, NotLogged, Session};

/// Columns shared by the log grid and the entry row, so that fields line up as in zLog.
/// Each column is a text key and a width in characters.
const COLUMNS: &[(&str, f32)] = &[
    ("column.status", 2.0),
    ("column.date", 5.0),
    ("column.time", 5.0),
    ("column.call", 11.0),
    ("column.rst", 4.0),
    ("column.sent", 8.0),
    ("column.rst", 4.0),
    ("column.rcvd", 8.0),
    ("column.multi", 7.0),
    ("column.band", 6.0),
    ("column.mode", 5.0),
    ("column.power", 2.0),
    ("column.op", 7.0),
    ("column.memo", 14.0),
    ("column.points", 3.0),
];

const ROW_HEIGHT: f32 = 18.0;

/// Entry fields that can hold the keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Call,
    RstSent,
    RstRcvd,
    Number,
    Memo,
}

impl Field {
    fn id(self) -> Id {
        Id::unique(("entry", self as u8))
    }
}

/// State of the window that is not part of the log.
#[derive(Default)]
pub struct View {
    /// Selected row of the log grid; `Some` while the grid has the keyboard.
    pub selected: Option<usize>,
    pub editing: Option<(usize, zlog_core::Qso)>,
    pub confirm_delete: Option<usize>,
    pub focus: Option<Field>,
    /// Select the whole text of this field when it is next drawn (after a dupe).
    pub select_all: Option<Field>,
    /// Column widths in points, shared by the grid and the entry row; empty until first drawn.
    pub widths: Vec<f32>,
    /// Column being resized: index, its width and the pointer x when the drag began.
    pub resizing: Option<(usize, f32, f32)>,
}

fn char_width(ui: &egui::Ui) -> f32 {
    ui.fonts_mut(|f| f.glyph_width(&egui::TextStyle::Monospace.resolve(ui.style()), '0'))
}

/// Current column widths, starting from [`COLUMNS`] in characters.
fn widths(ui: &egui::Ui, v: &mut View) -> Vec<f32> {
    if v.widths.is_empty() {
        let cw = char_width(ui);
        v.widths = COLUMNS.iter().map(|(_, w)| w * cw + 6.0).collect();
    }
    v.widths.clone()
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

fn focused_field(ctx: &egui::Context) -> Option<Field> {
    [
        Field::Call,
        Field::RstSent,
        Field::RstRcvd,
        Field::Number,
        Field::Memo,
    ]
    .into_iter()
    .find(|f| ctx.memory(|m| m.has_focus(f.id())))
}

/// Handle the entry-row keys before the fields are drawn, so the fields never see them.
pub fn handle_keys(ctx: &egui::Context, s: &mut Session, v: &mut View) {
    if v.editing.is_some() || v.confirm_delete.is_some() {
        return;
    }
    if let Some(sel) = v.selected {
        let n = s.log().qsos.len();
        if shortcut(ctx, Modifiers::NONE, Key::ArrowUp) {
            v.selected = Some(sel.saturating_sub(1));
        } else if shortcut(ctx, Modifiers::NONE, Key::ArrowDown) {
            v.selected = if sel + 1 < n { Some(sel + 1) } else { None };
            if v.selected.is_none() {
                v.focus = Some(Field::Call);
            }
        } else if shortcut(ctx, Modifiers::NONE, Key::Enter) {
            v.editing = Some((sel, s.log().qsos[sel].clone()));
        } else if shortcut(ctx, Modifiers::NONE, Key::Delete) {
            v.confirm_delete = Some(sel);
        } else if shortcut(ctx, Modifiers::NONE, Key::Escape) {
            v.selected = None;
            v.focus = Some(Field::Call);
        }
        return;
    }

    let field = focused_field(ctx);
    if shortcut(ctx, Modifiers::SHIFT, Key::B) {
        s.cycle_band(false);
    }
    if shortcut(ctx, Modifiers::SHIFT, Key::M) {
        s.cycle_mode(false);
    }
    if shortcut(ctx, Modifiers::SHIFT, Key::P) {
        s.entry.cycle_power();
    }
    // Shift+Up / Shift+Down change the S figure of the RST being edited.
    for (key, up) in [(Key::ArrowUp, true), (Key::ArrowDown, false)] {
        match field {
            Some(Field::RstSent) if shortcut(ctx, Modifiers::SHIFT, key) => {
                Entry::bump_s(&mut s.entry.rst_sent, up)
            }
            Some(Field::RstRcvd) if shortcut(ctx, Modifiers::SHIFT, key) => {
                Entry::bump_s(&mut s.entry.rst_rcvd, up)
            }
            _ => {}
        }
    }
    for (key, target) in [
        (Key::C, Field::Call),
        (Key::N, Field::Number),
        (Key::M, Field::Memo),
        (Key::R, Field::RstRcvd),
    ] {
        if shortcut(ctx, Modifiers::ALT, key) {
            v.focus = Some(target);
        }
    }
    // Ctrl+K clears callsign, number and memo (`actionClearCallAndRpt`).
    if shortcut(ctx, Modifiers::COMMAND, Key::K) {
        s.entry.clear_call_and_number();
        s.status = Default::default();
        v.focus = Some(Field::Call);
    }
    let space = |ctx: &egui::Context| shortcut(ctx, Modifiers::NONE, Key::Space);
    match field {
        // As zLog with dupes allowed: a dupe is reported, and the cursor still moves on.
        Some(Field::Call) if space(ctx) => {
            let dupe = s.check_call();
            s.fill_exchange();
            if !dupe {
                s.check_number();
            }
            v.focus = Some(Field::Number);
        }
        Some(Field::Memo) | None => {}
        Some(_) if space(ctx) => v.focus = Some(Field::Call),
        Some(_) => {}
    }
    if field.is_some() && shortcut(ctx, Modifiers::NONE, Key::Enter) {
        match s.log_entry(session::now_for(s.log())) {
            Ok(()) | Err(NotLogged::NoCallsign) => v.focus = Some(Field::Call),
            Err(NotLogged::InvalidNumber) => {
                v.focus = Some(Field::Number);
                v.select_all = Some(Field::Number);
            }
            Err(NotLogged::Refused) => v.focus = field,
        }
    }
    if field.is_some() && !s.log().qsos.is_empty() && shortcut(ctx, Modifiers::NONE, Key::ArrowUp) {
        v.selected = Some(s.log().qsos.len() - 1);
        ctx.memory_mut(|m| m.surrender_focus(field.unwrap().id()));
    }
}

/// The log grid with the last QSO at the bottom.
pub fn log_grid(ui: &mut egui::Ui, s: &Session, v: &mut View) {
    let widths = widths(ui, v);
    let mut drag: Option<(usize, egui::Response)> = None;
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .sense(Sense::click())
        .stick_to_bottom(true)
        .min_scrolled_height(0.0)
        .max_scroll_height(f32::INFINITY);
    for w in &widths {
        table = table.column(Column::exact(*w));
    }
    if let Some(sel) = v.selected {
        table = table.scroll_to_row(sel, None);
    }
    table
        .header(ROW_HEIGHT, |mut header| {
            for (i, (title, _)) in COLUMNS.iter().enumerate() {
                header.col(|ui| {
                    ui.label(RichText::new(tr(title)).monospace().strong());
                    // Drag the right edge of a heading to resize the column and its entry field.
                    let r = ui.max_rect();
                    let edge =
                        egui::Rect::from_x_y_ranges(r.right() - 3.0..=r.right() + 3.0, r.y_range());
                    let handle = ui.interact(edge, Id::unique(("column_edge", i)), Sense::drag());
                    if handle.hovered() || handle.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
                    }
                    if handle.dragged() || handle.drag_stopped() {
                        drag = Some((i, handle));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, s.log().qsos.len(), |mut row| {
                let i = row.index();
                let q = &s.log().qsos[i];
                row.set_selected(v.selected == Some(i));
                let (_, mo, d, h, mi, _) = q.time.to_ymd_hms();
                let power = q.power.code().map(String::from);
                let cells = [
                    if q.invalid {
                        "X"
                    } else if q.dupe {
                        "D"
                    } else {
                        ""
                    }
                    .to_string(),
                    format!("{mo:02}/{d:02}"),
                    format!("{h:02}:{mi:02}"),
                    q.callsign.clone(),
                    q.rst_sent.to_string(),
                    q.nr_sent.clone(),
                    q.rst_rcvd.to_string(),
                    q.nr_rcvd.clone(),
                    if q.new_multi1 {
                        q.multi1.clone()
                    } else {
                        String::new()
                    },
                    q.band.label().to_string(),
                    q.mode.label().to_string(),
                    power.unwrap_or_default(),
                    q.operator.clone(),
                    q.memo.clone(),
                    q.points.to_string(),
                ];
                let color = if q.dupe || q.invalid {
                    Some(Color32::from_rgb(200, 60, 60))
                } else {
                    None
                };
                for text in cells {
                    row.col(|ui| {
                        let mut t = RichText::new(text).monospace();
                        if let Some(c) = color {
                            t = t.color(c);
                        }
                        ui.add(egui::Label::new(t).selectable(false));
                    });
                }
                let response = row.response();
                if response.clicked() {
                    v.selected = Some(i);
                }
                if response.double_clicked() {
                    v.editing = Some((i, q.clone()));
                }
            });
        });
    // Width follows the pointer from where the drag began, however often egui redraws.
    match drag {
        Some((i, handle)) => {
            let x = handle.interact_pointer_pos().map_or(0.0, |p| p.x);
            let (_, start_width, start_x) = *v.resizing.get_or_insert((i, v.widths[i], x));
            v.widths[i] = (start_width + x - start_x).max(12.0);
            if handle.drag_stopped() {
                v.resizing = None;
            }
        }
        None => v.resizing = None,
    }
}

/// The entry row, aligned with the grid columns.
pub fn entry_row(ui: &mut egui::Ui, s: &mut Session, v: &mut View) {
    let widths = widths(ui, v);
    let width = |col: usize| widths[col];
    let now = session::now_for(s.log());
    let (_, mo, d, h, mi, _) = now.to_ymd_hms();
    let sent = s.sent_number();
    let operator = s.station.operator.clone();

    ui.horizontal(|ui| {
        fixed_label(ui, width(0), "");
        fixed_label(ui, width(1), &format!("{mo:02}/{d:02}"));
        fixed_label(ui, width(2), &format!("{h:02}:{mi:02}"));
        edit(ui, width(3), &mut s.entry.callsign, Field::Call, v);
        edit(ui, width(4), &mut s.entry.rst_sent, Field::RstSent, v);
        fixed_label(ui, width(5), &sent);
        edit(ui, width(6), &mut s.entry.rst_rcvd, Field::RstRcvd, v);
        let before = s.entry.nr_rcvd.clone();
        edit(ui, width(7), &mut s.entry.nr_rcvd, Field::Number, v);
        if s.entry.nr_rcvd != before {
            s.check_number();
        }
        fixed_label(ui, width(8), "");
        if button(ui, width(9), s.entry.band.label()) {
            s.cycle_band(false);
        }
        if button(ui, width(10), s.entry.mode.label()) {
            s.cycle_mode(false);
        }
        let code = s.entry.power.code().map(String::from).unwrap_or_default();
        if button(ui, width(11), &code) {
            s.entry.cycle_power();
        }
        fixed_label(ui, width(12), &operator);
        edit(ui, width(13), &mut s.entry.memo, Field::Memo, v);
    });
    s.entry.callsign = s.entry.callsign.to_uppercase();
    s.entry.nr_rcvd = s.entry.nr_rcvd.to_uppercase();
}

fn edit(ui: &mut egui::Ui, width: f32, text: &mut String, field: Field, v: &mut View) {
    if v.select_all == Some(field) {
        let mut state =
            egui::text_edit::TextEditState::load(ui.ctx(), field.id()).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::select_all_str(text)));
        state.store(ui.ctx(), field.id());
        v.select_all = None;
    }
    let response = ui.add_sized(
        [width, ROW_HEIGHT + 2.0],
        egui::TextEdit::singleline(text)
            .id(field.id())
            .font(egui::TextStyle::Monospace),
    );
    if v.focus == Some(field) {
        response.request_focus();
        v.focus = None;
    }
    if response.gained_focus() {
        v.selected = None;
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

pub fn status_bar(ui: &mut egui::Ui, s: &Session) {
    ui.horizontal(|ui| {
        let text = RichText::new(&s.status.text);
        ui.label(if s.status.warning {
            text.color(Color32::RED)
        } else {
            text
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(trf("status.qso_count", &[&s.log().qsos.len()]));
            ui.separator();
            ui.label(s.rules().name());
        });
    });
}
