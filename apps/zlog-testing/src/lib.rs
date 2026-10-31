pub mod cli;
pub mod layout;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use egui::accesskit::{NodeId, Role};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;

use zlog_gui::testing::app::ZLogApp;
use zlog_gui::testing::i18n::tr;
use zlog_gui::testing::session::Setup;

pub use zlog_gui;

#[macro_export]
macro_rules! gui_tests {
    ($register:expr, $ignore:literal) => {
        $crate::gui_tests!(@ $register, $ignore; zlog_menu_every_control_and_every_contest, received_rst_fills_every_entry_style, band_scope_memo_key_prompts_without_a_rig, rtty_console_breaks_lines_through_the_loopback, take_hit_key_copies_the_first_candidate, cfg_picker_and_missing_contest_file, old_other_question, main_window_keys_grid_and_edit_dialog, every_menu_item_and_window, toolbar_entry_row_and_every_panel, rig_simulator_and_keyer, start_time_and_delete_confirmation, call_history_suggestion_accepted_or_overwritten, entry_window_logs_and_sends, paper_log_entry_carries_time_and_band, first_contest_guide_runs_steps_and_stretches, capability_items_are_listed);
    };
    (@ $register:expr, $ignore:literal; $($name:ident),*) => {
        $(
            #[test]
            #[ignore = $ignore]
            fn $name() {
                $crate::install($register);
                $crate::$name();
            }
        )*
    };
}

static BASE: std::sync::OnceLock<fn(&mut zlog_ui::Builder)> = std::sync::OnceLock::new();

thread_local! {
    static FEED: std::cell::RefCell<Vec<zlog_ui::EntryAction>> = const { std::cell::RefCell::new(Vec::new()) };
}

struct Feeder;

impl zlog_ui::Panel for Feeder {
    fn id(&self) -> &str {
        "testing.feed"
    }

    fn title(&self, _language: &str) -> String {
        String::new()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Help
    }

    fn take_actions(&mut self) -> Vec<zlog_ui::EntryAction> {
        FEED.with(|f| std::mem::take(&mut *f.borrow_mut()))
    }
}

fn with_feeder(b: &mut zlog_ui::Builder) {
    if let Some(base) = BASE.get() {
        base(b);
    }
    b.panel(i32::MAX, |_| Feeder);
}

pub fn install(register: fn(&mut zlog_ui::Builder)) {
    let _ = BASE.set(register);
    zlog_gui::testing::registry::install(with_feeder);
}

pub fn feed(h: &mut H, action: zlog_ui::EntryAction) {
    FEED.with(|f| f.borrow_mut().push(action));
    h.ctx.request_repaint();
    settle(h);
}

pub static SERIAL: Mutex<()> = Mutex::new(());

static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub const TEST_LIMIT: std::time::Duration = std::time::Duration::from_secs(300);

fn watchdog(name: &str) {
    let run = RUNS.load(std::sync::atomic::Ordering::SeqCst);
    let name = name.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(TEST_LIMIT);
        if RUNS.load(std::sync::atomic::Ordering::SeqCst) == run {
            eprintln!("GUI test {name:?} still running after {TEST_LIMIT:?}; aborting");
            std::process::abort();
        }
    });
}

pub fn outside_effects() -> [usize; 3] {
    [
        zlog_ui::dialog::native_dialogs(),
        zlog_net::remote_connects(),
        zlog_process::external_spawns(),
    ]
}

pub struct Env {
    pub dir: PathBuf,
    outside: [usize; 3],
    _serial: MutexGuard<'static, ()>,
}

impl Env {
    pub fn new(name: &str) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        watchdog(name);
        zlog_gui::testing::i18n::set_language("en");
        zlog_net::set_offline(true);
        zlog_audio::use_no_devices(true);
        let dir = std::env::temp_dir().join(format!("zlog-gui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        zlog_gui::testing::settings::TEST_PATH
            .with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
        pick(None);
        Env {
            dir,
            outside: outside_effects(),
            _serial: serial,
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if !std::thread::panicking() {
            assert_eq!(
                outside_effects(),
                self.outside,
                "native dialogs, remote connects, external processes"
            );
        }
        zlog_gui::testing::i18n::set_language("en");
        zlog_gui::testing::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        pick(None);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn edit_text(key: &str) -> String {
    zlog_edit::text("en", key)
}

pub fn rig_text(key: &str) -> &'static str {
    Box::leak(zlog_rig::text("en", key).into_boxed_str())
}

pub fn pick(path: Option<&Path>) {
    zlog_ui::dialog::set_test_pick(path.map(Path::to_path_buf));
}

pub const STEP: f32 = 0.05;

pub type H = Harness<'static, ZLogApp>;

pub fn harness() -> H {
    let mut h = Harness::builder()
        .with_size([1400.0, 1000.0])
        .with_accessibility_check(false)
        .allow_missing_glyphs()
        .with_step_dt(STEP)
        .build_eframe(|_| ZLogApp::new());
    settle(&mut h);
    h
}

pub fn settle(h: &mut H) {
    h.run_steps(3);
}

#[derive(Debug, Clone)]
pub struct Ctl {
    pub id: NodeId,
    pub role: Role,
    pub label: String,
    pub window: String,
}

impl std::fmt::Display for Ctl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {:?} {:?}", self.window, self.role, self.label)
    }
}

pub const ACTIVE: &[Role] = &[
    Role::Button,
    Role::CheckBox,
    Role::RadioButton,
    Role::ComboBox,
    Role::Tab,
    Role::MenuItem,
    Role::MenuItemCheckBox,
    Role::MenuItemRadio,
    Role::Switch,
    Role::Link,
    Role::SpinButton,
    Role::Slider,
    Role::TextInput,
    Role::MultilineTextInput,
];

pub fn node_id(n: &egui_kittest::Node<'_>) -> NodeId {
    n.accesskit_node().locate().0
}

pub fn window_of(n: &egui_kittest::Node<'_>) -> String {
    let mut p = n.parent();
    while let Some(w) = p {
        if w.accesskit_node().role() == Role::Window {
            return w.accesskit_node().label().unwrap_or_default();
        }
        p = w.parent();
    }
    String::new()
}

pub fn controls(h: &H) -> Vec<Ctl> {
    h.root()
        .children_recursive()
        .filter(|n| {
            let a = n.accesskit_node();
            ACTIVE.contains(&a.role()) && !a.is_disabled() && !a.is_hidden()
        })
        .map(|n| Ctl {
            id: node_id(&n),
            role: n.accesskit_node().role(),
            label: n.accesskit_node().label().unwrap_or_default(),
            window: window_of(&n),
        })
        .collect()
}

pub fn ids(h: &H) -> HashSet<NodeId> {
    h.root().children_recursive().map(|n| node_id(&n)).collect()
}

pub fn act(h: &mut H, c: &Ctl) -> bool {
    {
        let same = |n: &egui_kittest::Node<'_>| node_id(n) == c.id;
        let alike = |n: &egui_kittest::Node<'_>| {
            let a = n.accesskit_node();
            !c.label.is_empty()
                && a.role() == c.role
                && a.label().unwrap_or_default() == c.label
                && !a.is_disabled()
                && window_of(n) == c.window
        };
        let root = h.root();
        let Some(n) = root
            .children_recursive()
            .find(same)
            .or_else(|| root.children_recursive().find(alike))
        else {
            return false;
        };
        match c.role {
            Role::TextInput | Role::MultilineTextInput => {
                n.focus();
                n.type_text("1");
            }
            Role::SpinButton | Role::Slider => n.increment_accesskit(),
            _ => n.click_accesskit(),
        }
    }
    settle(h);
    true
}

pub struct Crawl<'a> {
    pub reset: &'a dyn Fn(&mut H),
    pub open: Option<&'a dyn Fn(&mut H)>,
    pub deny: &'a dyn Fn(&Ctl) -> bool,
    pub budget: usize,
}

