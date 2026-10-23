//! Headless tests of the whole program: the real [`ZLogApp`] driven through
//! `egui_kittest`, clicking every menu, button and window, failing on any panic.
//!
//! Dialogs are immediate viewports, which a headless context embeds as windows in
//! the main one, so they can be found and clicked like any other widget. The tests
//! never touch the real settings file ([`crate::settings::TEST_PATH`]), never open a
//! file dialog ([`zlog_ui::dialog::set_test_pick`]), never reach the network
//! ([`zlog_net::set_offline`], and no Connect in the network windows), never start
//! the sound card, and open no serial port: the rig and the keyer use simulators.
//!
//! [`crawl`] does the clicking: from a starting state it activates every control
//! once, and whatever a control brings up (a menu, a submenu, a dialog, a page) is
//! crawled in turn, replaying the clicks that led there.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use egui::accesskit::{NodeId, Role};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;

use crate::app::ZLogApp;
use crate::i18n::tr;
use crate::session::Setup;

/// The language is process-wide, so these tests run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// A test's own folder, settings file and language; removed when dropped.
struct Env {
    dir: PathBuf,
    _serial: MutexGuard<'static, ()>,
}

impl Env {
    fn new(name: &str) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set_language("en");
        zlog_net::set_offline(true);
        let dir = std::env::temp_dir().join(format!("zlog-gui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
        pick(None);
        Env {
            dir,
            _serial: serial,
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        crate::i18n::set_language("en");
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        pick(None);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A text of the QSO edit dialog and its commands, in English.
fn edit_text(key: &str) -> String {
    zlog_edit::text("en", key)
}

/// What the file dialogs answer from now on.
fn pick(path: Option<&Path>) {
    zlog_ui::dialog::set_test_pick(path.map(Path::to_path_buf));
}

/// Seconds between frames: short enough for two clicks to make a double click.
const STEP: f32 = 0.05;

type H = Harness<'static, ZLogApp>;

fn harness() -> H {
    let mut h = Harness::builder()
        .with_size([1400.0, 1000.0])
        .with_accessibility_check(false)
        // Japanese text needs a system font, which a test machine may not have.
        .allow_missing_glyphs()
        .with_step_dt(STEP)
        .build_eframe(|_| ZLogApp::new());
    settle(&mut h);
    h
}

/// Run a few frames, as a person waits between clicks.
fn settle(h: &mut H) {
    h.run_steps(3);
}

/// A control on screen.
#[derive(Debug, Clone)]
struct Ctl {
    id: NodeId,
    role: Role,
    label: String,
    /// Title of the window it is in; empty in the main window.
    window: String,
}

impl std::fmt::Display for Ctl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {:?} {:?}", self.window, self.role, self.label)
    }
}

/// Roles a person can click, type into or change.
const ACTIVE: &[Role] = &[
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

fn node_id(n: &egui_kittest::Node<'_>) -> NodeId {
    n.accesskit_node().locate().0
}

fn window_of(n: &egui_kittest::Node<'_>) -> String {
    let mut p = n.parent();
    while let Some(w) = p {
        if w.accesskit_node().role() == Role::Window {
            return w.accesskit_node().label().unwrap_or_default();
        }
        p = w.parent();
    }
    String::new()
}

/// Every enabled control on screen, in tree order.
fn controls(h: &H) -> Vec<Ctl> {
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

fn ids(h: &H) -> HashSet<NodeId> {
    h.root().children_recursive().map(|n| node_id(&n)).collect()
}

/// Activate the control `c` as a person would (click, type, step); false when
/// it is not on screen. A control drawn again under another identifier (a menu
/// of another row, say) is found by its window, role and label.
fn act(h: &mut H, c: &Ctl) -> bool {
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

/// Where a crawl starts, and what it must not touch.
struct Crawl<'a> {
    /// Back to the starting state (dialogs closed, the log open).
    reset: &'a dyn Fn(&mut H),
    /// Bring up what is crawled (a menu, a dialog); its new controls are crawled.
    /// `None`: every control on screen.
    open: Option<&'a dyn Fn(&mut H)>,
    /// Controls never activated (the network, the sound card, real ports).
    deny: &'a dyn Fn(&Ctl) -> bool,
    /// At most this many activations.
    budget: usize,
}

impl Crawl<'_> {
    /// Back to the starting state and through `path`; false when a step is gone.
    fn replay(&self, h: &mut H, path: &[Ctl]) -> bool {
        (self.reset)(h);
        if let Some(open) = self.open {
            open(h);
        }
        path.iter().all(|c| act(h, c))
    }

    /// Activate every control once; returns what was activated.
    fn run(&self, h: &mut H) -> Vec<String> {
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

/// Network, sound, file dialogs of extensions and real ports, by window and label.
fn denied(c: &Ctl) -> bool {
    let w = c.window.as_str();
    let l = c.label.as_str();
    let network = matches!(w, "Cluster" | "Z-Link")
        && matches!(l, "Connect" | "Send spot" | "Send" | "Disconnect");
    let update =
        w.starts_with("Update") && matches!(l, "Check for the nightly" | "Update" | "Restart now");
    // OpenStreetMap tiles come from the network.
    let tiles = l.starts_with("Background (OSM)");
    // The keyer is connected in its own test, to the simulated keyer only.
    let keyer = w == "CW Keyer" && matches!(l, "Connect" | "WinKeyer" | "Serial port (DTR/RTS)");
    // CW4ISR's Start opens the sound card.
    let audio = (w == "CW4ISR" && l.contains("Start"))
        || (w == "Voice memories" && matches!(l, "Find devices" | "Record" | "Browse..."));
    // Extensions' own file dialogs (rfd) would block.
    let files = (w.contains("Check") && l == "Open...")
        || (w == "QSO rate" && matches!(l, "From a past log..." | "Save..."))
        || (w == "Submission" && l == "Save...");
    let port = l.starts_with("/dev/") || l.starts_with("COM");
    // Adding ZyLO plugins opens a file dialog.
    let plugins = l == "Add plugins...";
    // Start everyday logging opens a log in the documents folder, outside the test's folder.
    let quick = l == "Start everyday logging";
    // The toolbar's Connect buttons of the cluster and Z-Link.
    let toolbar = w.is_empty() && matches!(l, "Connect" | "Connecting..." | "Disconnect");
    network || toolbar || update || tiles || keyer || audio || files || port || quick || plugins
}

/// Click the control labelled `label` (the first one on screen).
fn click(h: &mut H, label: &str) {
    let c = find(h, label).unwrap_or_else(|| panic!("no control {label:?}"));
    assert!(act(h, &c));
}

/// Whether a control's label is `label`, or `label` and its shortcut ("Save Ctrl+S").
fn labelled(c: &Ctl, label: &str) -> bool {
    c.label == label
        || c.label
            .strip_prefix(label)
            .is_some_and(|rest| rest.starts_with(' '))
}

fn find(h: &H, label: &str) -> Option<Ctl> {
    controls(h).into_iter().find(|c| labelled(c, label))
}

fn find_in(h: &H, window: &str, label: &str) -> Option<Ctl> {
    controls(h)
        .into_iter()
        .find(|c| c.window == window && labelled(c, label))
}

fn click_in(h: &mut H, window: &str, label: &str) {
    let c =
        find_in(h, window, label).unwrap_or_else(|| panic!("no control {label:?} in {window:?}"));
    assert!(act(h, &c));
}

/// Replace the text of the field `c`.
fn set_text(h: &mut H, c: &Ctl, text: &str) {
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

fn key(h: &mut H, modifiers: Modifiers, k: Key) {
    h.key_press_modifiers(modifiers, k);
    settle(h);
}

fn type_text(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    settle(h);
}

/// Where the text `text` is drawn (a cell of the log grid, say).
fn text_at(h: &H, text: &str) -> Option<Pos2> {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.label().as_deref() == Some(text) || a.value().as_deref() == Some(text)
        })
        .map(|n| n.rect().center())
}

/// Press and release `button` at `pos`, `times` times in quick succession (one
/// event a frame, frames [`STEP`] apart).
fn click_at(h: &mut H, pos: Pos2, button: PointerButton, times: usize) {
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

/// Ask the window to close, as its title bar's close button does (embedded
/// dialogs see the main window's close request).
fn close_request(h: &mut H) {
    h.input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    settle(h);
}

/// Back to the main window with nothing else open, in English.
fn reset_main(h: &mut H) {
    crate::i18n::set_language("en");
    h.state_mut().close_dialogs();
    egui::Popup::close_all(&h.ctx);
    h.key_press(Key::Escape);
    settle(h);
}

fn registry_index(id: &str) -> usize {
    crate::registry::registry()
        .contests()
        .iter()
        .position(|c| c.id() == id)
        .unwrap_or_else(|| panic!("contest {id}"))
}

fn setup(dir: &Path, contest: &str, file: Option<PathBuf>) -> Setup {
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

/// A sample contest file of zLog.
fn sample_cfg() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../zlog-rule-ja/data/aktest.cfg")
}

/// Open a log and answer the start-time question of a new log.
fn open_log(h: &mut H, s: Setup) {
    h.state_mut().open_setup(s);
    settle(h);
    if start_asked(h) {
        click(h, tr("button.ok"));
    }
    assert!(h.state().log_state().is_some(), "log open");
}

/// Whether the start-time question is shown (a modal, not a window).
fn start_asked(h: &H) -> bool {
    text_at(h, tr("start.title")).is_some()
}

/// Controls that were not on screen in `before`.
fn new_controls(h: &H, before: &HashSet<NodeId>) -> Vec<Ctl> {
    controls(h)
        .into_iter()
        .filter(|c| !before.contains(&c.id))
        .collect()
}

/// Log a QSO from the entry row.
fn log_qso(h: &mut H, call: &str, number: &str) {
    key(h, Modifiers::COMMAND, Key::K);
    type_text(h, call);
    key(h, Modifiers::NONE, Key::Space);
    type_text(h, number);
    key(h, Modifiers::NONE, Key::Enter);
}

fn report(name: &str, done: &[String]) {
    eprintln!("{name}: {} controls activated", done.len());
    for d in done {
        eprintln!("  {d}");
    }
}

/// The zLog Menu: every group, contest, mode, station field and power button,
/// the CFG picker and its buttons, then OK for every contest of the registry,
/// each opening its main window, and Exit.
#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-gui -- --ignored"]
fn zlog_menu_every_control_and_every_contest() {
    let env = Env::new("menu");
    let mut h = harness();
    pick(Some(&sample_cfg()));
    let reset = |h: &mut H| {
        crate::i18n::set_language("en");
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

    // Every station field and the power buttons.
    let fields: Vec<Ctl> = controls(&h)
        .into_iter()
        .filter(|c| c.role == Role::TextInput && c.window.is_empty())
        .collect();
    for (i, f) in fields.iter().enumerate() {
        set_text(&mut h, f, &format!("JA{i}"));
    }
    // The first field is the search, which would hide contests.
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

    // OK for every contest, through the form.
    let registry = crate::registry::registry();
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
    }
    // The frame of the click asks to close.
    let exit = find(&h, tr("menu.file.exit")).expect("Exit");
    h.root()
        .children_recursive()
        .find(|n| node_id(n) == exit.id)
        .expect("Exit")
        .click_accesskit();
    h.step();
    let close = h
        .output()
        .viewport_output
        .values()
        .any(|v| v.commands.contains(&egui::ViewportCommand::Close));
    assert!(close, "Exit closes the program");
}

/// The CFG picker lists a folder, a file is chosen and opened; a log whose
/// contest file is gone asks to locate it (Cancel, then Locate).
#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-gui -- --ignored"]
fn cfg_picker_and_missing_contest_file() {
    let env = Env::new("cfg");
    let mut h = harness();
    let cfgs = env.dir.join("cfg");
    std::fs::create_dir_all(&cfgs).unwrap();
    let cfg = cfgs.join("aktest.cfg");
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
    let title = crate::registry::registry()
        .contest_files()
        .iter()
        .find_map(|k| k.title(&cfg))
        .expect("title of aktest.cfg");
    let row = |h: &H| {
        controls(h)
            .into_iter()
            .find(|c| c.window == picker && c.label == title)
            .expect("aktest.cfg listed")
    };
    let r = row(&h);
    assert!(act(&mut h, &r));
    click_in(&mut h, picker, tr("button.ok"));
    assert!(
        find_in(&h, picker, tr("button.ok")).is_none(),
        "window closed"
    );
    // Open again and Cancel; the folder is remembered.
    click(&mut h, zlog_cfg::window::text("en", "cfg_picker.entry"));
    let r = row(&h);
    assert!(act(&mut h, &r));
    click_in(&mut h, picker, tr("button.cancel"));
    // Double click chooses too.
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

    // The contest file is gone: typing the log's name asks to locate it.
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

/// An old .ZLO file with mode-5 QSOs asks whether to convert: No, then Yes.
#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-gui -- --ignored"]
fn old_other_question() {
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

/// The keys of the main window, the log grid's clicks, its right-click menu and
/// the edit dialog, for ALL JA, CQWW and a CFG contest.
#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-gui -- --ignored"]
fn main_window_keys_grid_and_edit_dialog() {
    let env = Env::new("main");
    for (contest, file) in [("allja", None), ("cqww", None), ("cfg", Some(sample_cfg()))] {
        let mut h = harness();
        let mut s = setup(&env.dir, contest, file.clone());
        if file.is_none()
            && !crate::registry::registry()
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
        // Keys of the entry row.
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
        // Enter with an empty callsign, and a dupe.
        key(&mut h, Modifiers::COMMAND, Key::K);
        key(&mut h, Modifiers::NONE, Key::Enter);
        type_text(&mut h, "JA1AAA");
        key(&mut h, Modifiers::NONE, Key::Space);
        key(&mut h, Modifiers::NONE, Key::Space);
        key(&mut h, Modifiers::COMMAND, Key::K);
        // Into the grid and around it; Enter edits, Delete asks, Insert inserts.
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
        // Undo and redo of the delete from the grid.
        let count = |h: &H| h.state().log_state().unwrap().1;
        let deleted = count(&h);
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::COMMAND, Key::Z);
        #[cfg(feature = "undo")]
        assert_eq!(count(&h), deleted + 1, "Ctrl+Z on the grid undoes");
        key(&mut h, Modifiers::COMMAND, Key::Y);
        #[cfg(feature = "undo")]
        assert_eq!(count(&h), deleted, "Ctrl+Y redoes");
        key(&mut h, Modifiers::COMMAND, Key::Z);
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::NONE, Key::Insert);
        if let Some(ok) = find(&h, tr("button.ok")) {
            act(&mut h, &ok);
        }
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
        key(&mut h, Modifiers::NONE, Key::Escape);
        reset_main(&mut h);

        // Single, double and right click on a row.
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
        // Every field of the edit dialog, then OK.
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
        // Closed from its title bar.
        let at = row(&h);
        click_at(&mut h, at, PointerButton::Primary, 2);
        assert!(find_in(&h, edit, tr("button.ok")).is_some());
        close_request(&mut h);
        assert!(find_in(&h, edit, tr("button.ok")).is_none(), "closed");
        // Every control of the edit dialog.
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
        // Every item of the right-click menu and its submenus.
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

/// Every menu of the menu bar, every item and submenu item, and whatever they
/// open (Options and its pages, every extension's window, the rig window, About).
#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-gui -- --ignored"]
fn every_menu_item_and_window() {
    let env = Env::new("menus");
    let mut h = harness();
    let s = setup(&env.dir, "allja", None);
    let path = s.path.clone();
    open_log(&mut h, s.clone());
    log_qso(&mut h, "JA1AAA", "10H");
    log_qso(&mut h, "JA2BBB", "11H");
    // Save As, Import and Export answer with a file of the test.
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
        // The File menu may have opened another log; keep a QSO to select.
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
        // Again with a row selected, for the Edit menu's QSO items.
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
    // The Window menu with dialogs open.
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
    // The language both ways.
    reset(&mut h);
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.language"));
    click(&mut h, "日本語");
    assert_eq!(crate::i18n::current(), "ja");
    egui::Popup::close_all(&h.ctx);
    h.run_steps(3);
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.language"));
    click(&mut h, "English");
    assert_eq!(crate::i18n::current(), "en");
    // The manual is only asked to be opened.
    reset(&mut h);
    click(&mut h, tr("menu.help"));
    click(&mut h, tr("menu.help.manual"));
    assert!(total > 100, "{total} controls");
}

/// Every control of the main window itself (toolbar, entry row) and of every
/// extension's window, each opened twice and closed from its title bar.
#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-gui -- --ignored"]
fn toolbar_entry_row_and_every_panel() {
    let env = Env::new("panels");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    log_qso(&mut h, "JA1AAA", "10H");
    // Every toolbar button, the File group's and the panels', also those hidden
    // by default; the file dialogs answer with a file of the test.
    pick(Some(&env.dir.join("picked")));
    let ids: Vec<String> = crate::registry::panels()
        .iter()
        .map(|p| p.id().to_string())
        .chain(["file.open", "file.save", "file.import", "file.export"].map(String::from))
        .collect();
    for id in &ids {
        h.state_mut().settings_mut().set_toolbar_shown(id, true);
    }
    let file = crate::commands::file_commands(&Default::default(), "en");
    // In each style the buttons are named by their labels, icons only included.
    for style in crate::settings::ToolbarStyle::ALL {
        h.state_mut().settings_mut().set_toolbar_style(style);
        settle(&mut h);
        let file = file
            .iter()
            .filter(|c| c.toolbar)
            .map(|c| ("File".to_string(), c.label.clone()));
        for (title, label) in file.chain(crate::registry::panels().iter().flat_map(|p| {
            let title = p.title("en");
            p.commands("en")
                .into_iter()
                .filter(|b| b.toolbar)
                .map(move |b| (title.clone(), b.label))
        })) {
            // Disabled ones too (the keyer's CQ without a keyer).
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
            // The menus have a test of their own; Open closes the log (below).
            denied(c)
                || (c.window.is_empty() && c.role == Role::Button && is_menu(&c.label))
                || (c.window.is_empty() && c.label == tr("toolbar.open"))
        },
        budget: 300,
    }
    .run(&mut h);
    report("main window", &done);
    // Save is disabled: logging a QSO saved the log.
    for b in ["Import", "Export"] {
        assert!(done.iter().any(|d| d.contains(b)), "toolbar {b} crawled");
    }
    // Each panel from its menu, twice, closed from the title bar.
    let panels: Vec<(String, String, zlog_ui::MenuGroup)> = crate::registry::panels()
        .iter()
        .filter(|p| p.has_window())
        .map(|p| {
            let item = zlog_ui::panel_commands(p.as_ref(), false, "en")[0]
                .menu_text()
                .to_string();
            (p.title("en"), item, p.menu())
        })
        .collect();
    for (title, item, group) in panels {
        for _ in 0..2 {
            reset_main(&mut h);
            click(&mut h, tr(crate::menus::group_key(group)));
            click(&mut h, &item);
            h.run_steps(5);
            assert!(
                controls(&h).iter().any(|c| c.window == title) || window_shown(&h, &title),
                "{title} open"
            );
            close_request(&mut h);
        }
    }
    // The toolbar's Open closes the log for the zLog Menu.
    reset_main(&mut h);
    click(&mut h, tr("toolbar.open"));
    settle(&mut h);
    assert!(h.state().log_state().is_none(), "Open closed the log");
}

fn window_shown(h: &H, title: &str) -> bool {
    h.root().children_recursive().any(|n| {
        n.accesskit_node().role() == Role::Window
            && n.accesskit_node().label().as_deref() == Some(title)
    })
}

fn is_menu(label: &str) -> bool {
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

/// The rig through the simulator: connect, tune, the rig window's controls,
/// disconnect; the CW keyer with the simulated keyer.
#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-gui -- --ignored"]
fn rig_simulator_and_keyer() {
    let env = Env::new("rig");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    click(&mut h, tr("menu.group.device"));
    click(&mut h, tr("menu.device.connect"));
    let rig = tr("window.rig");
    click_in(&mut h, rig, tr("rig.connect"));
    assert!(h.state().rig_connected());
    h.run_steps(10);
    let open = |h: &mut H| {
        if !window_shown(h, rig) {
            click(h, tr("menu.group.device"));
            click(h, tr("menu.rig.window"));
        }
    };
    let keep = |h: &mut H| {
        reset_main(h);
        if !h.state().rig_connected() {
            click(h, tr("menu.group.device"));
            click(h, tr("menu.device.connect"));
            click_in(h, rig, tr("rig.connect"));
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
    // Tune the simulator's dial and log on the new frequency.
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
    click(&mut h, tr("menu.group.device"));
    click(&mut h, tr("menu.device.disconnect"));
    assert!(!h.state().rig_connected());

    // The keyer: the simulated one, connected and crawled, then disconnected.
    let keyer = "CW Keyer";
    reset_main(&mut h);
    click(&mut h, tr("menu.group.device"));
    if find(&h, keyer).is_none() {
        return;
    }
    click(&mut h, keyer);
    let device = controls(&h)
        .into_iter()
        .find(|c| c.window == keyer && c.role == Role::ComboBox)
        .expect("keyer device");
    act(&mut h, &device);
    click(&mut h, "Simulated");
    click_in(&mut h, keyer, "Connect");
    let open = |h: &mut H| {
        if !window_shown(h, keyer) {
            click(h, tr("menu.group.device"));
            click(h, keyer);
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

/// The start-time dialog (File menu, and for a new log), and the delete
/// confirmation, each with OK and Cancel.
#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-gui -- --ignored"]
fn start_time_and_delete_confirmation() {
    let env = Env::new("start");
    let mut h = harness();
    h.state_mut().open_setup(setup(&env.dir, "allja", None));
    settle(&mut h);
    // A new log asks first: Cancel keeps the suggested time.
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
        click(&mut h, tr("start.judge"));
        click(&mut h, answer);
        assert!(!start_asked(&h), "{answer} closes it");
    }
    // Delete from the right-click menu: Cancel, then OK.
    for answer in [tr("button.cancel"), tr("button.ok")] {
        reset_main(&mut h);
        let at = text_at(&h, "JA1AAA").expect("row");
        click_at(&mut h, at, PointerButton::Secondary, 1);
        click(&mut h, &edit_text("delete"));
        click(&mut h, answer);
    }
    assert_eq!(h.state().log_state().map(|(_, n)| n), Some(0));
}
