use zlog_gui::testing::app::files::*;
use zlog_gui::testing::app::keys::*;
use zlog_gui::testing::app::panels::*;
use zlog_gui::testing::app::*;
use zlog_gui::testing::commands::{self};
use zlog_gui::testing::entry::Entry;
use zlog_gui::testing::main_window::View;
use zlog_gui::testing::options::Options;
use zlog_gui::testing::session::Session;

fn init() {
    zlog_gui::testing::registry::install(zlog_max::register);
}

#[test]
fn esc_on_the_overwrite_question_keeps_the_save_failed_dialog() {
    init();
    let dir = std::env::temp_dir().join(format!("zlog-gui-ow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    zlog_gui::testing::settings::TEST_PATH
        .with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
    let mut app = ZLogApp::new();
    app.save_failed = Some(SaveFailed {
        error: "disk full".into(),
        after: After::Quit,
    });
    app.overwrite = Some(Overwrite {
        path: dir.join("x.zlox"),
        then: Then::SaveAs(Some(After::Quit)),
    });
    let ctx = egui::Context::default();
    let esc = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [vec![], vec![esc]] {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.save_failed_dialog(ui.ctx());
            app.overwrite_dialog(ui.ctx());
        });
        out.textures_delta.clear();
    }
    assert!(app.overwrite.is_some(), "the question waits for its answer");
    assert!(
        app.save_failed.is_some(),
        "Esc did not close the dialog under it"
    );
    zlog_gui::testing::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn picking_a_callsign_clears_the_number() {
    init();
    let mut e = Entry::new(
        zlog_core::Band::B7,
        zlog_core::Mode::Cw,
        zlog_core::Power::P010,
    );
    e.callsign = "JA1ZLO".into();
    e.nr_rcvd = "10H".into();
    e.rst_rcvd = "579".into();
    pick_callsign(&mut e, "JA1YAA".into());
    assert_eq!(e.callsign, "JA1YAA");
    assert_eq!(e.nr_rcvd, "");
    assert_eq!(e.rst_rcvd, "579");
    pick_callsign(&mut e, String::new());
    assert_eq!((e.callsign.as_str(), e.nr_rcvd.as_str()), ("", ""));
}

struct Keyer(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl zlog_ui::Panel for Keyer {
    fn id(&self) -> &str {
        "keyer"
    }
    fn title(&self, _language: &str) -> String {
        "keyer".into()
    }
    fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
    fn on_key(&mut self, key: zlog_ui::EntryKey, _view: &zlog_ui::View<'_>) -> bool {
        let escape = key == zlog_ui::EntryKey::Escape;
        self.0.store(escape, std::sync::atomic::Ordering::SeqCst);
        escape
    }
}

#[derive(Default)]
struct Voice(Vec<zlog_ui::EntryAction>);

impl zlog_ui::Panel for Voice {
    fn id(&self) -> &str {
        "voice"
    }
    fn title(&self, _language: &str) -> String {
        "voice".into()
    }
    fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
    fn on_key(&mut self, key: zlog_ui::EntryKey, _view: &zlog_ui::View<'_>) -> bool {
        let escape = key == zlog_ui::EntryKey::Escape;
        if escape {
            self.0.push(zlog_ui::EntryAction::Transmit(false));
        }
        escape
    }
    fn take_actions(&mut self) -> Vec<zlog_ui::EntryAction> {
        std::mem::take(&mut self.0)
    }
}

struct Ptt(std::sync::Arc<std::sync::Mutex<Vec<bool>>>);

impl zlog_ui::Panel for Ptt {
    fn id(&self) -> &str {
        "ptt"
    }
    fn title(&self, _language: &str) -> String {
        "ptt".into()
    }
    fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
    fn transmit(&mut self, on: bool) -> bool {
        self.0.lock().unwrap().push(on);
        true
    }
}

#[test]
fn ctrl_z_with_no_text_field_focused_undoes() {
    init();
    assert!(no_text_to_undo(false, false, false, false), "after Esc");
    assert!(no_text_to_undo(true, true, true, false), "empty entry row");
    assert!(!no_text_to_undo(true, false, true, false), "text typed");
    assert!(
        !no_text_to_undo(false, true, true, false),
        "another text field"
    );
    assert!(!no_text_to_undo(false, true, false, true), "Options open");
}

#[test]
fn ctrl_z_from_an_empty_entry_row_undoes() {
    init();
    let undo = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
    let delete = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete);
    let listed = |enabled: bool| {
        let mut u = zlog_ui::Command::new("undo", "Undo", "").row_key(undo);
        if !enabled {
            u = u.disabled_because("nothing to undo");
        }
        vec![commands::Listed {
            source: commands::Source::Core(zlog_ui::MenuGroup::Edit),
            id: "edit".into(),
            title: "Edit".into(),
            group: zlog_ui::MenuGroup::Edit,
            commands: vec![
                u,
                zlog_ui::Command::new("delete", "Delete", "").row_key(delete),
            ],
        }]
    };
    let ctx = egui::Context::default();
    let press = |k: egui::KeyboardShortcut, all: &[commands::Listed], empty: bool| {
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: k.logical_key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: k.modifiers,
            }],
            ..Default::default()
        };
        let mut got = None;
        let mut out = ctx.run_ui(input, |ui| {
            got = row_command(ui.ctx(), all, false, empty, false);
        });
        out.textures_delta.clear();
        got.map(|r| r.map(|(_, id)| id))
    };
    assert_eq!(press(undo, &listed(true), true), Some(Ok("undo".into())));
    assert_eq!(press(undo, &listed(true), false), None, "text typed");
    assert_eq!(press(delete, &listed(true), true), None, "Delete types");
    assert_eq!(
        press(undo, &listed(false), true),
        Some(Err("nothing to undo".into()))
    );
    let mut e = Entry::new(
        zlog_core::Band::B7,
        zlog_core::Mode::Cw,
        zlog_core::Power::P010,
    );
    e.wipe();
    assert!(nothing_typed(&e));
    e.callsign = "JA1".into();
    assert!(!nothing_typed(&e));
}