impl Crawl<'_> {
    fn replay(&self, h: &mut H, path: &[Ctl]) -> bool {
        (self.reset)(h);
        if let Some(open) = self.open {
            open(h);
        }
        path.iter().all(|c| act(h, c))
    }

    pub fn run(&self, h: &mut H) -> Vec<String> {
        (self.reset)(h);
        let before = ids(h);
        let fresh = match self.open {
            Some(open) => {
                open(h);
                controls(h)
                    .into_iter()
                    .filter(|c| !before.contains(&c.id))
                    .collect()
            }
            None => controls(h),
        };
        assert!(!fresh.is_empty(), "nothing to crawl");
        let mut seen = HashSet::new();
        let mut done = Vec::new();
        self.visit(h, &mut Vec::new(), fresh, &mut seen, &mut done);
        (self.reset)(h);
        done
    }

    fn visit(
        &self,
        h: &mut H,
        path: &mut Vec<Ctl>,
        fresh: Vec<Ctl>,
        seen: &mut HashSet<NodeId>,
        done: &mut Vec<String>,
    ) {
        for c in fresh {
            if done.len() >= self.budget {
                return;
            }
            if seen.contains(&c.id) || (self.deny)(&c) {
                continue;
            }
            seen.insert(c.id);
            if !self.replay(h, path) {
                return;
            }
            let before = ids(h);
            if !act(h, &c) {
                continue;
            }
            done.push(c.to_string());
            let new: Vec<Ctl> = controls(h)
                .into_iter()
                .filter(|n| !before.contains(&n.id) && !seen.contains(&n.id))
                .collect();
            if !new.is_empty() && path.len() < 6 {
                path.push(c);
                self.visit(h, path, new, seen, done);
                path.pop();
            }
        }
    }
}

pub fn denied(c: &Ctl) -> bool {
    let w = c.window.as_str();
    let l = c.label.as_str();
    let update =
        w.starts_with("Update") && matches!(l, "Check for the nightly" | "Update" | "Restart now");
    let tiles = l.starts_with("Background (OSM)");
    let keyer = (w == "CW keyer" || w == tr("options.title"))
        && matches!(l, "Connect" | "WinKeyer" | "Serial port (DTR/RTS)");
    let audio = (w == "CW4ISR" && l.contains("Start"))
        || ((w == "Voice memory" || w == tr("options.title"))
            && matches!(l, "Find devices" | "Record"));
    let port = l.starts_with("/dev/") || l.starts_with("COM");
    let plugins = l == "Add plugins…";
    let quick = l == "Start everyday logging";
    let connection = w.is_empty()
        && [
            "Reconnect",
            "Disconnect",
            "Settings",
            "Stop TX",
            "Join",
            "Start Z-Link server",
        ]
        .iter()
        .any(|x| l.ends_with(x))
        || (w.is_empty() && l == "Open")
        || ["Start Z-Link server", "Join"]
            .iter()
            .any(|x| l.ends_with(x));
    update || tiles || keyer || audio || port || quick || plugins || connection
}

pub fn click(h: &mut H, label: &str) {
    let c = find(h, label).unwrap_or_else(|| panic!("no control {label:?}"));
    assert!(act(h, &c));
}

pub fn labelled(c: &Ctl, label: &str) -> bool {
    c.label == label
        || c.label
            .strip_prefix(label)
            .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with('…'))
}

pub fn find(h: &H, label: &str) -> Option<Ctl> {
    controls(h).into_iter().find(|c| labelled(c, label))
}

pub fn find_in(h: &H, window: &str, label: &str) -> Option<Ctl> {
    controls(h)
        .into_iter()
        .find(|c| c.window == window && labelled(c, label))
}

pub fn click_in(h: &mut H, window: &str, label: &str) {
    let c =
        find_in(h, window, label).unwrap_or_else(|| panic!("no control {label:?} in {window:?}"));
    assert!(act(h, &c));
}

pub fn set_text(h: &mut H, c: &Ctl, text: &str) {
    {
        let n = h
            .root()
            .children_recursive()
            .find(|n| node_id(n) == c.id)
            .expect("field");
        n.focus();
    }
    h.step();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press(Key::Backspace);
    if !text.is_empty() {
        h.event(Event::Text(text.into()));
    }
    settle(h);
}

pub fn key(h: &mut H, modifiers: Modifiers, k: Key) {
    let modifiers = Modifiers {
        ctrl: modifiers.ctrl || modifiers.command,
        ..modifiers
    };
    h.key_press_modifiers(modifiers, k);
    settle(h);
}

pub fn type_text(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    settle(h);
}

pub fn text_at(h: &H, text: &str) -> Option<Pos2> {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.label().as_deref() == Some(text) || a.value().as_deref() == Some(text)
        })
        .map(|n| n.rect().center())
}

pub fn click_at(h: &mut H, pos: Pos2, button: PointerButton, times: usize) {
    h.event(Event::PointerMoved(pos));
    for _ in 0..times {
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
    }
    settle(h);
}

pub fn close_request(h: &mut H) {
    h.input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    settle(h);
}

pub fn reset_main(h: &mut H) {
    zlog_gui::testing::i18n::set_language("en");
    h.state_mut().close_dialogs();
    egui::Popup::close_all(&h.ctx);
    h.key_press(Key::Escape);
    settle(h);
}

pub fn registry_index(id: &str) -> usize {
    zlog_gui::testing::registry::registry()
        .contests()
        .iter()
        .position(|c| c.id() == id)
        .unwrap_or_else(|| panic!("contest {id}"))
}

pub fn setup(dir: &Path, contest: &str, file: Option<PathBuf>) -> Setup {
    Setup {
        contest: if file.is_some() {
            0
        } else {
            registry_index(contest)
        },
        contest_file: file,
        mode: zlog_core::ContestMode::Mix,
        station: zlog_core::Station {
            callsign: "JA1ZLO".into(),
            ..Default::default()
        },
        coeff: 1.0,
        path: dir.join(format!("{contest}.zlox")),
        use_contest_period: true,
        convert_old_other: false,
    }
}

pub fn listed(h: &H, id: &str) -> bool {
    h.state()
        .listed_commands()
        .iter()
        .any(|c| c == id || c.ends_with(&format!("/{id}")))
}

pub fn capability_items_are_listed() {
    let env = Env::new("capabilities");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    for id in ["file/file.import", "file/file.export"] {
        assert!(listed(&h, id), "{id}: log formats are registered");
    }
    assert!(
        listed(&h, "file/file.start_time"),
        "contests with a period are registered"
    );
    assert!(listed(&h, "skip"), "a link is registered");
    let host = h
        .state()
        .listed_commands()
        .iter()
        .any(|c| c.starts_with("dock/"));
    assert!(
        host && listed(&h, "show.left"),
        "tool windows are registered"
    );
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.settings.options"));
    click(&mut h, tr("options.preferences"));
    assert!(
        text_at(&h, tr("options.entry_style")).is_some(),
        "several entry styles are registered"
    );
}

pub fn sample_cfg() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/extensions/zlog-lua/tests/fixtures/sample.cfg")
}

pub fn open_log(h: &mut H, s: Setup) {
    h.state_mut().open_setup(s);
    settle(h);
    if start_asked(h) {
        click(h, tr("button.ok"));
    }
    assert!(h.state().log_state().is_some(), "log open");
}

pub fn start_asked(h: &H) -> bool {
    text_at(h, &edit_text("start.title")).is_some()
}

pub fn new_controls(h: &H, before: &HashSet<NodeId>) -> Vec<Ctl> {
    controls(h)
        .into_iter()
        .filter(|c| !before.contains(&c.id))
        .collect()
}

pub fn log_qso(h: &mut H, call: &str, number: &str) {
    key(h, Modifiers::COMMAND, Key::K);
    type_text(h, call);
    key(h, Modifiers::NONE, Key::Space);
    type_text(h, number);
    key(h, Modifiers::NONE, Key::Enter);
}

pub fn report(name: &str, done: &[String]) {
    eprintln!("{name}: {} controls activated", done.len());
    for d in done {
        eprintln!("  {d}");
    }
}

