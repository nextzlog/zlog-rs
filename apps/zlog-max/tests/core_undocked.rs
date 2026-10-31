mod windows;

use egui::accesskit::Role;
use egui::PointerButton;
use windows::*;

fn core_without_dock(b: &mut zlog_ui::Builder) {
    b.extension(zlog_ui::builder::order::RULES + 3, || zlog_rule::Base);
    zlog_menu::register(b);
    zlog_toolbar::register(b);
    zlog_status::register(b);
}

#[test]
fn without_the_dock_every_tool_window_is_its_own_window() {
    zlog_gui::testing::registry::install(core_without_dock);
    let env = Env::new("undocked");
    env.write_settings("[dock]\nright = \"alpha\"\n");
    let mut h = harness(&env);
    open(&mut h, "Alpha");
    open(&mut h, "Beta");
    assert!(window(&h, "Alpha") && window(&h, "Beta"));
    assert!(has_text(&h, "Alpha body"));
    assert!(tab(&h, "Alpha").is_none() && tab(&h, "Beta").is_none());
    let at = text_pos(&h, "Alpha body");
    click_at(&mut h, at, PointerButton::Primary);
    assert_eq!(menu_item(&mut h, tr("menu.window"), "Dock"), None);
    assert_eq!(menu_item(&mut h, tr("menu.window"), "Dock Alpha"), None);
    click_at(&mut h, at + egui::vec2(0.0, 60.0), PointerButton::Secondary);
    assert!(node(&h, Role::Button, "Dock into the main window").is_none());
    h.state_mut().exit();
    assert!(
        env.settings().contains("right = \"alpha\""),
        "the section is left alone"
    );
}
