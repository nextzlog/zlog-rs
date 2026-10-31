use egui::{Key, KeyboardShortcut, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zlog_gui::testing::commands::{apply_shortcuts, list, CoreCommands, CORE_GROUPS};
use zlog_ui::keys::CommandOverrides;
use zlog_ui::Command;

fn init() {
    zlog_gui::testing::registry::install(zlog_std::register);
}

fn shown(commands: Vec<Command>, text: &str) -> bool {
    let mut h = Harness::new_ui(move |ui| {
        zlog_ui::chrome::menu_items(ui, &commands);
    });
    h.run();
    h.query_by_label_contains(text).is_some()
}

#[test]
fn plain_check_and_radio_items_show_their_keys() {
    let key = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
    let mut plain = Command::new("plain", "Plain", "");
    plain.shortcut = Some(key(Key::F7));
    let mut check = Command::new("check", "Check", "");
    check.checkable = true;
    check.shortcut = Some(key(Key::F8));
    let mut radio = Command::new("radio", "Radio", "");
    radio.radio = true;
    radio.selected = true;
    radio.shortcut = Some(key(Key::F9));
    for (c, text) in [(plain, "Ctrl+F7"), (check, "Ctrl+F8"), (radio, "Ctrl+F9")] {
        assert!(shown(vec![c], text), "{text}");
    }
}

#[test]
fn a_rebound_key_replaces_the_old_one_in_the_menus() {
    init();
    let core: Vec<CoreCommands> = CORE_GROUPS
        .iter()
        .map(|&group| CoreCommands {
            group,
            state: Default::default(),
            chosen: None,
        })
        .collect();
    let panels = zlog_gui::testing::registry::panels();
    let mut all = list(&core, panels.iter().map(|p| (p.as_ref(), false)), "en");
    let save = |all: &[zlog_gui::testing::commands::Listed]| {
        all.iter()
            .find(|l| l.id == "file")
            .and_then(|l| l.commands.iter().find(|c| c.id == "file.save"))
            .cloned()
            .unwrap()
    };
    let old = *save(&all).menu_key().unwrap();
    let new = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::F6);
    let mut o = CommandOverrides::default();
    o.shortcuts.insert("file/file.save".into(), Some(new));
    apply_shortcuts(&mut all, &o);
    let c = save(&all);
    assert_eq!(c.menu_key(), Some(&new));
    assert!(shown(vec![c.clone()], "Ctrl+Alt+F6"));
    assert!(!shown(vec![c.clone()], &zlog_ui::keys::format(&old)));
    let (mods, code) = zlog_mac::parse("Ctrl+Alt+F6").unwrap();
    assert_eq!(
        zlog_mac::shown(c.menu_key().unwrap()),
        zlog_mac::Shown::Native(mods, code)
    );
    assert_ne!(
        zlog_mac::parse(&zlog_ui::keys::format(&old)),
        Some((mods, code))
    );
}
