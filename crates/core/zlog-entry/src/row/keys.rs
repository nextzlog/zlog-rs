use egui::{Event, Key, Modifiers, RawInput};
use zlog_core::{Band, DelphiTime, Mode, Power};
use zlog_ui::{EntryCaps, EntryEnv, EntryField, EntryFields, EntryOps, EntryStyle, NotLogged};
use zlog_ui::{LogOptions, Section};

use super::*;

#[derive(Default)]
struct Fake {
    texts: std::collections::HashMap<u8, String>,
    band: Option<Band>,
    mode: Option<Mode>,
    power: Option<Power>,
    sent: Vec<u8>,
    logged: Vec<String>,
    forced: usize,
    tabbed: usize,
    roles: Vec<String>,
    bad_number: bool,
    no_time: bool,
    cycled: Vec<(&'static str, bool)>,
}

impl EntryFields for Fake {
    fn text(&self, field: EntryField) -> &str {
        self.texts.get(&(field as u8)).map_or("", String::as_str)
    }
    fn set_text(&mut self, field: EntryField, text: String) {
        self.texts.insert(field as u8, text);
    }
    fn band(&self) -> Band {
        self.band.unwrap_or(Band::B7)
    }
    fn mode(&self) -> Mode {
        self.mode.unwrap_or(Mode::Cw)
    }
    fn power(&self) -> Power {
        self.power.unwrap_or(Power::P010)
    }
    fn cq(&self) -> bool {
        false
    }
    fn operator(&self) -> &str {
        ""
    }
    fn sent(&self) -> String {
        String::new()
    }
}

impl EntryOps for Fake {
    fn cycle_band(&mut self, back: bool) {
        self.cycled.push(("band", back));
    }
    fn cycle_mode(&mut self, back: bool) {
        self.cycled.push(("mode", back));
    }
    fn set_band(&mut self, band: Band) {
        self.band = Some(band);
    }
    fn set_mode(&mut self, mode: Mode) {
        self.mode = Some(mode);
    }
    fn set_power(&mut self, power: Power) {
        self.power = Some(power);
    }
    fn cycle_power(&mut self, back: bool) {
        self.cycled.push(("power", back));
    }
    fn toggle_cq(&mut self) {}
    fn bump_s(&mut self, field: EntryField, up: bool) {
        let t = format!("{}{}", self.text(field), if up { "+" } else { "-" });
        self.set_text(field, t);
    }
    fn clear(&mut self) {
        self.texts.clear();
    }
    fn complete_call(&mut self) -> bool {
        false
    }
    fn check(&mut self) -> Result<(), NotLogged> {
        if self.bad_number {
            Err(NotLogged::InvalidNumber)
        } else {
            Ok(())
        }
    }
    fn log_with(&mut self, _options: LogOptions) -> Result<(), NotLogged> {
        self.check()?;
        self.log_forced()?;
        self.forced -= 1;
        Ok(())
    }
    fn log_forced(&mut self) -> Result<(), NotLogged> {
        let call = self.text(EntryField::Call).to_string();
        if call.is_empty() {
            return Err(NotLogged::NoCallsign);
        }
        self.logged.push(call);
        self.forced += 1;
        self.texts.clear();
        Ok(())
    }
}

impl EntryEnv for Fake {
    fn caps(&self) -> EntryCaps {
        EntryCaps {
            time: !self.no_time,
            ..EntryCaps::default()
        }
    }
    fn now(&self) -> DelphiTime {
        DelphiTime::from_ymd_hms(2026, 10, 24, 12, 0, 30)
    }
    fn qso_count(&self) -> usize {
        self.logged.len()
    }
    fn send(&mut self, n: u8) {
        self.sent.push(n);
    }
    fn tab_sent(&mut self) {
        self.tabbed += 1;
    }
    fn run_role(&mut self, role: &str) {
        self.roles.push(role.into());
    }
}

fn press(
    row: &mut ZlogRow,
    e: &mut Fake,
    field: Option<EntryField>,
    modifiers: Modifiers,
    key: Key,
) {
    send(row, e, field, vec![key_event(modifiers, key)]);
}

fn key_event(modifiers: Modifiers, key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn send(row: &mut ZlogRow, e: &mut Fake, field: Option<EntryField>, events: Vec<Event>) {
    let ctx = egui::Context::default();
    let input = RawInput {
        events,
        ..RawInput::default()
    };
    ctx.begin_pass(input);
    if let Some(f) = field {
        ctx.memory_mut(|m| m.request_focus(id(f)));
    }
    row.keys(&ctx, e);
    ctx.end_pass().textures_delta.clear();
}

fn call(e: &mut Fake, c: &str) {
    e.set_text(EntryField::Call, c.into());
}

const CALL: Option<EntryField> = Some(EntryField::Call);

#[test]
fn shift_r_and_shift_s_lower_the_rst_of_the_focused_side() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    e.set_text(EntryField::RstSent, "599".into());
    e.set_text(EntryField::RstRcvd, "59".into());
    press(
        &mut r,
        &mut e,
        Some(EntryField::RstSent),
        Modifiers::SHIFT,
        Key::R,
    );
    assert_eq!(e.text(EntryField::RstSent), "499");
    press(&mut r, &mut e, CALL, Modifiers::SHIFT, Key::R);
    assert_eq!(e.text(EntryField::RstRcvd), "49");
    press(
        &mut r,
        &mut e,
        Some(EntryField::RstSent),
        Modifiers::SHIFT,
        Key::S,
    );
    assert_eq!(e.text(EntryField::RstSent), "499-");
    press(&mut r, &mut e, CALL, Modifiers::SHIFT, Key::S);
    assert_eq!(e.text(EntryField::RstRcvd), "49-");
    e.set_text(EntryField::RstRcvd, "59".into());
    press(
        &mut r,
        &mut e,
        Some(EntryField::Memo),
        Modifiers::SHIFT,
        Key::R,
    );
    assert_eq!(e.text(EntryField::RstRcvd), "59", "the memo types capitals");
}

#[test]
fn r_goes_down_and_wraps_to_five() {
    assert_eq!(decrease_r("599").as_deref(), Some("499"));
    assert_eq!(decrease_r("199").as_deref(), Some("599"));
    assert_eq!(decrease_r("59").as_deref(), Some("49"));
    assert_eq!(decrease_r("19").as_deref(), Some("59"));
    assert_eq!(decrease_r(""), None);
}

#[test]
fn alt_o_asks_for_the_operator() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    press(&mut r, &mut e, CALL, Modifiers::ALT, Key::O);
    assert_eq!(e.roles, [OPERATOR_ROLE]);
}

