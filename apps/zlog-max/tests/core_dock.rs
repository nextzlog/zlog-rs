mod windows;

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::NodeT;
use windows::*;

fn init() {
    zlog_gui::testing::registry::install(zlog_std::register);
}

fn dock_text(key: &str, title: &str) -> String {
    match key {
        "dock" => format!("Dock {title}"),
        _ => format!("Float {title}"),
    }
}

fn use_window(h: &mut H, title: &str) {
    let at = text_pos(h, &format!("{title} body"));
    click_at(h, at, PointerButton::Primary);
}

fn dock_by_menu(h: &mut H, title: &str) {
    dock_into_by_menu(h, title, "Right panel");
}

fn submenu_items(h: &H, labels: &[&str]) -> Vec<(Pos2, bool, String)> {
    let mut found: Vec<(Pos2, bool, String)> = h
        .root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            let label = a.label()?;
            let ok = a.role() == Role::CheckBox && labels.contains(&label.as_str());
            ok.then(|| {
                (
                    n.rect().center(),
                    a.toggled() == Some(egui::accesskit::Toggled::True),
                    label,
                )
            })
        })
        .collect();
    let right = found.iter().map(|f| f.0.x).fold(f32::MIN, f32::max);
    found.retain(|f| f.0.x >= right - 1.0);
    found.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    found
}

const AREAS: [&str; 3] = ["Left panel", "Right panel", "Bottom panel"];

fn choose_area(h: &mut H, area: &str) {
    let at = submenu_items(h, &[area])
        .first()
        .unwrap_or_else(|| panic!("no area {area}"))
        .0;
    click_at(h, at, PointerButton::Primary);
    egui::Popup::close_all(&h.ctx);
    settle(h);
}

fn window_submenu(h: &mut H, title: &str) -> Vec<(Pos2, bool, String)> {
    use_window(h, title);
    press(h, Role::Button, tr("menu.window"));
    press(h, Role::Button, &dock_text("dock", title));
    submenu_items(h, &AREAS)
}

fn dock_into_by_menu(h: &mut H, title: &str, area: &str) {
    window_submenu(h, title);
    choose_area(h, area);
}

fn in_area(h: &H, title: &str, area: &str) -> bool {
    let at = tab(h, title).expect("tab");
    match area {
        "Left panel" => at.x < 300.0 && at.y < 400.0,
        "Right panel" => at.x > 600.0 && at.y < 400.0,
        _ => at.y > 400.0,
    }
}

#[test]
fn the_window_menu_docks_into_each_area_and_lists_the_last_area_first() {
    init();
    let env = Env::new("dock-into");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    let first = |items: &[(Pos2, bool, String)]| (items[0].2.clone(), items[0].1);
    for area in AREAS {
        let items = window_submenu(&mut h, "Alpha");
        assert_eq!(items.len(), 3, "{items:?}");
        choose_area(&mut h, area);
        assert!(!window(&h, "Alpha"), "{area}");
        assert!(in_area(&h, "Alpha", area), "{area} {:?}", tab(&h, "Alpha"));
        menu(&mut h, tr("menu.window"), &dock_text("float", "Alpha"));
        assert!(window(&h, "Alpha"));
        let items = window_submenu(&mut h, "Alpha");
        assert_eq!(first(&items), (area.to_string(), true), "{items:?}");
        assert!(items[1..].iter().all(|i| !i.1));
        egui::Popup::close_all(&h.ctx);
        settle(&mut h);
    }
}

#[test]
fn a_new_window_lists_the_right_area_first() {
    init();
    let env = Env::new("dock-into-new");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    let items = window_submenu(&mut h, "Alpha");
    let labels: Vec<&str> = items.iter().map(|i| i.2.as_str()).collect();
    assert_eq!(labels, ["Right panel", "Left panel", "Bottom panel"]);
    assert!(items.iter().all(|i| !i.1));
}

