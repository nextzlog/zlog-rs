//! The main window (`TMainForm`): menu, toolbar, log grid and status bar; the
//! entry row is drawn by the chosen [`zlog_ui::EntryStyle`].
//!
//! Keys follow the zLog manual: the entry style takes the keys of the entry, and
//! in the log Up and Down move, Esc goes back to the entry, and Enter edits and
//! Delete deletes.

use crate::i18n::{tr, trf};
use egui::{Event, Id, Key, Modifiers, RichText, Sense};
use egui_extras::{Column, TableBuilder};
use zlog_core::{Band, Mode, Power};

use zlog_ui::{EntryField, EntryStyle};

use crate::row::Row;
use crate::session::{self, Session};

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

/// State of the window that is not part of the log.
#[derive(Default)]
pub struct View {
    /// Selected row of the log grid; `Some` while the grid has the keyboard.
    pub selected: Option<usize>,
    /// A row was double-clicked: run the command whose row key is Enter
    /// ([`zlog_ui::Command::row_key`]) at the next frame.
    pub activate: bool,
    /// Column widths in points, shared by the grid and the entry row; empty until first drawn.
    pub widths: Vec<f32>,
    /// Column being resized: index, its width and the pointer x when the drag began.
    pub resizing: Option<(usize, f32, f32)>,
    /// The start-time dialog, while open.
    pub start_time: Option<StartTimeForm>,
    /// What was chosen in the grid's right-click menu this frame, for the QSO with
    /// this ID.
    pub grid_action: Option<(zlog_core::QsoId, GridAction)>,
    /// Shift+X was pressed in the entry row: select the other rig (`actionToggleRig`).
    pub toggle_rig: bool,
}

/// Fields of the start-time dialog (`TStartTimeDialog`).
#[derive(Debug, Clone, PartialEq)]
pub struct StartTimeForm {
    pub date: String,
    pub time: String,
    /// Judge every QSO against the new period (zLog asks this after OK of Correct
    /// start time, default No).
    pub judge: bool,
    /// Opened for a new log (no judge question; Cancel keeps the suggested time).
    pub new_log: bool,
}

impl StartTimeForm {
    pub fn new(start: zlog_core::DelphiTime, new_log: bool) -> Self {
        let (y, mo, d, h, mi, _) = start.to_ymd_hms();
        StartTimeForm {
            date: format!("{y:04}-{mo:02}-{d:02}"),
            time: format!("{h:02}:{mi:02}"),
            judge: false,
            new_log,
        }
    }

    /// The time typed, if it is a valid date and time.
    pub fn parse(&self) -> Option<zlog_core::DelphiTime> {
        zlog_ui::parse_time(&format!("{} {}", self.date.trim(), self.time.trim()))
    }
}

/// Items of the log grid's right-click menu (zLog `GridMenu` in main.dfm).
#[derive(Debug, Clone, PartialEq)]
pub enum GridAction {
    /// A command listed in the right-click menu ([`zlog_ui::Command::context`]),
    /// run on the selected QSO at the next frame.
    Command(crate::commands::Source, String),
    /// `menuChangeBand`.
    Band(Band),
    /// `menuChangeMode`.
    Mode(Mode),
    /// `menuChangePower`.
    Power(Power),
    /// An item an extension added: the panel's index and the item's identifier.
    Extension(usize, String),
}

/// Items extensions add to the right-click menu of a QSO: one list per menu group,
/// each item with the index of its panel.
pub type ExtensionItems = Vec<Vec<(usize, zlog_ui::QsoMenuItem)>>;

/// Carry out a submenu item of the grid menu on the QSO with this ID.
pub fn apply_grid_action(s: &mut Session, id: zlog_core::QsoId, action: GridAction) {
    if s.find(id).is_none() {
        s.status = session::Status::warning(tr("status.qso_gone"));
        return;
    }
    match action {
        GridAction::Band(b) => {
            s.change_band(&[id], b);
        }
        GridAction::Mode(m) => {
            s.change_mode(&[id], m);
        }
        GridAction::Power(p) => {
            s.change_power(&[id], p);
        }
        // Carried out by the application, which has the panels.
        GridAction::Command(..) | GridAction::Extension(..) => {}
    }
}

/// The commands of the right-click menu of a QSO, by provider.
pub type ContextCommands = Vec<(crate::commands::Source, Vec<zlog_ui::Command>)>;

/// The right-click menu of a grid row (zLog `GridMenu`): the QSO commands, shared
/// with the Edit menu, then the submenus and the extensions' items.
pub fn grid_menu(
    ui: &mut egui::Ui,
    s: &Session,
    context: &ContextCommands,
    extra: &ExtensionItems,
) -> Option<GridAction> {
    let mut chosen = None;
    for (source, commands) in context {
        if let Some(id) = crate::commands::menu_items(ui, commands) {
            chosen = Some(GridAction::Command(*source, id));
        }
    }
    if !context.is_empty() {
        ui.separator();
    }
    chosen.or(qso_submenus(ui, s, extra))
}