pub fn zlog_menu_every_control_and_every_contest() {
    let env = Env::new("menu");
    let mut h = harness();
    pick(Some(&sample_cfg()));
    let reset = |h: &mut H| {
        zlog_gui::testing::i18n::set_language("en");
        h.state_mut().close_dialogs();
        egui::Popup::close_all(&h.ctx);
        settle(h);
    };
    let deny = |c: &Ctl| denied(c) || c.label == tr("button.ok");
    let done = Crawl {
        reset: &reset,
        open: None,
        deny: &deny,
        budget: 2000,
    }
    .run(&mut h);
    report("zLog Menu", &done);
    pick(None);

    let fields: Vec<Ctl> = controls(&h)
        .into_iter()
        .filter(|c| c.role == Role::TextInput && c.window.is_empty())
        .collect();
    for (i, f) in fields.iter().enumerate() {
        set_text(&mut h, f, &format!("JA{i}"));
    }
    set_text(&mut h, &fields[0], "");
    for p in [
        "H",
        "M",
        "L",
        "P",
        zlog_open::text("en", "menu_form.power_by_band"),
    ] {
        click(&mut h, p);
    }

    let registry = zlog_gui::testing::registry::registry();
    for (i, c) in registry.contests().iter().enumerate() {
        reset(&mut h);
        click(&mut h, &c.group("en"));
        let caption = c.caption("en");
        let radio = controls(&h)
            .into_iter()
            .find(|n| n.role == Role::RadioButton && n.label == caption)
            .unwrap_or_else(|| panic!("contest {caption}"));
        assert!(act(&mut h, &radio));
        let path_field = controls(&h)
            .into_iter()
            .rfind(|n| n.role == Role::TextInput && n.window.is_empty())
            .expect("file name field");
        let path = env.dir.join(format!("contest{i}.zlox"));
        set_text(&mut h, &path_field, &path.display().to_string());
        click(&mut h, tr("button.ok"));
        for _ in 0..2 {
            if start_asked(&h) {
                click(&mut h, tr("button.ok"));
            }
        }
        let (name, _) = h
            .state()
            .log_state()
            .unwrap_or_else(|| panic!("{} did not open", c.id()));
        eprintln!("opened {} as {name}", c.id());
        log_qso(&mut h, "JA2AAA", "100");
        h.run_steps(5);
        reset_main(&mut h);
        click(&mut h, tr("menu.file"));
        click(&mut h, tr("menu.file.open"));
        assert!(h.state().log_state().is_none(), "back in the zLog Menu");
        assert!(
            find(&h, tr("menu.file.exit")).is_none(),
            "no Exit after reopening"
        );
        click(&mut h, tr("button.cancel"));
        assert_eq!(
            h.state().log_path(),
            Some(path.as_path()),
            "Cancel goes back"
        );
        reset_main(&mut h);
        click(&mut h, tr("menu.file"));
        click(&mut h, tr("menu.file.open"));
    }
}

pub fn cfg_picker_and_missing_contest_file() {
    let env = Env::new("cfg");
    let mut h = harness();
    let cfgs = env.dir.join("cfg");
    std::fs::create_dir_all(&cfgs).unwrap();
    let cfg = cfgs.join("sample.cfg");
    std::fs::copy(sample_cfg(), &cfg).unwrap();

    click(&mut h, zlog_cfg::window::text("en", "cfg_picker.entry"));
    let picker = zlog_cfg::window::text("en", "cfg_picker.title");
    pick(Some(&cfgs));
    click_in(
        &mut h,
        picker,
        zlog_cfg::window::text("en", "cfg_picker.choose_folder"),
    );
    pick(None);
    settle(&mut h);
    let title = zlog_gui::testing::registry::registry()
        .contest_files()
        .iter()
        .find_map(|k| k.title(&cfg))
        .expect("title of sample.cfg");
    let row = |h: &H| {
        controls(h)
            .into_iter()
            .find(|c| c.window == picker && c.label == title)
            .expect("sample.cfg listed")
    };
    let r = row(&h);
    assert!(act(&mut h, &r));
    click_in(&mut h, picker, tr("button.ok"));
    assert!(
        find_in(&h, picker, tr("button.ok")).is_none(),
        "window closed"
    );
    click(&mut h, zlog_cfg::window::text("en", "cfg_picker.entry"));
    let r = row(&h);
    assert!(act(&mut h, &r));
    click_in(&mut h, picker, tr("button.cancel"));
    click(&mut h, zlog_cfg::window::text("en", "cfg_picker.entry"));
    let label = row(&h).label;
    let at = text_at(&h, &label).expect("listed");
    click_at(&mut h, at, PointerButton::Primary, 2);

    let log = env.dir.join("cfg.zlox");
    let path_field = controls(&h)
        .into_iter()
        .rfind(|n| n.role == Role::TextInput && n.window.is_empty())
        .unwrap();
    set_text(&mut h, &path_field, &log.display().to_string());
    click(&mut h, tr("button.ok"));
    if start_asked(&h) {
        click(&mut h, tr("button.ok"));
    }
    let (name, _) = h.state().log_state().expect("CFG contest open");
    eprintln!("CFG contest: {name}");
    log_qso(&mut h, "JA1YAA", "10");
    reset_main(&mut h);
    click(&mut h, tr("menu.file"));
    click(&mut h, tr("menu.file.open"));

    std::fs::remove_file(&cfg).unwrap();
    for answer in [
        tr("button.cancel"),
        zlog_open::text("en", "menu_form.locate"),
    ] {
        let path_field = controls(&h)
            .into_iter()
            .rfind(|n| n.role == Role::TextInput && n.window.is_empty())
            .unwrap();
        set_text(&mut h, &path_field, "");
        set_text(&mut h, &path_field, &log.display().to_string());
        h.run_steps(3);
        pick(Some(&sample_cfg()));
        click(&mut h, answer);
        pick(None);
    }
    click(&mut h, tr("button.ok"));
    assert!(h.state().log_state().is_some());
}

pub fn old_other_question() {
    let env = Env::new("old-other");
    let mut h = harness();
    for answer in [tr("button.no"), tr("button.yes")] {
        let zlo = env.dir.join("old.zlo");
        let mut log = zlog_core::Log::default();
        log.qsos.push(zlog_core::Qso {
            callsign: "JA2AAA".into(),
            mode: zlog_core::Mode::Ft4,
            ..Default::default()
        });
        log.save(&zlo, zlog_core::Format::Zlo).unwrap();
        let path_field = controls(&h)
            .into_iter()
            .rfind(|n| n.role == Role::TextInput && n.window.is_empty())
            .unwrap();
        set_text(&mut h, &path_field, &zlo.display().to_string());
        click(&mut h, tr("button.ok"));
        assert!(h.state().asking_old_other());
        click(&mut h, answer);
        assert!(h.state().log_state().is_some(), "opened after {answer}");
        reset_main(&mut h);
        click(&mut h, tr("menu.file"));
        click(&mut h, tr("menu.file.open"));
    }
}

