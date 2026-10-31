#[allow(dead_code)]
mod windows;

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::NodeT;
use windows::*;
use zlog_core::{Mode, Power};
use zlog_ui::{EntryAction, EntryField};

fn init() {
    zlog_gui::testing::registry::install(zlog_std::register);
}

fn call_field(h: &mut H) {
    h.state_mut().focus_entry(EntryField::Call);
    settle(h);
}

fn key(h: &mut H, modifiers: Modifiers, k: Key) {
    let modifiers = Modifiers {
        ctrl: modifiers.ctrl || modifiers.command,
        ..modifiers
    };
    h.key_press_modifiers(modifiers, k);
    settle(h);
}

fn type_text(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    settle(h);
}

fn shift_letter(h: &mut H, k: Key, c: &str) {
    let events = &mut h.input_mut().events;
    events.push(Event::Key {
        key: k,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::SHIFT,
    });
    events.push(Event::Text(c.into()));
    settle(h);
}

fn entry(h: &H) -> zlog_gui::testing::entry::Entry {
    h.state().entry().expect("entry")
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn ctrl_g_recalls_the_kept_entry_without_logging() {
    init();
    let env = Env::new("recall");
    let mut h = harness_with(&env, false);
    call_field(&mut h);
    type_text(&mut h, "JA1ZLO");
    key(&mut h, Modifiers::COMMAND, Key::U);
    key(&mut h, Modifiers::ALT, Key::W);
    assert_eq!(entry(&h).callsign, "");
    key(&mut h, Modifiers::COMMAND, Key::G);
    assert_eq!(entry(&h).callsign, "JA1ZLO");
    assert_eq!(h.state().log_state().map(|l| l.1), Some(0), "not logged");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn shift_letter_shortcuts_type_nothing() {
    init();
    let env = Env::new("shift");
    let mut h = harness_with(&env, false);
    call_field(&mut h);
    type_text(&mut h, "JA1YWX");
    let band = entry(&h).band;
    for (k, c) in [(Key::B, "B"), (Key::M, "M"), (Key::P, "P"), (Key::Z, "Z")] {
        shift_letter(&mut h, k, c);
    }
    let e = entry(&h);
    assert_eq!(e.callsign, "JA1YWX");
    assert_ne!(e.band, band, "Shift+B still acts");
    call_field(&mut h);
    type_text(&mut h, "A");
    assert_eq!(entry(&h).callsign, "JA1YWXA", "typing goes on afterwards");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn ctrl_shift_p_steps_the_power_back() {
    init();
    let env = Env::new("power");
    let mut h = harness_with(&env, false);
    let mut e = entry(&h);
    e.power = Power::P010;
    h.state_mut().set_entry(e);
    call_field(&mut h);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::P);
    assert_eq!(entry(&h).power, Power::P010.prev_code());
    key(&mut h, Modifiers::SHIFT, Key::P);
    assert_eq!(entry(&h).power, Power::P010);
    assert_eq!(entry(&h).callsign, "");
    key(&mut h, Modifiers::SHIFT, Key::P);
    assert_eq!(entry(&h).power, Power::P001, "zLog wraps H to P");
    let mut e = entry(&h);
    e.power = Power::P020;
    h.state_mut().set_entry(e);
    key(&mut h, Modifiers::SHIFT, Key::P);
    assert_eq!(entry(&h).power, Power::P025, "zLog steps 20 W to 25 W");
    assert_eq!(entry(&h).callsign, "");
}

struct Decoder {
    actions: Vec<EntryAction>,
}

impl zlog_ui::Panel for Decoder {
    fn id(&self) -> &str {
        "decoder"
    }

    fn title(&self, _language: &str) -> String {
        "Decoder".into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {
        ui.label("Decoder body");
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn a_received_rst_fills_the_zlog_row() {
    init();
    let env = Env::new("rst");
    let mut h = harness_with(&env, false);
    let mut e = entry(&h);
    e.mode = Mode::Cw;
    e.rst_rcvd = "599".into();
    h.state_mut().set_entry(e);
    call_field(&mut h);
    type_text(&mut h, "JA1ZLO");
    h.state_mut().push_panel(Box::new(Decoder {
        actions: vec![EntryAction::RstRcvd(59), EntryAction::RstRcvd(579)],
    }));
    settle(&mut h);
    let e = entry(&h);
    assert_eq!(e.rst_rcvd, "579", "a two-digit report is no CW report");
    assert_eq!(e.callsign, "JA1ZLO");
    assert!(has_text(&h, "579"), "the row shows it");
    assert_eq!(h.state().entry_field(&h.ctx), Some(EntryField::Call));
}

fn choices(h: &H) -> Vec<(String, egui::Rect)> {
    h.root()
        .children_recursive()
        .filter(|n| {
            let a = n.accesskit_node();
            a.role() == egui::accesskit::Role::Button && a.toggled().is_some() && !a.is_hidden()
        })
        .map(|n| (n.accesskit_node().label().unwrap_or_default(), n.rect()))
        .collect()
}

fn open_choice(h: &mut H, label: &str) -> Vec<String> {
    let before = choices(h).len();
    press(h, egui::accesskit::Role::Button, label);
    let shown = choices(h);
    assert!(shown.len() > before, "{label:?} opens a list");
    let screen = h.ctx.content_rect();
    for (text, r) in &shown[before..] {
        assert!(screen.contains_rect(*r), "{text:?} clipped: {r:?}");
        assert!(!text.is_empty(), "item has a label");
    }
    shown[before..].iter().map(|(t, _)| t.clone()).collect()
}

fn cycled<T: PartialEq + Copy>(h: &mut H, k: Key, get: impl Fn(&H) -> T) -> Vec<T> {
    let first = get(h);
    let mut seen = vec![first];
    for _ in 0..64 {
        key(h, Modifiers::COMMAND | Modifiers::SHIFT, k);
        let now = get(h);
        if now == first {
            break;
        }
        seen.push(now);
    }
    seen
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn band_mode_and_power_open_a_list_of_the_cycled_values() {
    init();
    let env = Env::new("choice");
    let mut h = harness_with(&env, false);
    call_field(&mut h);
    type_text(&mut h, "JA1ZLO");

    let mut bands = cycled(&mut h, Key::B, |h| entry(h).band);
    let start = bands[0];
    let mut listed = open_choice(&mut h, start.label());
    listed.sort();
    let mut want: Vec<String> = bands.iter().map(|b| b.label().to_string()).collect();
    want.sort();
    assert_eq!(listed, want, "band list");
    bands.rotate_left(1);
    let target = bands[0];
    press(&mut h, egui::accesskit::Role::Button, target.label());
    assert_eq!(entry(&h).band, target);
    assert_eq!(h.state().entry_field(&h.ctx), Some(EntryField::Call));
    type_text(&mut h, "A");
    assert_eq!(entry(&h).callsign, "JA1ZLOA", "focus is back");

    let modes = cycled(&mut h, Key::M, |h| entry(h).mode);
    let listed = open_choice(&mut h, modes[0].label());
    let want: Vec<String> = modes.iter().map(|m| m.label().to_string()).collect();
    let mut sorted = (listed.clone(), want.clone());
    sorted.0.sort();
    sorted.1.sort();
    assert_eq!(sorted.0, sorted.1, "mode list");
    let target = *modes.last().unwrap();
    press(&mut h, egui::accesskit::Role::Button, target.label());
    assert_eq!(entry(&h).mode, target);
    assert_eq!(h.state().entry_field(&h.ctx), Some(EntryField::Call));

    let mut e = entry(&h);
    e.power = Power::P010;
    h.state_mut().set_entry(e);
    settle(&mut h);
    let listed = open_choice(&mut h, "H");
    assert_eq!(listed, ["P (QRP)", "L (Low)", "M (Medium)", "H (High)"]);
    press(&mut h, egui::accesskit::Role::Button, "M (Medium)");
    assert_eq!(entry(&h).power, Power::P005);
    assert_eq!(h.state().entry_field(&h.ctx), Some(EntryField::Call));
    shift_letter(&mut h, Key::P, "P");
    assert_eq!(entry(&h).power, Power::P010, "Shift+P still cycles");
}
