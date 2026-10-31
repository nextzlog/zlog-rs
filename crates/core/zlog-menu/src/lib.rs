use zlog_ui::chrome::{menu_items, Menu, MenuBar, MenuItem, Pick};

#[derive(Default)]
pub struct InWindow;

impl MenuBar for InWindow {
    fn show(&mut self, ui: &mut egui::Ui, menus: &[Menu]) -> Option<Pick> {
        let mut chosen = None;
        egui::MenuBar::new().ui(ui, |ui| {
            for menu in menus {
                ui.menu_button(&menu.title, |ui| {
                    if let Some(p) = items(ui, &menu.items) {
                        chosen = Some(p);
                    }
                });
            }
        });
        chosen
    }
}

pub fn items(ui: &mut egui::Ui, items: &[MenuItem]) -> Option<Pick> {
    let mut chosen = None;
    for item in items {
        match item {
            MenuItem::Command(source, c) => {
                if let Some(id) = menu_items(ui, std::slice::from_ref(c)) {
                    chosen = Some(Pick {
                        source: *source,
                        id,
                    });
                }
            }
            MenuItem::Separator => {
                ui.separator();
            }
            MenuItem::Note(text) => {
                ui.weak(text);
            }
        }
    }
    chosen
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.menu_bar(|| InWindow);
}