pub fn main_window_keys_grid_and_edit_dialog() {
    let env = Env::new("main");
    for (contest, file) in [("allja", None), ("cqww", None), ("cfg", Some(sample_cfg()))] {
        let mut h = harness();
        let mut s = setup(&env.dir, contest, file.clone());
        if file.is_none()
            && !zlog_gui::testing::registry::registry()
                .contests()
                .iter()
                .any(|c| c.id() == contest)
        {
            panic!("no contest {contest}");
        }
        s.path = env.dir.join(format!("{contest}.zlox"));
        open_log(&mut h, s);
        let calls = ["JA1AAA", "JA2BBB", "JA3CCC", "JA1AAA", "7K1DDD"];
        for (i, call) in calls.iter().enumerate() {
            let number = match contest {
                "allja" => format!("{}H", 10 + i),
                "cqww" => format!("{}", 20 + i),
                _ => format!("01010{}", 1 + i),
            };
            log_qso(&mut h, call, &number);
        }
        let (_, logged) = h.state().log_state().unwrap();
        assert!(
            logged >= 3,
            "{contest}: {logged} QSOs, {:?}",
            h.state().entry_state()
        );
        type_text(&mut h, "JA9ZZZ");
        for k in [Key::B, Key::M, Key::P, Key::O] {
            key(&mut h, Modifiers::SHIFT, k);
        }
        key(&mut h, Modifiers::COMMAND, Key::K);
        key(&mut h, Modifiers::NONE, Key::Escape);
        for n in 1..=12 {
            let k = Key::from_name(&format!("F{n}")).unwrap();
            key(&mut h, Modifiers::NONE, k);
        }
        key(&mut h, Modifiers::SHIFT, Key::F1);
        key(&mut h, Modifiers::SHIFT, Key::ArrowUp);
        key(&mut h, Modifiers::SHIFT, Key::ArrowDown);
        for k in [Key::C, Key::N, Key::M, Key::R] {
            key(&mut h, Modifiers::ALT, k);
        }
        key(&mut h, Modifiers::COMMAND, Key::K);
        key(&mut h, Modifiers::NONE, Key::Enter);
        type_text(&mut h, "JA1AAA");
        key(&mut h, Modifiers::NONE, Key::Space);
        key(&mut h, Modifiers::NONE, Key::Space);
        key(&mut h, Modifiers::COMMAND, Key::K);
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::NONE, Key::ArrowDown);
        key(&mut h, Modifiers::NONE, Key::Enter);
        assert!(find_in(&h, &edit_text("dialog.edit"), tr("button.cancel")).is_some());
        click_in(&mut h, &edit_text("dialog.edit"), tr("button.cancel"));
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::NONE, Key::Delete);
        click(&mut h, tr("button.cancel"));
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::NONE, Key::Delete);
        click(&mut h, tr("button.ok"));
        let count = |h: &H| h.state().log_state().unwrap().1;
        let deleted = count(&h);
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        let step = |h: &mut H, side: &str| {
            click(h, tr("menu.edit"));
            click(h, &format!("{side}:"));
        };
        step(&mut h, "Undo");
        assert_eq!(count(&h), deleted + 1, "Edit > Undo");
        step(&mut h, "Redo");
        assert_eq!(count(&h), deleted, "Edit > Redo");
        step(&mut h, "Undo");
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::NONE, Key::Insert);
        if let Some(ok) = find(&h, tr("button.ok")) {
            act(&mut h, &ok);
        }
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::NONE, Key::Escape);
        reset_main(&mut h);

        let row = |h: &H| {
            calls
                .iter()
                .find_map(|c| text_at(h, c))
                .expect("a row on screen")
        };
        let at = row(&h);
        click_at(&mut h, at, PointerButton::Primary, 1);
        click_at(&mut h, at, PointerButton::Primary, 2);
        assert!(
            find_in(&h, &edit_text("dialog.edit"), tr("button.ok")).is_some(),
            "double click edits"
        );
        let edit = edit_text("dialog.edit");
        let edit = edit.as_str();
        let fields: Vec<Ctl> = controls(&h)
            .into_iter()
            .filter(|c| c.window == edit && c.role == Role::TextInput)
            .collect();
        for (i, f) in fields.iter().enumerate() {
            set_text(&mut h, f, &format!("{}", 5 + i));
        }
        click_in(&mut h, edit, tr("button.ok"));
        let at = row(&h);
        click_at(&mut h, at, PointerButton::Primary, 2);
        assert!(find_in(&h, edit, tr("button.ok")).is_some());
        close_request(&mut h);
        assert!(find_in(&h, edit, tr("button.ok")).is_none(), "closed");
        let open_edit = |h: &mut H| {
            let at = row(h);
            click_at(h, at, PointerButton::Primary, 2);
        };
        let done = Crawl {
            reset: &reset_main,
            open: Some(&open_edit),
            deny: &denied,
            budget: 400,
        }
        .run(&mut h);
        report(&format!("{contest}: edit dialog"), &done);
        let open_menu = |h: &mut H| {
            let at = row(h);
            click_at(h, at, PointerButton::Secondary, 1);
        };
        let done = Crawl {
            reset: &reset_main,
            open: Some(&open_menu),
            deny: &denied,
            budget: 400,
        }
        .run(&mut h);
        report(&format!("{contest}: grid menu"), &done);
        assert!(done.iter().any(|d| d.contains(tr("grid.band"))));
    }
}

pub fn every_menu_item_and_window() {
    let env = Env::new("menus");
    let mut h = harness();
    let s = setup(&env.dir, "allja", None);
    let path = s.path.clone();
    open_log(&mut h, s.clone());
    log_qso(&mut h, "JA1AAA", "10H");
    log_qso(&mut h, "JA2BBB", "11H");
    pick(Some(&env.dir.join("picked")));
    let reset = |h: &mut H| {
        reset_main(h);
        if h.state().log_state().is_none() {
            let mut s = s.clone();
            s.path = path.clone();
            h.state_mut().open_setup(s);
            settle(h);
            h.state_mut().close_dialogs();
            settle(h);
        }
        if text_at(h, "JA1AAA").is_none() {
            log_qso(h, "JA1AAA", "10H");
            reset_main(h);
        }
    };
    let mut menus = vec![
        tr("menu.file"),
        tr("menu.edit"),
        tr("menu.group.search"),
        tr("menu.group.report"),
        tr("menu.group.device"),
        tr("menu.group.network"),
        tr("menu.group.tools"),
        tr("menu.settings"),
        tr("menu.window"),
        tr("menu.help"),
    ];
    menus.retain(|m| find(&h, m).is_some());
    let mut total = 0;
    for menu in &menus {
        let open = |h: &mut H| click(h, menu);
        let done = Crawl {
            reset: &reset,
            open: Some(&open),
            deny: &denied,
            budget: 300,
        }
        .run(&mut h);
        report(menu, &done);
        total += done.len();
        if *menu == tr("menu.edit") {
            let open = |h: &mut H| {
                let at = text_at(h, "JA1AAA").expect("row");
                click_at(h, at, PointerButton::Primary, 1);
                click(h, menu);
            };
            let done = Crawl {
                reset: &reset,
                open: Some(&open),
                deny: &denied,
                budget: 400,
            }
            .run(&mut h);
            report("Edit with a row", &done);
        }
    }
    let open = |h: &mut H| {
        click(h, tr("menu.help"));
        click(h, tr("menu.help.about"));
        click(h, tr("menu.settings"));
        click(h, tr("menu.settings.options"));
        click(h, tr("menu.window"));
    };
    let done = Crawl {
        reset: &reset,
        open: Some(&open),
        deny: &denied,
        budget: 200,
    }
    .run(&mut h);
    report("Window with dialogs", &done);
    reset(&mut h);
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.language"));
    click(&mut h, "日本語");
    assert_eq!(zlog_gui::testing::i18n::current(), "ja");
    egui::Popup::close_all(&h.ctx);
    h.run_steps(3);
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.language"));
    click(&mut h, "English");
    assert_eq!(zlog_gui::testing::i18n::current(), "en");
    reset(&mut h);
    click(&mut h, tr("menu.help"));
    click(&mut h, tr("menu.help.manual"));
    assert!(total > 100, "{total} controls");
}