#[test]
fn docking_into_a_hidden_area_shows_it() {
    init();
    let env = Env::new("dock-into-hidden");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    open(&mut h, "Beta");
    dock_into_by_menu(&mut h, "Alpha", "Bottom panel");
    menu(&mut h, tr("menu.window"), "Bottom panel");
    assert!(tab(&h, "Alpha").is_none(), "hidden");
    dock_into_by_menu(&mut h, "Beta", "Bottom panel");
    assert!(in_area(&h, "Beta", "Bottom panel"));
    assert!(tab(&h, "Alpha").is_some(), "the area is shown again");
}

#[test]
fn a_tool_window_docks_and_floats_back() {
    init();
    let env = Env::new("dock-float");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    assert!(window(&h, "Alpha"), "a tool window floats by default");
    assert!(tab(&h, "Alpha").is_none());
    dock_by_menu(&mut h, "Alpha");
    assert!(!window(&h, "Alpha"), "docked, no own window");
    let at = tab(&h, "Alpha").expect("tab");
    assert!(at.x > 600.0, "docked on the right by default");
    assert!(has_text(&h, "Alpha body"));
    assert!(
        env.settings().contains("right = \"alpha\""),
        "{}",
        env.settings()
    );
    menu(&mut h, tr("menu.window"), &dock_text("float", "Alpha"));
    assert!(window(&h, "Alpha"), "floating again");
    assert!(tab(&h, "Alpha").is_none());
    assert!(
        env.settings().contains("right = \"\""),
        "{}",
        env.settings()
    );
}

#[test]
fn two_docked_windows_share_tabs_and_move_by_drag_and_menu() {
    init();
    let env = Env::new("dock-tabs");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    dock_by_menu(&mut h, "Alpha");
    open(&mut h, "Beta");
    dock_by_menu(&mut h, "Beta");
    let (a, b) = (
        tab(&h, "Alpha").expect("alpha"),
        tab(&h, "Beta").expect("beta"),
    );
    assert!((a.y - b.y).abs() < 1.0, "one tab strip {a:?} {b:?}");
    assert!(has_text(&h, "Beta body") && !has_text(&h, "Alpha body"));
    click_at(&mut h, a, PointerButton::Primary);
    assert!(has_text(&h, "Alpha body") && !has_text(&h, "Beta body"));
    drag(&mut h, b, a);
    let (a, b) = (
        tab(&h, "Alpha").expect("alpha"),
        tab(&h, "Beta").expect("beta"),
    );
    assert!(b.x < a.x, "dragged before Alpha");
    assert!(
        env.settings().contains("right = \"beta,alpha\""),
        "{}",
        env.settings()
    );
    drag(&mut h, b, egui::pos2(20.0, 400.0));
    let b = tab(&h, "Beta").expect("beta");
    assert!(b.x < 300.0, "dragged into the left area");
    assert!(
        env.settings().contains("left = \"beta\""),
        "{}",
        env.settings()
    );
    click_at(&mut h, b, PointerButton::Secondary);
    press(&mut h, Role::Button, "Move to the bottom");
    let b = tab(&h, "Beta").expect("beta");
    assert!(b.y > 400.0, "moved to the bottom");
    let a = tab(&h, "Alpha").expect("alpha");
    click_at(&mut h, a, PointerButton::Secondary);
    press(&mut h, Role::Button, "Float");
    assert!(window(&h, "Alpha"));
    let close = node(&h, Role::Button, "Close").expect("close").0;
    click_at(&mut h, close, PointerButton::Primary);
    assert!(tab(&h, "Beta").is_none() && !window(&h, "Beta"), "closed");
    open(&mut h, "Beta");
    assert!(tab(&h, "Beta").is_some(), "reopens docked");
}

