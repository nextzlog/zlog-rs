use zlog_gui::testing::i18n::tr;
use zlog_testing::{click, click_in, harness, open_log, settle, setup, Env, H};

fn init() {
    zlog_gui::testing::registry::install(with_probe_keyer);
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn every_extension_window_and_menu_item() {
    init();
    zlog_testing::every_added_window_and_menu_item(zlog_std::register);
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn shortcuts_tab_frame_time() {
    init();
    let env = Env::new("bind-frames");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "base", None));
    click(&mut h, tr("menu.settings"));
    click(&mut h, tr("menu.settings.options"));
    click(&mut h, "Shortcuts");
    settle(&mut h);
    let frames = 200;
    let start = std::time::Instant::now();
    for _ in 0..frames {
        h.step();
    }
    let each = start.elapsed() / frames;
    eprintln!("shortcuts tab frame: {each:?}");
    assert!(each < std::time::Duration::from_millis(100), "{each:?}");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn the_ft8_window_opens_with_its_controls() {
    init();
    let env = Env::new("ft8");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    let panel = zlog_gui::testing::registry::panels()
        .into_iter()
        .find(|p| p.id() == "ft8")
        .expect("registered");
    let window = zlog_ui::panel_commands(panel.as_ref(), false, "en")
        .into_iter()
        .find(|c| c.is_window())
        .expect("window command");
    click(
        &mut h,
        tr(zlog_gui::testing::menus::group_key(panel.menu())),
    );
    click(&mut h, &window.menu_text());
    h.run_steps(5);
    let title = window.window_title();
    let shown = zlog_testing::controls(&h);
    for label in ["FT8", "FT4", "Even", "Odd", "Enable Tx", "Open…", "Start"] {
        assert!(
            shown
                .iter()
                .any(|c| c.window == title && zlog_testing::labelled(c, label)),
            "{label}"
        );
    }
    click_in(&mut h, &title, "Odd");
    h.run_steps(3);
    zlog_testing::close_request(&mut h);
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn shift_v_toggles_the_so2r_tx_rig_and_types_nothing() {
    use egui::{Event, Key, Modifiers};
    init();
    let env = Env::new("so2r-tx");
    {
        let mut h = harness();
        let s = h.state_mut().settings_mut();
        s.section("panel.so2r")
            .insert("device".into(), "sim".into());
        let keyer = s.section("panel.keyer");
        for (k, v) in [
            ("device", "probe"),
            ("F1", "E"),
            ("speed", "50"),
            ("cq_repeat", "0"),
            ("cq_max", "1000"),
        ] {
            keyer.insert(k.into(), v.into());
        }
        s.save().unwrap();
    }
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    h.state_mut()
        .set_entry(zlog_gui::testing::entry::Entry::new(
            zlog_core::Band::B7,
            zlog_core::Mode::Cw,
            zlog_core::Power::P010,
        ));
    h.state_mut().focus_entry(zlog_ui::EntryField::Call);
    h.run_steps(10);
    probe::take();
    zlog_testing::key(&mut h, Modifiers::COMMAND, Key::Z);
    assert!(probe::sends_within(&mut h, 2) >= 2, "Ctrl+Z repeats CQ");
    zlog_testing::type_text(&mut h, "JA1");
    h.run_steps(5);
    assert!(zlog_testing::label_starting(&h, "TX1").is_some());
    zlog_testing::key(&mut h, Modifiers::SHIFT, Key::Z);
    assert!(probe::sends_within(&mut h, 2) >= 2, "Shift+Z repeats CQ");
    h.input_mut().events.extend([
        Event::Key {
            key: Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        },
        Event::Text("V".into()),
    ]);
    settle(&mut h);
    assert!(zlog_testing::label_starting(&h, "TX2").is_some());
    assert_eq!(h.state().entry().unwrap().callsign, "JA1");
    assert!(
        probe::take().contains(&"abort".to_string()),
        "Shift+V stops CQ"
    );
    probe::wait(&mut h);
    assert_eq!(probe::sends(&probe::take()), 0, "CQ repeat stopped");
    h.input_mut().events.push(Event::Key {
        key: Key::V,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::SHIFT,
    });
    settle(&mut h);
    assert!(zlog_testing::label_starting(&h, "TX1").is_some());
    assert!(zlog_testing::label_starting(&h, "TX2").is_none());
    zlog_testing::close_request(&mut h);
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn the_cw4isr_extension_page_checks_patterns_and_keeps_its_settings() {
    use egui::accesskit::{Role, Toggled};
    use egui_kittest::kittest::NodeT;
    init();
    let env = Env::new("cw4isr-ext");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    let open_page = |h: &mut H| {
        click(h, tr("menu.settings"));
        click(h, tr("menu.settings.options"));
        click_in(h, tr("options.title"), "CW4ISR");
        settle(h);
    };
    let enable = "Fill the entry row from a clicked station";
    let ticked = |h: &H| {
        h.root().children_recursive().any(|n| {
            let a = n.accesskit_node();
            a.role() == Role::CheckBox
                && a.label().as_deref() == Some(enable)
                && a.toggled() == Some(Toggled::True)
        })
    };
    let pattern = |h: &H| {
        zlog_testing::controls(h)
            .into_iter()
            .find(|c| c.role == Role::TextInput && c.window == "Options")
            .expect("callsign pattern")
    };
    open_page(&mut h);
    assert!(ticked(&h), "on by default");
    click(&mut h, enable);
    assert!(!ticked(&h));
    click(&mut h, enable);
    assert!(ticked(&h));
    let field = pattern(&h);
    zlog_testing::set_text(&mut h, &field, "[A-Z");
    assert!(zlog_testing::label_starting(&h, "Invalid callsign pattern").is_some());
    let field = pattern(&h);
    zlog_testing::set_text(&mut h, &field, "JA[0-9][A-Z]{3}");
    assert!(zlog_testing::label_starting(&h, "Invalid callsign pattern").is_none());
    click_in(&mut h, "Options", "OK");
    let section = h
        .state_mut()
        .settings_mut()
        .section("panel.cw-decoder")
        .clone();
    assert_ne!(section.get("extension").map(String::as_str), Some("false"));
    assert_eq!(
        section.get("extension.callsign").map(String::as_str),
        Some("JA[0-9][A-Z]{3}")
    );
    open_page(&mut h);
    assert!(ticked(&h), "the setting persists");
    click_in(&mut h, "Options", "Cancel");
    zlog_testing::close_request(&mut h);
}

fn with_probe_keyer(b: &mut zlog_ui::Builder) {
    zlog_max::register(b);
    b.driver::<dyn zlog_keyer::KeyerDrv>(0, || Box::new(probe::ProbeKeyer));
}

mod probe {
    use std::sync::{Arc, Mutex, OnceLock};

    use zlog_keyer::{Keyer, KeyerDrv, KeyerEvent};
    use zlog_ui::device::{DeviceDriver, PortHub};
    use zlog_ui::Section;

    fn sent() -> Arc<Mutex<Vec<String>>> {
        static SENT: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();
        SENT.get_or_init(Arc::default).clone()
    }

    pub fn take() -> Vec<String> {
        std::mem::take(&mut *sent().lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn sends(sent: &[String]) -> usize {
        sent.iter().filter(|s| *s == "send E").count()
    }

    pub fn sends_within(h: &mut super::H, n: usize) -> usize {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut count = 0;
        while count < n && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(30));
            h.run_steps(1);
            count += sends(&take());
        }
        count
    }

    pub fn wait(h: &mut super::H) {
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(30));
            h.run_steps(1);
        }
    }

    pub struct ProbeKeyer;

    impl DeviceDriver for ProbeKeyer {
        fn id(&self) -> &str {
            "probe"
        }

        fn title(&self, _language: &str) -> String {
            "probe".into()
        }

        fn simulated(&self) -> bool {
            true
        }
    }

    impl KeyerDrv for ProbeKeyer {
        fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
            let timing = zlog_keyer::timing_of(settings);
            Ok(Box::new(Recording(zlog_keyer_sim::SimKeyer::new(timing))))
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
            self.0.set_wpm(wpm);
        }

        fn wpm(&self) -> u32 {
            self.0.wpm()
        }

        fn set_ptt(&mut self, on: bool) {
            self.0.set_ptt(on);
        }

        fn busy(&self) -> bool {
            self.0.busy()
        }

        fn poll(&mut self) -> Vec<KeyerEvent> {
            self.0.poll()
        }
    }
}