pub fn toolbar_entry_row_and_every_panel() {
    let env = Env::new("panels");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    log_qso(&mut h, "JA1AAA", "10H");
    pick(Some(&env.dir.join("picked")));
    let ids: Vec<String> = zlog_gui::testing::registry::panels()
        .iter()
        .flat_map(|p| {
            let id = p.id().to_string();
            zlog_ui::panel_commands(p.as_ref(), false, "en")
                .into_iter()
                .filter(|c| c.toolbar)
                .map(move |c| c.toolbar_key.unwrap_or_else(|| id.clone()))
        })
        .chain(["file.open", "file.save", "file.import", "file.export"].map(String::from))
        .collect();
    for id in &ids {
        h.state_mut().settings_mut().set_toolbar_shown(id, true);
    }
    let file = zlog_gui::testing::commands::file_commands(&Default::default(), "en");
    for style in zlog_gui::testing::settings::ToolbarStyle::ALL {
        h.state_mut().settings_mut().set_toolbar_style(style);
        settle(&mut h);
        let file = file
            .iter()
            .filter(|c| c.toolbar)
            .map(|c| ("File".to_string(), c.label.clone()));
        for (title, label) in
            file.chain(zlog_gui::testing::registry::panels().iter().flat_map(|p| {
                let title = p.title("en");
                zlog_ui::panel_commands(p.as_ref(), false, "en")
                    .into_iter()
                    .filter(|b| b.toolbar)
                    .map(move |b| (title.clone(), b.label))
            }))
        {
            let on_screen = h
                .root()
                .children_recursive()
                .any(|n| n.accesskit_node().label().is_some_and(|l| l == label));
            assert!(on_screen, "{style:?} {title}: toolbar button {label}");
        }
    }
    h.state_mut()
        .settings_mut()
        .set_toolbar_style(Default::default());
    settle(&mut h);
    let done = Crawl {
        reset: &reset_main,
        open: None,
        deny: &|c: &Ctl| {
            denied(c)
                || (c.window.is_empty() && c.role == Role::Button && is_menu(&c.label))
                || (c.window.is_empty() && c.label == tr("toolbar.open"))
        },
        budget: 300,
    }
    .run(&mut h);
    report("main window", &done);
    for b in ["Import", "Export"] {
        assert!(done.iter().any(|d| d.contains(b)), "toolbar {b} crawled");
    }
    let panels: Vec<(String, String, zlog_ui::MenuGroup)> = zlog_gui::testing::registry::panels()
        .iter()
        .filter(|p| p.window_kind().has_window())
        .map(|p| {
            let w = &zlog_ui::panel_commands(p.as_ref(), false, "en")[0];
            (w.window_title(), w.menu_text().to_string(), p.menu())
        })
        .collect();
    for (title, item, group) in panels {
        for _ in 0..2 {
            reset_main(&mut h);
            click(&mut h, tr(zlog_gui::testing::menus::group_key(group)));
            click(&mut h, &item);
            if find(&h, tr("menu.device.show")).is_some() {
                click(&mut h, tr("menu.device.show"));
            }
            h.run_steps(5);
            assert!(
                controls(&h).iter().any(|c| c.window == title) || window_shown(&h, &title),
                "{title} open"
            );
            close_request(&mut h);
        }
    }
    reset_main(&mut h);
    click(&mut h, tr("toolbar.open"));
    settle(&mut h);
    assert!(h.state().log_state().is_none(), "Open closed the log");
}

pub fn added_panels(base: fn(&mut zlog_ui::Builder)) -> Vec<Box<dyn zlog_ui::Panel>> {
    let mut b = zlog_ui::Builder::new();
    base(&mut b);
    let known: HashSet<String> = b
        .build_panels(zlog_gui::testing::registry::registry)
        .iter()
        .map(|p| p.id().to_string())
        .collect();
    zlog_gui::testing::registry::panels()
        .into_iter()
        .filter(|p| !known.contains(p.id()))
        .collect()
}

pub fn every_added_window_and_menu_item(base: fn(&mut zlog_ui::Builder)) {
    let env = Env::new("added");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    log_qso(&mut h, "JA1AAA", "10H");
    let added = added_panels(base);
    assert!(!added.is_empty(), "the extensions add panels");
    let mut opened = 0;
    let mut items = 0;
    for p in &added {
        let group = tr(zlog_gui::testing::menus::group_key(p.menu()));
        let commands = zlog_ui::panel_commands(p.as_ref(), false, "en");
        for c in commands.iter().filter(|c| c.menu && c.enabled) {
            reset_main(&mut h);
            if find(&h, group).is_none() {
                continue;
            }
            click(&mut h, group);
            let nested = p.menu() == zlog_ui::MenuGroup::Device
                && commands.iter().filter(|c| c.menu).count() > 1;
            if nested {
                click(&mut h, &p.title("en"));
            }
            let label = if nested && c.is_window() {
                tr("menu.device.show").to_string()
            } else {
                c.menu_text().to_string()
            };
            let Some(item) = find(&h, &label) else {
                continue;
            };
            assert!(act(&mut h, &item));
            if let Some(first) = c.children.iter().find(|k| k.enabled) {
                if let Some(k) = find(&h, &first.menu_text()) {
                    assert!(act(&mut h, &k));
                }
            }
            h.run_steps(5);
            if c.is_window() {
                let title = c.window_title();
                assert!(
                    controls(&h).iter().any(|x| x.window == title) || window_shown(&h, &title),
                    "{title} open"
                );
                opened += 1;
                close_request(&mut h);
            }
            items += 1;
        }
    }
    report(
        &format!("added: {opened} windows"),
        &[format!("{items} menu items")],
    );
    assert!(
        opened > 0 && items >= opened,
        "{opened} windows, {items} items"
    );
}

pub fn window_shown(h: &H, title: &str) -> bool {
    h.root().children_recursive().any(|n| {
        n.accesskit_node().role() == Role::Window
            && n.accesskit_node().label().as_deref() == Some(title)
    })
}

pub fn is_menu(label: &str) -> bool {
    [
        "menu.file",
        "menu.edit",
        "menu.group.search",
        "menu.group.report",
        "menu.group.device",
        "menu.group.network",
        "menu.group.tools",
        "menu.settings",
        "menu.window",
        "menu.help",
    ]
    .iter()
    .any(|k| tr(k) == label)
}

pub fn device_menu(h: &mut H, submenu: &str, item: &str) {
    click(h, tr("menu.group.device"));
    click(h, submenu);
    click(h, item);
}

pub fn rig_simulator_and_keyer() {
    let env = Env::new("rig");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    device_menu(
        &mut h,
        rig_text("window.rig"),
        rig_text("menu.device.connect"),
    );
    let rig = rig_text("window.rig");
    assert!(h.state().rig_connected());
    h.run_steps(10);
    let open = |h: &mut H| {
        if !window_shown(h, rig) {
            device_menu(h, rig, tr("menu.device.show"));
        }
    };
    let keep = |h: &mut H| {
        reset_main(h);
        if !h.state().rig_connected() {
            device_menu(h, rig, rig_text("menu.device.connect"));
            reset_main(h);
        }
    };
    let done = Crawl {
        reset: &keep,
        open: Some(&open),
        deny: &denied,
        budget: 200,
    }
    .run(&mut h);
    report("rig window", &done);
    open(&mut h);
    let dial = controls(&h)
        .into_iter()
        .find(|c| c.window == rig && c.role == Role::SpinButton)
        .expect("dial");
    for _ in 0..3 {
        act(&mut h, &dial);
    }
    h.run_steps(10);
    log_qso(&mut h, "JA1AAA", "10H");
    reset_main(&mut h);
    assert!(h.state().rig_connected(), "still connected after the crawl");
    device_menu(&mut h, rig, rig_text("menu.device.disconnect"));
    assert!(!h.state().rig_connected());

    let keyer = "CW keyer";
    reset_main(&mut h);
    click(&mut h, tr("menu.group.device"));
    if find(&h, keyer).is_none() {
        return;
    }
    reset_main(&mut h);
    let title = tr("options.title");
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.settings.options"));
    click_in(&mut h, title, keyer);
    let device = controls(&h)
        .into_iter()
        .find(|c| c.window == title && c.role == Role::ComboBox)
        .expect("keyer device");
    act(&mut h, &device);
    click(&mut h, "Simulated");
    click_in(&mut h, title, tr("button.ok"));
    device_menu(&mut h, keyer, tr("menu.device.show"));
    let open = |h: &mut H| {
        if !window_shown(h, keyer) {
            device_menu(h, keyer, tr("menu.device.show"));
        }
    };
    let done = Crawl {
        reset: &reset_main,
        open: Some(&open),
        deny: &denied,
        budget: 150,
    }
    .run(&mut h);
    report("keyer", &done);
    for n in 1..=4 {
        let k = Key::from_name(&format!("F{n}")).unwrap();
        key(&mut h, Modifiers::NONE, k);
    }
    key(&mut h, Modifiers::NONE, Key::Escape);
    open(&mut h);
    if find_in(&h, keyer, "Disconnect").is_some() {
        click_in(&mut h, keyer, "Disconnect");
    }
}

