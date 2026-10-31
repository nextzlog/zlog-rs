use egui::{Align, Layout, Rect, Response, Sense, TextEdit, Ui, UiBuilder};

pub const GAP: f32 = 6.0;
pub const MARGIN: f32 = 6.0;

pub fn pane<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new().inner_margin(MARGIN).show(ui, add).inner
}

pub const KEY_WIDTH: f32 = 44.0;
pub const HANDLE: f32 = 8.0;

pub fn toolbar<R>(ui: &mut Ui, status: &[(&str, bool)], add: impl FnOnce(&mut Ui) -> R) -> R {
    let top = ui.cursor().top();
    let status: Vec<_> = status.iter().filter(|(t, _)| !t.is_empty()).collect();
    let gap = ui.spacing().item_spacing.x;
    let galleys: Vec<_> = status
        .iter()
        .map(|(t, warn)| {
            let color = if *warn {
                ui.visuals().warn_fg_color
            } else {
                crate::palette::weak(ui)
            };
            egui::RichText::new(*t).color(color)
        })
        .collect();
    let need: f32 = galleys
        .iter()
        .map(|t| {
            egui::WidgetText::from(t.clone())
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x
                + gap
        })
        .sum();
    let (r, beside) = ui
        .horizontal_wrapped(|ui| {
            let r = add(ui);
            let row = ui.cursor();
            let right = ui.max_rect().right();
            let start = row.min.x + gap;
            let fits = !galleys.is_empty() && right - start >= need;
            if fits {
                let rect = egui::Rect::from_min_max(
                    egui::pos2(start, row.min.y),
                    egui::pos2(right, row.min.y + ui.spacing().interact_size.y),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::right_to_left(Align::Center)),
                    |ui| {
                        for t in galleys.iter().rev() {
                            ui.label(t.clone());
                        }
                    },
                );
            }
            (r, fits)
        })
        .inner;
    if !beside && !galleys.is_empty() {
        ui.with_layout(
            Layout::right_to_left(Align::Min).with_main_wrap(true),
            |ui| {
                for t in galleys.iter().rev() {
                    ui.add(egui::Label::new(t.clone()).wrap());
                }
            },
        );
    }
    ui.separator();
    let used = ui.min_rect().bottom() - top;
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::unique(TOOLBAR_HEIGHT), used));
    r
}

pub const TOOLBAR_HEIGHT: &str = "zlog_toolbar_height";

pub fn toolbar_height(ctx: &egui::Context) -> Option<f32> {
    ctx.data(|d| d.get_temp(egui::Id::unique(TOOLBAR_HEIGHT)))
}

pub const COMPACT: f32 = 360.0;

pub fn compact(ui: &Ui) -> bool {
    ui.max_rect().width() < COMPACT
}

pub fn tool(ui: &mut Ui, icon: &str, label: &str) -> Response {
    toggle(ui, icon, label, false)
}

pub fn toggle(ui: &mut Ui, icon: &str, label: &str, on: bool) -> Response {
    let text = egui::RichText::new(label);
    let short = !icon.is_empty() && compact(ui);
    let r = if icon.is_empty() {
        ui.add(egui::Button::selectable(on, text))
    } else {
        let glyph = egui::RichText::new(icon).family(crate::icon::family());
        if short {
            ui.add(egui::Button::selectable(on, glyph))
        } else {
            ui.add(egui::Button::selectable(on, (glyph, text)))
        }
    };
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, label));
    if short {
        r.on_hover_text(label)
    } else {
        r
    }
}

pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    let pad = ui.spacing().button_padding.x * 2.0;
    let need: f32 = options
        .iter()
        .map(|(_, l)| {
            egui::WidgetText::from(*l)
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                )
                .size()
                .x
                + pad
        })
        .sum::<f32>()
        + 4.0 * options.len() as f32;
    if ui.max_rect().right() - ui.cursor().min.x < need
        && ui.cursor().min.x > ui.max_rect().min.x + 1.0
    {
        ui.end_row();
    }
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    egui::Frame::new()
        .stroke(stroke)
        .corner_radius(ui.visuals().widgets.inactive.corner_radius)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                for (v, label) in options {
                    if toggle(ui, "", label, *current == *v).clicked() && *current != *v {
                        *current = *v;
                        changed = true;
                    }
                }
            });
        });
    changed
}

pub fn line(ui: &mut Ui, text: &str, tip: &str) -> Response {
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        line_left(ui, text, tip)
    })
    .inner
}

fn line_left(ui: &mut Ui, text: &str, tip: &str) -> Response {
    let r = ui.add(
        egui::Label::new(egui::RichText::new(text).color(crate::palette::weak(ui))).truncate(),
    );
    if tip.is_empty() {
        r
    } else {
        r.on_hover_text(tip)
    }
}

