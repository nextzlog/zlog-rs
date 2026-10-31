use crate::i18n::tr;
use egui::{Event, Key, Modifiers};
use zlog_core::{Band, Mode, Power};
use zlog_ui::chrome::{LogTable, TableColumn, TableRow, TableView};

use zlog_ui::{EntryField, EntryStyle};

use crate::row::Row;
use crate::session::{self, Session};

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

pub fn held_row(selected: Option<usize>, qsos: &[zlog_core::Qso]) -> Option<zlog_core::QsoId> {
    selected.and_then(|i| qsos.get(i)).map(zlog_core::QsoId::of)
}

pub fn follow_row(
    selected: Option<usize>,
    held: Option<zlog_core::QsoId>,
    qsos: &[zlog_core::Qso],
) -> Option<usize> {
    let i = selected?;
    match held {
        Some(id) if held_row(Some(i), qsos) != Some(id) => qsos
            .iter()
            .position(|q| zlog_core::QsoId::of(q) == id)
            .or(Some(i)),
        _ => Some(i),
    }
}

#[derive(Default)]
pub struct View {
    pub selected: Option<usize>,
    pub activate: bool,
    pub widths: Vec<f32>,
    pub start_time: Option<zlog_ui::time::StartTimeForm>,
    pub grid_action: Option<(zlog_core::QsoId, GridAction)>,
    pub toggle_rig: bool,
    pub has_rig: bool,
    pub suggestions: Vec<zlog_ui::Suggestion>,
    pub sends: Vec<u8>,
    pub messages: Vec<String>,
    pub run_role: Option<String>,
    pub qsy: Option<u64>,
    pub entry_width: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GridAction {
    Command(crate::commands::Source, String),
    Band(Band),
    Mode(Mode),
    Power(Power),
    Extension(usize, String),
}

pub(crate) type ExtensionItems = Vec<Vec<(usize, zlog_ui::QsoMenuItem)>>;

pub(crate) fn apply_grid_action(s: &mut Session, id: zlog_core::QsoId, action: GridAction) {
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
        GridAction::Command(..) | GridAction::Extension(..) => {}
    }
}

pub(crate) type ContextCommands = Vec<(crate::commands::Source, Vec<zlog_ui::Command>)>;

pub fn grid_menu(
    ui: &mut egui::Ui,
    s: &Session,
    context: &ContextCommands,
    extra: &ExtensionItems,
) -> Option<GridAction> {
    let mut chosen = None;
    for (source, commands) in context {
        if let Some(id) = zlog_ui::chrome::menu_items(ui, commands) {
            chosen = Some(GridAction::Command(*source, id));
        }
    }
    if !context.is_empty() {
        ui.separator();
    }
    chosen.or(qso_submenus(ui, s, extra))
}

pub(crate) fn qso_submenus(
    ui: &mut egui::Ui,
    s: &Session,
    extra: &ExtensionItems,
) -> Option<GridAction> {
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
                .add_enabled(
                    it.enabled,
                    egui::Button::new(zlog_ui::command::with_ellipsis(&it.label, it.dialog)),
                )
                .clicked()
            {
                chosen = Some(GridAction::Extension(*panel, it.id.clone()));
                ui.close();
            }
        }
    }
    chosen
}

fn columns() -> Vec<TableColumn> {
    COLUMNS
        .iter()
        .map(|(title, chars)| TableColumn {
            title: tr(title).to_string(),
            chars: *chars,
        })
        .collect()
}

fn widths(ui: &egui::Ui, v: &mut View) -> Vec<f32> {
    if v.widths.len() != COLUMNS.len() {
        v.widths = zlog_ui::chrome::column_widths(ui, &columns());
    }
    v.widths.clone()
}

pub(crate) fn table_row(q: &zlog_core::Qso) -> TableRow {
    let (_, mo, d, h, mi, _) = q.time.to_ymd_hms();
    let mark = if q.invalid {
        "X"
    } else if q.dupe {
        "D"
    } else {
        ""
    };
    TableRow {
        cells: vec![
            mark.to_string(),
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
            q.power.code().map(String::from).unwrap_or_default(),
            q.operator.clone(),
            q.memo.clone(),
            q.points.to_string(),
        ],
        warning: q.dupe || q.invalid,
    }
}

pub(crate) fn grid_width(ui: &egui::Ui, v: &mut View) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    widths(ui, v).iter().map(|w| w + gap).sum()
}

