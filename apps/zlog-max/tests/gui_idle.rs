use zlog_gui::testing::i18n::tr;
use zlog_testing::{harness, open_log, rig_text, settle, setup, Env, H};

fn init() {
    zlog_gui::testing::registry::install(zlog_max::register);
}

fn assert_clock_only(h: &mut H, what: &str) {
    for _ in 0..4 {
        settle(h);
    }
    for n in 0..40 {
        let left = until_the_minute();
        h.step();
        let delay = h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(
            delay + std::time::Duration::from_millis(50) >= left,
            "{what}: frame {n} repaints after {delay:?}, the minute ends in {left:?}: {:?}",
            h.ctx.repaint_causes()
        );
    }
}

fn until_the_minute() -> std::time::Duration {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    std::time::Duration::from_millis(60_000 - (now.as_millis() % 60_000) as u64)
}

fn assert_idle(h: &mut H, what: &str) {
    for _ in 0..4 {
        settle(h);
    }
    for n in 0..40 {
        h.step();
        let delay = h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(
            delay >= std::time::Duration::from_millis(250),
            "{what}: frame {n} repaints after {delay:?}: {:?}",
            h.ctx.repaint_causes()
        );
    }
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn an_idle_log_with_docked_panels_and_simulators_repaints_only_for_the_clock() {
    init();
    let env = Env::new("idle");
    {
        let mut h = harness();
        let s = h.state_mut().settings_mut();
        let dock = s.section("dock");
        dock.insert("right".into(), "keyer|search".into());
        dock.insert("bottom".into(), "voice".into());
        s.section("windows")
            .insert("docked".into(), "keyer,search,voice".into());
        s.save().unwrap();
    }
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    assert_clock_only(&mut h, "docked panels");
    zlog_testing::device_menu(
        &mut h,
        rig_text("window.rig"),
        rig_text("menu.device.connect"),
    );
    assert!(h.state().rig_connected(), "the menu command connects");
    zlog_testing::device_menu(&mut h, rig_text("window.rig"), tr("menu.device.show"));
    assert_idle(&mut h, "a connected simulated rig in a floating window");
    h.state_mut().close_dialogs();
    h.set_size(egui::vec2(640.0, 560.0));
    assert_idle(&mut h, "narrow");
}