pub fn column_width(ui: &Ui, n: usize, min: f32) -> f32 {
    let n = n.max(1) as f32;
    let gap = ui.spacing().item_spacing.x;
    ((ui.available_width() - gap * (n - 1.0)) / n)
        .floor()
        .max(min)
}

pub fn cell(ui: &mut Ui, text: egui::RichText) -> Response {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(text))
        .inner
}

pub fn tick_step(spacing: f32, label: f32) -> usize {
    [1, 2, 3, 4, 6, 12, 24]
        .into_iter()
        .find(|s| spacing * *s as f32 >= label)
        .unwrap_or(24)
}

pub fn bar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let r = ui.horizontal_wrapped(add).inner;
    ui.add_space(GAP);
    r
}

pub fn status(ui: &mut Ui, text: &str, warning: Option<&str>) {
    wide(ui, |ui| {
        if !text.is_empty() {
            ui.add(
                egui::Label::new(egui::RichText::new(text).color(crate::palette::weak(ui))).wrap(),
            );
        }
        if let Some(w) = warning.filter(|w| !w.is_empty()) {
            let warn = egui::RichText::new(w).color(ui.visuals().warn_fg_color);
            ui.add(egui::Label::new(warn).wrap());
        }
    });
}

pub fn caption(ui: &mut Ui, text: &str) {
    ui.add_space(GAP / 2.0);
    ui.colored_label(crate::palette::weak(ui), text);
}

pub fn slider(ui: &mut Ui, slider: egui::Slider<'_>) -> Response {
    ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(60.0);
    ui.add(slider)
}

pub fn columns(room: f32, n: usize, min: f32, spacing: f32) -> usize {
    let n = n.max(1);
    let fit = ((room + spacing) / (min + spacing))
        .floor()
        .clamp(1.0, n as f32) as usize;
    n.div_ceil(n.div_ceil(fit))
}

pub fn grid(ui: &mut Ui, n: usize, min: f32, mut cell: impl FnMut(&mut Ui, usize)) {
    if n == 0 {
        return;
    }
    let spacing = ui.spacing().item_spacing.x;
    let room = ui.available_width();
    let cols = columns(room, n, min, spacing);
    let width = ((room - spacing * (cols as f32 - 1.0)) / cols as f32).floor();
    let height = ui.spacing().interact_size.y;
    ui.vertical(|ui| {
        for row in (0..n).collect::<Vec<_>>().chunks(cols) {
            ui.horizontal(|ui| {
                for &i in row {
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, height),
                        Layout::top_down_justified(Align::Center),
                        |ui| {
                            ui.set_width(width);
                            cell(ui, i)
                        },
                    );
                }
            });
        }
    });
}

pub fn field(ui: &mut Ui, edit: TextEdit<'_>, trailing: impl FnOnce(&mut Ui)) -> Response {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            trailing(ui);
            ui.add(edit.desired_width(ui.available_width()))
        })
        .inner
    })
    .inner
}

pub fn wide<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.with_layout(Layout::top_down_justified(Align::Min), add)
        .inner
}

pub fn rest<R>(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    egui::ScrollArea::both()
        .id_salt(salt)
        .auto_shrink(false)
        .show(ui, add)
        .inner
}

pub fn split(
    ui: &mut Ui,
    ratio: &mut f32,
    first: impl FnOnce(&mut Ui),
    second: impl FnOnce(&mut Ui),
) -> bool {
    let room = ui.available_rect_before_wrap();
    let total = (room.height() - HANDLE).max(2.0 * 40.0);
    let top = (total * ratio.clamp(0.0, 1.0)).clamp(40.0, total - 40.0);
    let a = Rect::from_min_size(room.min, egui::vec2(room.width(), top));
    let handle = Rect::from_min_size(a.left_bottom(), egui::vec2(room.width(), HANDLE));
    let b = Rect::from_min_max(
        handle.left_bottom(),
        egui::pos2(room.right(), room.min.y + total + HANDLE),
    );
    child(ui, a, first);
    let id = ui.scope_id().with("split_handle");
    let r = ui
        .interact(handle, id, Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, "split"));
    let stroke = if r.hovered() || r.dragged() {
        ui.visuals().widgets.hovered.fg_stroke
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    ui.painter()
        .hline(handle.x_range(), handle.center().y, stroke);
    child(ui, b, second);
    ui.allocate_rect(Rect::from_min_max(room.min, b.max), Sense::hover());
    let moved = r.dragged() && r.drag_delta().y != 0.0;
    if moved {
        *ratio = ((top + r.drag_delta().y) / total).clamp(0.05, 0.95);
    }
    moved
}