#[test]
fn rig_switches_go_first() {
    init();
    use zlog_ui::EntryAction as A;
    let order = device_first(vec![
        A::Transmit(true),
        A::SelectRig(2),
        A::Cq(true),
        A::SelectRig(1),
    ]);
    assert!(matches!(
        order.as_slice(),
        [
            A::SelectRig(2),
            A::SelectRig(1),
            A::Transmit(true),
            A::Cq(true)
        ]
    ));
}

#[test]
fn closing_forwards_ptt() {
    init();
    struct Box2(Vec<zlog_ui::EntryAction>);
    impl zlog_ui::Panel for Box2 {
        fn id(&self) -> &str {
            "so2r"
        }
        fn title(&self, _language: &str) -> String {
            "so2r".into()
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
        fn on_key(&mut self, key: zlog_ui::EntryKey, _view: &zlog_ui::View<'_>) -> bool {
            self.0.push(zlog_ui::EntryAction::Transmit(false));
            key == zlog_ui::EntryKey::Escape
        }
        fn take_actions(&mut self) -> Vec<zlog_ui::EntryAction> {
            std::mem::take(&mut self.0)
        }
    }
    struct Port(std::sync::Arc<std::sync::Mutex<Vec<bool>>>);
    impl zlog_ui::Panel for Port {
        fn id(&self) -> &str {
            "port"
        }
        fn title(&self, _language: &str) -> String {
            "port".into()
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
        fn transmit(&mut self, on: bool) -> bool {
            self.0.lock().unwrap().push(on);
            true
        }
    }
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let slot = |panel: Box<dyn zlog_ui::Panel>| PanelSlot {
        panel,
        open: false,
        was_open: false,
        title: String::new(),
    };
    let mut panels = vec![
        slot(Box::new(Box2(Vec::new()))),
        slot(Box::new(Port(sent.clone()))),
    ];
    let dir = std::env::temp_dir().join(format!("zlog-gui-port-{}", std::process::id()));
    let session = open_in(&dir);
    let view = panel_view(None, None, None, &session, &[], RigNow::default());
    stop_transmitting(&mut panels, &view);
    assert_eq!(*sent.lock().unwrap(), [false]);
    drop(session);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn closing_the_log_turns_ptt_off_at_once() {
    init();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut panels = vec![
        PanelSlot {
            panel: Box::new(Voice::default()),
            open: false,
            was_open: false,
            title: String::new(),
        },
        PanelSlot {
            panel: Box::new(Ptt(asked.clone())),
            open: false,
            was_open: false,
            title: String::new(),
        },
    ];
    let dir = std::env::temp_dir().join(format!("zlog-gui-ptt-{}", std::process::id()));
    let session = open_in(&dir);
    let view = panel_view(None, None, None, &session, &[], RigNow::default());
    stop_transmitting(&mut panels, &view);
    assert_eq!(*asked.lock().unwrap(), [false]);
    std::fs::remove_dir_all(dir).unwrap();
}

fn open_in(dir: &std::path::Path) -> Session {
    std::fs::create_dir_all(dir).unwrap();
    let reg = zlog_core::Registry::new()
        .with(zlog_cfg::Cfg)
        .with(zlog_rule_ja::Japan);
    let setup = zlog_gui::testing::session::Setup {
        contest: 0,
        contest_file: None,
        mode: zlog_core::ContestMode::Mix,
        station: zlog_core::Station::default(),
        coeff: 1.0,
        path: dir.join("log.zlox"),
        use_contest_period: true,
        convert_old_other: false,
    };
    Session::open(setup, reg).unwrap()
}

#[test]
fn escape_aborts_cw_before_the_grid_sees_it() {
    init();
    let dir = std::env::temp_dir().join(format!("zlog-gui-esc-{}", std::process::id()));
    let mut session = open_in(&dir);
    let mut view = View {
        selected: Some(0),
        ..View::default()
    };
    let aborted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut panels = vec![PanelSlot {
        panel: Box::new(Keyer(aborted.clone())),
        open: false,
        was_open: false,
        title: String::new(),
    }];
    let options = Options::new(Vec::new());
    let rig = RigNow::default();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        route_keys(
            ui.ctx(),
            &mut session,
            &mut view,
            None,
            &mut panels,
            &options,
            &[],
            rig,
        );
    });
    out.textures_delta.clear();
    assert!(
        aborted.load(std::sync::atomic::Ordering::SeqCst),
        "CW aborted"
    );
    assert_eq!(view.selected, Some(0), "the grid did not get Esc");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn enter_messages_reach_the_panel_of_the_mode() {
    init();
    type Got = std::sync::Arc<std::sync::Mutex<Vec<(&'static str, u8)>>>;
    struct Player(&'static str, bool, Got);
    impl zlog_ui::Panel for Player {
        fn id(&self) -> &str {
            self.0
        }
        fn title(&self, _language: &str) -> String {
            self.0.into()
        }
        fn on_key(&mut self, key: zlog_ui::EntryKey, view: &zlog_ui::View<'_>) -> bool {
            let phone = view.mode != zlog_core::Mode::Cw;
            match key {
                zlog_ui::EntryKey::Reserved { n } if phone == self.1 => {
                    self.2.lock().unwrap().push((self.0, n));
                    true
                }
                _ => false,
            }
        }
    }
    let got = Got::default();
    let slot = |panel: Box<dyn zlog_ui::Panel>| PanelSlot {
        panel,
        open: false,
        was_open: false,
        title: String::new(),
    };
    let mut panels = vec![
        slot(Box::new(Player("keyer", false, got.clone()))),
        slot(Box::new(Player("voice", true, got.clone()))),
    ];
    let dir = std::env::temp_dir().join(format!("zlog-gui-enter-{}", std::process::id()));
    let mut session = open_in(&dir);
    let mut view = View::default();
    for (mode, n) in [(zlog_core::Mode::Ssb, 1), (zlog_core::Mode::Cw, 3)] {
        session.entry.mode = mode;
        view.sends.push(n);
        let played = send_messages(
            &mut view,
            &session,
            &mut panels,
            None,
            &[],
            RigNow::default(),
        );
        assert_eq!(played, Some(true));
    }
    assert!(view.sends.is_empty());
    session.entry.mode = zlog_core::Mode::Rtty;
    view.sends.push(2);
    panels.remove(1);
    let played = send_messages(
        &mut view,
        &session,
        &mut panels,
        None,
        &[],
        RigNow::default(),
    );
    assert_eq!(played, Some(false));
    assert_eq!(*got.lock().unwrap(), [("voice", 1), ("keyer", 3)]);
    drop(session);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn entry_shortcuts_work_with_a_grid_row_selected() {
    init();
    let dir = std::env::temp_dir().join(format!("zlog-gui-shift-{}", std::process::id()));
    let mut session = open_in(&dir);
    let mut view = View {
        selected: Some(0),
        ..View::default()
    };
    let mut style = zlog_gui::testing::registry::entry_style("zlog").unwrap();
    let options = Options::new(Vec::new());
    let rig = RigNow::default();
    let ctx = egui::Context::default();
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let mut press = |e: egui::Event, session: &mut Session, view: &mut View| {
        let input = egui::RawInput {
            events: vec![e],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            route_keys(
                ui.ctx(),
                session,
                view,
                Some(style.as_mut()),
                &mut [],
                &options,
                &[],
                rig,
            );
        });
        out.textures_delta.clear();
    };
    let band = session.entry.band;
    press(
        key(egui::Key::B, egui::Modifiers::SHIFT),
        &mut session,
        &mut view,
    );
    assert_ne!(session.entry.band, band, "Shift+B changed the band");
    assert_eq!(view.selected, Some(0));
    press(
        key(egui::Key::C, egui::Modifiers::ALT),
        &mut session,
        &mut view,
    );
    assert_eq!(view.selected, None, "Alt+C left the grid");
    std::fs::remove_dir_all(dir).unwrap();
}

struct Adds(zlog_ui::MenuGroup, &'static str);

impl zlog_ui::Panel for Adds {
    fn id(&self) -> &str {
        self.1
    }
    fn title(&self, _language: &str) -> String {
        self.1.into()
    }
    fn menu(&self) -> zlog_ui::MenuGroup {
        self.0
    }
    fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
    fn qso_menu(&self, _language: &str, qso: &zlog_core::Qso) -> Vec<zlog_ui::QsoMenuItem> {
        vec![zlog_ui::QsoMenuItem::new(self.1, &qso.callsign)]
    }
}

#[test]
fn qso_menu_items_are_grouped_by_menu_group() {
    init();
    use zlog_ui::MenuGroup as G;
    let slot = |g, id| PanelSlot {
        panel: Box::new(Adds(g, id)),
        open: false,
        was_open: false,
        title: String::new(),
    };
    let panels = [
        slot(G::Network, "cluster"),
        slot(G::Search, "search"),
        slot(G::Network, "soumu"),
    ];
    let q = zlog_core::Qso {
        callsign: "JA1ZLO".into(),
        ..Default::default()
    };
    let items = qso_items(&panels, &q);
    let ids: Vec<Vec<(usize, String)>> = items
        .iter()
        .map(|g| g.iter().map(|(i, it)| (*i, it.id.clone())).collect())
        .collect();
    assert_eq!(
        ids,
        [
            vec![(1, "search".to_string())],
            vec![(0, "cluster".to_string()), (2, "soumu".to_string())],
        ]
    );
    assert_eq!(items[0][0].1.label, "JA1ZLO");
}
