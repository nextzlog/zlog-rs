use muda::accelerator::Accelerator;
use muda::{
    CheckMenuItem, Menu as Native, MenuEvent, MenuItem as Item, PredefinedMenuItem, Submenu,
};
use std::sync::mpsc::{channel, Receiver};
use zlog_ui::chrome::{Menu, MenuBar, MenuItem, Pick};
use zlog_ui::command::Command;

#[derive(Default)]
pub struct InMenuBar {
    native: Option<Native>,
    shown: String,
    events: Option<Receiver<MenuEvent>>,
}

impl MenuBar for InMenuBar {
    fn show(&mut self, ui: &mut egui::Ui, menus: &[Menu]) -> Option<Pick> {
        let events = self.events.get_or_insert_with(|| {
            let (tx, rx) = channel();
            let ctx = ui.ctx().clone();
            MenuEvent::set_event_handler(Some(move |e| {
                let _ = tx.send(e);
                ctx.request_repaint();
            }));
            rx
        });
        let event = events.try_recv().ok();
        let key = format!("{menus:?}");
        if key != self.shown {
            let native = build(menus);
            native.init_for_nsapp();
            self.native = Some(native);
            self.shown = key;
        }
        let event = event?;
        let (source, id) = event.id().0.split_once(':')?;
        Some(Pick {
            source: source.parse().ok()?,
            id: id.to_string(),
        })
    }
}

fn build(menus: &[Menu]) -> Native {
    let native = Native::new();
    let app = Submenu::new("zLog", true);
    let _ = app.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);
    let _ = native.append(&app);
    for menu in menus {
        let sub = Submenu::new(&menu.title, true);
        for item in &menu.items {
            match item {
                MenuItem::Command(source, c) => command(&sub, *source, c),
                MenuItem::Separator => {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                }
                MenuItem::Note(text) => {
                    let _ = sub.append(&Item::new(text, false, None));
                }
            }
        }
        let _ = native.append(&sub);
    }
    native
}

fn command(parent: &Submenu, source: usize, c: &Command) {
    if !c.menu {
        return;
    }
    let id = format!("{source}:{}", c.id);
    let (text, accelerator) = match c.menu_key().map(crate::shown) {
        Some(crate::Shown::Native(mods, code)) => (
            c.menu_text().to_string(),
            Some(Accelerator::new(mods, code)),
        ),
        Some(crate::Shown::Hidden) | None => (c.menu_text().to_string(), None),
    };
    let text = text.as_str();
    let _ = if !c.children.is_empty() {
        let sub = Submenu::new(text, c.enabled);
        for child in &c.children {
            command(&sub, source, child);
        }
        parent.append(&sub)
    } else if c.checkable || c.radio {
        parent.append(&CheckMenuItem::with_id(
            id,
            text,
            c.enabled,
            c.selected,
            accelerator,
        ))
    } else {
        parent.append(&Item::with_id(id, text, c.enabled, accelerator))
    };
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.menu_bar(InMenuBar::default);
}