#[test]
fn dialogs_refuse_docking() {
    init();
    let env = Env::new("dock-dialog");
    env.write_settings("[dock]\nright = \"gamma\"\n");
    let mut h = harness(&env);
    open(&mut h, "Gamma…");
    assert!(
        window(&h, "Gamma"),
        "a dialog stays a window even when listed"
    );
    assert!(tab(&h, "Gamma").is_none());
    use_window(&mut h, "Gamma");
    assert_eq!(menu_item(&mut h, tr("menu.window"), "Dock"), Some(false));
    let at = text_pos(&h, "Gamma body");
    click_at(&mut h, at + egui::vec2(0.0, 60.0), PointerButton::Secondary);
    assert!(node(&h, Role::Button, "Dock into the main window").is_none());
}

#[test]
fn a_floating_window_offers_no_context_menu_and_docks_from_the_window_menu() {
    init();
    let env = Env::new("dock-context");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    let at = text_pos(&h, "Alpha body");
    click_at(&mut h, at + egui::vec2(0.0, 60.0), PointerButton::Secondary);
    assert!(node(&h, Role::Button, "Dock into the main window").is_none());
    window_submenu(&mut h, "Alpha");
    choose_area(&mut h, "Right panel");
    assert!(tab(&h, "Alpha").is_some());
}

#[test]
fn the_layout_is_restored_at_start() {
    init();
    let env = Env::new("dock-restore");
    {
        let mut h = harness(&env);
        open(&mut h, "Alpha");
        dock_by_menu(&mut h, "Alpha");
        let a = tab(&h, "Alpha").expect("alpha");
        drag(&mut h, a, egui::pos2(400.0, 680.0));
        assert!(tab(&h, "Alpha").expect("alpha").y > 400.0);
        h.state_mut().exit();
    }
    let saved = env.settings();
    assert!(saved.contains("bottom = \"alpha\""), "{saved}");
    assert!(
        !saved.contains("bottom.size"),
        "only a size the user chose is saved: {saved}"
    );
    let h = harness(&env);
    let a = tab(&h, "Alpha").expect("restored docked");
    assert!(a.y > 400.0, "in the bottom area");
    assert!(!window(&h, "Alpha"));
}

fn columns(h: &H) -> Vec<f32> {
    let call = h
        .root()
        .children_recursive()
        .find(|n| n.accesskit_node().value().as_deref() == Some("Call"))
        .expect("call header")
        .rect()
        .min
        .x;
    let mut xs: Vec<f32> = h
        .root()
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == Role::TextInput && window_of(n).is_none())
        .map(|n| (n.rect().min.x - call).round())
        .collect();
    xs.sort_by(f32::total_cmp);
    xs
}

#[test]
fn the_grid_and_the_entry_row_stay_aligned_beside_docked_areas() {
    init();
    let env = Env::new("dock-align");
    let mut h = harness(&env);
    let alone = columns(&h);
    assert!(!alone.is_empty());
    open(&mut h, "Alpha");
    dock_by_menu(&mut h, "Alpha");
    let header = text_pos(&h, "Call");
    assert_eq!(columns(&h), alone, "right");
    for (area, check) in [("Move to the left", 0usize), ("Move to the bottom", 1)] {
        let a = tab(&h, "Alpha").expect("alpha");
        click_at(&mut h, a, PointerButton::Secondary);
        press(&mut h, Role::Button, area);
        assert_eq!(columns(&h), alone, "{area}");
        let now = text_pos(&h, "Call");
        let a = tab(&h, "Alpha").expect("alpha");
        match check {
            0 => assert!(
                now.x > header.x + 40.0 && a.x < now.x,
                "left area beside the log"
            ),
            _ => {
                let input = h
                    .root()
                    .children_recursive()
                    .filter(|n| n.accesskit_node().role() == Role::TextInput)
                    .map(|n| n.rect().max.y)
                    .fold(0.0, f32::max);
                assert!(a.y > input, "bottom area below the entry row");
            }
        }
    }
}

