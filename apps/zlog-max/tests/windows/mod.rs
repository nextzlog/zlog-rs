use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;

use zlog_gui::testing::app::ZLogApp;
pub use zlog_gui::testing::i18n::tr;
use zlog_gui::testing::session::Setup;

pub type H = Harness<'static, ZLogApp>;

static SERIAL: Mutex<()> = Mutex::new(());

pub struct Env {
    pub dir: PathBuf,
    _serial: MutexGuard<'static, ()>,
}

impl Env {
    pub fn new(name: &str) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        zlog_gui::testing::i18n::set_language("en");
        zlog_net::set_offline(true);
        let dir = std::env::temp_dir().join(format!("zlog-win-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        zlog_gui::testing::settings::TEST_PATH
            .with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
        Env {
            dir,
            _serial: serial,
        }
    }

    pub fn settings(&self) -> String {
        std::fs::read_to_string(self.dir.join("settings.toml")).unwrap_or_default()
    }

    pub fn write_settings(&self, text: &str) {
        std::fs::write(self.dir.join("settings.toml"), text).unwrap();
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        zlog_gui::testing::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub struct ToolPanel {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: zlog_ui::WindowKind,
}

impl zlog_ui::Panel for ToolPanel {
    fn id(&self) -> &str {
        self.id
    }

    fn title(&self, _language: &str) -> String {
        self.title.into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        self.kind
    }

    fn ui(&mut self, ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {
        ui.label(format!("{} body", self.title));
    }
}

pub fn harness(env: &Env) -> H {
    harness_with(env, true)
}

pub fn harness_with(env: &Env, tools: bool) -> H {
    let mut h = Harness::builder()
        .with_size([1200.0, 800.0])
        .with_accessibility_check(false)
        .allow_missing_glyphs()
        .with_step_dt(0.05)
        .build_eframe(|_| ZLogApp::new());
    let panels = [
        ("alpha", "Alpha", zlog_ui::WindowKind::Tool),
        ("beta", "Beta", zlog_ui::WindowKind::Tool),
        ("delta", "Delta", zlog_ui::WindowKind::Tool),
        ("omega", "Omega", zlog_ui::WindowKind::Tool),
        ("gamma", "Gamma", zlog_ui::WindowKind::Dialog),
    ];
    for (id, title, kind) in panels.into_iter().filter(|_| tools) {
        h.state_mut()
            .push_panel(Box::new(ToolPanel { id, title, kind }));
    }
    let setup = Setup {
        contest: 0,
        contest_file: None,
        mode: zlog_core::ContestMode::Mix,
        station: zlog_core::Station {
            callsign: "JA1ZLO".into(),
            ..Default::default()
        },
        coeff: 1.0,
        path: env.dir.join("windows.zlox"),
        use_contest_period: true,
        convert_old_other: false,
    };
    h.state_mut().open_setup(setup);
    settle(&mut h);
    assert!(h.state().log_state().is_some(), "log open");
    h
}

pub fn settle(h: &mut H) {
    h.run_steps(3);
}

pub fn window_of(n: &egui_kittest::Node<'_>) -> Option<String> {
    let mut p = n.parent();
    while let Some(w) = p {
        if w.accesskit_node().role() == Role::Window {
            return w.accesskit_node().label();
        }
        p = w.parent();
    }
    None
}

pub fn node(h: &H, role: Role, label: &str) -> Option<(Pos2, bool, Option<String>)> {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.role() == role && named(a.label().as_deref(), label) && !a.is_hidden()
        })
        .map(|n| {
            (
                n.rect().center(),
                !n.accesskit_node().is_disabled(),
                window_of(&n),
            )
        })
}

pub fn has_text(h: &H, text: &str) -> bool {
    h.root().children_recursive().any(|n| {
        let a = n.accesskit_node();
        a.label().as_deref() == Some(text) || a.value().as_deref() == Some(text)
    })
}

pub fn window(h: &H, title: &str) -> bool {
    node(h, Role::Window, title).is_some()
}

pub fn tab(h: &H, title: &str) -> Option<Pos2> {
    node(h, Role::Tab, title).map(|(p, _, _)| p)
}

pub fn press(h: &mut H, role: Role, label: &str) {
    {
        let n = h
            .root()
            .children_recursive()
            .find(|n| {
                let a = n.accesskit_node();
                a.role() == role && named(a.label().as_deref(), label) && !a.is_disabled()
            })
            .unwrap_or_else(|| panic!("no {role:?} {label:?}"));
        n.click_accesskit();
    }
    settle(h);
}

pub fn menu(h: &mut H, title: &str, item: &str) {
    press(h, Role::Button, title);
    let role = [Role::Button, Role::MenuItem, Role::CheckBox]
        .into_iter()
        .find(|r| node(h, *r, item).is_some())
        .unwrap_or_else(|| panic!("no menu item {item:?}"));
    press(h, role, item);
    egui::Popup::close_all(&h.ctx);
    settle(h);
}

pub fn menu_item(h: &mut H, title: &str, item: &str) -> Option<bool> {
    press(h, Role::Button, title);
    let found = [Role::Button, Role::MenuItem]
        .into_iter()
        .find_map(|r| node(h, r, item))
        .map(|(_, enabled, _)| enabled);
    egui::Popup::close_all(&h.ctx);
    settle(h);
    found
}

pub fn open(h: &mut H, title: &str) {
    menu(h, tr("menu.group.tools"), title);
}

pub fn click_at(h: &mut H, pos: Pos2, button: PointerButton) {
    h.event(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    settle(h);
}

pub fn text_pos(h: &H, text: &str) -> Pos2 {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.role() == Role::Label && a.value().as_deref() == Some(text)
        })
        .map(|n| n.rect().center())
        .unwrap_or_else(|| panic!("no text {text:?}"))
}

fn named(text: Option<&str>, label: &str) -> bool {
    text.is_some_and(|t| t == label || t.strip_suffix('…') == Some(label))
}