#[test]
fn keyer_keys_ask_for_their_roles() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    for (m, k) in [
        (Modifiers::CTRL, Key::Z),
        (Modifiers::SHIFT, Key::Z),
        (Modifiers::SHIFT, Key::Y),
        (Modifiers::SHIFT, Key::U),
        (Modifiers::ALT, Key::K),
    ] {
        press(&mut r, &mut e, CALL, m, k);
    }
    assert_eq!(
        e.roles,
        [
            CQ_REPEAT_ROLE,
            CQ_HOLD_ROLE,
            SPEED_UP_ROLE,
            SPEED_DOWN_ROLE,
            KEYBOARD_ROLE
        ]
    );
    e.roles.clear();
    press(
        &mut r,
        &mut e,
        Some(EntryField::Memo),
        Modifiers::SHIFT,
        Key::Z,
    );
    assert!(e.roles.is_empty(), "the memo types capitals");
    press(
        &mut r,
        &mut e,
        Some(EntryField::Memo),
        Modifiers::CTRL,
        Key::Z,
    );
    assert_eq!(e.roles, [CQ_REPEAT_ROLE]);
}

#[test]
fn emacs_keys_edit_the_focused_field() {
    let t = "JA1ZLO";
    assert_eq!(apply(EditOp::Home, t, 3), (t.into(), 0));
    assert_eq!(apply(EditOp::End, t, 3), (t.into(), 6));
    assert_eq!(apply(EditOp::Left, t, 3), (t.into(), 2));
    assert_eq!(apply(EditOp::Left, t, 0), (t.into(), 0));
    assert_eq!(apply(EditOp::Right, t, 3), (t.into(), 4));
    assert_eq!(apply(EditOp::Right, t, 6), (t.into(), 6));
    assert_eq!(apply(EditOp::DeleteAt, t, 3), ("JA1LO".into(), 3));
    assert_eq!(apply(EditOp::DeleteAt, t, 6), (t.into(), 6));
    assert_eq!(apply(EditOp::DeleteLeft, t, 3), ("JAZLO".into(), 2));
    assert_eq!(apply(EditOp::DeleteLeft, t, 0), (t.into(), 0));
    assert_eq!(apply(EditOp::DeleteRight, t, 3), ("JA1".into(), 3));
    assert_eq!(apply(EditOp::Clear, t, 3), (String::new(), 0));
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    for (key, op) in EDIT_KEYS {
        press(
            &mut r,
            &mut e,
            Some(EntryField::Number),
            Modifiers::CTRL,
            key,
        );
        assert_eq!(r.edit_op, Some((EntryField::Number, op)), "{key:?}");
    }
    r.edit_op = None;
    for key in [Key::A, Key::H, Key::W, Key::Z, Key::K] {
        let mac_cmd = Modifiers::MAC_CMD | Modifiers::COMMAND;
        press(&mut r, &mut e, Some(EntryField::Number), mac_cmd, key);
        assert_eq!(r.edit_op, None, "{key:?}");
    }
    assert!(e.roles.is_empty());
}