pub fn start_time_and_delete_confirmation() {
    let env = Env::new("start");
    let mut h = harness();
    h.state_mut().open_setup(setup(&env.dir, "allja", None));
    settle(&mut h);
    assert!(start_asked(&h), "a new log asks for the start time");
    click(&mut h, tr("button.cancel"));
    assert!(!start_asked(&h));
    log_qso(&mut h, "JA1AAA", "10H");
    for answer in [tr("button.cancel"), tr("button.ok")] {
        reset_main(&mut h);
        click(&mut h, tr("menu.file"));
        let before = ids(&h);
        click(&mut h, tr("menu.file.start_time"));
        assert!(start_asked(&h));
        let fields: Vec<Ctl> = new_controls(&h, &before)
            .into_iter()
            .filter(|c| c.role == Role::TextInput)
            .collect();
        assert_eq!(fields.len(), 2, "date and time");
        set_text(&mut h, &fields[1], "bad");
        set_text(&mut h, &fields[1], "00:00");
        click(&mut h, &edit_text("start.judge"));
        click(&mut h, answer);
        assert!(!start_asked(&h), "{answer} closes it");
    }
    for answer in [tr("button.cancel"), tr("button.ok")] {
        reset_main(&mut h);
        let at = text_at(&h, "JA1AAA").expect("row");
        click_at(&mut h, at, PointerButton::Secondary, 1);
        click(&mut h, &edit_text("delete"));
        click(&mut h, answer);
    }
    assert_eq!(h.state().log_state().map(|(_, n)| n), Some(0));
}

pub fn call_history_suggestion_accepted_or_overwritten() {
    let env = Env::new("hist");
    let file = env.dir.join("hist.txt");
    std::fs::write(&file, "CALL,EXCH\nJA1AAA,10H\n").unwrap();
    let path = file.display().to_string().replace('\\', "\\\\");
    std::fs::write(
        env.dir.join("settings.toml"),
        format!("[\"panel.hist\"]\nfile1 = \"{path}\"\nname_memo = \"false\"\n"),
    )
    .unwrap();
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    let number = |h: &H| h.state().entry_state().unwrap().2;
    const OFFER: &str = "History: number 10H (Alt+A)";
    key(&mut h, Modifiers::COMMAND, Key::K);
    type_text(&mut h, "JA1AAA");
    for _ in 0..200 {
        if text_at(&h, OFFER).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.step();
    }
    assert!(text_at(&h, OFFER).is_some(), "suggestion offered");
    assert_eq!(number(&h), "", "never typed by itself");
    key(&mut h, Modifiers::ALT, Key::A);
    assert_eq!(number(&h), "10H", "accepted");
    key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(h.state().log_state().map(|(_, n)| n), Some(1));
    key(&mut h, Modifiers::COMMAND, Key::K);
    type_text(&mut h, "JA1AAA");
    key(&mut h, Modifiers::ALT, Key::N);
    type_text(&mut h, "11H");
    assert_eq!(number(&h), "11H");
    assert!(text_at(&h, "History: number 10H, not what is typed").is_some());
    key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(h.state().log_state().map(|(_, n)| n), Some(2));
    assert_eq!(number(&h), "");
}

pub fn harness_with_style(env: &Env, style: &str) -> H {
    std::fs::write(
        env.dir.join("settings.toml"),
        format!("[preferences]\nentry_style = \"{style}\"\n"),
    )
    .unwrap();
    harness()
}

pub fn window_rect(h: &H, title: &str) -> Option<egui::Rect> {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.role() == Role::Window && a.label().as_deref() == Some(title)
        })
        .map(|n| n.rect())
}

pub fn field_width(h: &H, text: &str) -> f32 {
    h.root()
        .children_recursive()
        .find(|n| n.accesskit_node().value().as_deref() == Some(text))
        .map(|n| n.rect().width())
        .unwrap_or_else(|| panic!("no field holding {text:?}"))
}

pub fn label_starting(h: &H, prefix: &str) -> Option<String> {
    h.root().children_recursive().find_map(|n| {
        let a = n.accesskit_node();
        let l = a.label().or_else(|| a.value())?;
        l.starts_with(prefix).then_some(l)
    })
}

pub fn entry_window_logs_and_sends() {
    let env = Env::new("form");
    let mut h = harness_with_style(&env, "form");
    open_log(&mut h, setup(&env.dir, "allja", None));
    const TITLE: &str = "Entry window";
    assert!(window_rect(&h, TITLE).is_some(), "the window is open");
    type_text(&mut h, "JA1AAA");
    key(&mut h, Modifiers::NONE, Key::Space);
    type_text(&mut h, "10H");
    key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(
        h.state().log_state().map(|(_, n)| n),
        Some(1),
        "{:?}",
        h.state().entry_state()
    );
    let state = h.state().entry_state().unwrap();
    assert_eq!((state.1.as_str(), state.2.as_str()), ("", ""), "cleared");
    type_text(&mut h, "JA1BBB");
    assert_eq!(h.state().entry_state().unwrap().1, "JA1BBB");
    click_in(&mut h, TITLE, "F1\n");
    assert_eq!(h.state().entry_state().unwrap().0, tr("status.not_sent"));
    key(&mut h, Modifiers::NONE, Key::Space);
    type_text(&mut h, "11H");
    key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(h.state().log_state().map(|(_, n)| n), Some(2));
    type_text(&mut h, "JA1CCC");
    let before = field_width(&h, "JA1CCC");
    let r = window_rect(&h, TITLE).unwrap();
    let edge = egui::pos2(r.max.x - 1.0, r.center().y);
    h.event(Event::PointerMoved(edge));
    h.step();
    let press = |pressed, pos| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.event(press(true, edge));
    h.step();
    let to = edge + egui::vec2(300.0, 0.0);
    for i in 1..=6 {
        h.event(Event::PointerMoved(edge + egui::vec2(50.0 * i as f32, 0.0)));
        h.step();
    }
    h.event(press(false, to));
    settle(&mut h);
    let after = field_width(&h, "JA1CCC");
    assert!(
        after > before + 200.0,
        "fields stretch: {before} -> {after}"
    );
}

pub fn paper_log_entry_carries_time_and_band() {
    let env = Env::new("paper");
    let mut h = harness_with_style(&env, "paper");
    open_log(&mut h, setup(&env.dir, "allja", None));
    let count = |h: &H| h.state().log_state().map(|(_, n)| n).unwrap();
    let fill = |h: &mut H, alt: Key, text: &str| {
        if alt == Key::T {
            key(h, Modifiers::ALT, Key::D);
            key(h, Modifiers::NONE, Key::Tab);
        } else {
            key(h, Modifiers::ALT, alt);
        }
        type_text(h, text);
    };
    let qso = |h: &mut H, call: &str| {
        fill(h, Key::C, call);
        key(h, Modifiers::NONE, Key::Space);
        type_text(h, "10H");
        key(h, Modifiers::NONE, Key::Enter);
    };
    fill(&mut h, Key::D, "2001-01-01");
    fill(&mut h, Key::T, "2358");
    qso(&mut h, "JA1AAA");
    assert_eq!(count(&h), 1);
    let status = h.state().entry_state().unwrap().0;
    assert!(status.contains("invalid"), "out of period warned: {status}");
    let logged = label_starting(&h, "Logged #1: JA1AAA 2001-01-01 23:58")
        .unwrap_or_else(|| panic!("{:?}", label_starting(&h, "Logged")));
    assert!(text_at(&h, "23:58").is_some());
    key(&mut h, Modifiers::NONE, Key::Plus);
    key(&mut h, Modifiers::COMMAND, Key::ArrowUp);
    assert!(text_at(&h, "2001-01-02").is_some(), "the next day");
    assert!(text_at(&h, "00:00").is_some());
    let band = |l: &str| l.split(' ').nth(5).unwrap_or_default().to_string();
    key(&mut h, Modifiers::SHIFT, Key::B);
    qso(&mut h, "JA1BBB");
    let second = label_starting(&h, "Logged #2: JA1BBB 2001-01-02 00:00").expect("logged");
    assert_ne!(band(&second), band(&logged), "band changed");
    fill(&mut h, Key::T, "05");
    qso(&mut h, "JA1CCC");
    let third = label_starting(&h, "Logged #3: JA1CCC 2001-01-02 00:05").expect("minutes");
    assert_eq!(band(&third), band(&second), "band sticky");
    fill(&mut h, Key::T, "03");
    qso(&mut h, "JA1DDD");
    assert!(label_starting(&h, "Logged #4: JA1DDD 2001-01-02 01:03").is_some());
    assert_eq!(count(&h), 4);
    fill(&mut h, Key::C, "JA1EEE");
    let before = field_width(&h, "JA1EEE");
    h.set_size(egui::vec2(1800.0, 1000.0));
    settle(&mut h);
    let after = field_width(&h, "JA1EEE");
    assert!(after > before + 80.0, "fields stretch: {before} -> {after}");
}

