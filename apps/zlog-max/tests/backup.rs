use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use zlog_gui::testing::app::ZLogApp;
use zlog_gui::testing::i18n::tr;
use zlog_gui::testing::session::Setup;

type H = Harness<'static, ZLogApp>;

fn settle(h: &mut H) {
    h.run_steps(3);
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

fn setup(dir: &Path) -> Setup {
    let contest = zlog_gui::testing::registry::registry()
        .contests()
        .iter()
        .position(|c| c.id() == "allja")
        .unwrap();
    Setup {
        contest,
        contest_file: None,
        mode: zlog_core::ContestMode::Mix,
        station: zlog_core::Station {
            callsign: "JA1ZLO".into(),
            ..Default::default()
        },
        coeff: 1.0,
        path: dir.join("allja.zlox"),
        use_contest_period: false,
        convert_old_other: false,
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn alt_b_asks_for_a_folder_and_each_change_is_backed_up() {
    zlog_gui::testing::registry::install(zlog_max::register);
    zlog_gui::testing::i18n::set_language("en");
    zlog_net::set_offline(true);
    let dir: PathBuf = std::env::temp_dir().join(format!("zlog-gui-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    zlog_gui::testing::settings::TEST_PATH
        .with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
    let folder = dir.join("bak");
    zlog_ui::dialog::set_test_pick(Some(folder.clone()));
    let mut h = Harness::builder()
        .with_size([1400.0, 1000.0])
        .with_accessibility_check(false)
        .allow_missing_glyphs()
        .with_step_dt(0.05)
        .build_eframe(|_| ZLogApp::new());
    settle(&mut h);
    h.state_mut().open_setup(setup(&dir));
    settle(&mut h);
    if let Some(ok) = h.query_by_label(tr("button.ok")) {
        ok.click();
        settle(&mut h);
    }
    assert!(h.state().log_state().is_some(), "log open");
    let copy = folder.join("allja.zlox.BAK");
    key(&mut h, Modifiers::ALT, Key::B);
    assert!(copy.exists(), "backup after Alt+B");
    assert_eq!(
        std::fs::read(&copy).unwrap(),
        std::fs::read(dir.join("allja.zlox")).unwrap()
    );
    zlog_ui::dialog::set_test_pick(None);
    let before = std::fs::read(&copy).unwrap().len();
    key(&mut h, Modifiers::COMMAND, Key::K);
    type_text(&mut h, "JA1YAA");
    key(&mut h, Modifiers::NONE, Key::Space);
    type_text(&mut h, "10H");
    key(&mut h, Modifiers::NONE, Key::Enter);
    let start = std::time::Instant::now();
    while std::fs::read(&copy).unwrap().len() == before && start.elapsed() < zlog_backup::SETTLE * 3
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
        settle(&mut h);
    }
    let after = std::fs::read(&copy).unwrap();
    assert!(after.len() > before, "backup after the QSO");
    assert_eq!(after, std::fs::read(dir.join("allja.zlox")).unwrap());
    drop(h);
    zlog_gui::testing::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
    let _ = std::fs::remove_dir_all(&dir);
}