#[test]
fn ctrl_u_keeps_five_and_ctrl_g_recalls_without_logging() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    for c in ["JA1A", "JA1B", "JA1C", "JA1D", "JA1E", "JA1F"] {
        call(&mut e, c);
        press(&mut r, &mut e, CALL, Modifiers::CTRL, Key::U);
    }
    assert_eq!(r.kept.len(), 5);
    e.set_band(Band::B14);
    e.set_text(EntryField::Time, "11:00".into());
    call(&mut e, "");
    press(&mut r, &mut e, CALL, Modifiers::CTRL, Key::G);
    assert!(e.logged.is_empty(), "zLog PullQSO only fills the entry");
    assert_eq!(e.text(EntryField::Call), "JA1F");
    assert_eq!(e.text(EntryField::Time), "", "the clock again");
    assert_eq!(e.band(), Band::B7);
    press(&mut r, &mut e, CALL, Modifiers::CTRL, Key::G);
    assert_eq!(e.text(EntryField::Call), "JA1E");
    assert_eq!(r.kept.len(), 5, "a recalled entry goes to the back");
}

#[test]
fn shift_f_switches_the_cw_bank() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    press(&mut r, &mut e, CALL, Modifiers::SHIFT, Key::F);
    assert_eq!(e.roles, [BANK_ROLE]);
    press(
        &mut r,
        &mut e,
        Some(EntryField::Memo),
        Modifiers::SHIFT,
        Key::F,
    );
    assert_eq!(e.roles, [BANK_ROLE], "the memo types capitals");
}

#[test]
fn ctrl_shift_b_and_m_go_back() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    let back = Modifiers::CTRL | Modifiers::SHIFT;
    press(&mut r, &mut e, CALL, back, Key::B);
    press(&mut r, &mut e, CALL, back, Key::M);
    press(&mut r, &mut e, CALL, Modifiers::SHIFT, Key::B);
    assert_eq!(e.cycled, [("band", true), ("mode", true), ("band", false)]);
}

#[test]
fn ctrl_shift_p_goes_back_and_shift_p_forward() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    press(
        &mut r,
        &mut e,
        CALL,
        Modifiers::CTRL | Modifiers::SHIFT,
        Key::P,
    );
    press(&mut r, &mut e, CALL, Modifiers::SHIFT, Key::P);
    assert_eq!(e.cycled, [("power", true), ("power", false)]);
}

#[test]
fn alt_w_clears_the_entry_and_returns_to_the_call() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    call(&mut e, "JA1ZLO");
    e.set_text(EntryField::Number, "100".into());
    press(
        &mut r,
        &mut e,
        Some(EntryField::Number),
        Modifiers::ALT,
        Key::W,
    );
    assert_eq!(e.text(EntryField::Call), "");
    assert_eq!(e.text(EntryField::Number), "");
    assert_eq!(r.focus, Some(EntryField::Call));
}

