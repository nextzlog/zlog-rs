use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use zlog_testing::layout::{self, Fit};

fn init() {
    zlog_testing::install(zlog_max::register);
}

const WINDOWS: &[(&str, Fit)] = &[
    ("keyer", Fit::Fixed),
    ("voice", Fit::Fills),
    ("rig", Fit::Fixed),
    ("cluster", Fit::Fills),
    ("link", Fit::Fills),
    ("link-freq", Fit::Fills),
    ("scope", Fit::Fills),
    ("qsy", Fit::Fills),
    ("rtty", Fit::Fixed),
    ("call-candidates", Fit::Fills),
    ("this-station", Fit::Scrolls),
    ("rate", Fit::Fills),
    ("rate-analysis", Fit::Fills),
    ("map", Fit::Fills),
    ("search", Fit::Fills),
    ("hist", Fit::Fixed),
    ("score", Fit::Fixed),
    ("multipliers", Fit::Scrolls),
    ("so2r", Fit::Fixed),
    ("so2r-queue", Fit::Fills),
    ("cw-decoder", Fit::External),
    ("ft8", Fit::Fixed),
    ("es", Fit::Fixed),
    ("ports", Fit::Fixed),
];

#[test]
fn every_tool_window_fits_at_280_and_spans_at_700() {
    init();
    layout::assert_layout(WINDOWS, "allja");
}

#[test]
fn every_tool_window_works_in_a_tall_narrow_panel() {
    init();
    layout::assert_tall(WINDOWS, "allja");
}

#[test]
fn function_keys_share_the_width_evenly() {
    init();
    let env = zlog_testing::Env::new("fkeys");
    for w in [layout::NARROW, layout::WIDE] {
        let h = layout::panel("keyer", "allja", &env.dir, [w, 480.0]);
        let keys: Vec<_> = layout::widgets(&h)
            .into_iter()
            .filter(|(r, l, _)| {
                let key = l.split('\n').next().unwrap_or_default();
                *r == Role::Button && key.starts_with('F') && key.len() <= 3 && l.contains('\n')
            })
            .collect();
        assert_eq!(keys.len(), 12, "F1-F12 at {w}");
        let width = keys[0].2.width();
        assert!(
            keys.iter().all(|k| (k.2.width() - width).abs() < 1.0),
            "same width at {w}"
        );
        let lo = keys.iter().map(|k| k.2.min.x).fold(f32::MAX, f32::min);
        let hi = keys.iter().map(|k| k.2.max.x).fold(f32::MIN, f32::max);
        assert!(
            hi - lo >= 0.9 * h.state().body.width(),
            "F keys span at {w}"
        );
    }
}

#[test]
fn the_rate_window_has_no_tabs() {
    init();
    let env = zlog_testing::Env::new("rate-tabs");
    let h = layout::panel("rate", "allja", &env.dir, [layout::WIDE, 480.0]);
    let w = layout::widgets(&h);
    assert!(w.iter().all(|(r, _, _)| *r != Role::Tab));
    for old in ["Graph", "Table", "Analysis"] {
        assert!(w.iter().all(|(_, l, _)| l != old), "{old}");
    }
    assert!(
        layout::node_rect(&h, Role::Splitter).is_some(),
        "chart and table split"
    );
}

#[test]
fn the_rate_target_dialog_edits_a_draft_and_ok_saves() {
    init();
    let env = zlog_testing::Env::new("rate-targets");
    let mut h = layout::panel("rate", "allja", &env.dir, [900.0, 700.0]);
    let edit = |h: &mut layout::L, button: &str| {
        h.state_mut().panel.run_command("targets");
        h.run_steps(4);
        assert!(h.state().panel.modal(), "the dialog opens");
        h.root()
            .children_recursive()
            .find(|n| n.accesskit_node().role() == Role::SpinButton)
            .expect("a target cell")
            .set_value_accesskit(7.0);
        h.run_steps(3);
        h.get_by_label(button).click_accesskit();
        h.run_steps(3);
    };
    let targets = |h: &layout::L| {
        layout::saved(h)
            .into_iter()
            .filter(|(k, _)| k.starts_with("target."))
            .count()
    };
    edit(&mut h, "Cancel");
    assert_eq!(targets(&h), 0, "Cancel drops the draft");
    edit(&mut h, "OK");
    assert_eq!(targets(&h), 1, "OK saves the draft");
}

#[test]
fn rate_and_candidate_dividers_move_and_persist() {
    init();
    layout::split_persists("rate", "allja");
    layout::split_persists("call-candidates", "allja");
    layout::split_persists("link", "allja");
}

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn the_analysis_window_opens_from_the_menu() {
    init();
    let env = zlog_testing::Env::new("analysis-menu");
    let mut h = zlog_testing::harness();
    zlog_testing::open_log(&mut h, zlog_testing::setup(&env.dir, "allja", None));
    let window_of = |id: &str| {
        let panel = zlog_gui::testing::registry::panels()
            .into_iter()
            .find(|p| p.id() == id)
            .expect("registered");
        let window = zlog_ui::panel_commands(panel.as_ref(), false, "en")
            .into_iter()
            .find(|c| c.is_window())
            .expect("window command");
        (panel.menu(), window)
    };
    let (menu, rate) = window_of("rate");
    let (group, window) = window_of("rate-analysis");
    assert_eq!(menu, group, "next to the rate window");
    let group = zlog_gui::testing::menus::group_key(group);
    zlog_testing::click(&mut h, zlog_gui::testing::i18n::tr(group));
    assert!(zlog_testing::find(&h, &rate.menu_text()).is_some());
    zlog_testing::click(&mut h, &window.menu_text());
    h.run_steps(5);
    let title = window.window_title();
    let shown = zlog_testing::controls(&h);
    for label in ["Copy", "Save…"] {
        assert!(
            shown
                .iter()
                .any(|c| c.window == title && zlog_testing::labelled(c, label)),
            "{label} in {title}"
        );
    }
    zlog_testing::close_request(&mut h);
}

#[test]
fn the_z_link_window_fits_the_dock_and_spans_wide_in_both_roles() {
    init();
    use zlog_link::role::Role as LinkRole;
    for role in [LinkRole::Client, LinkRole::Server] {
        let env = zlog_testing::Env::new(&format!("link-{}", role.key()));
        let mut h = layout::panel("link", "allja", &env.dir, layout::TALL);
        zlog_link::shared().lock().unwrap().settings.role = role;
        layout::resize(&mut h, layout::TALL);
        let key = role.key();
        assert!(
            layout::overlaps(&h).is_empty(),
            "{key}: {:?}",
            layout::overlaps(&h)
        );
        assert!(
            layout::clipped(&h).is_empty(),
            "{key}: {:?}",
            layout::clipped(&h)
        );
        assert!(layout::fills_height(&h), "{key} fills the dock");
        layout::resize(&mut h, [layout::WIDE, 480.0]);
        assert!(
            layout::clipped(&h).is_empty(),
            "{key}: {:?}",
            layout::clipped(&h)
        );
        assert!(layout::span(&h) >= 0.9, "{key} spans {}", layout::span(&h));
        zlog_link::shared().lock().unwrap().settings.role = LinkRole::Client;
    }
}
