use zlog_ui::chrome::{menu_items, Pick, ToolBar};
use zlog_ui::toolbar::ToolbarStyle;
use zlog_ui::{icon, Command};

#[derive(Default)]
pub struct ButtonRow;

impl ToolBar for ButtonRow {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        groups: &[Vec<(usize, &Command)>],
        style: ToolbarStyle,
    ) -> Option<Pick> {
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            for (n, group) in groups.iter().enumerate() {
                if n > 0 {
                    ui.separator();
                }
                for (source, c) in group {
                    if let Some(id) = toolbar_button(ui, c, style) {
                        chosen = Some(Pick {
                            source: *source,
                            id,
                        });
                    }
                }
            }
        });
        chosen
    }
}

pub fn toolbar_button(ui: &mut egui::Ui, c: &Command, style: ToolbarStyle) -> Option<String> {
    let shortcut = c.shortcut.as_ref().map(|k| ui.ctx().format_shortcut(k));
    let tip = c.full_tooltip(shortcut.as_deref());
    let button = button(c, style);
    if !c.children.is_empty() {
        let mut chosen = None;
        let r = ui
            .add_enabled_ui(c.enabled, |ui| {
                egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                    chosen = menu_items(ui, &c.children);
                })
            })
            .inner
            .0;
        named(r, c).on_hover_text(&tip).on_disabled_hover_text(&tip);
        return chosen;
    }
    let r = named(ui.add_enabled(c.enabled, button), c);
    r.on_hover_text(&tip)
        .on_disabled_hover_text(&tip)
        .clicked()
        .then(|| c.id.clone())
}

fn named(r: egui::Response, c: &Command) -> egui::Response {
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, c.enabled, c.selected, &c.label)
    });
    r
}

fn button(c: &Command, style: ToolbarStyle) -> egui::Button<'static> {
    let icon = |g: &str| egui::RichText::new(g).family(icon::family());
    let label = egui::RichText::new(c.label.clone());
    match (c.icon, style) {
        (Some(g), ToolbarStyle::Icons) => egui::Button::selectable(c.selected, icon(g)),
        (Some(g), ToolbarStyle::IconsAndLabels) => {
            egui::Button::selectable(c.selected, (icon(g), label))
        }
        _ => egui::Button::selectable(c.selected, label),
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.tool_bar(|| ButtonRow);
}