/// The Band, Mode and Power submenus of a QSO, then the extensions' items.
pub fn qso_submenus(ui: &mut egui::Ui, s: &Session, extra: &ExtensionItems) -> Option<GridAction> {
    let mut chosen = None;
    let mut item = |ui: &mut egui::Ui, text: &str, action: GridAction| {
        if ui.button(text).clicked() {
            chosen = Some(action);
            ui.close();
        }
    };
    let (lo, hi) = s.rules().band_range();
    let warc = s.rules().uses_warc();
    ui.menu_button(tr("grid.band"), |ui| {
        for b in Band::range(lo, hi).filter(|b| warc || !b.is_warc()) {
            item(ui, b.label(), GridAction::Band(b));
        }
    });
    ui.menu_button(tr("grid.mode"), |ui| {
        for &m in Mode::ALL {
            item(ui, m.label(), GridAction::Mode(m));
        }
    });
    ui.menu_button(tr("grid.power"), |ui| {
        for p in [Power::P001, Power::P002, Power::P005, Power::P010] {
            let code = p.code().map(String::from).unwrap_or_default();
            item(ui, &code, GridAction::Power(p));
        }
    });
    drop(item);
    for group in extra {
        ui.separator();
        for (panel, it) in group {
            if ui
                .add_enabled(it.enabled, egui::Button::new(&it.label))
                .clicked()
            {
                chosen = Some(GridAction::Extension(*panel, it.id.clone()));
                ui.close();
            }
        }
    }
    chosen
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

/// Handle the keys of the log grid while it has the keyboard (the commands' row
/// keys are handled by the application), else hand them to the entry style,
/// before anything is drawn; nothing while a dialog is `modal`. `hz` is the rig frequency, recorded
/// in a QSO logged with Enter.
pub fn handle_keys(
    ctx: &egui::Context,
    s: &mut Session,
    v: &mut View,
    style: Option<&mut (dyn EntryStyle + 'static)>,
    modal: bool,
    hz: Option<u64>,
) {
    if modal || v.start_time.is_some() {
        return;
    }
    let Some(sel) = v.selected else {
        if let Some(style) = style {
            style.keys(
                ctx,
                &mut Row {
                    session: s,
                    view: v,
                    hz,
                },
            );
        }
        return;
    };
    let mut focus_entry = false;
    let n = s.log().qsos.len();
    if shortcut(ctx, Modifiers::NONE, Key::ArrowUp) {
        v.selected = Some(sel.min(n).saturating_sub(1));
    } else if shortcut(ctx, Modifiers::NONE, Key::ArrowDown) {
        v.selected = if sel + 1 < n { Some(sel + 1) } else { None };
        focus_entry = v.selected.is_none();
    } else if shortcut(ctx, Modifiers::NONE, Key::Escape) {
        v.selected = None;
        focus_entry = true;
    }
    let Some(style) = style else {
        return;
    };
    if focus_entry {
        style.focus(EntryField::Call);
        return;
    }
    // zLog's shortcuts are the form's, so the entry row's (Shift+B, Ctrl+K...) work
    // with a grid row selected too; one moving to an entry field leaves the grid.
    let to_entry = ctx.input(|i| {
        i.events.iter().any(|e| match e {
            Event::Key {
                key,
                pressed: true,
                modifiers: m,
                ..
            } => {
                (m.alt && matches!(key, Key::C | Key::N | Key::M | Key::R))
                    || (m.command && *key == Key::K)
            }
            _ => false,
        })
    });
    style.keys(
        ctx,
        &mut Row {
            session: s,
            view: v,
            hz,
        },
    );
    if to_entry {
        v.selected = None;
    }
}

/// The log grid with the last QSO at the bottom.
/// `context` gives the commands and `extra` the extensions' items of a QSO's
/// right-click menu.
pub fn log_grid(
    ui: &mut egui::Ui,
    s: &Session,
    v: &mut View,
    context: &ContextCommands,
    extra: &dyn Fn(&zlog_core::Qso) -> ExtensionItems,
) {
    let widths = widths(ui, v);
    let warn = zlog_ui::palette::error(ui);
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
                    Some(warn)
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
                    v.selected = Some(i);
                    v.activate = true;
                }
                // Right click selects the row and opens zLog's grid menu on it.
                if response.secondary_clicked() {
                    v.selected = Some(i);
                }
                response.context_menu(|ui| {
                    if let Some(a) = grid_menu(ui, s, context, &extra(q)) {
                        v.grid_action = Some((zlog_core::QsoId::of(q), a));
                    }
                });
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

/// The entry row drawn by `style`, with the grid's column widths. Returns whether
/// the operator typed into the callsign or number field (zLog `OtherKeyPressed`).
pub fn entry_row(
    ui: &mut egui::Ui,
    s: &mut Session,
    v: &mut View,
    style: &mut dyn EntryStyle,
    hz: Option<u64>,
) -> bool {
    let typed_before = (s.entry.callsign.clone(), s.entry.nr_rcvd.clone());
    widths(ui, v);
    style.ui(
        ui,
        &mut Row {
            session: s,
            view: v,
            hz,
        },
    );
    (s.entry.callsign.clone(), s.entry.nr_rcvd.clone()) != typed_before
}

/// The status line, then what the panels show (`items`), then the counts on the right.
pub fn status_bar(ui: &mut egui::Ui, s: &Session, items: &[zlog_ui::StatusItem]) {
    ui.horizontal(|ui| {
        let text = RichText::new(&s.status.text);
        ui.label(if s.status.warning {
            text.color(zlog_ui::palette::error(ui))
        } else {
            text
        });
        for item in items {
            ui.separator();
            let text = RichText::new(&item.text);
            let label = ui.label(if item.warning {
                text.color(zlog_ui::palette::error(ui))
            } else {
                text
            });
            if let Some(tip) = &item.tooltip {
                label.on_hover_text(tip);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(trf("status.qso_count", &[&s.log().qsos.len()]));
            ui.separator();
            // CQ in blue, S&P in fuchsia, as zLog's `panelCQMode`.
            ui.label(if s.entry.cq {
                RichText::new(tr("status.cq")).color(zlog_ui::palette::cq(ui))
            } else {
                RichText::new(tr("status.sp")).color(zlog_ui::palette::sp(ui))
            });
            ui.separator();
            ui.label(s.rules().name());
        });
    });
}