#[test]
fn side_areas_take_the_full_height_and_the_bottom_area_sits_between_them() {
    init();
    let env = Env::new("dock-sides");
    let mut h = harness(&env);
    let alone = columns(&h);
    for (t, area) in [
        ("Alpha", "Move to the left"),
        ("Beta", "Move to the bottom"),
    ] {
        open(&mut h, t);
        dock_by_menu(&mut h, t);
        let at = tab(&h, t).unwrap();
        click_at(&mut h, at, PointerButton::Secondary);
        press(&mut h, Role::Button, area);
    }
    let (a, b) = (tab(&h, "Alpha").unwrap(), tab(&h, "Beta").unwrap());
    assert!(a.x < 300.0 && b.y > 400.0, "{a:?} {b:?}");
    assert!(
        b.x > a.x + 60.0,
        "the bottom area starts right of the left area"
    );
    assert!(text_pos(&h, "Call").x > a.x + 60.0);
    assert_eq!(columns(&h), alone);
}

#[test]
fn a_right_area_splits_into_two_stacks_and_joins_again() {
    init();
    let env = Env::new("dock-split");
    {
        let mut h = harness(&env);
        open(&mut h, "Alpha");
        dock_by_menu(&mut h, "Alpha");
        open(&mut h, "Beta");
        dock_by_menu(&mut h, "Beta");
        let b = tab(&h, "Beta").expect("beta");
        let body = text_pos(&h, "Beta body");
        drag(&mut h, b, egui::pos2(body.x, 700.0));
        assert!(
            has_text(&h, "Alpha body") && has_text(&h, "Beta body"),
            "both shown"
        );
        let (a, b) = (tab(&h, "Alpha").unwrap(), tab(&h, "Beta").unwrap());
        assert!(b.y > a.y + 100.0, "Beta below Alpha");
        assert!(
            env.settings().contains("right = \"alpha|beta\""),
            "{}",
            env.settings()
        );
        let at = egui::pos2(a.x, b.y - 13.5);
        drag(&mut h, at, at - egui::vec2(0.0, 150.0));
        let b2 = tab(&h, "Beta").unwrap();
        assert!(b2.y < b.y - 100.0, "splitter moved");
        h.state_mut().exit();
    }
    let saved = env.settings();
    let ratio: f32 = saved
        .lines()
        .find_map(|l| l.strip_prefix("\"right.split\" = \""))
        .and_then(|r| r.split(',').next())
        .and_then(|r| r.parse().ok())
        .unwrap_or_else(|| panic!("{saved}"));
    assert!(ratio < 0.4, "{saved}");
    let mut h = harness(&env);
    let (a, b) = (tab(&h, "Alpha").unwrap(), tab(&h, "Beta").unwrap());
    assert!(has_text(&h, "Alpha body") && has_text(&h, "Beta body"));
    assert!(
        b.y > a.y + 50.0 && b.y < 450.0,
        "ratio restored {a:?} {b:?}"
    );
    drag(&mut h, b, a);
    settle(&mut h);
    let (a, b) = (tab(&h, "Alpha").unwrap(), tab(&h, "Beta").unwrap());
    assert!(
        (a.y - b.y).abs() < 1.0,
        "one stack again {a:?} {b:?} {}",
        env.settings()
    );
    assert!(
        env.settings().contains("right = \"beta,alpha\""),
        "{}",
        env.settings()
    );
    assert!(!env.settings().contains("right.split"));
}

fn reopen_setting(h: &mut H, env: &Env, on: bool) {
    let shown = env.settings().contains("reopen = \"0\"") != on;
    if !shown {
        menu(h, tr("menu.settings"), "Reopen windows at start");
    }
}

fn session_with_four_windows(env: &Env, reopen: bool) {
    let mut h = harness(env);
    reopen_setting(&mut h, env, reopen);
    for t in ["Alpha", "Beta", "Delta", "Omega"] {
        open(&mut h, t);
    }
    dock_by_menu(&mut h, "Delta");
    dock_by_menu(&mut h, "Omega");
    let o = tab(&h, "Omega").unwrap();
    click_at(&mut h, o, PointerButton::Secondary);
    press(&mut h, Role::Button, "Move to the left");
    let close = |h: &H| node(h, Role::Window, "Beta").is_some();
    assert!(close(&h));
    open(&mut h, "Beta");
    assert!(!window(&h, "Beta"), "closed before quit");
    h.state_mut().exit();
}

