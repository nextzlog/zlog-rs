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

fn init() {
    zlog_gui::testing::registry::install(zlog_std::register);
}

static SERIAL: Mutex<()> = Mutex::new(());

struct Env {
    dir: PathBuf,
    _serial: MutexGuard<'static, ()>,
}

impl Env {
    fn new(name: &str) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        zlog_gui::testing::i18n::set_language("en");
        zlog_net::set_offline(true);
        let dir = std::env::temp_dir().join(format!("zlog-gui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        zlog_gui::testing::settings::TEST_PATH
            .with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
        pick(None);
        Env {
            dir,
            _serial: serial,
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        zlog_gui::testing::i18n::set_language("en");
        zlog_gui::testing::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        pick(None);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn pick(path: Option<&Path>) {
    zlog_ui::dialog::set_test_pick(path.map(Path::to_path_buf));
}

const STEP: f32 = 0.05;

type H = Harness<'static, ZLogApp>;

fn harness() -> H {
    let mut h = Harness::builder()
        .with_size([1400.0, 1000.0])
        .with_accessibility_check(false)
        .allow_missing_glyphs()
        .with_step_dt(STEP)
        .build_eframe(|_| ZLogApp::new());
    settle(&mut h);
    h
}

fn settle(h: &mut H) {
    h.run_steps(3);
}

#[derive(Debug, Clone)]
struct Ctl {
    id: NodeId,
    role: Role,
    label: String,
    window: String,
}

impl std::fmt::Display for Ctl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {:?} {:?}", self.window, self.role, self.label)
    }
}

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

struct Crawl<'a> {
    reset: &'a dyn Fn(&mut H),
    open: Option<&'a dyn Fn(&mut H)>,
    deny: &'a dyn Fn(&Ctl) -> bool,
    budget: usize,
}

impl Crawl<'_> {
    fn replay(&self, h: &mut H, path: &[Ctl]) -> bool {
        (self.reset)(h);
        if let Some(open) = self.open {
            open(h);
        }
        path.iter().all(|c| act(h, c))
    }

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

fn denied(c: &Ctl) -> bool {
    let w = c.window.as_str();
    let l = c.label.as_str();
    let network = matches!(w, "PacketCluster" | "Z-Link")
        && [
            "Connect",
            "Send spot",
            "Send",
            "Disconnect",
            "Join",
            "Search LAN",
            "Start Z-Link server",
        ]
        .iter()
        .any(|x| l.ends_with(x));
    let update =
        w.starts_with("Update") && matches!(l, "Check for the nightly" | "Update" | "Restart now");
    let tiles = l.starts_with("Background (OSM)");
    let keyer = w == "CW keyer" && matches!(l, "Connect" | "WinKeyer" | "Serial port (DTR/RTS)");
    let audio = (w == "CW4ISR" && l.contains("Start"))
        || (w == "Voice memory" && matches!(l, "Find devices" | "Record" | "Browse…"));
    let files = (w.contains("Check") && l == "Open…")
        || (w == "QSO rate" && matches!(l, "From a past log…" | "Save…"))
        || (w == "Submission" && l == "Save…");
    let port = l.starts_with("/dev/") || l.starts_with("COM");
    let plugins = l == "Add plugins…";
    let quick = l == "Start everyday logging";
    let toolbar = w.is_empty()
        && [
            "Connect",
            "Connecting…",
            "Disconnect",
            "Settings",
            "Stop TX",
        ]
        .iter()
        .any(|x| l.ends_with(x));
    network || toolbar || update || tiles || keyer || audio || files || port || quick || plugins
}

fn click(h: &mut H, label: &str) {
    let c = find(h, label).unwrap_or_else(|| panic!("no control {label:?}"));
    assert!(act(h, &c));
}

fn labelled(c: &Ctl, label: &str) -> bool {
    c.label == label
        || c.label
            .strip_prefix(label)
            .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with('…'))
}

fn find(h: &H, label: &str) -> Option<Ctl> {
    controls(h).into_iter().find(|c| labelled(c, label))
}

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

fn text_at(h: &H, text: &str) -> Option<Pos2> {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.label().as_deref() == Some(text) || a.value().as_deref() == Some(text)
        })
        .map(|n| n.rect().center())
}

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

fn close_request(h: &mut H) {
    h.input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    settle(h);
}

fn reset_main(h: &mut H) {
    zlog_gui::testing::i18n::set_language("en");
    h.state_mut().close_dialogs();
    egui::Popup::close_all(&h.ctx);
    h.key_press(Key::Escape);
    settle(h);
}

