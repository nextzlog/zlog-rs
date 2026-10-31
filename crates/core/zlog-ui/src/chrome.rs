use crate::toolbar::ToolbarStyle;
use crate::{Command, Section, StatusItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub source: usize,
    pub id: String,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum MenuItem {
    Command(usize, Command),
    Separator,
    Note(String),
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
}

pub trait MenuBar {
    fn show(&mut self, ui: &mut egui::Ui, menus: &[Menu]) -> Option<Pick>;
}

pub trait ToolBar {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        groups: &[Vec<(usize, &Command)>],
        style: ToolbarStyle,
    ) -> Option<Pick>;
}

#[derive(Debug, Clone, Default)]
pub struct StatusLine {
    pub message: StatusItem,
    pub items: Vec<StatusItem>,
    pub cq: bool,
    pub cq_text: String,
    pub count: String,
    pub contest: String,
}

pub trait StatusBar {
    fn show(&mut self, ui: &mut egui::Ui, line: &StatusLine);
}

#[derive(Debug, Clone)]
pub struct TableColumn {
    pub title: String,
    pub chars: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableRow {
    pub cells: Vec<String>,
    pub warning: bool,
}

pub struct TableView<'a> {
    pub columns: &'a [TableColumn],
    pub rows: usize,
    pub row: &'a dyn Fn(usize) -> TableRow,
    pub widths: &'a mut Vec<f32>,
    pub selected: &'a mut Option<usize>,
    pub activate: &'a mut bool,
    pub menu: &'a mut dyn FnMut(&mut egui::Ui, usize),
}

pub trait LogTable {
    fn show(&mut self, ui: &mut egui::Ui, view: TableView<'_>);
}

pub fn column_widths(ui: &egui::Ui, columns: &[TableColumn]) -> Vec<f32> {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let cw = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    columns.iter().map(|c| c.chars * cw + 6.0).collect()
}

pub struct HostedWindow<'a> {
    pub id: String,
    pub title: String,
    pub open: &'a mut bool,
    pub body: Box<dyn FnMut(&mut egui::Ui) -> bool + 'a>,
}

pub trait WindowHost {
    fn hosts(&self, id: &str) -> bool;

    fn reserve_log(&mut self, _width: f32) {}

    fn show(&mut self, ui: &mut egui::Ui, windows: &mut [HostedWindow<'_>], language: &str);

    fn floating(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        language: &str,
        body: &mut dyn FnMut(&mut egui::Ui),
    );

    fn commands(&self, windows: &[(&str, String)], language: &str) -> Vec<Command>;

    fn run(&mut self, id: &str, windows: &[(&str, String)]);

    fn focus(&mut self, id: &str) -> bool;

    fn take_changed(&mut self) -> bool;

    fn section(&self) -> &str;

    fn load(&mut self, section: &Section);

    fn save(&self, section: &mut Section);
}

pub fn menu_items(ui: &mut egui::Ui, commands: &[Command]) -> Option<String> {
    let mut chosen = None;
    for c in commands.iter().filter(|c| c.menu) {
        if !c.children.is_empty() {
            ui.add_enabled_ui(c.enabled, |ui| {
                ui.menu_button(c.menu_text(), |ui| {
                    if let Some(id) = menu_items(ui, &c.children) {
                        chosen = Some(id);
                    }
                })
            })
            .response
            .on_disabled_hover_text(c.full_tooltip(None));
            continue;
        }
        let r = if c.radio || c.checkable {
            let toggle = |ui: &mut egui::Ui| {
                if c.radio {
                    ui.add_enabled(c.enabled, egui::RadioButton::new(c.selected, c.menu_text()))
                } else {
                    let mut on = c.selected;
                    ui.add_enabled(c.enabled, egui::Checkbox::new(&mut on, c.menu_text()))
                }
            };
            match c.menu_key().map(|k| ui.ctx().format_shortcut(k)) {
                None => toggle(ui),
                Some(key) => {
                    ui.horizontal(|ui| {
                        let r = toggle(ui);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_enabled(
                                c.enabled,
                                egui::Label::new(egui::RichText::new(key).weak()),
                            );
                        });
                        r
                    })
                    .inner
                }
            }
        } else {
            let mut b = egui::Button::new(c.menu_text());
            if let Some(k) = c.menu_key() {
                b = b.shortcut_text(ui.ctx().format_shortcut(k));
            }
            ui.add_enabled(c.enabled, b)
        };
        let r = match &c.disabled_reason {
            Some(why) => r.on_disabled_hover_text(why),
            None => r,
        };
        if r.clicked() {
            chosen = Some(c.id.clone());
            if !c.checkable && !c.radio {
                ui.close();
            }
        }
    }
    chosen
}