#[test]
fn ctrl_o_ctrl_p_and_shift_t_set_the_time() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    press(&mut r, &mut e, CALL, Modifiers::CTRL, Key::O);
    assert_eq!(
        (e.text(EntryField::Date), e.text(EntryField::Time)),
        ("2026-10-24", "11:59:30")
    );
    press(&mut r, &mut e, CALL, Modifiers::CTRL, Key::P);
    press(&mut r, &mut e, CALL, Modifiers::CTRL, Key::P);
    assert_eq!(e.text(EntryField::Time), "12:01:30");
    press(&mut r, &mut e, CALL, Modifiers::SHIFT, Key::T);
    assert_eq!(e.text(EntryField::Time), "", "the clock again");
    e.no_time = true;
    press(&mut r, &mut e, CALL, Modifiers::CTRL, Key::P);
    assert_eq!(e.text(EntryField::Time), "");
}

#[test]
fn tab_sends_f2_and_marks_the_call() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    call(&mut e, "JA1ZLO");
    press(&mut r, &mut e, CALL, Modifiers::NONE, Key::Tab);
    assert_eq!((e.sent.as_slice(), e.tabbed), ([2].as_slice(), 1));
    assert_eq!(r.focus, Some(EntryField::Number));
}

#[test]
fn down_and_semicolon_thank_and_log() {
    for key in [Key::ArrowDown, Key::Semicolon] {
        let mut r = ZlogRow::default();
        let mut e = Fake::default();
        call(&mut e, "JA1ZLO");
        e.bad_number = true;
        press(&mut r, &mut e, CALL, Modifiers::NONE, key);
        assert_eq!(e.sent, [5], "NR?");
        e.bad_number = false;
        press(
            &mut r,
            &mut e,
            Some(EntryField::Number),
            Modifiers::NONE,
            key,
        );
        assert_eq!(e.sent, [5, 3]);
        r.messages_sent(&mut e);
        assert_eq!(e.logged, ["JA1ZLO"]);
    }
}

#[test]
fn shift_enter_logs_by_force() {
    let mut r = ZlogRow::default();
    r.load(&Section::new());
    let mut e = Fake {
        bad_number: true,
        ..Fake::default()
    };
    call(&mut e, "JA1ZLO");
    press(
        &mut r,
        &mut e,
        Some(EntryField::Number),
        Modifiers::NONE,
        Key::Enter,
    );
    assert!(e.logged.is_empty());
    press(
        &mut r,
        &mut e,
        Some(EntryField::Number),
        Modifiers::SHIFT,
        Key::Enter,
    );
    assert_eq!(
        (e.logged.as_slice(), e.forced),
        (["JA1ZLO".to_string()].as_slice(), 1)
    );
}

#[test]
fn every_new_key_is_reserved() {
    let keys = ZlogRow::default().reserved_keys();
    let k = egui::KeyboardShortcut::new;
    for s in [
        k(Modifiers::SHIFT, Key::R),
        k(Modifiers::SHIFT, Key::S),
        k(Modifiers::SHIFT, Key::T),
        k(Modifiers::SHIFT, Key::Enter),
        k(Modifiers::ALT, Key::O),
        k(Modifiers::NONE, Key::Tab),
        k(Modifiers::NONE, Key::ArrowDown),
        k(Modifiers::NONE, Key::Semicolon),
        k(Modifiers::CTRL, Key::A),
        k(Modifiers::CTRL, Key::E),
        k(Modifiers::CTRL, Key::B),
        k(Modifiers::CTRL, Key::F),
        k(Modifiers::CTRL, Key::D),
        k(Modifiers::CTRL, Key::H),
        k(Modifiers::CTRL, Key::J),
        k(Modifiers::CTRL, Key::W),
        k(Modifiers::CTRL, Key::U),
        k(Modifiers::CTRL, Key::G),
        k(Modifiers::CTRL, Key::O),
        k(Modifiers::CTRL, Key::P),
        k(Modifiers::CTRL, Key::Z),
        k(Modifiers::CTRL | Modifiers::SHIFT, Key::P),
        k(Modifiers::SHIFT, Key::Z),
        k(Modifiers::SHIFT, Key::Y),
        k(Modifiers::SHIFT, Key::U),
        k(Modifiers::ALT, Key::K),
        k(Modifiers::ALT, Key::W),
        k(Modifiers::SHIFT, Key::F),
        k(Modifiers::CTRL | Modifiers::SHIFT, Key::B),
        k(Modifiers::CTRL | Modifiers::SHIFT, Key::M),
    ] {
        assert!(keys.contains(&s), "{s:?}");
    }
}