fn shortcut(ctx: &egui::Context, modifiers: Modifiers, key: Key) -> bool {
    ctx.input_mut(|i| {
        let hit = zlog_ui::keys::consume_exact(i, modifiers, key);
        if hit {
            i.events.retain(|e| !matches!(e, Event::Text(_)));
        }
        hit
    })
}

pub(crate) fn handle_keys(
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
    let own = style.as_deref().and_then(|s| s.focused(ctx)).is_some();
    if !own && zlog_ui::keys::text_focused(ctx) {
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
    let to_entry = ctx.input(|i| {
        i.events.iter().any(|e| match e {
            Event::Key {
                key,
                pressed: true,
                modifiers: m,
                ..
            } => {
                (m.alt && matches!(key, Key::C | Key::N | Key::M | Key::R))
                    || (m.ctrl && *key == Key::K)
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

pub(crate) fn log_table(
    ui: &mut egui::Ui,
    grid: &mut dyn LogTable,
    s: &Session,
    v: &mut View,
    context: &ContextCommands,
    extra: &dyn Fn(&zlog_core::Qso) -> ExtensionItems,
) {
    widths(ui, v);
    let qsos = &s.log().qsos;
    let row = |i: usize| table_row(&qsos[i]);
    let columns = columns();
    let action = &mut v.grid_action;
    let mut menu = |ui: &mut egui::Ui, i: usize| {
        let q = &qsos[i];
        if let Some(a) = grid_menu(ui, s, context, &extra(q)) {
            *action = Some((zlog_core::QsoId::of(q), a));
        }
    };
    grid.show(
        ui,
        TableView {
            columns: &columns,
            rows: qsos.len(),
            row: &row,
            widths: &mut v.widths,
            selected: &mut v.selected,
            activate: &mut v.activate,
            menu: &mut menu,
        },
    );
}

pub(crate) fn entry_row(
    ui: &mut egui::Ui,
    s: &mut Session,
    v: &mut View,
    style: &mut dyn EntryStyle,
    hz: Option<u64>,
    host: Option<&mut (dyn zlog_ui::chrome::WindowHost + 'static)>,
) -> bool {
    let typed = style.focused(ui.ctx()) == Some(zlog_ui::EntryField::Call)
        && ui.input(|i| {
            let plain = i.events.iter().all(|e| match e {
                egui::Event::Key { modifiers, .. } => modifiers.is_none(),
                _ => true,
            });
            plain
                && i.modifiers.is_none()
                && i.events.iter().any(|e| match e {
                    egui::Event::Text(t) => t.chars().any(|c| c.is_ascii_alphanumeric()),
                    _ => false,
                })
        });
    widths(ui, v);
    let mut row = Row {
        session: s,
        view: v,
        hz,
    };
    match host {
        Some(h) => match style.own_window().map(str::to_string) {
            Some(id) if h.hosts(&id) => {}
            Some(id) => {
                let lang = crate::i18n::current();
                style.floating_ui(ui, &mut row, &mut |ui, body| {
                    h.floating(ui, &id, lang, body)
                })
            }
            None => style.ui(ui, &mut row),
        },
        None => style.ui(ui, &mut row),
    }
    typed
}

#[cfg(test)]
mod tests {
    #[test]
    fn table_rows_mark_dupes_and_fill_every_column() {
        let mut q = zlog_core::Qso {
            callsign: "JA1ZLO".into(),
            ..Default::default()
        };
        let row = super::table_row(&q);
        assert_eq!(row.cells.len(), super::COLUMNS.len());
        assert_eq!(row.cells[3], "JA1ZLO");
        assert!(!row.warning);
        q.callsign = "JA1YWX".into();
        q.dupe = true;
        let row = super::table_row(&q);
        assert_eq!((row.cells[0].as_str(), row.warning), ("D", true));
    }

    #[test]
    fn the_selection_follows_its_qso_when_a_remote_insert_shifts_the_rows() {
        let q = |call: &str, id: i32| zlog_core::Qso {
            callsign: call.into(),
            reserve3: id * 100,
            ..Default::default()
        };
        let before = [q("A", 1), q("B", 2), q("C", 3)];
        let held = super::held_row(Some(1), &before);
        let after = [q("X", 9), q("A", 1), q("B", 2), q("C", 3)];
        assert_eq!(super::follow_row(Some(1), held, &after), Some(2));
        let gone = [q("A", 1), q("C", 3)];
        assert_eq!(super::follow_row(Some(1), held, &gone), Some(1));
        assert_eq!(super::follow_row(Some(1), held, &before), Some(1));
        assert_eq!(super::follow_row(None, held, &after), None);
    }
}
