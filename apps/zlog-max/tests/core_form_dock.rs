#[allow(dead_code)]
mod windows;

use egui::accesskit::Role;
use egui::{Event, PointerButton};
use egui_kittest::kittest::NodeT;
use windows::*;

fn register(b: &mut zlog_ui::Builder) {
    zlog_std::register(b);
    zlog_form::register(b);
}

fn init() {
    zlog_gui::testing::registry::install(register);
}

fn choose_area(h: &mut H, area: &str) {
    let at = h
        .root()
        .children_recursive()
        .filter(|n| {
            let a = n.accesskit_node();
            a.role() == Role::CheckBox && a.label().as_deref() == Some(area)
        })
        .map(|n| n.rect().center())
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap_or_else(|| panic!("no area {area}"));
    click_at(h, at, PointerButton::Primary);
    egui::Popup::close_all(&h.ctx);
    settle(h);
}

fn logged(h: &H) -> usize {
    h.state().log_state().map(|(_, n)| n).unwrap()
}

fn form_harness(env: &Env) -> H {
    if env.settings().is_empty() {
        env.write_settings("[preferences]\nentry_style = \"form\"\n");
    }
    harness(env)
}

#[test]
fn the_entry_window_docks_logs_a_qso_and_floats_again() {
    init();
    const FORM: &str = "Entry window";
    let env = Env::new("dock-form");
    let mut h = form_harness(&env);
    assert!(window(&h, FORM), "the entry window floats at first");
    press(&mut h, Role::Button, tr("menu.window"));
    press(&mut h, Role::Button, &format!("Dock {FORM}"));
    choose_area(&mut h, "Right panel");
    assert!(tab(&h, FORM).is_some(), "docked as a tab");
    assert!(!window(&h, FORM), "no floating window while docked");
    h.event(Event::Text("JA1AAA".into()));
    settle(&mut h);
    assert_eq!(h.state().entry_state().unwrap().1, "JA1AAA");
    h.key_press(egui::Key::Space);
    settle(&mut h);
    h.event(Event::Text("10H".into()));
    settle(&mut h);
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(logged(&h), 1, "{:?}", h.state().entry_state());
    assert_eq!(h.state().entry_state().unwrap().1, "", "cleared");
    drop(h);
    let mut h = form_harness(&env);
    assert!(
        tab(&h, FORM).is_some() && !window(&h, FORM),
        "the placement is remembered"
    );
    menu(&mut h, tr("menu.window"), &format!("Float {FORM}"));
    assert!(window(&h, FORM), "floats again");
    assert!(tab(&h, FORM).is_none());
    h.event(Event::Text("JA1BBB".into()));
    settle(&mut h);
    assert_eq!(h.state().entry_state().unwrap().1, "JA1BBB");
}

type Keys = std::rc::Rc<std::cell::RefCell<Vec<u8>>>;

struct FakeKeyer(Keys);

impl zlog_ui::Panel for FakeKeyer {
    fn id(&self) -> &str {
        "fake_keyer"
    }

    fn title(&self, _language: &str) -> String {
        "Fake keyer".into()
    }

    fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}

    fn key_messages(&self, _view: &zlog_ui::View<'_>) -> Option<Vec<String>> {
        let mut m = vec![String::new(); 12];
        m[0] = "CQ TEST JA1ZLO".into();
        m[1] = "5NN 1317M".into();
        Some(m)
    }

    fn on_key(&mut self, key: zlog_ui::EntryKey, _view: &zlog_ui::View<'_>) -> bool {
        match key {
            zlog_ui::EntryKey::Function { n, .. } | zlog_ui::EntryKey::Reserved { n } => {
                self.0.borrow_mut().push(n);
                true
            }
            _ => false,
        }
    }
}

fn sends(h: &mut H, keys: &Keys, k: egui::Key) -> Vec<u8> {
    h.event(Event::Text("JA1CCC".into()));
    settle(h);
    h.key_press(k);
    settle(h);
    std::mem::take(&mut *keys.borrow_mut())
}

#[test]
fn function_keys_send_from_the_entry_window_floating_and_docked() {
    init();
    const FORM: &str = "Entry window";
    let env = Env::new("form-keys");
    let mut h = form_harness(&env);
    let keys = Keys::default();
    h.state_mut().push_panel(Box::new(FakeKeyer(keys.clone())));
    settle(&mut h);
    assert!(window(&h, FORM));
    assert_eq!(sends(&mut h, &keys, egui::Key::F1), [1], "F1 floating");
    press(&mut h, Role::Button, tr("menu.window"));
    press(&mut h, Role::Button, &format!("Dock {FORM}"));
    choose_area(&mut h, "Right panel");
    assert!(tab(&h, FORM).is_some());
    key_escape(&mut h);
    assert_eq!(sends(&mut h, &keys, egui::Key::F2), [2], "F2 docked");
}

fn key_escape(h: &mut H) {
    h.key_press(egui::Key::Escape);
    settle(h);
}

#[test]
fn the_message_buttons_name_what_each_key_sends() {
    init();
    let env = Env::new("form-fkeys");
    let mut h = form_harness(&env);
    let keys = Keys::default();
    h.state_mut().push_panel(Box::new(FakeKeyer(keys.clone())));
    settle(&mut h);
    let f1 = node(&h, Role::Button, "F1\nCQ TEST JA1ZLO").expect("F1 shows its message");
    assert!(node(&h, Role::Button, "F2\n5NN 1317M").is_some());
    assert!(node(&h, Role::Button, "F3\n").is_some(), "an empty key");
    click_at(&mut h, f1.0, PointerButton::Primary);
    settle(&mut h);
    assert_eq!(
        std::mem::take(&mut *keys.borrow_mut()),
        [1],
        "a click sends F1"
    );
}
