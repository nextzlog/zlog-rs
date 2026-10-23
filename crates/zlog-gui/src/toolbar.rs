//! The toolbar tab of Options: which buttons are shown, in what order within their
//! group, and what they show. It edits the `window` section (`hidden_toolbars`,
//! `shown_toolbars`, `toolbar_order`, `toolbar_style`), written on OK like every
//! tab. The buttons are those of the commands now, which the application hands
//! over through [`Rows`] while Options is open.

use std::cell::RefCell;
use std::rc::Rc;

use egui::Ui;
use zlog_ui::SettingsPage;

use crate::commands::{in_order, ToolbarRow};
use crate::i18n::tr;
use crate::settings::{Settings, ToolbarStyle};

/// The rows of every toolbar button in menu order, set by the application.
pub type Rows = Rc<RefCell<Vec<ToolbarRow>>>;

/// The section the tab edits.
const SECTION: &str = "window";

pub struct ToolbarPage {
    rows: Rows,
}

impl ToolbarPage {
    pub fn new(rows: Rows) -> Self {
        ToolbarPage { rows }
    }
}

/// A move asked for in the list: the row at the first index goes to the second.
type Move = (usize, usize);

impl SettingsPage for ToolbarPage {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, _language: &str) -> String {
        tr("options.toolbar").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut zlog_ui::Section, _language: &str) {
        let mut s = Settings::only(SECTION, section);
        let mut rows = self.rows.borrow().clone();
        in_order(&mut rows, &s.toolbar_order());
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("options.toolbar.style"));
            let now = s.toolbar_style();
            for style in ToolbarStyle::ALL {
                let key = format!("toolbar.style.{}", style.code());
                if ui.radio(now == style, tr(&key)).clicked() {
                    s.set_toolbar_style(style);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui
                .button(tr("options.toolbar.reset"))
                .on_hover_text(tr("options.toolbar.reset.tip"))
                .clicked()
            {
                s.reset_toolbar();
            }
        });
        ui.add(egui::Label::new(egui::RichText::new(tr("options.toolbar.note")).weak()).wrap());
        ui.add_space(4.0);
        let moved = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| list(ui, &rows, &mut s))
            .inner;
        if let Some((from, to)) = moved {
            let row = rows.remove(from);
            rows.insert(to, row);
            let keys: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
            let order = merged_order(&s.toolbar_order(), &keys);
            s.set_toolbar_order(&order);
        }
        *section = s.take(SECTION);
    }
}

/// The order saved, `saved`, with the buttons there now in their new order
/// `present`: the places of buttons absent now (of a plugin not loaded) are kept.
fn merged_order(saved: &[String], present: &[String]) -> Vec<String> {
    let mut next = present.iter();
    let mut out: Vec<String> = saved
        .iter()
        .filter_map(|k| {
            if present.contains(k) {
                next.next().cloned()
            } else {
                Some(k.clone())
            }
        })
        .collect();
    out.extend(next.cloned());
    out
}

/// The rows by group, each with its check box, its arrows and a handle to drag
/// it; returns the move asked for.
fn list(ui: &mut Ui, rows: &[ToolbarRow], s: &mut Settings) -> Option<Move> {
    let mut moved = None;
    let same = |a: usize, b: usize| rows.get(b).is_some_and(|r| r.group == rows[a].group);
    for (i, row) in rows.iter().enumerate() {
        if i == 0 || rows[i - 1].group != row.group {
            if i > 0 {
                ui.add_space(6.0);
            }
            ui.label(egui::RichText::new(tr(crate::menus::group_key(row.group))).strong());
        }
        let r = ui
            .horizontal(|ui| {
                let id = ui.scope_id().with(("toolbar_row", &row.key));
                ui.dnd_drag_source(id, i, |ui| ui.add(egui::Label::new("≡").selectable(false)))
                    .response
                    .on_hover_text(tr("options.toolbar.drag"));
                let mut shown = s.toolbar_shown(&row.key, row.default);
                if ui.checkbox(&mut shown, &row.label).changed() {
                    s.set_toolbar_shown(&row.key, shown);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let down = i + 1;
                    if ui
                        .add_enabled(same(i, down), egui::Button::new("▼").small())
                        .on_hover_text(tr("options.toolbar.down"))
                        .clicked()
                    {
                        moved = Some((i, down));
                    }
                    let up = i.wrapping_sub(1);
                    if ui
                        .add_enabled(i > 0 && same(i, up), egui::Button::new("▲").small())
                        .on_hover_text(tr("options.toolbar.up"))
                        .clicked()
                    {
                        moved = Some((i, up));
                    }
                });
            })
            .response;
        // A row dropped on another of its group takes its place.
        if let Some(from) = r.dnd_release_payload::<usize>() {
            if *from != i && same(*from, i) {
                moved = Some((*from, i));
            }
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|k| k.to_string()).collect()
    }

    /// Moving a button keeps the saved places of buttons not loaded now.
    #[test]
    fn absent_buttons_keep_their_places() {
        let saved = keys(&["a", "plugin", "b", "c"]);
        assert_eq!(
            merged_order(&saved, &keys(&["b", "a", "c"])),
            keys(&["b", "plugin", "a", "c"])
        );
        // A button new since the order was saved goes at the end.
        assert_eq!(
            merged_order(&saved, &keys(&["a", "new", "b", "c"])),
            keys(&["a", "plugin", "new", "b", "c"])
        );
        assert_eq!(merged_order(&[], &keys(&["a"])), keys(&["a"]));
    }
}