#[test]
fn shift_down_in_rst_lowers_s() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    call(&mut e, "JA1ZLO");
    e.set_text(EntryField::RstRcvd, "59".into());
    press(
        &mut r,
        &mut e,
        Some(EntryField::RstRcvd),
        Modifiers::SHIFT,
        Key::ArrowUp,
    );
    let up = e.text(EntryField::RstRcvd).to_string();
    press(
        &mut r,
        &mut e,
        Some(EntryField::RstRcvd),
        Modifiers::SHIFT,
        Key::ArrowDown,
    );
    assert_ne!(e.text(EntryField::RstRcvd), up);
    assert!(e.sent.is_empty(), "{:?}", e.sent);
}

#[test]
fn shift_tab_moves_to_the_previous_field() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    call(&mut e, "JA1ZLO");
    press(
        &mut r,
        &mut e,
        Some(EntryField::Number),
        Modifiers::SHIFT,
        Key::Tab,
    );
    assert_eq!(e.tabbed, 0);
    assert!(e.sent.is_empty());
    assert_eq!(r.focus, Some(EntryField::RstRcvd));
    press(&mut r, &mut e, CALL, Modifiers::SHIFT, Key::Tab);
    assert_eq!(r.focus, Some(EntryField::Memo));
}

#[test]
fn plain_keys_ignore_shift_and_alt() {
    for (modifiers, key) in [
        (Modifiers::ALT, Key::Enter),
        (Modifiers::SHIFT, Key::Semicolon),
        (Modifiers::ALT, Key::Tab),
        (Modifiers::ALT, Key::ArrowDown),
    ] {
        let mut r = ZlogRow::default();
        let mut e = Fake::default();
        call(&mut e, "JA1ZLO");
        e.set_text(EntryField::Number, "100".into());
        press(&mut r, &mut e, CALL, modifiers, key);
        assert!(e.sent.is_empty(), "{modifiers:?} {key:?}: {:?}", e.sent);
        assert_eq!(e.tabbed, 0);
        assert_eq!(e.qso_count(), 0);
    }
}

#[test]
fn a_semicolon_typed_with_shift_completes_the_qso() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    call(&mut e, "JA1ZLO");
    e.bad_number = true;
    send(
        &mut r,
        &mut e,
        CALL,
        vec![
            key_event(Modifiers::SHIFT, Key::Semicolon),
            Event::Text(";".into()),
        ],
    );
    assert_eq!(e.sent, [5]);
    assert_eq!(e.text(EntryField::Call), "JA1ZLO");
}

#[test]
fn a_colon_typed_on_the_semicolon_key_is_left_alone() {
    let mut r = ZlogRow::default();
    let mut e = Fake::default();
    call(&mut e, "JA1ZLO");
    send(
        &mut r,
        &mut e,
        CALL,
        vec![
            key_event(Modifiers::SHIFT, Key::Semicolon),
            Event::Text(":".into()),
        ],
    );
    assert!(e.sent.is_empty());
}

#[test]
fn shift_space_acts_like_space() {
    let run = |modifiers| {
        let mut r = ZlogRow::default();
        let mut e = Fake::default();
        call(&mut e, "JA1ZLO");
        press(&mut r, &mut e, CALL, modifiers, Key::Space);
        (e.sent, r.focus)
    };
    let plain = run(Modifiers::NONE);
    assert!(plain != (Vec::new(), None));
    assert_eq!(run(Modifiers::SHIFT), plain);
}

#[test]
fn escape_in_any_field_returns_to_the_call_field() {
    for field in FIELDS {
        let mut r = ZlogRow::default();
        let mut e = Fake::default();
        let ctx = egui::Context::default();
        let mut pass = |r: &mut ZlogRow, events: Vec<Event>| {
            let input = RawInput {
                events,
                ..RawInput::default()
            };
            ctx.run_ui(input, |ui| {
                r.ui(ui, &mut e);
                r.keys(ui.ctx(), &mut e);
            })
            .textures_delta
            .clear();
        };
        r.focus(field);
        pass(&mut r, Vec::new());
        pass(&mut r, Vec::new());
        assert_eq!(r.focused(&ctx), Some(field));
        pass(&mut r, vec![key_event(Modifiers::NONE, Key::Escape)]);
        assert_eq!(r.focused(&ctx), Some(EntryField::Call), "{field:?}");
        pass(&mut r, Vec::new());
        assert_eq!(r.focused(&ctx), Some(EntryField::Call), "{field:?}");
    }
}