#[test]
fn open_windows_come_back_at_the_next_start() {
    init();
    let env = Env::new("dock-session");
    session_with_four_windows(&env, true);
    let h = harness(&env);
    assert!(window(&h, "Alpha"), "floating restored");
    assert!(
        !window(&h, "Beta") && tab(&h, "Beta").is_none(),
        "closed stays closed"
    );
    assert!(tab(&h, "Delta").expect("delta").x > 600.0, "right area");
    assert!(tab(&h, "Omega").expect("omega").x < 300.0, "left area");
    assert!(!window(&h, "Delta") && !window(&h, "Omega"));
}

#[test]
fn without_reopen_only_docked_windows_come_back() {
    init();
    let env = Env::new("dock-session-off");
    session_with_four_windows(&env, false);
    assert!(
        env.settings().contains("reopen = \"0\""),
        "{}",
        env.settings()
    );
    let mut h = harness(&env);
    assert!(!window(&h, "Alpha"), "floating not reopened");
    assert!(
        tab(&h, "Delta").is_some() && tab(&h, "Omega").is_some(),
        "docked always"
    );
    open(&mut h, "Alpha");
    assert!(window(&h, "Alpha"));
}

#[test]
fn a_closed_docked_window_reopens_in_its_place() {
    init();
    let env = Env::new("dock-reopen");
    {
        let mut h = harness(&env);
        open(&mut h, "Alpha");
        dock_by_menu(&mut h, "Alpha");
        open(&mut h, "Beta");
        dock_by_menu(&mut h, "Beta");
        let b = tab(&h, "Beta").unwrap();
        let body = text_pos(&h, "Beta body");
        drag(&mut h, b, egui::pos2(body.x, 700.0));
        let before = tab(&h, "Beta").unwrap();
        open(&mut h, "Beta");
        assert!(tab(&h, "Beta").is_none() && !window(&h, "Beta"), "closed");
        open(&mut h, "Beta");
        let again = tab(&h, "Beta").expect("docked again");
        assert!((again - before).length() < 2.0, "{again:?} {before:?}");
        open(&mut h, "Beta");
        h.state_mut().exit();
    }
    let mut h = harness(&env);
    assert!(tab(&h, "Beta").is_none(), "closed at quit");
    open(&mut h, "Beta");
    let b = tab(&h, "Beta").expect("docked after restart");
    let a = tab(&h, "Alpha").expect("alpha");
    assert!(b.y > a.y + 100.0 && !window(&h, "Beta"), "in its split");
}

#[test]
fn the_window_menu_docks_a_just_opened_window() {
    init();
    let env = Env::new("dock-just-opened");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    open(&mut h, "Beta");
    press(&mut h, Role::Button, tr("menu.window"));
    press(&mut h, Role::Button, &dock_text("dock", "Beta"));
    choose_area(&mut h, "Right panel");
    assert!(tab(&h, "Beta").is_some() && window(&h, "Alpha"));
    open(&mut h, "Delta");
    press(&mut h, Role::Button, tr("menu.window"));
    press(&mut h, Role::Button, &dock_text("dock", "Delta"));
    choose_area(&mut h, "Right panel");
    assert!(tab(&h, "Delta").is_some());
}

