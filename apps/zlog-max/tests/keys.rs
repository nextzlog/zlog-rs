use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use egui::accesskit::{NodeId, Role};
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;
use zlog_core::{Band, Mode};
use zlog_gui::testing::app::ZLogApp;
use zlog_gui::testing::entry::Entry;
use zlog_gui::testing::i18n::tr;
use zlog_gui::testing::session::Setup;
use zlog_keyer::{Keyer, KeyerDrv};
use zlog_ui::device::{DeviceDriver, PortHub};
use zlog_ui::{EntryField, Section};

const FIELDS: [EntryField; 5] = [
    EntryField::Call,
    EntryField::RstSent,
    EntryField::RstRcvd,
    EntryField::Number,
    EntryField::Memo,
];

const PROBE: &str = "probe";

fn sent() -> Arc<Mutex<Vec<String>>> {
    static SENT: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();
    SENT.get_or_init(Arc::default).clone()
}

fn take_sent() -> Vec<String> {
    std::mem::take(&mut *sent().lock().unwrap_or_else(|e| e.into_inner()))
}

struct ProbeKeyer;

impl DeviceDriver for ProbeKeyer {
    fn id(&self) -> &str {
        PROBE
    }

    fn title(&self, _language: &str) -> String {
        PROBE.into()
    }

    fn simulated(&self) -> bool {
        true
    }
}

struct Recording(zlog_keyer_sim::SimKeyer);

impl Recording {
    fn log(&self, what: String) {
        sent().lock().unwrap_or_else(|e| e.into_inner()).push(what);
    }
}

impl Keyer for Recording {
    fn caps(&self) -> zlog_keyer::KeyerCaps {
        self.0.caps()
    }

    fn send(&mut self, text: &str) {
        self.log(format!("send {text}"));
        self.0.send(text);
    }

    fn abort(&mut self) {
        self.log("abort".into());
        self.0.abort();
    }

    fn set_wpm(&mut self, wpm: u32) {
        self.log(format!("wpm {wpm}"));
        self.0.set_wpm(wpm);
    }

    fn wpm(&self) -> u32 {
        self.0.wpm()
    }

    fn set_ptt(&mut self, on: bool) {
        self.log(format!("ptt {on}"));
        self.0.set_ptt(on);
    }

    fn busy(&self) -> bool {
        self.0.busy()
    }

    fn poll(&mut self) -> Vec<zlog_keyer::KeyerEvent> {
        self.0.poll()
    }
}

impl KeyerDrv for ProbeKeyer {
    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
        let timing = zlog_keyer::timing_of(settings);
        Ok(Box::new(Recording(zlog_keyer_sim::SimKeyer::new(timing))))
    }
}

fn register(b: &mut zlog_ui::Builder) {
    zlog_max::register(b);
    b.driver::<dyn KeyerDrv>(0, || Box::new(ProbeKeyer));
}

static SERIAL: Mutex<()> = Mutex::new(());

struct Env {
    dir: PathBuf,
    _serial: MutexGuard<'static, ()>,
}