pub fn first_contest_guide_runs_steps_and_stretches() {
    let env = Env::new("guide");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    let t = |k| zlog_guide::text("en", k);
    let title = t("title");
    click(&mut h, tr("menu.help"));
    click(&mut h, &t("menu"));
    assert!(window_rect(&h, &title).is_some(), "the guide is open");
    click_in(&mut h, &title, &format!("2. {}", t("step.2")));
    click_in(&mut h, &title, tr("options.station"));
    settle(&mut h);
    assert!(
        window_rect(&h, tr("options.title")).is_some(),
        "Options opened"
    );
    click_in(&mut h, tr("options.title"), tr("button.cancel"));
    click_in(&mut h, &title, &format!("5. {}", t("step.4")));
    for _ in 0..2 {
        click_in(&mut h, &title, "Score");
        settle(&mut h);
        assert!(window_rect(&h, "Score").is_some(), "the score window");
    }
    let rects = |h: &H| -> Vec<(String, egui::Rect)> {
        h.root()
            .children_recursive()
            .filter(|n| window_of(n) == title && ACTIVE.contains(&n.accesskit_node().role()))
            .map(|n| (n.accesskit_node().label().unwrap_or_default(), n.rect()))
            .collect()
    };
    let all = rects(&h);
    let row = [t("next"), t("back"), t("close")];
    let bottom = all
        .iter()
        .filter(|(l, _)| row.contains(l))
        .map(|(_, r)| r.min.y)
        .fold(f32::INFINITY, f32::min);
    assert!(bottom.is_finite(), "{all:?}");
    for (l, r) in all.iter().filter(|(l, _)| !row.contains(l)) {
        assert!(r.max.y <= bottom, "{l:?} below the buttons");
    }
    let order: Vec<&String> = all
        .iter()
        .filter(|(l, _)| row.contains(l))
        .map(|(l, _)| l)
        .collect();
    assert_eq!(
        order,
        row.iter().collect::<Vec<_>>(),
        "Next first, Close last"
    );
    click_in(&mut h, &title, &format!("4. {}", t("step.3")));
    let text = t("step.3.text");
    let width = |h: &H| {
        h.root()
            .children_recursive()
            .filter(|n| {
                let a = n.accesskit_node();
                a.label()
                    .or_else(|| a.value())
                    .is_some_and(|l| text.starts_with(&l[..l.len().min(20)]) && !l.is_empty())
            })
            .map(|n| n.rect().width())
            .fold(0.0, f32::max)
    };
    let press = |pressed, pos| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let resize = |h: &mut H, dx: f32| {
        let r = window_rect(h, &title).unwrap();
        let edge = egui::pos2(r.max.x - 1.0, r.center().y);
        h.event(Event::PointerMoved(edge));
        h.step();
        h.event(press(true, edge));
        h.step();
        for i in 1..=6 {
            h.event(Event::PointerMoved(
                edge + egui::vec2(dx * i as f32 / 6.0, 0.0),
            ));
            h.step();
        }
        h.event(press(false, edge + egui::vec2(dx, 0.0)));
        settle(h);
    };
    let wide = width(&h);
    resize(&mut h, -250.0);
    let before = width(&h);
    assert!(before < wide - 100.0, "text wraps: {wide} -> {before}");
    resize(&mut h, 300.0);
    let after = width(&h);
    assert!(
        after > before + 200.0,
        "text stretches: {before} -> {after}"
    );
    click_in(&mut h, &title, &t("close"));
    assert!(window_rect(&h, &title).is_none());
}

pub fn field_holding(h: &H, text: &str) -> bool {
    h.root().children_recursive().any(|n| {
        let a = n.accesskit_node();
        matches!(a.role(), Role::TextInput) && a.value().as_deref() == Some(text)
    })
}

pub fn received_rst_fills_every_entry_style() {
    use zlog_core::Mode;
    use zlog_ui::EntryAction::{Qsy, RstRcvd};
    for style in ["zlog", "paper", "form"] {
        let env = Env::new(&format!("rst-{style}"));
        let mut h = harness_with_style(&env, style);
        open_log(&mut h, setup(&env.dir, "allja", None));
        feed(&mut h, Qsy(7_010_000, Some(Mode::Cw)));
        feed(&mut h, RstRcvd(579));
        assert!(field_holding(&h, "579"), "{style}: CW takes 3 digits");
        feed(&mut h, RstRcvd(57));
        assert!(field_holding(&h, "579"), "{style}: CW refuses 2 digits");
        feed(&mut h, Qsy(7_100_000, Some(Mode::Ssb)));
        feed(&mut h, RstRcvd(57));
        assert!(field_holding(&h, "57"), "{style}: phone takes 2 digits");
        feed(&mut h, RstRcvd(599));
        assert!(field_holding(&h, "57"), "{style}: phone refuses 3 digits");
        assert!(!field_holding(&h, "599"), "{style}");
    }
}

pub fn band_scope_memo_key_prompts_without_a_rig() {
    let env = Env::new("scope-memo");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    key(&mut h, Modifiers::COMMAND, Key::K);
    type_text(&mut h, "JA1YWX");
    key(&mut h, Modifiers::COMMAND, Key::N);
    assert!(
        text_at(&h, "Frequency (kHz) JA1YWX").is_some(),
        "asks the frequency"
    );
    type_text(&mut h, "7012.5");
    assert!(
        field_holding(&h, "7012.5"),
        "typed into the prompt {:?}",
        h.state().entry_state()
    );
    key(&mut h, Modifiers::NONE, Key::Enter);
    assert!(
        text_at(&h, "Frequency (kHz) JA1YWX").is_none(),
        "closed {:?}",
        h.state().entry_state()
    );
    assert_eq!(
        h.state().log_state().map(|(_, n)| n),
        Some(0),
        "nothing logged"
    );
    assert_eq!(h.state().entry_state().unwrap().1, "", "entry cleared");
    click(&mut h, tr("menu.group.report"));
    click(&mut h, "Band scope");
    let row = |h: &H, text: &str| {
        h.root().children_recursive().any(|n| {
            let l = n.accesskit_node().label().unwrap_or_default();
            l.trim_start().starts_with(text)
        })
    };
    assert!(row(&h, "7012.5* JA1YWX"), "{:?}", label_starting(&h, "  "));
    key(&mut h, Modifiers::COMMAND, Key::K);
    type_text(&mut h, "JA1ZLO");
    key(&mut h, Modifiers::COMMAND, Key::N);
    type_text(&mut h, "7013");
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(text_at(&h, "Frequency (kHz) JA1ZLO").is_none(), "cancelled");
    assert!(!row(&h, "7013.0* JA1ZLO"));
    assert_eq!(
        h.state().entry_state().unwrap().1,
        "JA1ZLO",
        "kept on cancel"
    );
}