#[test]
fn dock_areas_hide_and_show_from_the_window_menu() {
    init();
    let env = Env::new("dock-areas");
    let columns_alone = {
        let mut h = harness(&env);
        let alone = columns(&h);
        open(&mut h, "Alpha");
        dock_by_menu(&mut h, "Alpha");
        let shown = text_pos(&h, "Call");
        menu(&mut h, tr("menu.window"), "Right panel");
        assert!(tab(&h, "Alpha").is_none() && !window(&h, "Alpha"), "hidden");
        assert_eq!(columns(&h), alone, "alignment kept");
        assert!((text_pos(&h, "Call").x - shown.x).abs() < 1.0);
        assert!(
            env.settings().contains("\"right.shown\" = \"0\""),
            "{}",
            env.settings()
        );
        h.state_mut().exit();
        alone
    };
    let mut h = harness(&env);
    assert!(tab(&h, "Alpha").is_none(), "still hidden after restart");
    menu(&mut h, tr("menu.window"), "Right panel");
    assert!(tab(&h, "Alpha").is_some(), "shown again");
    assert_eq!(columns(&h), columns_alone);
    menu(&mut h, tr("menu.window"), "Right panel");
    open(&mut h, "Alpha");
    open(&mut h, "Alpha");
    assert!(tab(&h, "Alpha").is_some(), "opening a panel shows its area");
    menu(&mut h, tr("menu.window"), "Left panel");
    assert!(
        has_text(&h, "Drag a tab here"),
        "an empty area shows a hint"
    );
    let a = tab(&h, "Alpha").unwrap();
    let hint = text_pos(&h, "Drag a tab here");
    drag(&mut h, a, hint);
    assert!(
        tab(&h, "Alpha").expect("alpha").x < 300.0,
        "dropped into the empty area"
    );
}

#[test]
fn the_close_mark_in_a_tab_closes_that_panel() {
    init();
    let env = Env::new("dock-close-mark");
    let mut h = harness(&env);
    for t in ["Alpha", "Beta"] {
        open(&mut h, t);
        dock_by_menu(&mut h, t);
    }
    assert!(has_text(&h, "Beta body"));
    let a = tab(&h, "Alpha").unwrap();
    let gap = text_pos(&h, "Beta body").y - a.y;
    assert!(
        gap < 30.0 + zlog_ui::layout::MARGIN,
        "no gap under the tab bar beyond the pane margin: {gap}"
    );
    let mark = h
        .root()
        .children_recursive()
        .filter(|n| {
            let x = n.accesskit_node();
            x.role() == Role::Button && x.label().as_deref() == Some("Close")
        })
        .map(|n| n.rect().center())
        .filter(|p| (p.y - a.y).abs() < 4.0 && p.x > a.x)
        .min_by(|p, q| p.x.total_cmp(&q.x))
        .expect("close mark of Alpha");
    h.event(Event::PointerMoved(a));
    h.step();
    click_at(&mut h, mark, PointerButton::Primary);
    assert!(tab(&h, "Alpha").is_none(), "Alpha closed");
    assert!(
        tab(&h, "Beta").is_some() && has_text(&h, "Beta body"),
        "Beta kept"
    );
    open(&mut h, "Alpha");
    let again = tab(&h, "Alpha").expect("reopens docked");
    assert!((again.y - a.y).abs() < 1.0);
}

fn layout(h: &H) -> Vec<(String, i32, i32)> {
    ["Alpha", "Beta", "Delta"]
        .into_iter()
        .filter_map(|t| tab(h, t).map(|p| (t.to_string(), p.x as i32, p.y as i32)))
        .collect()
}

#[test]
fn closed_stacks_come_back_whatever_the_reopening_order() {
    init();
    let env = Env::new("dock-order");
    let mut h = harness(&env);
    for t in ["Alpha", "Beta", "Delta"] {
        open(&mut h, t);
        dock_by_menu(&mut h, t);
    }
    let d = tab(&h, "Delta").unwrap();
    let body = text_pos(&h, "Delta body");
    drag(&mut h, d, egui::pos2(body.x, 700.0));
    let full = layout(&h);
    assert_eq!(full.len(), 3);
    assert!(full[2].2 > full[0].2 + 100 && full[0].2 == full[1].2);
    let saved = env.settings();
    for order in [["Alpha", "Beta", "Delta"], ["Delta", "Beta", "Alpha"]] {
        for t in ["Alpha", "Beta", "Delta"] {
            open(&mut h, t);
        }
        assert!(layout(&h).is_empty(), "all closed");
        for t in order {
            open(&mut h, t);
        }
        assert_eq!(layout(&h), full, "{order:?}");
    }
    for t in ["Alpha", "Beta", "Delta"] {
        open(&mut h, t);
    }
    open(&mut h, "Delta");
    let alone = tab(&h, "Delta").expect("delta");
    assert!(alone.y < full[0].2 as f32 + 2.0, "alone in its area");
    assert!(alone.x > 600.0);
    assert_eq!(
        env.settings().lines().find(|l| l.starts_with("right =")),
        saved.lines().find(|l| l.starts_with("right =")),
    );
}

