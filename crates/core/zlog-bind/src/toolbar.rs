use egui::Ui;
use zlog_ui::toolbar::{self as bar, in_order, Rows, ToolbarRow, ToolbarStyle, SECTION};
use zlog_ui::SettingsPage;

use crate::texts;

pub struct ToolbarPage {
    rows: Rows,
}

impl ToolbarPage {
    pub fn new(rows: Rows) -> Self {
        ToolbarPage { rows }
    }
}

type Move = (usize, usize);

impl SettingsPage for ToolbarPage {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "options.toolbar").into()
    }

    fn ui(&mut self, ui: &mut Ui, s: &mut zlog_ui::Section, language: &str) {
        let tr = |key: &str| texts().get(language, key).to_string();
        let mut rows = self.rows.borrow().clone();
        in_order(&mut rows, &bar::order(s));
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("options.toolbar.style"));
            let now = bar::style(s);
            for style in ToolbarStyle::ALL {
                let key = format!("toolbar.style.{}", style.code());
                if ui.radio(now == style, tr(&key)).clicked() {
                    bar::set_style(s, style);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui
                .button(tr("options.toolbar.reset"))
                .on_hover_text(tr("options.toolbar.reset.tip"))
                .clicked()
            {
                bar::reset(s);
            }
        });
        ui.add(egui::Label::new(egui::RichText::new(tr("options.toolbar.note")).weak()).wrap());
        ui.add_space(4.0);
        let moved = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| list(ui, &rows, s, language))
            .inner;
        if let Some((from, to)) = moved {
            let row = rows.remove(from);
            rows.insert(to, row);
            let keys: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
            let order = merged_order(&bar::order(s), &keys);
            bar::set_order(s, &order);
        }
    }
}

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

fn list(ui: &mut Ui, rows: &[ToolbarRow], s: &mut zlog_ui::Section, lang: &str) -> Option<Move> {
    let tr = |key: &str| texts().get(lang, key).to_string();
    let mut moved = None;
    let same = |a: usize, b: usize| rows.get(b).is_some_and(|r| r.group == rows[a].group);
    for (i, row) in rows.iter().enumerate() {
        if i == 0 || rows[i - 1].group != row.group {
            if i > 0 {
                ui.add_space(6.0);
            }
            ui.label(egui::RichText::new(&row.group_title).strong());
        }
        let r = ui
            .horizontal(|ui| {
                let id = ui.scope_id().with(("toolbar_row", &row.key));
                ui.dnd_drag_source(id, i, |ui| ui.add(egui::Label::new("≡").selectable(false)))
                    .response
                    .on_hover_text(tr("options.toolbar.drag"));
                let mut shown = bar::shown(s, &row.key, row.default);
                if ui.checkbox(&mut shown, &row.label).changed() {
                    bar::set_shown(s, &row.key, shown);
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

    #[test]
    fn absent_buttons_keep_their_places() {
        let saved = keys(&["a", "plugin", "b", "c"]);
        assert_eq!(
            merged_order(&saved, &keys(&["b", "a", "c"])),
            keys(&["b", "plugin", "a", "c"])
        );
        assert_eq!(
            merged_order(&saved, &keys(&["a", "new", "b", "c"])),
            keys(&["a", "plugin", "new", "b", "c"])
        );
        assert_eq!(merged_order(&[], &keys(&["a"])), keys(&["a"]));
    }
}