pub fn rtty_console_breaks_lines_through_the_loopback() {
    let env = Env::new("rtty-loop");
    std::fs::write(
        env.dir.join("settings.toml"),
        "[\"panel.rtty\"]\ndevice = \"loopback\"\n\"loopback.rate\" = \"8000\"\nbaud = \"100\"\nptt = \"false\"\n",
    )
    .unwrap();
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    feed(
        &mut h,
        zlog_ui::EntryAction::Qsy(14_080_000, Some(zlog_core::Mode::Rtty)),
    );
    device_menu(&mut h, "RTTY", tr("menu.device.show"));
    click_in(&mut h, "RTTY", "Start");
    let field = controls(&h)
        .into_iter()
        .find(|c| c.window == "RTTY" && c.role == Role::TextInput)
        .expect("send text");
    for text in ["CQ JA1ZLO", "DE JA1YWX"] {
        set_text(&mut h, &field, text);
        assert!(field_holding(&h, text), "typed {text}");
        click_in(&mut h, "RTTY", "Send");
    }
    let lines = |h: &H| {
        let mut words: Vec<(i32, f32, String)> = h
            .root()
            .children_recursive()
            .filter(|n| n.accesskit_node().role() == Role::Label && window_of(n) == "RTTY")
            .map(|n| {
                let r = n.rect();
                let text = n.accesskit_node().value().unwrap_or_default();
                (r.center().y.round() as i32, r.min.x, text)
            })
            .collect();
        words.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut rows: Vec<(i32, String)> = Vec::new();
        for (y, _, text) in words {
            match rows.last_mut() {
                Some((at, row)) if *at == y => row.push_str(&text),
                _ => rows.push((y, text)),
            }
        }
        rows.into_iter().map(|(_, r)| r).collect::<Vec<_>>()
    };
    let start = std::time::Instant::now();
    loop {
        let got = lines(&h);
        let row = |t: &str| got.iter().position(|l| l.trim_end() == t);
        if let (Some(a), Some(b)) = (row("CQ JA1ZLO"), row("DE JA1YWX")) {
            assert!(a < b, "{got:?}");
            assert!(got.iter().all(|l| !l.starts_with(' ')), "{got:?}");
            break;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "{got:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.step();
    }
}

pub fn take_hit_key_copies_the_first_candidate() {
    let env = Env::new("take-hit");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    log_qso(&mut h, "JA1YWX", "10H");
    assert_eq!(h.state().log_state().map(|(_, n)| n), Some(1));
    let call = |h: &H| h.state().entry_state().unwrap().1;
    key(&mut h, Modifiers::COMMAND, Key::K);
    type_text(&mut h, "1YW");
    key(&mut h, Modifiers::COMMAND, Key::I);
    assert_eq!(call(&h), "1YW", "the closed window takes nothing");
    click(&mut h, tr("menu.group.search"));
    click(&mut h, "Call candidates");
    assert!(window_shown(&h, "Call candidates"));
    key(&mut h, Modifiers::COMMAND, Key::I);
    assert_eq!(call(&h), "JA1YWX");
}

pub fn resize_window(h: &mut H, title: &str, width: f32) {
    let r = window_rect(h, title).expect(title);
    let edge = egui::pos2(r.max.x - 1.0, r.center().y);
    let press = |pressed, pos| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.event(Event::PointerMoved(edge));
    h.step();
    h.event(press(true, edge));
    h.step();
    let delta = width - r.width();
    let n = (delta.abs() / 40.0).ceil().max(1.0) as usize;
    for i in 1..=n {
        let at = edge + egui::vec2(delta * i as f32 / n as f32, 0.0);
        h.event(Event::PointerMoved(at));
        h.step();
    }
    h.event(press(false, edge + egui::vec2(delta, 0.0)));
    settle(h);
    let got = window_rect(h, title).expect(title).width();
    assert!(
        (got - width).abs() < 8.0,
        "{title} is {got} wide, not {width}"
    );
}

#[derive(Debug)]
struct Shown {
    fields: Vec<f32>,
    numbers: Vec<f32>,
    clipped: Vec<String>,
}

fn shown_in(h: &H, title: &str) -> Shown {
    let window = window_rect(h, title).expect(title).expand(1.0);
    let mut shown = Shown {
        fields: Vec::new(),
        numbers: Vec::new(),
        clipped: Vec::new(),
    };
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        let role = a.role();
        let wanted = [
            Role::TextInput,
            Role::PasswordInput,
            Role::MultilineTextInput,
            Role::ComboBox,
            Role::SpinButton,
            Role::Button,
        ];
        if !wanted.contains(&role) || window_of(&n) != title {
            continue;
        }
        let r = n.rect();
        match role {
            Role::TextInput | Role::PasswordInput | Role::MultilineTextInput | Role::ComboBox => {
                shown.fields.push(r.width())
            }
            Role::SpinButton => shown.numbers.push(r.width()),
            Role::Button if r.min.x < window.min.x || r.max.x > window.max.x => {
                shown.clipped.push(a.label().unwrap_or_default())
            }
            _ => {}
        }
    }
    shown
}

pub fn assert_options_flex(h: &mut H, pages: &[&str]) {
    let title = tr("options.title");
    click(h, tr("menu.settings"));
    click(h, tr("menu.settings.options"));
    for page in pages {
        click_in(h, title, page);
        resize_window(h, title, 480.0);
        let narrow = shown_in(h, title);
        resize_window(h, title, 900.0);
        let wide = shown_in(h, title);
        assert!(!narrow.fields.is_empty(), "{page}: {narrow:?}");
        assert_eq!(narrow.fields.len(), wide.fields.len(), "{page}");
        for (a, b) in narrow.fields.iter().zip(&wide.fields) {
            assert!(b - a > 200.0, "{page}: a field grows from {a} to {b}");
        }
        for (a, b) in narrow.numbers.iter().zip(&wide.numbers) {
            assert!(
                (b - a).abs() < 1.0,
                "{page}: a number stays {a} wide, not {b}"
            );
        }
        assert!(narrow.clipped.is_empty(), "{page}: {:?}", narrow.clipped);
        assert!(wide.clipped.is_empty(), "{page}: {:?}", wide.clipped);
    }
    click(h, tr("button.cancel"));
}

pub fn options_fields_follow_the_dialog_width(pages: &[&str]) {
    let env = Env::new("flex");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    assert_options_flex(&mut h, pages);
}

fn field_rects(h: &H, title: &str) -> Vec<egui::Rect> {
    let fields = [
        Role::TextInput,
        Role::PasswordInput,
        Role::MultilineTextInput,
        Role::ComboBox,
    ];
    h.root()
        .children_recursive()
        .filter(|n| fields.contains(&n.accesskit_node().role()) && window_of(n) == title)
        .map(|n| n.rect())
        .collect()
}

const DRAWN_BY_MODEMS: &[(&str, usize)] = &[("RTTY", 2), ("FT8 / FT4", 3)];

pub fn assert_options_aligned(h: &mut H, pages: &[&str]) {
    let title = tr("options.title");
    click(h, tr("menu.settings"));
    click(h, tr("menu.settings.options"));
    for page in pages {
        click_in(h, title, page);
        resize_window(h, title, 480.0);
        let narrow = field_rects(h, title);
        resize_window(h, title, 900.0);
        let wide = field_rects(h, title);
        assert_eq!(narrow.len(), wide.len(), "{page}");
        for (width, rects) in [(480, &narrow), (900, &wide)] {
            let flexible: Vec<&egui::Rect> = rects
                .iter()
                .zip(narrow.iter().zip(&wide))
                .filter(|(_, (a, b))| b.width() - a.width() > 100.0)
                .map(|(r, _)| r)
                .collect();
            let edges = |r: &egui::Rect| ((r.min.x * 2.0).round(), (r.max.x * 2.0).round());
            let most = flexible
                .iter()
                .map(|r| edges(r))
                .max_by_key(|e| flexible.iter().filter(|r| edges(r) == *e).count());
            let apart: Vec<&&egui::Rect> =
                flexible.iter().filter(|r| Some(edges(r)) != most).collect();
            let allowed = DRAWN_BY_MODEMS
                .iter()
                .find(|(p, _)| p == page)
                .map_or(0, |(_, n)| *n);
            assert!(
                apart.len() <= allowed,
                "{page} at {width}: {apart:?} are not aligned; all {rects:?}"
            );
        }
    }
    click(h, tr("button.cancel"));
}

pub fn options_fields_line_up(pages: &[&str]) {
    let env = Env::new("aligned");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    assert_options_aligned(&mut h, pages);
}