fn child(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    let layout = *ui.layout();
    let clip = rect.intersect(ui.clip_rect());
    let mut c = ui.new_child(UiBuilder::new().max_rect(rect).layout(layout));
    c.set_clip_rect(clip);
    add(&mut c);
}

pub fn load_ratio(section: &crate::Section, key: &str, default: f32) -> f32 {
    section
        .get(key)
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|r| (0.0..=1.0).contains(r))
        .unwrap_or(default)
}

pub fn save_ratio(section: &mut crate::Section, key: &str, ratio: f32) {
    section.insert(key.into(), format!("{ratio:.3}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_prefer_even_rows() {
        assert_eq!(columns(700.0, 12, KEY_WIDTH, 4.0), 12);
        assert_eq!(columns(300.0, 12, KEY_WIDTH, 4.0), 6);
        assert_eq!(columns(280.0, 12, KEY_WIDTH, 4.0), 4);
        assert_eq!(columns(200.0, 12, KEY_WIDTH, 4.0), 4);
        assert_eq!(columns(10.0, 12, KEY_WIDTH, 4.0), 1);
        assert_eq!(columns(300.0, 5, 80.0, 4.0), 3);
    }

    #[test]
    fn toolbar_status_wraps_inside_the_margins() {
        use egui_kittest::kittest::Queryable;
        for width in [200.0, 280.0, 300.0, 360.0, 420.0, 520.0, 700.0] {
            let mut h = egui_kittest::Harness::builder()
                .with_size([width, 200.0])
                .build_ui(|ui| {
                    pane(ui, |ui| {
                        toolbar(
                            ui,
                            &[("Last 100 QSOs 12.34 QSOs/hr (max 56.78)", true)],
                            |ui| {
                                for l in ["Connect", "Bank A", "Bank B", "CQ repeat", "Stop"] {
                                    tool(ui, "", l);
                                }
                            },
                        )
                    });
                });
            h.run();
            let inner = width - 8.0 - MARGIN;
            let label = h
                .get_by_label("Last 100 QSOs 12.34 QSOs/hr (max 56.78)")
                .rect();
            assert!(label.max.x <= inner + 0.5, "{width}: {label:?}");
            let mut rects = vec![label];
            for b in h.query_all_by_role(egui::accesskit::Role::Button) {
                assert!(b.rect().max.x <= inner + 0.5, "{width}: {:?}", b.rect());
                rects.push(b.rect());
            }
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    assert!(
                        !a.shrink(0.5).intersects(b.shrink(0.5)),
                        "{width}: {a:?} {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn narrow_tools_show_only_the_icon_and_keep_the_label() {
        use egui_kittest::kittest::Queryable;
        for (width, iconic) in [(280.0, true), (700.0, false)] {
            let mut h = egui_kittest::Harness::builder()
                .with_size([width, 200.0])
                .build_ui_state(
                    |ui, ready: &mut bool| {
                        if !*ready {
                            let mut fonts = egui::FontDefinitions::default();
                            crate::icon::add_font(&mut fonts);
                            ui.ctx().set_fonts(fonts);
                            *ready = true;
                            return;
                        }
                        toolbar(ui, &[], |ui| {
                            tool(ui, crate::icon::FLAG, "Targets");
                        })
                    },
                    false,
                );
            h.run();
            let b = h.get_by_label("Targets").rect();
            assert_eq!(b.width() < 40.0, iconic, "{width}: {b:?}");
        }
    }

    #[test]
    fn segmented_selects_one_option() {
        use egui_kittest::kittest::Queryable;
        let mut h = egui_kittest::Harness::builder()
            .with_size([280.0, 100.0])
            .build_ui_state(
                |ui, v: &mut u8| {
                    segmented(ui, v, &[(0, "Total"), (1, "Band"), (2, "Range")]);
                },
                0u8,
            );
        h.run();
        h.get_by_label("Band").click();
        h.run();
        assert_eq!(*h.state(), 1);
        let a = h.get_by_label("Total").rect();
        let b = h.get_by_label("Band").rect();
        assert!(b.min.x - a.max.x < 1.0, "{a:?} {b:?}");
    }

    #[test]
    fn ticks_thin_out_when_narrow() {
        assert_eq!(tick_step(30.0, 18.0), 1);
        assert_eq!(tick_step(10.0, 18.0), 2);
        assert_eq!(tick_step(5.0, 18.0), 4);
        assert_eq!(tick_step(0.5, 18.0), 24);
    }

    #[test]
    fn split_ratio_round_trips() {
        let mut s = crate::Section::new();
        save_ratio(&mut s, "split", 0.25);
        assert_eq!(load_ratio(&s, "split", 0.5), 0.25);
        assert_eq!(load_ratio(&crate::Section::new(), "split", 0.5), 0.5);
    }
}
