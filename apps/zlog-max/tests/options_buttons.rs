use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::NodeT;
use zlog_gui::testing::i18n::tr;
use zlog_testing::{click, click_at, harness, open_log, settle, setup, window_rect, Env, H};

fn open(h: &mut H, size: [f32; 2]) {
    click(h, tr("menu.settings"));
    click(h, tr("menu.settings.options"));
    let r = window_rect(h, tr("options.title")).expect("Options");
    let from = r.max - egui::vec2(2.0, 2.0);
    drag(
        h,
        from,
        r.min + egui::Vec2::from(size) - egui::vec2(2.0, 2.0),
    );
}

fn drag(h: &mut H, from: Pos2, to: Pos2) {
    let press = |pressed, pos| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.event(Event::PointerMoved(from));
    h.step();
    h.event(press(true, from));
    h.step();
    for i in 1..=10 {
        h.event(Event::PointerMoved(from + (to - from) * i as f32 / 10.0));
        h.step();
    }
    h.event(press(false, to));
    settle(h);
}

fn button(h: &H, label: &str) -> Option<Pos2> {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.role() == Role::Button
                && a.label().as_deref() == Some(label)
                && zlog_testing::window_of(n) == tr("options.title")
        })
        .map(|n| n.rect().center())
}

fn pages(h: &H) -> Vec<String> {
    let skip = [tr("button.ok"), tr("button.cancel")];
    zlog_testing::controls(h)
        .into_iter()
        .filter(|c| c.window == tr("options.title") && c.role == Role::Button)
        .filter(|c| !skip.contains(&c.label.as_str()) && c.label.chars().count() > 1)
        .map(|c| c.label)
        .collect()
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn ok_and_cancel_answer_on_every_page_and_size() {
    zlog_testing::install(zlog_max::register);
    let env = Env::new("options-buttons");
    let mut h = harness();
    open_log(&mut h, setup(&env.dir, "allja", None));
    let title = tr("options.title");
    for size in [[280.0, 800.0], [640.0, 560.0]] {
        open(&mut h, size);
        let all = pages(&h);
        assert!(all.len() > 10, "{all:?}");
        for (i, page) in all.iter().enumerate() {
            if button(&h, tr("button.ok")).is_none() {
                open(&mut h, size);
            }
            click(&mut h, page);
            for _ in 0..4 {
                h.step();
                let why = &h.output().platform_output.request_discard_reasons;
                assert!(why.is_empty(), "{page} at {size:?} redraws: {why:?}");
            }
            let label = [tr("button.ok"), tr("button.cancel")][i % 2];
            let at = button(&h, label).expect(label);
            click_at(&mut h, at, PointerButton::Primary, 1);
            assert!(
                window_rect(&h, title).is_none(),
                "{label} on {page} at {size:?} left Options open"
            );
        }
    }
}
