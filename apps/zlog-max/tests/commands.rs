use zlog_gui::testing::commands::*;
use zlog_ui::keys::CommandOverrides;
use zlog_ui::{MenuGroup, MenuSection};

fn init() {
    zlog_gui::testing::registry::install(zlog_max::register);
}

fn core() -> Vec<CoreCommands> {
    let state = CoreState {
        unsaved: true,
        period: true,
        period_possible: true,
        formats: vec![("ADIF".into(), true, true)],
    };
    CORE_GROUPS
        .iter()
        .map(|g| CoreCommands {
            group: *g,
            state: state.clone(),
            chosen: None,
        })
        .collect()
}

fn all() -> Vec<Listed> {
    let panels = zlog_gui::testing::registry::panels();
    list(&core(), panels.iter().map(|p| (p.as_ref(), false)), "en")
}

#[test]
fn the_device_menu_starts_with_the_rig() {
    init();
    let all = all();
    let device = menu_sections(&all, MenuGroup::Device);
    assert_eq!(device[0].0, MenuSection::Rig);
    assert_eq!(device[0].1[0].1.id, "device.connect");
}

#[test]
fn names_chosen_rename_the_edit_and_score_commands() {
    init();
    let fresh = all();
    let mut all = fresh.clone();
    let mut o = CommandOverrides::default();
    o.names.insert("edit/delete".into(), "Erase".into());
    o.names.insert("score/window".into(), "Points".into());
    apply_shortcuts(&mut all, &o);
    let rows = toolbar_rows(&all, &[]);
    let erase = all
        .iter()
        .flat_map(|l| &l.commands)
        .find(|c| c.id == "delete")
        .unwrap();
    assert!(erase.context && erase.menu_text() == "Erase");
    let score = all.iter().find(|l| l.id == "score").unwrap();
    let w = score.commands.iter().find(|c| c.is_window()).unwrap();
    assert_eq!(w.window_title(), "Points");
    assert!(rows.iter().any(|r| r.key == "score" && r.label == "Points"));
    let before = fresh.iter().find(|l| l.id == "score").unwrap();
    let w = before.commands.iter().find(|c| c.is_window()).unwrap();
    assert_eq!(
        w.window_title(),
        zlog_ui::command::window_title(&w.menu_text())
    );
}
