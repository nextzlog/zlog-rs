use egui::{Id, RichText, Sense};
use egui_extras::{Column, TableBuilder};
use zlog_ui::chrome::{LogTable, TableView};

const ROW_HEIGHT: f32 = 18.0;
const MIN_WIDTH: f32 = 12.0;

#[derive(Default)]
pub struct Table {
    resizing: Option<(usize, f32, f32)>,
}

impl Table {
    fn resize(&mut self, widths: &mut [f32], drag: Option<(usize, egui::Response)>) {
        let Some((i, handle)) = drag else {
            self.resizing = None;
            return;
        };
        let x = handle.interact_pointer_pos().map_or(0.0, |p| p.x);
        self.drag_to(widths, i, x);
        if handle.drag_stopped() {
            self.resizing = None;
        }
    }

    fn drag_to(&mut self, widths: &mut [f32], i: usize, x: f32) {
        let (_, start_width, start_x) = *self.resizing.get_or_insert((i, widths[i], x));
        widths[i] = (start_width + x - start_x).max(MIN_WIDTH);
    }
}

impl LogTable for Table {
    fn show(&mut self, ui: &mut egui::Ui, v: TableView<'_>) {
        if v.widths.len() != v.columns.len() {
            *v.widths = zlog_ui::chrome::column_widths(ui, v.columns);
        }
        let warn = zlog_ui::palette::error(ui);
        let mut drag: Option<(usize, egui::Response)> = None;
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .sense(Sense::click())
            .stick_to_bottom(true)
            .min_scrolled_height(0.0)
            .max_scroll_height(f32::INFINITY);
        for w in v.widths.iter() {
            table = table.column(Column::exact(*w));
        }
        if let Some(sel) = *v.selected {
            table = table.scroll_to_row(sel, None);
        }
        let (selected, activate, menu) = (v.selected, v.activate, v.menu);
        table
            .header(ROW_HEIGHT, |mut header| {
                for (i, column) in v.columns.iter().enumerate() {
                    header.col(|ui| {
                        ui.label(RichText::new(&column.title).monospace().strong());
                        let r = ui.max_rect();
                        let edge = egui::Rect::from_x_y_ranges(
                            r.right() - 3.0..=r.right() + 3.0,
                            r.y_range(),
                        );
                        let handle =
                            ui.interact(edge, Id::unique(("column_edge", i)), Sense::drag());
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
                body.rows(ROW_HEIGHT, v.rows, |mut row| {
                    let i = row.index();
                    let data = (v.row)(i);
                    row.set_selected(*selected == Some(i));
                    for text in data.cells {
                        row.col(|ui| {
                            let mut t = RichText::new(text).monospace();
                            if data.warning {
                                t = t.color(warn);
                            }
                            ui.add(egui::Label::new(t).selectable(false));
                        });
                    }
                    let response = row.response();
                    if response.clicked() || response.secondary_clicked() {
                        *selected = Some(i);
                    }
                    if response.double_clicked() {
                        *selected = Some(i);
                        *activate = true;
                    }
                    response.context_menu(|ui| menu(ui, i));
                });
            });
        self.resize(v.widths, drag);
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.log_table(Table::default);
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_ui::chrome::{TableColumn, TableRow};

    fn columns() -> Vec<TableColumn> {
        ["コール", "交信"]
            .iter()
            .map(|t| TableColumn {
                title: t.to_string(),
                chars: 8.0,
            })
            .collect()
    }

    fn row(i: usize) -> TableRow {
        TableRow {
            cells: vec![["JA1ZLO", "JA1YWX"][i % 2].to_string(), i.to_string()],
            warning: i == 1,
        }
    }

    fn pass(
        table: &mut Table,
        widths: &mut Vec<f32>,
        selected: &mut Option<usize>,
        events: Vec<egui::Event>,
    ) -> bool {
        let ctx = egui::Context::default();
        let mut activate = false;
        for events in [Vec::new(), events] {
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                ..Default::default()
            };
            let cols = columns();
            ctx.run_ui(input, |ui| {
                table.show(
                    ui,
                    TableView {
                        columns: &cols,
                        rows: 2,
                        row: &row,
                        widths,
                        selected,
                        activate: &mut activate,
                        menu: &mut |_, _| {},
                    },
                )
            })
            .textures_delta
            .clear();
        }
        activate
    }

    #[test]
    fn widths_follow_the_columns() {
        let (mut table, mut widths, mut selected) = (Table::default(), vec![1.0], None);
        pass(&mut table, &mut widths, &mut selected, Vec::new());
        assert_eq!(widths.len(), 2);
        assert!(widths.iter().all(|w| *w > 8.0));
        assert_eq!(selected, None);
    }

    #[test]
    fn kept_widths_are_not_reset() {
        let (mut table, mut widths, mut selected) = (Table::default(), vec![40.0, 50.0], None);
        pass(&mut table, &mut widths, &mut selected, Vec::new());
        assert_eq!(widths, vec![40.0, 50.0]);
    }

    #[test]
    fn a_click_selects_and_a_double_click_activates() {
        let at = egui::pos2(10.0, ROW_HEIGHT * 2.5);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let click = vec![egui::Event::PointerMoved(at), press(true), press(false)];
        let (mut table, mut widths, mut selected) = (Table::default(), Vec::new(), None);
        assert!(!pass(&mut table, &mut widths, &mut selected, click.clone()));
        assert_eq!(selected, Some(1));
        let mut twice = click.clone();
        twice.extend(click);
        selected = None;
        assert!(pass(&mut table, &mut widths, &mut selected, twice));
        assert_eq!(selected, Some(1));
    }

    #[test]
    fn dragging_an_edge_keeps_a_minimum_width() {
        let mut table = Table::default();
        let mut widths = vec![40.0, 50.0];
        table.drag_to(&mut widths, 0, 100.0);
        table.drag_to(&mut widths, 0, 130.0);
        assert_eq!(widths[0], 70.0);
        table.drag_to(&mut widths, 0, 0.0);
        assert_eq!(widths[0], MIN_WIDTH);
        assert_eq!(widths[1], 50.0);
    }
}