fn drag(h: &mut H, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for n in 1..=8 {
        let t = n as f32 / 8.0;
        h.event(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    settle(h);
}

fn four_docked(h: &mut H) {
    for t in ["Alpha", "Beta", "Delta"] {
        open(h, t);
        dock_by_menu(h, t);
    }
    let d = tab(h, "Delta").unwrap();
    click_at(h, d, PointerButton::Primary);
    let body = text_pos(h, "Delta body");
    drag(h, d, egui::pos2(body.x, 700.0));
    open(h, "Omega");
    dock_by_menu(h, "Omega");
    let o = tab(h, "Omega").unwrap();
    click_at(h, o, PointerButton::Secondary);
    press(h, Role::Button, "Move to the bottom");
    let a = tab(h, "Alpha").unwrap();
    click_at(h, a, PointerButton::Secondary);
    press(h, Role::Button, "Move to the left");
    h.event(Event::PointerMoved(egui::pos2(500.0, 300.0)));
    settle(h);
}

fn assert_idle(h: &mut H, what: &str) {
    settle(h);
    for n in 0..40 {
        let left = until_the_minute();
        h.step();
        let delay = h.output().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(
            delay + std::time::Duration::from_millis(50) >= left,
            "{what}: frame {n} asks for a repaint after {delay:?}, the minute ends in {left:?}: {:?}",
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

#[test]
fn an_idle_log_repaints_only_for_the_clock() {
    init();
    let env = Env::new("dock-idle");
    let mut h = harness(&env);
    assert_idle(&mut h, "log alone");
    four_docked(&mut h);
    assert_idle(&mut h, "docked panels in three areas and a split");
    open(&mut h, "Gamma…");
    assert!(window(&h, "Gamma"));
    assert_idle(&mut h, "with a floating window");
    for size in [[640.0, 560.0], [400.0, 300.0], [1200.0, 800.0]] {
        h.set_size(egui::vec2(size[0], size[1]));
        assert_idle(&mut h, &format!("after a resize to {size:?}"));
    }
}

#[test]
fn the_window_menu_targets_a_restored_panel_after_a_restart() {
    init();
    let env = Env::new("dock-restart-target");
    {
        let mut h = harness(&env);
        open(&mut h, "Alpha");
        dock_by_menu(&mut h, "Alpha");
        h.state_mut().exit();
    }
    let mut h = harness(&env);
    assert!(tab(&h, "Alpha").is_some());
    assert_eq!(
        menu_item(&mut h, tr("menu.window"), &dock_text("float", "Alpha")),
        Some(true)
    );
    menu(&mut h, tr("menu.window"), &dock_text("float", "Alpha"));
    assert!(window(&h, "Alpha"), "the restored panel floats");
}

#[test]
fn a_hidden_right_area_leaves_its_room_to_the_log_and_the_left_area() {
    init();
    let env = Env::new("dock-hidden-room");
    let mut h = harness(&env);
    for (t, area) in [("Alpha", "Move to the left"), ("Beta", "")] {
        open(&mut h, t);
        dock_by_menu(&mut h, t);
        if !area.is_empty() {
            let at = tab(&h, t).unwrap();
            click_at(&mut h, at, PointerButton::Secondary);
            press(&mut h, Role::Button, area);
        }
    }
    h.set_size(egui::vec2(560.0, 700.0));
    settle(&mut h);
    let with_right = text_pos(&h, "Call").x;
    menu(&mut h, tr("menu.window"), "Right panel");
    settle(&mut h);
    let without = text_pos(&h, "Call").x;
    assert!(
        without > with_right + 30.0,
        "the left area takes its full width: {with_right} -> {without}"
    );
}

#[test]
fn open_windows_reach_the_settings_only_on_the_throttled_save() {
    init();
    let env = Env::new("dock-throttled");
    let mut h = harness(&env);
    let floating = |h: &mut H| {
        h.state_mut()
            .settings_mut()
            .section("windows")
            .get("floating")
            .cloned()
            .unwrap_or_default()
    };
    let before = floating(&mut h);
    open(&mut h, "Alpha");
    assert!(window(&h, "Alpha"));
    for _ in 0..30 {
        h.step();
    }
    assert_eq!(floating(&mut h), before, "not written every frame");
    h.state_mut().make_panel_save_due();
    h.step();
    assert_eq!(floating(&mut h), "alpha", "written on the save");
}

fn click_label(h: &mut H, label: &str) {
    let found = h
        .root()
        .children_recursive()
        .find(|n| n.accesskit_node().label().as_deref() == Some(label));
    found.unwrap_or_else(|| panic!("no {label:?}")).click();
    settle(h);
}

fn listed(h: &H, id: &str) -> bool {
    h.state()
        .listed_commands()
        .iter()
        .any(|c| c == id || c.ends_with(&format!("/{id}")))
}

fn host_listed(h: &H) -> bool {
    h.state()
        .listed_commands()
        .iter()
        .any(|c| c.starts_with("dock/"))
}

#[test]
fn import_and_export_are_hidden_without_a_log_format() {
    init();
    let env = Env::new("hidden-formats");
    let h = harness_with(&env, false);
    assert!(listed(&h, "file/file.open"));
    assert!(!listed(&h, "file/file.import"));
    assert!(!listed(&h, "file/file.export"));
}

#[test]
fn correct_start_time_is_hidden_without_a_contest_period() {
    init();
    let env = Env::new("hidden-start");
    let h = harness_with(&env, false);
    assert!(!listed(&h, "file/file.start_time"));
}

#[test]
fn skip_this_step_is_hidden_without_a_link() {
    init();
    let env = Env::new("hidden-skip");
    let h = harness_with(&env, false);
    assert!(listed(&h, "undo"));
    assert!(!listed(&h, "skip"));
}

#[test]
fn window_host_items_appear_only_with_a_tool_window() {
    init();
    let env = Env::new("hidden-dock");
    let h = harness_with(&env, false);
    assert!(!host_listed(&h));
    assert!(!listed(&h, "show.left"));
    drop(h);
    let h = harness_with(&env, true);
    assert!(host_listed(&h));
    assert!(listed(&h, "show.left"));
}

#[test]
fn the_entry_style_choice_is_hidden_with_one_style() {
    init();
    let env = Env::new("hidden-style");
    let mut h = harness_with(&env, false);
    press(&mut h, Role::Button, tr("menu.settings"));
    press(&mut h, Role::Button, tr("menu.settings.options"));
    settle(&mut h);
    click_label(&mut h, tr("options.preferences"));
    assert!(has_text(&h, tr("options.use_contest_period")));
    assert!(!has_text(&h, tr("options.entry_style")));
}

#[test]
fn quitting_with_open_windows_keeps_the_layout() {
    init();
    let env = Env::new("dock-quit");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    open(&mut h, "Delta");
    dock_by_menu(&mut h, "Delta");
    h.input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    h.run_steps(6);
    h.state_mut().exit();
    let h = harness(&env);
    assert!(window(&h, "Alpha"), "floating restored");
    assert!(tab(&h, "Delta").expect("delta").x > 600.0, "right area");
}