impl Env {
    fn new(name: &str) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        zlog_gui::testing::registry::install(register);
        zlog_gui::testing::i18n::set_language("en");
        zlog_net::set_offline(true);
        let dir = std::env::temp_dir().join(format!("zlog-keys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        zlog_gui::testing::settings::TEST_PATH
            .with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
        take_sent();
        Env {
            dir,
            _serial: serial,
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        zlog_gui::testing::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

type H = Harness<'static, ZLogApp>;

fn harness() -> H {
    let mut h = Harness::builder()
        .with_size([1400.0, 1000.0])
        .with_accessibility_check(false)
        .allow_missing_glyphs()
        .with_step_dt(0.05)
        .build_eframe(|_| ZLogApp::new());
    h.run_steps(3);
    h
}

fn setup(dir: &Path) -> Setup {
    Setup {
        contest: zlog_gui::testing::registry::registry()
            .contests()
            .iter()
            .position(|c| c.id() == "allja")
            .unwrap(),
        contest_file: None,
        mode: zlog_core::ContestMode::Mix,
        station: zlog_core::Station {
            callsign: "JA1ZLO".into(),
            ..Default::default()
        },
        coeff: 1.0,
        path: dir.join("allja.zlox"),
        use_contest_period: true,
        convert_old_other: false,
    }
}

fn node_id(n: &egui_kittest::Node<'_>) -> NodeId {
    n.accesskit_node().locate().0
}

fn click(h: &mut H, label: &str) -> bool {
    let found = {
        let n = h.root().children_recursive().find(|n| {
            let l = n.accesskit_node().label().unwrap_or_default();
            l == label
                || l.strip_prefix(label)
                    .is_some_and(|r| r.starts_with(' ') || r.starts_with('…'))
        });
        if let Some(n) = &n {
            n.click();
        }
        n.is_some()
    };
    h.run_steps(5);
    found
}

fn text_inputs(h: &H) -> HashSet<NodeId> {
    h.root()
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == Role::TextInput)
        .map(|n| node_id(&n))
        .collect()
}

fn focus_node(h: &mut H, id: NodeId) {
    {
        let n = h
            .root()
            .children_recursive()
            .find(|n| node_id(n) == id)
            .expect("text field");
        n.focus();
    }
    h.run_steps(2);
}

fn press(h: &mut H, modifiers: Modifiers, key: Key, text: Option<&str>) {
    let modifiers = Modifiers {
        ctrl: modifiers.ctrl || modifiers.command,
        ..modifiers
    };
    let mut events = vec![Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }];
    events.extend(text.map(|t| Event::Text(t.into())));
    events.push(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    });
    h.input_mut().events.extend(events);
    h.run_steps(3);
}

fn type_text(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    h.run_steps(3);
}

fn field(h: &H) -> Option<EntryField> {
    h.state().entry_field(&h.ctx)
}

fn entry(h: &H) -> Entry {
    h.state().entry().expect("log open")
}

fn open_log(h: &mut H, dir: &Path) {
    h.state_mut().open_setup(setup(dir));
    h.run_steps(5);
    assert!(click(h, tr("button.ok")), "start time asked");
    h.run_steps(3);
}

fn focus(h: &mut H, f: EntryField) {
    h.state_mut().focus_entry(f);
    h.run_steps(3);
    assert_eq!(field(h), Some(f));
}

fn filled() -> Entry {
    let mut e = Entry::new(Band::B7, Mode::Cw, zlog_core::Power::P010);
    e.callsign = "JA1ABC".into();
    e.nr_rcvd = "10H".into();
    e.memo = "MEMO".into();
    e
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn typing_works_right_after_opening_a_log() {
    let env = Env::new("open");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    assert_eq!(field(&h), Some(EntryField::Call));
    type_text(&mut h, "JA7XYZ");
    assert_eq!(entry(&h).callsign, "JA7XYZ");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn focus_comes_back_after_a_dialog() {
    let env = Env::new("dialog");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    focus(&mut h, EntryField::Number);
    assert!(click(&mut h, tr("menu.file")));
    assert!(click(&mut h, tr("menu.file.start_time")));
    assert!(click(&mut h, tr("button.cancel")), "start time asked");
    assert_eq!(field(&h), Some(EntryField::Number));
    type_text(&mut h, "11");
    assert_eq!(entry(&h).nr_rcvd, "11");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn escape_returns_to_the_call_field_from_every_field() {
    let env = Env::new("escape");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    for f in FIELDS {
        h.state_mut().set_entry(filled());
        focus(&mut h, f);
        press(&mut h, Modifiers::NONE, Key::Escape, None);
        assert_eq!(field(&h), Some(EntryField::Call), "Esc in {f:?}");
        type_text(&mut h, "Q");
        let call = entry(&h).callsign;
        assert!(
            call.len() == 7 && call.contains('Q'),
            "typing after Esc in {f:?}: {call}"
        );
    }
}

struct Probe {
    entry: Entry,
    field: Option<EntryField>,
    qsos: usize,
    status: Option<String>,
}

impl Probe {
    fn of(h: &H) -> Self {
        let mut entry = entry(h);
        entry.date.clear();
        entry.time.clear();
        Probe {
            entry,
            field: field(h),
            qsos: h.state().log_state().map_or(0, |s| s.1),
            status: h.state().status_text(),
        }
    }

    fn same(&self, other: &Probe) -> bool {
        self.entry == other.entry
            && self.field == other.field
            && self.qsos == other.qsos
            && self.status == other.status
    }
}

const ENTRY_KEYS: [(&str, Modifiers, Key, Option<&str>); 12] = [
    ("Shift+B", Modifiers::SHIFT, Key::B, Some("B")),
    ("Shift+M", Modifiers::SHIFT, Key::M, Some("M")),
    ("Shift+P", Modifiers::SHIFT, Key::P, Some("P")),
    ("Shift+O", Modifiers::SHIFT, Key::O, Some("O")),
    ("Shift+R", Modifiers::SHIFT, Key::R, Some("R")),
    ("Alt+C", Modifiers::ALT, Key::C, None),
    ("Alt+N", Modifiers::ALT, Key::N, None),
    ("Alt+M", Modifiers::ALT, Key::M, None),
    ("Ctrl+K", Modifiers::COMMAND, Key::K, None),
    ("Ctrl+W", Modifiers::COMMAND, Key::W, None),
    ("Enter", Modifiers::NONE, Key::Enter, None),
    ("Esc", Modifiers::NONE, Key::Escape, None),
];

fn fires(
    h: &mut H,
    at: &mut dyn FnMut(&mut H),
    key: Option<(Modifiers, Key, Option<&str>)>,
) -> Probe {
    h.state_mut().set_entry(filled());
    h.state_mut().select_qso(None);
    at(h);
    if let Some((m, k, t)) = key {
        press(h, m, k, t);
    }
    type_text(h, "q");
    let p = Probe::of(h);
    h.state_mut().close_dialogs();
    egui::Popup::close_all(&h.ctx);
    h.run_steps(2);
    p
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn entry_keys_act_in_every_field_and_wait_while_another_field_is_typed_in() {
    let env = Env::new("matrix");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    for f in FIELDS {
        let mut at = |h: &mut H| focus(h, f);
        let null = fires(&mut h, &mut at, None);
        for (name, m, k, t) in ENTRY_KEYS {
            let stays = match k {
                Key::C | Key::Escape => f == EntryField::Call,
                Key::N => f == EntryField::Number,
                Key::M if m == Modifiers::ALT => f == EntryField::Memo,
                _ => false,
            };
            if stays {
                continue;
            }
            let with = fires(&mut h, &mut at, Some((m, k, t)));
            assert!(!with.same(&null), "{name} does nothing in {f:?}");
        }
    }
    let entry_fields = text_inputs(&h);
    h.state_mut().open_panel("search");
    h.run_steps(5);
    let panel = *text_inputs(&h)
        .difference(&entry_fields)
        .next()
        .expect("search field");
    let mut at = |h: &mut H| {
        h.state_mut().open_panel("search");
        h.run_steps(3);
        focus_node(h, panel);
        assert_eq!(field(h), None);
    };
    let null = fires(&mut h, &mut at, None);
    for (name, m, k, t) in ENTRY_KEYS {
        if k == Key::Escape {
            continue;
        }
        let with = fires(&mut h, &mut at, Some((m, k, t)));
        assert!(
            with.same(&null),
            "{name} reached the entry from the search field"
        );
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn time_keys_shift_and_set_the_entry_time() {
    let env = Env::new("time");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    let mut e = filled();
    e.date = "2000-01-01".into();
    e.time = "03:33:00".into();
    h.state_mut().set_entry(e);
    focus(&mut h, EntryField::Call);
    press(&mut h, Modifiers::COMMAND, Key::P, None);
    assert_eq!(entry(&h).time, "03:34:00");
    press(&mut h, Modifiers::COMMAND, Key::O, None);
    press(&mut h, Modifiers::COMMAND, Key::O, None);
    assert_eq!(entry(&h).time, "03:32:00");
    press(&mut h, Modifiers::SHIFT, Key::T, Some("T"));
    let e = entry(&h);
    assert_eq!(
        (e.date.as_str(), e.time.as_str()),
        ("", ""),
        "the clock again"
    );
    assert_eq!(e.callsign, "JA1ABC");
}

fn devices(dir: &Path) {
    let mut h = harness();
    let settings = h.state_mut().settings_mut();
    let keyer = settings.section("panel.keyer");
    keyer.insert("device".into(), PROBE.into());
    keyer.insert("speed".into(), "20".into());
    for n in 0..12u8 {
        let letter = char::from(b'A' + n);
        keyer.insert(format!("F{}", n + 1), format!("K{letter}"));
        keyer.insert(format!("F{}B", n + 1), format!("Q{letter}"));
    }
    let wav = dir.join("cq.wav");
    write_wav(&wav, 8000 * 3);
    let voice = settings.section("panel.voice");
    voice.insert("device".into(), "loopback".into());
    voice.insert("loopback.rate".into(), "8000".into());
    voice.insert("file1".into(), wav.display().to_string());
    voice.insert("ptt".into(), "true".into());
    settings.save().unwrap();
}

fn write_wav(path: &Path, samples: u32) {
    let mut b = Vec::new();
    let data = samples * 2;
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(8000u32.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for i in 0..samples {
        let v = if i % 20 < 10 { 8000i16 } else { -8000 };
        b.extend(v.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn function_keys_send_the_memories_and_escape_stops_them() {
    let env = Env::new("keyer");
    devices(&env.dir);
    let mut h = harness();
    open_log(&mut h, &env.dir);
    h.state_mut().set_entry(filled());
    focus(&mut h, EntryField::Call);
    h.run_steps(10);
    take_sent();
    for n in 1..=12u8 {
        let k = Key::from_name(&format!("F{n}")).unwrap();
        press(&mut h, Modifiers::NONE, k, None);
        let letter = char::from(b'A' + n - 1);
        assert_eq!(
            take_sent(),
            ["abort".into(), format!("send K{letter}")],
            "F{n}"
        );
    }
    for n in 1..=8u8 {
        let k = Key::from_name(&format!("F{n}")).unwrap();
        press(&mut h, Modifiers::SHIFT, k, None);
        let letter = char::from(b'A' + n - 1);
        assert_eq!(
            take_sent(),
            ["abort".into(), format!("send Q{letter}")],
            "Shift+F{n}"
        );
    }
    press(&mut h, Modifiers::NONE, Key::F1, None);
    take_sent();
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    assert_eq!(take_sent(), ["abort"], "Esc stops the keyer");
    assert_eq!(field(&h), Some(EntryField::Call));

    let mut e = filled();
    e.mode = Mode::Ssb;
    h.state_mut().set_entry(e);
    h.state_mut().open_panel("voice");
    focus(&mut h, EntryField::Call);
    press(&mut h, Modifiers::NONE, Key::F1, None);
    h.run_steps(3);
    let played = take_sent();
    assert!(
        played.contains(&"ptt true".to_string()),
        "F1 plays the voice memory: {played:?}"
    );
    assert!(
        !played.iter().any(|s| s.starts_with("send")),
        "no CW on phone"
    );
    focus(&mut h, EntryField::Call);
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    h.run_steps(3);
    let stopped = take_sent();
    assert!(
        stopped.contains(&"ptt false".to_string()),
        "Esc stops the voice: {stopped:?}"
    );
    assert_eq!(field(&h), Some(EntryField::Call));
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn band_and_mode_keys_move_the_simulated_rig() {
    let env = Env::new("rig");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    let rig = zlog_rig::text("en", "window.rig");
    assert!(click(&mut h, tr("menu.group.device")));
    assert!(click(&mut h, &rig));
    assert!(click(&mut h, &zlog_rig::text("en", "menu.device.connect")));
    h.run_steps(10);
    assert!(h.state().rig_connected());
    h.state_mut().close_dialogs();
    h.run_steps(3);
    focus(&mut h, EntryField::Call);
    for (m, k, t) in [
        (Modifiers::SHIFT, Key::B, "B"),
        (Modifiers::SHIFT, Key::M, "M"),
    ] {
        let before = entry(&h);
        press(&mut h, m, k, Some(t));
        h.run_steps(20);
        let after = entry(&h);
        assert!(
            after.band != before.band || after.mode != before.mode,
            "{k:?} changed nothing"
        );
        h.run_steps(20);
        assert_eq!(entry(&h), after, "the rig follows the entry");
    }
}

fn fast_cq(dir: &Path) {
    devices(dir);
    let mut h = harness();
    let settings = h.state_mut().settings_mut();
    let keyer = settings.section("panel.keyer");
    keyer.insert("F1".into(), "E".into());
    keyer.insert("speed".into(), "50".into());
    keyer.insert("cq_repeat".into(), "0".into());
    keyer.insert("cq_max".into(), "1000".into());
    settings.save().unwrap();
}

fn wait(h: &mut H) {
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(30));
        h.run_steps(1);
    }
}

fn sends(sent: &[String]) -> usize {
    sent.iter().filter(|s| *s == "send E").count()
}

fn keyer_log(env: &Env) -> H {
    fast_cq(&env.dir);
    let mut h = harness();
    open_log(&mut h, &env.dir);
    h.state_mut()
        .set_entry(Entry::new(Band::B7, Mode::Cw, zlog_core::Power::P010));
    focus(&mut h, EntryField::Call);
    h.run_steps(10);
    take_sent();
    h
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn ctrl_z_repeats_cq_until_a_call_is_typed() {
    let env = Env::new("ctrl-z");
    let mut h = keyer_log(&env);
    press(&mut h, Modifiers::COMMAND, Key::Z, None);
    wait(&mut h);
    let sent = take_sent();
    assert!(sends(&sent) >= 2, "Ctrl+Z repeats F1: {sent:?}");
    press(&mut h, Modifiers::COMMAND, Key::Z, None);
    type_text(&mut h, "JA");
    assert_eq!(entry(&h).callsign, "JA");
    assert!(take_sent().contains(&"abort".to_string()), "typing stops");
    wait(&mut h);
    assert_eq!(sends(&take_sent()), 0, "stopped");
    press(&mut h, Modifiers::COMMAND, Key::Z, None);
    wait(&mut h);
    assert!(sends(&take_sent()) >= 1);
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    assert!(take_sent().contains(&"abort".to_string()), "Esc stops");
    wait(&mut h);
    assert_eq!(sends(&take_sent()), 0);
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn shift_z_repeats_cq_until_escape() {
    let env = Env::new("shift-z");
    let mut h = keyer_log(&env);
    press(&mut h, Modifiers::SHIFT, Key::Z, Some("Z"));
    assert_eq!(entry(&h).callsign, "", "Shift+Z types nothing");
    wait(&mut h);
    assert!(sends(&take_sent()) >= 2, "Shift+Z repeats F1");
    type_text(&mut h, "JA");
    wait(&mut h);
    let sent = take_sent();
    assert!(sends(&sent) >= 1, "typing keeps it: {sent:?}");
    assert!(!sent.contains(&"abort".to_string()));
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    assert!(take_sent().contains(&"abort".to_string()), "Esc stops");
    wait(&mut h);
    let rest = take_sent();
    assert_eq!(sends(&rest), 0, "{rest:?}");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn shift_y_and_u_change_the_cw_speed() {
    let env = Env::new("speed");
    let mut h = keyer_log(&env);
    press(&mut h, Modifiers::SHIFT, Key::U, Some("U"));
    assert_eq!(take_sent(), ["wpm 49"], "Shift+U is QRS");
    press(&mut h, Modifiers::SHIFT, Key::Y, Some("Y"));
    assert_eq!(take_sent(), ["wpm 50"], "Shift+Y is QRQ");
    press(&mut h, Modifiers::SHIFT, Key::Y, Some("Y"));
    assert!(take_sent().is_empty(), "zLog stops at 50 WPM");
    assert_eq!(entry(&h).callsign, "", "the keys type nothing");
    focus(&mut h, EntryField::Memo);
    press(&mut h, Modifiers::SHIFT, Key::Y, Some("Y"));
    assert!(take_sent().is_empty());
    assert_eq!(entry(&h).memo, "Y", "the memo types capitals");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn alt_k_types_cw_and_escape_returns_to_the_entry() {
    let env = Env::new("alt-k");
    let mut h = keyer_log(&env);
    press(&mut h, Modifiers::ALT, Key::K, None);
    h.run_steps(5);
    assert_eq!(field(&h), None, "the CW keyboard has the focus");
    type_text(&mut h, "tu");
    assert_eq!(take_sent(), ["send TU"]);
    assert_eq!(entry(&h).callsign, "");
    wait(&mut h);
    wait(&mut h);
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    h.run_steps(5);
    assert_eq!(field(&h), Some(EntryField::Call), "Esc goes back when idle");
    press(&mut h, Modifiers::ALT, Key::K, None);
    h.run_steps(5);
    assert_eq!(field(&h), None, "again");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn shift_f_switches_the_bank_of_the_function_keys_only() {
    let env = Env::new("shift-f");
    let mut h = keyer_log(&env);
    let played = |h: &mut H| {
        press(h, Modifiers::NONE, Key::F2, None);
        let sent = take_sent();
        press(h, Modifiers::NONE, Key::Escape, None);
        take_sent();
        sent.into_iter()
            .filter(|s| s.starts_with("send"))
            .collect::<Vec<_>>()
    };
    let a = played(&mut h);
    press(&mut h, Modifiers::SHIFT, Key::F, Some("F"));
    assert_eq!(entry(&h).callsign, "", "Shift+F types nothing");
    let b = played(&mut h);
    assert!(!a.is_empty() && !b.is_empty() && a != b, "{a:?} {b:?}");
    press(&mut h, Modifiers::SHIFT, Key::F, Some("F"));
    assert_eq!(played(&mut h), a, "Shift+F again is bank A");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn ctrl_z_is_cq_and_not_undo() {
    let env = Env::new("undo");
    let mut h = keyer_log(&env);
    h.state_mut().set_entry(filled());
    focus(&mut h, EntryField::Number);
    press(&mut h, Modifiers::NONE, Key::Enter, None);
    let qsos = |h: &H| h.state().log_state().map_or(0, |s| s.1);
    assert_eq!(qsos(&h), 1, "logged");
    focus(&mut h, EntryField::Call);
    take_sent();
    press(&mut h, Modifiers::COMMAND, Key::Z, None);
    wait(&mut h);
    assert_eq!(qsos(&h), 1, "Ctrl+Z undoes nothing");
    assert!(sends(&take_sent()) >= 1, "Ctrl+Z calls CQ");
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    h.state_mut().select_qso(Some(0));
    h.run_steps(3);
    press(&mut h, Modifiers::COMMAND, Key::Z, None);
    assert_eq!(qsos(&h), 1, "not on the grid either");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn keyer_keys_wait_while_another_field_is_typed_in() {
    let env = Env::new("keyer-guard");
    let mut h = keyer_log(&env);
    let entry_fields = text_inputs(&h);
    h.state_mut().open_panel("search");
    h.run_steps(5);
    let panel = *text_inputs(&h)
        .difference(&entry_fields)
        .next()
        .expect("search field");
    for (m, k, t) in [
        (Modifiers::COMMAND, Key::Z, None),
        (Modifiers::SHIFT, Key::Z, Some("Z")),
        (Modifiers::SHIFT, Key::Y, Some("Y")),
        (Modifiers::SHIFT, Key::U, Some("U")),
        (Modifiers::ALT, Key::K, None),
    ] {
        focus_node(&mut h, panel);
        press(&mut h, m, k, t);
        wait(&mut h);
        assert!(take_sent().is_empty(), "{k:?} reached the keyer");
        assert_eq!(field(&h), None);
    }
}

fn plain_key(key: Key, text: &str) -> Vec<Event> {
    vec![
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
        Event::Text(text.into()),
        Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    ]
}

#[derive(Debug, Clone, Copy)]
enum Start {
    CtrlZ,
    ShiftZ,
    Button,
}

fn start_cq(h: &mut H, start: Start) {
    match start {
        Start::CtrlZ => press(h, Modifiers::COMMAND, Key::Z, None),
        Start::ShiftZ => press(h, Modifiers::SHIFT, Key::Z, Some("Z")),
        Start::Button => assert!(click(h, "CQ"), "the CQ button"),
    }
}

type CqStop = (
    Start,
    EntryField,
    &'static str,
    Modifiers,
    Key,
    Option<&'static str>,
    bool,
);

const CQ_STOPS: [CqStop; 14] = [
    (
        Start::CtrlZ,
        EntryField::Call,
        "a",
        Modifiers::NONE,
        Key::A,
        Some("a"),
        true,
    ),
    (
        Start::CtrlZ,
        EntryField::Call,
        "7",
        Modifiers::NONE,
        Key::Num7,
        Some("7"),
        true,
    ),
    (
        Start::CtrlZ,
        EntryField::Call,
        "/",
        Modifiers::NONE,
        Key::Slash,
        Some("/"),
        false,
    ),
    (
        Start::CtrlZ,
        EntryField::Call,
        "Backspace",
        Modifiers::NONE,
        Key::Backspace,
        None,
        false,
    ),
    (
        Start::CtrlZ,
        EntryField::Call,
        "Shift+A",
        Modifiers::SHIFT,
        Key::A,
        Some("A"),
        false,
    ),
    (
        Start::CtrlZ,
        EntryField::Number,
        "1",
        Modifiers::NONE,
        Key::Num1,
        Some("1"),
        false,
    ),
    (
        Start::CtrlZ,
        EntryField::Number,
        "a",
        Modifiers::NONE,
        Key::A,
        Some("a"),
        false,
    ),
    (
        Start::CtrlZ,
        EntryField::RstRcvd,
        "5",
        Modifiers::NONE,
        Key::Num5,
        Some("5"),
        false,
    ),
    (
        Start::CtrlZ,
        EntryField::Memo,
        "a",
        Modifiers::NONE,
        Key::A,
        Some("a"),
        false,
    ),
    (
        Start::ShiftZ,
        EntryField::Call,
        "a",
        Modifiers::NONE,
        Key::A,
        Some("a"),
        false,
    ),
    (
        Start::ShiftZ,
        EntryField::Call,
        "7",
        Modifiers::NONE,
        Key::Num7,
        Some("7"),
        false,
    ),
    (
        Start::Button,
        EntryField::Call,
        "a",
        Modifiers::NONE,
        Key::A,
        Some("a"),
        false,
    ),
    (
        Start::Button,
        EntryField::Call,
        "7",
        Modifiers::NONE,
        Key::Num7,
        Some("7"),
        false,
    ),
    (
        Start::Button,
        EntryField::Number,
        "1",
        Modifiers::NONE,
        Key::Num1,
        Some("1"),
        false,
    ),
];

fn type_key(h: &mut H, m: Modifiers, k: Key, t: Option<&str>) {
    match (m, t) {
        (Modifiers::NONE, Some(t)) => {
            h.input_mut().events.extend(plain_key(k, t));
            h.run_steps(3);
        }
        _ => press(h, m, k, t),
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn cw_cq_repeat_stops_only_for_letters_and_digits_typed_in_the_call_field() {
    let env = Env::new("cq-stops");
    let mut h = keyer_log(&env);
    for (start, f, name, m, k, t, stops) in CQ_STOPS {
        h.state_mut()
            .set_entry(Entry::new(Band::B7, Mode::Cw, zlog_core::Power::P010));
        focus(&mut h, f);
        start_cq(&mut h, start);
        focus(&mut h, f);
        wait(&mut h);
        assert!(sends(&take_sent()) >= 1, "{start:?} calls CQ");
        type_key(&mut h, m, k, t);
        let after = take_sent();
        wait(&mut h);
        let later = sends(&take_sent());
        let what = format!("{start:?}, {name} in {f:?}: {after:?}, {later} later");
        if stops {
            assert!(after.contains(&"abort".to_string()) && later == 0, "{what}");
        } else {
            assert!(
                !after.contains(&"abort".to_string()) && later >= 1,
                "{what}"
            );
        }
        press(&mut h, Modifiers::NONE, Key::Escape, None);
        wait(&mut h);
        take_sent();
    }
}

fn short_voice(dir: &Path) {
    devices(dir);
    let mut h = harness();
    let settings = h.state_mut().settings_mut();
    let wav = dir.join("short.wav");
    write_wav(&wav, 400);
    let voice = settings.section("panel.voice");
    voice.insert("file1".into(), wav.display().to_string());
    voice.insert("cq_repeat".into(), "0".into());
    voice.insert("cq_max".into(), "1000".into());
    settings.save().unwrap();
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn voice_cq_repeat_stops_only_for_letters_and_digits_typed_in_the_call_field() {
    let env = Env::new("voice-stops");
    short_voice(&env.dir);
    let mut h = harness();
    open_log(&mut h, &env.dir);
    h.state_mut().open_panel("voice");
    h.run_steps(5);
    for (start, f, name, m, k, t, stops) in CQ_STOPS
        .into_iter()
        .filter(|r| !matches!(r.0, Start::Button))
    {
        h.state_mut()
            .set_entry(Entry::new(Band::B7, Mode::Ssb, zlog_core::Power::P010));
        focus(&mut h, f);
        take_sent();
        start_cq(&mut h, start);
        focus(&mut h, f);
        wait(&mut h);
        let started = take_sent();
        assert!(
            started.contains(&"ptt true".to_string()),
            "{start:?} plays the voice CQ: {started:?}"
        );
        type_key(&mut h, m, k, t);
        wait(&mut h);
        wait(&mut h);
        let later = take_sent();
        let what = format!("{start:?}, {name} in {f:?}: {later:?}");
        let again = later.iter().filter(|s| *s == "ptt true").count();
        if stops {
            assert_eq!(again, 0, "{what}");
        } else {
            assert!(again >= 1, "{what}");
        }
        press(&mut h, Modifiers::NONE, Key::Escape, None);
        wait(&mut h);
        take_sent();
    }
}

fn speed_log(env: &Env, wpm: u32) -> H {
    fast_cq(&env.dir);
    {
        let mut h = harness();
        let s = h.state_mut().settings_mut();
        s.section("panel.keyer")
            .insert("speed".into(), wpm.to_string());
        s.save().unwrap();
    }
    let mut h = harness();
    open_log(&mut h, &env.dir);
    focus(&mut h, EntryField::Call);
    h.run_steps(10);
    take_sent();
    h
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn speed_keys_follow_zlog_and_come_down_from_above_fifty() {
    let cases: [(u32, Key, &[&str]); 8] = [
        (5, Key::U, &[]),
        (5, Key::Y, &["wpm 6"]),
        (50, Key::Y, &[]),
        (50, Key::U, &["wpm 49"]),
        (55, Key::U, &["wpm 54"]),
        (55, Key::Y, &[]),
        (60, Key::U, &["wpm 59"]),
        (60, Key::Y, &[]),
    ];
    for (wpm, k, want) in cases {
        let env = Env::new("speed-range");
        let mut h = speed_log(&env, wpm);
        let text = if k == Key::U { "U" } else { "Y" };
        press(&mut h, Modifiers::SHIFT, k, Some(text));
        assert_eq!(take_sent(), want, "Shift+{text} at {wpm} WPM");
        assert_eq!(entry(&h).callsign, "", "the key types nothing");
    }
}

fn with_layout(extra: &str) {
    let mut h = harness();
    let s = h.state_mut().settings_mut();
    let mut section = String::new();
    for line in extra.lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_string();
        } else if let Some((k, v)) = line.split_once(" = ") {
            let k = k.trim_matches('"');
            let v = v.trim_matches('"');
            s.section(&section).insert(k.into(), v.into());
        }
    }
    s.save().unwrap();
}

fn alt_k_sends_from_the_keyboard(layout: &str, what: &str) {
    let env = Env::new("alt-k-docked");
    fast_cq(&env.dir);
    with_layout(layout);
    let mut h = harness();
    open_log(&mut h, &env.dir);
    focus(&mut h, EntryField::Call);
    h.run_steps(10);
    take_sent();
    press(&mut h, Modifiers::ALT, Key::K, None);
    h.run_steps(5);
    assert_eq!(field(&h), None, "{what}: the CW keyboard has the focus");
    type_text(&mut h, "tu");
    assert_eq!(take_sent(), ["send TU"], "{what}");
    assert_eq!(entry(&h).callsign, "", "{what}");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn alt_k_shows_a_docked_keyer_wherever_it_is() {
    alt_k_sends_from_the_keyboard(
        "[dock]\nright = \"keyer\"\n[windows]\ndocked = \"keyer\"\n",
        "front tab",
    );
    alt_k_sends_from_the_keyboard(
        "[dock]\nright = \"keyer\"\n\"right.shown\" = \"0\"\n[windows]\ndocked = \"keyer\"\n",
        "hidden area",
    );
    alt_k_sends_from_the_keyboard(
        "[dock]\nright = \"keyer,search\"\n\"right.active\" = \"search\"\n[windows]\ndocked = \"keyer,search\"\n",
        "back tab",
    );
    alt_k_sends_from_the_keyboard(
        "[dock]\nbottom = \"search|keyer\"\n[windows]\ndocked = \"keyer,search\"\n",
        "split bottom area",
    );
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn escape_in_the_cw_keyboard_follows_zlog() {
    let env = Env::new("keyboard-escape");
    let mut h = keyer_log(&env);
    press(&mut h, Modifiers::SHIFT, Key::Z, Some("Z"));
    wait(&mut h);
    press(&mut h, Modifiers::ALT, Key::K, None);
    h.run_steps(5);
    assert_eq!(field(&h), None);
    type_text(&mut h, "cq");
    assert!(has_text_value(&h, "CQ"), "typed into the keyboard");
    take_sent();
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    assert!(take_sent().contains(&"abort".to_string()), "Esc stops CQ");
    wait(&mut h);
    assert_eq!(sends(&take_sent()), 0);
    assert_eq!(field(&h), None, "while sending the focus stays");
    assert!(!has_text_value(&h, "CQ"), "the keyboard text is cleared");
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    h.run_steps(5);
    assert_eq!(field(&h), Some(EntryField::Call), "idle: back to the entry");
}

fn has_text_value(h: &H, text: &str) -> bool {
    h.root()
        .children_recursive()
        .any(|n| n.accesskit_node().value().as_deref() == Some(text))
}

fn search_field(h: &H, entry_fields: &HashSet<NodeId>) -> Option<NodeId> {
    text_inputs(h).difference(entry_fields).next().copied()
}

fn mouse_click(h: &mut H, at: egui::Pos2, button: egui::PointerButton) {
    h.event(Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run_steps(6);
}

fn button_at(h: &H, label: &str) -> Option<egui::Pos2> {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.role() == Role::Button && a.label().as_deref() == Some(label)
        })
        .map(|n| n.rect().center())
}

fn tab_at(h: &H, title: &str) -> egui::Pos2 {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.role() == Role::Tab && a.label().as_deref() == Some(title)
        })
        .map(|n| n.rect().center())
        .expect("tab")
}

fn bounds(n: &egui_kittest::Node<'_>) -> Option<egui::Rect> {
    n.accesskit_node().bounding_box().map(|b| {
        egui::Rect::from_min_max(
            egui::pos2(b.x0 as f32, b.y0 as f32),
            egui::pos2(b.x1 as f32, b.y1 as f32),
        )
    })
}

fn menu_bar_click(h: &mut H, item: &str) -> bool {
    let tops: Vec<egui::Pos2> = h
        .root()
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == Role::Button)
        .filter_map(|n| bounds(&n))
        .filter(|r| r.max.y < 30.0)
        .map(|r| r.center())
        .collect();
    for top in tops {
        mouse_click(h, top, egui::PointerButton::Primary);
        let found = h
            .root()
            .children_recursive()
            .find(|n| {
                let l = n.accesskit_node().label().unwrap_or_default();
                bounds(n).is_some_and(|r| r.min.y > 30.0)
                    && n.accesskit_node().role() != Role::Tab
                    && (l == item || l.strip_prefix(item).is_some_and(|r| r.starts_with(' ')))
            })
            .and_then(|n| bounds(&n))
            .map(|r| r.center());
        if let Some(at) = found {
            mouse_click(h, at, egui::PointerButton::Primary);
            return true;
        }
        egui::Popup::close_all(&h.ctx);
        h.run_steps(2);
    }
    false
}

fn menu_bar_submenu_click(h: &mut H, item: &str, child: &str) -> bool {
    if !menu_bar_click(h, item) {
        return false;
    }
    h.run_steps(3);
    let parent = h
        .root()
        .children_recursive()
        .find(|n| {
            let l = n.accesskit_node().label().unwrap_or_default();
            bounds(n).is_some_and(|r| r.min.y > 30.0)
                && l.strip_prefix(item).is_some_and(|r| r.starts_with(' '))
        })
        .and_then(|n| bounds(&n));
    let Some(parent) = parent else {
        return false;
    };
    let found = h
        .root()
        .children_recursive()
        .find(|n| {
            n.accesskit_node().label().as_deref() == Some(child)
                && bounds(n).is_some_and(|r| r.min.x >= parent.max.x - 4.0)
        })
        .and_then(|n| bounds(&n))
        .map(|r| r.center());
    let Some(at) = found else {
        return false;
    };
    mouse_click(h, at, egui::PointerButton::Primary);
    true
}

#[derive(Debug, Clone, Copy)]
enum Way {
    CloseMark,
    TabMenuClose,
    MenuToggle,
    HideArea,
    Float,
    EscapeInPanel,
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn typing_reaches_the_entry_after_every_way_of_leaving_a_focused_panel() {
    for way in [
        Way::CloseMark,
        Way::TabMenuClose,
        Way::MenuToggle,
        Way::HideArea,
        Way::Float,
        Way::EscapeInPanel,
    ] {
        let env = Env::new("panel-focus");
        devices(&env.dir);
        with_layout("[dock]\nright = \"search\"\n[windows]\ndocked = \"search\"\n");
        let mut h = harness();
        open_log(&mut h, &env.dir);
        let entry_fields: HashSet<NodeId> = h
            .root()
            .children_recursive()
            .filter(|n| {
                n.accesskit_node().role() == Role::TextInput
                    && bounds(n).is_some_and(|r| r.min.x < 1100.0)
            })
            .map(|n| node_id(&n))
            .collect();
        focus(&mut h, EntryField::Number);
        let panel = search_field(&h, &entry_fields).expect("search field");
        focus_node(&mut h, panel);
        assert_eq!(field(&h), None, "{way:?}: the panel has the focus");
        match way {
            Way::CloseMark => {
                let at = button_at(&h, "Close").expect("close mark");
                mouse_click(&mut h, at, egui::PointerButton::Primary);
            }
            Way::TabMenuClose => {
                let at = tab_at(&h, "Find QSOs");
                mouse_click(&mut h, at, egui::PointerButton::Secondary);
                let at = button_at(&h, "Close").expect("menu close");
                mouse_click(&mut h, at, egui::PointerButton::Primary);
            }
            Way::MenuToggle => assert!(menu_bar_click(&mut h, "Find QSOs"), "menu item"),
            Way::HideArea => assert!(menu_bar_click(&mut h, "Right panel"), "menu item"),
            Way::Float => {
                let at = tab_at(&h, "Find QSOs");
                mouse_click(&mut h, at, egui::PointerButton::Secondary);
                let at = button_at(&h, "Float").expect("float");
                mouse_click(&mut h, at, egui::PointerButton::Primary);
            }
            Way::EscapeInPanel => press(&mut h, Modifiers::NONE, Key::Escape, None),
        }
        h.run_steps(6);
        let f = field(&h);
        type_text(&mut h, "5");
        let e = entry(&h);
        let typed = e.callsign == "5" || e.nr_rcvd == "5";
        assert!(
            typed || (f.is_none() && matches!(way, Way::Float)),
            "{way:?}: field {f:?}, call {:?}, number {:?}",
            e.callsign,
            e.nr_rcvd
        );
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn opening_and_docking_panels_keep_typing_in_the_entry() {
    let env = Env::new("open-focus");
    devices(&env.dir);
    let mut h = harness();
    open_log(&mut h, &env.dir);
    focus(&mut h, EntryField::Call);
    let mut typed = String::new();
    let mut step = |h: &mut H, what: &str| {
        h.run_steps(6);
        type_text(h, "A");
        typed.push('A');
        assert_eq!(entry(h).callsign, typed, "{what}: field {:?}", field(h));
    };
    assert!(menu_bar_click(&mut h, "Find QSOs"), "open by menu");
    step(&mut h, "opened by the menu");
    assert!(
        menu_bar_submenu_click(&mut h, "Dock", "Right panel"),
        "dock by menu"
    );
    step(&mut h, "docked by the Window menu");
    assert!(menu_bar_click(&mut h, "Right panel"));
    step(&mut h, "area hidden");
    assert!(menu_bar_click(&mut h, "Right panel"));
    step(&mut h, "area shown");
    assert!(menu_bar_click(&mut h, "Float"));
    step(&mut h, "floated by the Window menu");
    assert!(menu_bar_click(&mut h, "Find QSOs"));
    step(&mut h, "closed by the menu");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn an_idle_log_with_docked_panels_and_simulators_repaints_only_for_the_clock() {
    let env = Env::new("idle");
    devices(&env.dir);
    with_layout(
        "[dock]\nright = \"keyer|search\"\nbottom = \"voice\"\n[windows]\ndocked = \"keyer,search,voice\"\n",
    );
    let mut h = harness();
    open_log(&mut h, &env.dir);
    let idle = |h: &mut H, what: &str| {
        for _ in 0..4 {
            h.run_steps(5);
        }
        for n in 0..40 {
            h.step();
            let delay = root_delay(h);
            assert!(
                delay >= std::time::Duration::from_millis(250),
                "{what}: frame {n} repaints after {delay:?}: {:?}",
                h.ctx.repaint_causes()
            );
        }
    };
    settle_idle(&mut h);
    assert_clock_only(&mut h, "docked panels");
    connect_simulated_rig(&mut h);
    idle(&mut h, "docked panels and a connected simulated rig");
    assert!(click(&mut h, tr("menu.file")));
    assert!(click(&mut h, tr("menu.file.start_time")));
    idle(&mut h, "a floating dialog");
    assert!(click(&mut h, tr("button.cancel")));
    h.set_size(egui::vec2(640.0, 560.0));
    idle(&mut h, "narrow");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn the_setup_wizard_tries_the_simulators_and_reads_the_frequency() {
    let env = Env::new("wizard");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    let start = |h: &mut H| {
        assert!(click(h, tr("menu.settings")));
        assert!(click(h, tr("menu.settings.wizard")));
        assert!(click(h, tr("wizard.use.try")));
        assert!(click(h, tr("wizard.next")));
    };
    let next_enabled = |h: &H| {
        h.root().children_recursive().any(|n| {
            let a = n.accesskit_node();
            a.role() == Role::Button
                && a.label().as_deref() == Some(tr("wizard.next"))
                && !a.is_disabled()
        })
    };
    h.state_mut()
        .settings_mut()
        .section("station")
        .insert("callsign".into(), String::new());
    start(&mut h);
    assert!(!next_enabled(&h), "the callsign is required");
    h.state_mut().close_dialogs();
    h.state_mut()
        .settings_mut()
        .section("station")
        .insert("callsign".into(), "JA1ZLO".into());
    h.run_steps(3);
    start(&mut h);
    assert!(next_enabled(&h));
    assert!(click(&mut h, tr("wizard.next")));
    assert!(click(&mut h, tr("wizard.test_rig")));
    h.run_steps(20);
    let ok = h.root().children_recursive().any(|n| {
        let a = n.accesskit_node();
        a.label()
            .or_else(|| a.value())
            .is_some_and(|l| l.starts_with("Connected: frequency read"))
    });
    assert!(ok, "the simulated rig answers");
    assert!(h.state().rig_connected());
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn the_toolbar_indicator_shows_the_rig_and_disconnects_it() {
    let env = Env::new("indicator");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    let rig1 = zlog_rig::text("en", "options.rig");
    let state = |s: &str| format!("{rig1}: {}", zlog_ui_text(s));
    let has = |h: &H, label: &str| {
        h.root()
            .children_recursive()
            .any(|n| n.accesskit_node().label().as_deref() == Some(label))
    };
    assert!(!has(&h, &state("connection.connected")), "no rig set up");
    connect_simulated_rig(&mut h);
    assert!(has(&h, &state("connection.connected")));
    assert!(click(&mut h, &state("connection.connected")));
    let disconnect = zlog_ui_text("connection.disconnect");
    let found = {
        let n = h.root().children_recursive().find(|n| {
            n.accesskit_node().role() == Role::Button
                && n.accesskit_node()
                    .label()
                    .is_some_and(|l| l.ends_with(disconnect.as_str()))
        });
        if let Some(n) = &n {
            n.click();
        }
        n.is_some()
    };
    assert!(found, "the popover offers Disconnect");
    h.run_steps(5);
    assert!(!h.state().rig_connected());
    assert!(!has(&h, &state("connection.connected")));
}

fn zlog_ui_text(key: &str) -> String {
    zlog_ui::connection::text("en", key)
}

fn connect_simulated_rig(h: &mut H) {
    let rig = zlog_rig::text("en", "window.rig");
    assert!(click(h, tr("menu.group.device")));
    assert!(click(h, &rig));
    assert!(click(h, &zlog_rig::text("en", "menu.device.connect")));
    h.run_steps(10);
    assert!(h.state().rig_connected());
    h.state_mut().close_dialogs();
    h.run_steps(3);
}

fn titles(h: &H) -> Vec<String> {
    h.output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|v| {
            v.commands
                .iter()
                .filter_map(|c| match c {
                    egui::ViewportCommand::Title(t) => Some(t.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn click_titles(h: &mut H, label: &str) -> Vec<String> {
    let n = h.root().children_recursive().find(|n| {
        let l = n.accesskit_node().label();
        l.as_deref()
            .is_some_and(|l| l == label || l.strip_suffix('…') == Some(label))
    });
    n.expect(label).click();
    titles_in(h, 10)
}

fn titles_in(h: &mut H, frames: usize) -> Vec<String> {
    let mut all = Vec::new();
    for _ in 0..frames {
        h.step();
        all.extend(titles(h));
    }
    all
}

#[test]
#[ignore]
fn the_window_title_follows_the_log_and_returns_to_the_product_name() {
    let env = Env::new("title");
    let product = zlog_ui::product::NAME.to_string();
    let mut h = harness();
    assert!(
        titles_in(&mut h, 5).is_empty(),
        "the start title is the product name"
    );
    h.state_mut().open_setup(setup(&env.dir));
    let mut open = titles_in(&mut h, 3);
    open.extend(click_titles(&mut h, tr("button.ok")));
    assert_eq!(open.len(), 1, "sent once on change: {open:?}");
    assert!(open[0].starts_with(&product) && open[0].ends_with(".zlox"));
    assert!(click(&mut h, tr("menu.file")));
    let back = click_titles(&mut h, tr("menu.file.open"));
    assert!(h.state().log_state().is_none());
    assert_eq!(back, std::slice::from_ref(&product), "back to the start");
    h.state_mut().open_setup(setup(&env.dir));
    let mut again = titles_in(&mut h, 5);
    if let Some(n) = h
        .root()
        .children_recursive()
        .find(|n| n.accesskit_node().label().as_deref() == Some(tr("button.ok")))
    {
        n.click();
    }
    again.extend(titles_in(&mut h, 10));
    assert_eq!(again, open, "the log title is sent again");
}

fn labelled(h: &H, label: &str) -> Vec<egui::Rect> {
    h.root()
        .children_recursive()
        .filter(|n| {
            let a = n.accesskit_node();
            a.label().as_deref() == Some(label) || a.value().as_deref() == Some(label)
        })
        .filter_map(|n| bounds(&n))
        .collect()
}

fn multipliers_columns(h: &H) -> (egui::Rect, egui::Rect, egui::Rect) {
    let head = labelled(h, "1.9")[0];
    let row = labelled(h, "101")[0];
    let mark = labelled(h, ".")
        .into_iter()
        .chain(labelled(h, "*"))
        .filter(|r| (r.center().y - row.center().y).abs() < 2.0)
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("marks of the first row");
    (head, row, mark)
}

fn wheel(h: &mut H, at: egui::Pos2, delta: egui::Vec2) {
    h.event(Event::PointerMoved(at));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: egui::MouseWheelSource::Wheel,
    });
    h.run_steps(10);
}

#[test]
#[ignore]
fn the_multipliers_header_stays_aligned_and_scrolls_with_the_rows() {
    let env = Env::new("multipliers");
    with_layout(
        "[dock]\nright = \"multipliers\"\n\"right.size\" = \"300\"\n[windows]\ndocked = \"multipliers\"\n",
    );
    let mut h = harness();
    open_log(&mut h, &env.dir);
    h.run_steps(5);
    let (head, row, mark) = multipliers_columns(&h);
    assert!(
        (head.center().x - mark.center().x).abs() < 1.0,
        "{head:?} {mark:?}"
    );
    for at in [row.center(), head.center()] {
        let before = multipliers_columns(&h);
        wheel(&mut h, at, egui::vec2(-120.0, 0.0));
        let (head, row, mark) = multipliers_columns(&h);
        let moved = before.0.min.x - head.min.x;
        assert!(moved > 10.0, "scrolled from {at:?}: {moved}");
        assert!((before.1.min.x - row.min.x - moved).abs() < 1.0);
        assert!(
            (head.center().x - mark.center().x).abs() < 1.0,
            "{head:?} {mark:?}"
        );
        assert_eq!(head.min.y, before.0.min.y);
    }
    wheel(&mut h, row.center(), egui::vec2(0.0, -200.0));
    let after = labelled(&h, "1.9")[0];
    assert_eq!(after.min.y, head.min.y, "the header stays on top");
    assert!(labelled(&h, "101")
        .first()
        .is_none_or(|r| r.min.y < row.min.y - 10.0));
}

fn root_delay(h: &H) -> std::time::Duration {
    h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay
}

fn until_the_minute() -> std::time::Duration {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    std::time::Duration::from_millis(60_000 - (now.as_millis() % 60_000) as u64)
}

fn settle_idle(h: &mut H) {
    for _ in 0..4 {
        h.run_steps(5);
    }
}

fn assert_clock_only(h: &mut H, what: &str) {
    for n in 0..40 {
        let left = until_the_minute();
        h.step();
        let delay = root_delay(h);
        assert!(
            delay + std::time::Duration::from_millis(50) >= left
                && delay <= std::time::Duration::from_secs(61),
            "{what}: frame {n} repaints after {delay:?}, the minute ends in {left:?}: {:?}",
            h.ctx.repaint_causes()
        );
    }
}

fn assert_fast(h: &mut H, what: &str, most: u64) {
    for n in 0..10 {
        h.step();
        let delay = root_delay(h);
        assert!(
            delay <= std::time::Duration::from_millis(most),
            "{what}: frame {n} repaints after {delay:?}"
        );
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn the_clock_repaints_by_the_minute_and_busy_panels_ask_for_their_own() {
    let env = Env::new("clock");
    let mut h = keyer_log(&env);
    settle_idle(&mut h);
    assert_clock_only(&mut h, "idle");
    press(&mut h, Modifiers::COMMAND, Key::Z, None);
    assert_fast(&mut h, "CQ repeat with the keyer window closed", 100);
    press(&mut h, Modifiers::NONE, Key::Escape, None);
    wait(&mut h);
    settle_idle(&mut h);
    assert_clock_only(&mut h, "CQ stopped");
    connect_simulated_rig(&mut h);
    assert_fast(&mut h, "a connected rig is polled", 250);
}

fn log_call(h: &mut H, call: &str) {
    let mut e = Entry::new(Band::B7, Mode::Cw, zlog_core::Power::P010);
    e.callsign = call.into();
    e.nr_rcvd = "10H".into();
    h.state_mut().set_entry(e);
    focus(h, EntryField::Number);
    press(h, Modifiers::NONE, Key::Enter, None);
}

fn retype(h: &mut H, text: &str) {
    h.state_mut()
        .set_entry(Entry::new(Band::B7, Mode::Cw, zlog_core::Power::P010));
    focus(h, EntryField::Call);
    type_text(h, text);
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn ctrl_i_takes_the_first_partial_check_hit_and_leaves_the_caret_at_the_end() {
    let env = Env::new("ctrl-i");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    log_call(&mut h, "JA1ZLO");
    log_call(&mut h, "JA1YWX");
    assert_eq!(h.state().log_state().map(|s| s.1), Some(2));
    h.state_mut().open_panel("call-candidates");
    h.run_steps(5);

    retype(&mut h, "ZL");
    press(&mut h, Modifiers::COMMAND, Key::I, None);
    assert_eq!(entry(&h).callsign, "JA1ZLO");
    assert_eq!(field(&h), Some(EntryField::Call));
    type_text(&mut h, "/1");
    assert_eq!(entry(&h).callsign, "JA1ZLO/1", "the caret is at the end");

    retype(&mut h, "JA1");
    press(&mut h, Modifiers::COMMAND, Key::I, None);
    assert_eq!(entry(&h).callsign, "JA1ZLO", "the first line of several");

    retype(&mut h, "QQ");
    press(&mut h, Modifiers::COMMAND, Key::I, None);
    assert_eq!(entry(&h).callsign, "QQ", "no hit, no change");

    retype(&mut h, "YW");
    h.state_mut().focus_entry(EntryField::Memo);
    h.run_steps(3);
    press(&mut h, Modifiers::COMMAND, Key::I, None);
    assert_eq!(entry(&h).callsign, "JA1YWX");
    assert_eq!(field(&h), Some(EntryField::Memo), "the focus stays");
}

#[test]
fn the_dock_layout_and_open_windows_come_back_after_a_restart() {
    let env = Env::new("layout-restart");
    let mut h = harness();
    open_log(&mut h, &env.dir);
    assert!(menu_bar_click(&mut h, "Find QSOs"), "open by menu");
    h.run_steps(6);
    assert!(
        menu_bar_submenu_click(&mut h, "Dock", "Right panel"),
        "dock by menu"
    );
    h.run_steps(6);
    assert!(menu_bar_click(&mut h, "QSO rate"), "open by menu");
    h.run_steps(6);
    h.input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    h.run_steps(6);
    h.state_mut().exit();
    let saved = std::fs::read_to_string(env.dir.join("settings.toml")).unwrap();
    assert!(saved.contains("right = \"search\""), "{saved}");
    let mut h = harness();
    h.state_mut().open_setup(setup(&env.dir));
    h.run_steps(5);
    click(&mut h, tr("button.ok"));
    h.run_steps(6);
    let has = |role: Role, label: &str| {
        h.root().children_recursive().any(|n| {
            let a = n.accesskit_node();
            a.role() == role && a.label().is_some_and(|l| l.starts_with(label))
        })
    };
    assert!(has(Role::Tab, "Find QSOs"), "docked panel restored");
    assert!(has(Role::Window, "QSO rate"), "floating window restored");
}