fn registry_index(id: &str) -> usize {
    zlog_gui::testing::registry::registry()
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

fn open_log(h: &mut H, s: Setup) {
    h.state_mut().open_setup(s);
    settle(h);
    if start_asked(h) {
        click(h, tr("button.ok"));
    }
    assert!(h.state().log_state().is_some(), "log open");
}

fn start_asked(_: &H) -> bool {
    false
}

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

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn zlog_menu_every_control_and_every_contest() {
    init();
    let env = Env::new("menu");
    let mut h = harness();
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

#[test]
fn open_from_log_then_cancel_returns_to_the_log() {
    init();
    let env = Env::new("reopen");
    let mut h = harness();
    assert!(
        find(&h, tr("button.cancel")).is_none(),
        "no Cancel at first launch"
    );
    assert!(
        find(&h, tr("menu.file.exit")).is_some(),
        "Exit at first launch"
    );
    let s = setup(&env.dir, "base", None);
    let path = s.path.clone();
    open_log(&mut h, s);
    log_qso(&mut h, "JA1AAA", "10H");
    h.run_steps(5);
    reset_main(&mut h);
    click(&mut h, tr("menu.file"));
    click(&mut h, tr("menu.file.open"));
    assert!(h.state().log_state().is_none(), "back in the zLog Menu");
    assert!(
        find(&h, tr("menu.file.exit")).is_none(),
        "Cancel replaces Exit"
    );
    click(&mut h, tr("button.cancel"));
    settle(&mut h);
    assert_eq!(h.state().log_path(), Some(path.as_path()));
    assert_eq!(
        h.state().log_state().map(|(_, n)| n),
        Some(1),
        "no data lost"
    );
}

#[test]
fn first_launch_exit_closes_the_program() {
    init();
    let _env = Env::new("first-exit");
    let mut h = harness();
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
    assert!(h.state().log_state().is_none());
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn every_menu_item_and_window() {
    init();
    let env = Env::new("menus");
    let mut h = harness();
    let s = setup(&env.dir, "base", None);
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

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn toolbar_entry_row_and_every_panel() {
    init();
    let env = Env::new("panels");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
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
    assert!(!done.is_empty(), "main window crawled");
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

type Steps = std::sync::Arc<Mutex<Vec<&'static str>>>;

struct ExitPanel(Steps);

impl zlog_ui::Panel for ExitPanel {
    fn id(&self) -> &str {
        "exit-test"
    }

    fn title(&self, _language: &str) -> String {
        "Exit Test".into()
    }

    fn quit_cancelled(&mut self) -> bool {
        self.0.lock().unwrap().push("cancelled");
        false
    }

    fn on_exit(&mut self) {
        self.0.lock().unwrap().push("exit");
    }
}

struct ClosedLink(Steps);

impl zlog_core::Observer for ClosedLink {
    fn name(&self) -> &str {
        "closed-link"
    }

    fn on_event(&mut self, _cx: &mut zlog_core::Context<'_>, event: &zlog_core::LogEvent) {
        if *event == zlog_core::LogEvent::Closed {
            self.0.lock().unwrap().push("detach");
        }
    }
}

impl zlog_core::Link for ClosedLink {
    fn poll(&mut self, _cx: &mut zlog_core::Context<'_>) {}
}

struct ClosedExtension(Steps);

impl zlog_core::Extension for ClosedExtension {
    fn name(&self) -> &str {
        "closed-link"
    }

    fn links(&mut self) -> Vec<Box<dyn zlog_core::Link>> {
        vec![Box::new(ClosedLink(self.0.clone()))]
    }
}

fn steps(steps: &Steps) -> Vec<&'static str> {
    steps.lock().unwrap().clone()
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn panel_exit_hook_runs_once_on_quit() {
    init();
    let _env = Env::new("exit-hook");
    let mut h = harness();
    let seen = Steps::default();
    h.state_mut().push_panel(Box::new(ExitPanel(seen.clone())));
    h.state_mut().fail_save(true);
    close_request(&mut h);
    assert!(steps(&seen).is_empty(), "the save failed");
    click(&mut h, tr("button.cancel"));
    assert_eq!(steps(&seen), ["cancelled"], "cancelled quit");
    close_request(&mut h);
    assert_eq!(steps(&seen), ["cancelled", "exit"], "completed quit");
    h.state_mut().exit();
    close_request(&mut h);
    assert_eq!(steps(&seen), ["cancelled", "exit"], "once per process");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn quitting_closes_the_log_before_the_panels_exit() {
    init();
    let env = Env::new("exit-order");
    let mut h = harness();
    let seen = Steps::default();
    h.state_mut().push_panel(Box::new(ExitPanel(seen.clone())));
    let s = setup(&env.dir, "base", None);
    h.state_mut()
        .open_setup_with(s, ClosedExtension(seen.clone()));
    assert!(h.state().log_state().is_some());
    close_request(&mut h);
    assert_eq!(steps(&seen), ["detach", "exit"]);
    assert!(h.state().log_state().is_none());
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn quitting_without_saving_after_a_failed_save_still_quits_once() {
    init();
    let env = Env::new("exit-unsaved");
    let mut h = harness();
    let seen = Steps::default();
    h.state_mut().push_panel(Box::new(ExitPanel(seen.clone())));
    let s = setup(&env.dir, "base", None);
    h.state_mut()
        .open_setup_with(s, ClosedExtension(seen.clone()));
    h.state_mut().fail_save(true);
    close_request(&mut h);
    assert!(
        steps(&seen).is_empty(),
        "panels keep running while the save failed"
    );
    click(&mut h, tr("unsaved.quit"));
    close_request(&mut h);
    assert_eq!(
        steps(&seen),
        ["detach", "exit"],
        "no quit_cancelled, so a restart is kept"
    );
}

#[test]
fn shortcut_tab_edits_one_command_and_ok_keeps_it() {
    init();
    let env = Env::new("bind");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.settings.options"));
    click(&mut h, "Shortcuts");
    let row = |h: &H| {
        controls(h)
            .into_iter()
            .find(|c| c.role == Role::Button && c.label.starts_with("Save as ZLOX"))
    };
    let save_as = row(&h).expect("Save as ZLOX row");
    assert!(!save_as.label.contains('•'), "{}", save_as.label);
    assert!(act(&mut h, &save_as));
    settle(&mut h);
    let header = |h: &H, name: &str| text_at(h, &format!("File > {name}")).is_some();
    assert!(header(&h, "Save as ZLOX…"));
    key(&mut h, Modifiers::NONE, Key::ArrowDown);
    assert!(header(&h, "Save as ZLO…"));
    key(&mut h, Modifiers::NONE, Key::ArrowUp);
    assert!(header(&h, "Save as ZLOX…"));
    let tip = controls(&h)
        .into_iter()
        .find(|c| c.role == Role::MultilineTextInput)
        .expect("tooltip field");
    set_text(&mut h, &tip, "Mine");
    click(&mut h, "Record");
    key(
        &mut h,
        Modifiers::CTRL | Modifiers::COMMAND | Modifiers::SHIFT,
        Key::L,
    );
    let marked = row(&h).expect("Save as ZLOX row");
    assert!(marked.label.contains('•'), "{}", marked.label);
    click(&mut h, tr("button.ok"));
    let bind = h.state_mut().settings_mut().section("bind").clone();
    assert_eq!(bind["tip:file/file.save_as"], "Mine");
    assert_eq!(bind["file/file.save_as"], "Ctrl+Shift+L");
}

type Typed = std::sync::Arc<Mutex<String>>;

struct TypingPanel(Typed);

impl zlog_ui::Panel for TypingPanel {
    fn id(&self) -> &str {
        "typing-test"
    }

    fn title(&self, _language: &str) -> String {
        "Typing Test".into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {
        let mut text = self.0.lock().unwrap();
        ui.add(egui::TextEdit::singleline(&mut *text).id_salt("typing-test"));
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn typing_in_a_docked_panel_leaves_the_entry_row_alone() {
    init();
    let env = Env::new("docked-typing");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    log_qso(&mut h, "JA1AAA", "10H");
    key(&mut h, Modifiers::NONE, Key::Enter);
    log_qso(&mut h, "JA1BBB", "20H");
    key(&mut h, Modifiers::NONE, Key::Enter);
    let typed = Typed::new(Mutex::new("dock".into()));
    h.state_mut()
        .push_panel(Box::new(TypingPanel(typed.clone())));
    let dock = h.state_mut().settings_mut().section("dock");
    dock.insert("right".into(), "typing-test".into());
    h.state_mut().reload_host();
    h.state_mut().open_panel("typing-test");
    settle(&mut h);
    let field = h
        .root()
        .children_recursive()
        .find(|n| n.accesskit_node().value().as_deref() == Some("dock"))
        .map(|n| Ctl {
            id: node_id(&n),
            role: n.accesskit_node().role(),
            label: String::new(),
            window: window_of(&n),
        })
        .expect("docked field");
    assert!(field.window.is_empty(), "docked, not floating");
    set_text(&mut h, &field, "");
    let band = h.state().entry_band();
    for (k, c) in [
        (Key::B, "B"),
        (Key::M, "M"),
        (Key::P, "P"),
        (Key::O, "O"),
        (Key::X, "X"),
        (Key::R, "R"),
        (Key::S, "S"),
        (Key::T, "T"),
    ] {
        h.key_press_modifiers(Modifiers::SHIFT, k);
        h.event(Event::Text(c.into()));
        settle(&mut h);
    }
    assert_eq!(*typed.lock().unwrap(), "BMPOXRST");
    assert_eq!(h.state().entry_band(), band);
    h.state_mut().select_qso(Some(1));
    for k in [Key::ArrowUp, Key::ArrowDown, Key::Escape] {
        key(&mut h, Modifiers::NONE, k);
        assert_eq!(h.state().selected_qso(), Some(1), "{k:?}");
    }
    assert_eq!(h.state().log_state().map(|l| l.1), Some(2));
}

struct DockFile {
    exported: std::sync::Arc<Mutex<Option<zlog_ui::Section>>>,
    import: Option<zlog_ui::Section>,
}

impl zlog_ui::SettingsFile for DockFile {
    fn id(&self) -> &str {
        "dock-file"
    }

    fn commands(&self, _language: &str) -> Vec<zlog_ui::Command> {
        Vec::new()
    }

    fn run_command(&mut self, _id: &str, env: &mut zlog_ui::SettingsEnv<'_>) {
        *self.exported.lock().unwrap() = env.sections.get("dock").cloned();
    }

    fn dialogs(&mut self, _ctx: &egui::Context, env: &mut zlog_ui::SettingsEnv<'_>) -> bool {
        let Some(s) = self.import.take() else {
            return false;
        };
        env.sections.insert("dock".into(), s);
        true
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn settings_export_and_import_carry_the_dock_layout() {
    init();
    let env = Env::new("dock-file");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    h.state_mut()
        .push_panel(Box::new(TypingPanel(Typed::default())));
    let set = |h: &mut H, area: &str| {
        let dock = h.state_mut().settings_mut().section("dock");
        dock.clear();
        dock.insert(area.into(), "typing-test".into());
    };
    set(&mut h, "right");
    h.state_mut().reload_host();
    h.state_mut().open_panel("typing-test");
    settle(&mut h);
    set(&mut h, "left");
    let exported = std::sync::Arc::new(Mutex::new(None));
    let mut import = zlog_ui::Section::new();
    import.insert("bottom".into(), "typing-test".into());
    h.state_mut().push_settings_file(Box::new(DockFile {
        exported: exported.clone(),
        import: None,
    }));
    let ctx = h.ctx.clone();
    h.state_mut().run_settings_file(&ctx, Some("export"));
    let dock = exported.lock().unwrap().clone().expect("exported [dock]");
    assert_eq!(dock["right"], "typing-test", "{dock:?}");
    h.state_mut().push_settings_file(Box::new(DockFile {
        exported,
        import: Some(import),
    }));
    h.state_mut().run_settings_file(&ctx, None);
    settle(&mut h);
    h.state_mut().save_host();
    let dock = h.state_mut().settings_mut().section("dock").clone();
    assert_eq!(dock["bottom"], "typing-test", "{dock:?}");
    assert_eq!(dock["right"], "", "{dock:?}");
}

fn window_rect(h: &H, title: &str) -> egui::Rect {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.role() == Role::Window && a.label().as_deref() == Some(title)
        })
        .map(|n| n.rect())
        .unwrap_or_else(|| panic!("no window {title:?}"))
}

fn resize_window(h: &mut H, title: &str, width: f32) {
    let r = window_rect(h, title);
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
    let got = window_rect(h, title).width();
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
    let window = window_rect(h, title).expand(1.0);
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

fn assert_options_flex(h: &mut H, pages: &[&str]) {
    let title = tr("options.title");
    click(h, tr("menu.settings"));
    click(h, tr("menu.settings.options"));
    for page in pages {
        click(h, page);
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

#[test]
fn options_fields_follow_the_dialog_width() {
    init();
    let env = Env::new("flex");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    assert_options_flex(
        &mut h,
        &[
            tr("options.station"),
            tr("options.preferences"),
            "Entry row",
            "Shortcuts",
        ],
    );
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

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-app -- --ignored"]
fn options_fields_line_up() {
    init();
    let env = Env::new("aligned");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    let title = tr("options.title");
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.settings.options"));
    for page in [
        tr("options.station"),
        tr("options.preferences"),
        "Entry row",
        "Display",
        "Shortcuts",
        "Toolbar",
    ] {
        click(&mut h, page);
        resize_window(&mut h, title, 480.0);
        let narrow = field_rects(&h, title);
        resize_window(&mut h, title, 900.0);
        let wide = field_rects(&h, title);
        assert_eq!(narrow.len(), wide.len(), "{page}");
        for (width, rects) in [(480, &narrow), (900, &wide)] {
            let flexible: Vec<&egui::Rect> = rects
                .iter()
                .zip(narrow.iter().zip(&wide))
                .filter(|(_, (a, b))| b.width() - a.width() > 100.0)
                .map(|(r, _)| r)
                .collect();
            for r in &flexible {
                let first = flexible[0];
                assert!(
                    (r.min.x - first.min.x).abs() < 1.5 && (r.max.x - first.max.x).abs() < 1.5,
                    "{page} at {width}: field {r:?} is not aligned with {first:?}"
                );
            }
        }
    }
    click(&mut h, tr("button.cancel"));
}
