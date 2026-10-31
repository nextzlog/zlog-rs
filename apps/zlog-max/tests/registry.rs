fn init() {
    zlog_gui::testing::registry::install(zlog_max::register);
}

#[test]
fn the_drivers_listed_are_those_of_the_features() {
    init();
    use zlog_ui::device::{AudioDrv, DeviceDriver, ModemDrv};
    fn ids<T: ?Sized + DeviceDriver>(v: &[Box<T>]) -> Vec<String> {
        v.iter().map(|d| d.id().to_string()).collect()
    }
    let mut d = zlog_gui::testing::registry::drivers();
    let keyers = {
        let k = d.take::<dyn zlog_keyer::KeyerDrv>();
        assert!(k.iter().any(|k| k.simulated()));
        ids(&k)
    };
    assert_eq!(
        keyers,
        ["winkeyer", "serial", "usbif4cw", "simulated", "remote"]
    );
    let boxes = {
        let b = d.take::<dyn zlog_so2r::So2rDrv>();
        assert!(b.iter().any(|b| b.simulated()));
        ids(&b)
    };
    assert_eq!(
        boxes,
        ["neo", "neo_keyer", "otrsp", "usbif4cw", "sim", "remote"]
    );
    let audio = d.take::<dyn AudioDrv>();
    assert_eq!(ids(&audio), ["sound", "loopback"]);
    assert_eq!(ids(&d.take::<dyn ModemDrv>()), ["rtty", "cw", "ft8", "ft4"]);
    assert!(!d.take::<&'static zlog_rig::Model>().is_empty());
    assert_eq!(d.take::<zlog_rig::SimFactory>().len(), 1);
    assert_eq!(d.take::<dyn zlog_summary::WebSubmit>().len(), 1);
    assert_eq!(d.take::<dyn zlog_ui::rig::RigSource>().len(), 1);
    assert!(d.is_empty());
}

#[test]
fn toolbar_buttons_have_icons_and_tooltips() {
    init();
    for lang in ["en", "ja"] {
        for p in zlog_gui::testing::registry::panels() {
            for b in p.commands(lang).into_iter().filter(|b| b.toolbar) {
                let name = format!("{lang} {} {}", p.id(), b.label);
                assert!(b.icon.is_some(), "{name}: no icon");
                assert!(!b.description.is_empty(), "{name}");
                assert!(!b.description.contains(" — "), "{name}: {}", b.description);
                if !b.is_window() && b.toolbar_key.is_none() {
                    assert!(b.state.is_some(), "{name}: no state");
                }
                if !b.enabled {
                    let why = b.disabled_reason.as_deref().expect(&name);
                    assert!(b.full_tooltip(None).ends_with(why), "{name}");
                }
            }
        }
    }
    let ja: Vec<_> = zlog_gui::testing::registry::panels()
        .iter()
        .flat_map(|p| p.commands("ja"))
        .filter(|b| !b.enabled)
        .collect();
    assert!(ja.len() >= 2 && ja.iter().all(|b| b.disabled_reason.is_some()));
}

#[test]
fn the_guide_roles_are_provided_by_the_panels() {
    init();
    let all: Vec<zlog_ui::Command> = zlog_gui::testing::registry::panels()
        .iter()
        .flat_map(|p| zlog_ui::panel_commands(p.as_ref(), false, "en"))
        .collect();
    let roles: Vec<&str> = zlog_ui::role_commands(&all)
        .iter()
        .map(|c| c.role)
        .collect();
    for role in ["score", "multi", "check", "history", "elog", "jarl_upload"] {
        assert!(roles.contains(&role), "{role}");
    }
}

#[test]
fn the_updater_follows_this_repository() {
    init();
    assert_eq!(
        env!("CARGO_PKG_REPOSITORY"),
        "https://github.com/nextzlog/zmax"
    );
    assert_eq!(zlog_ui::product::REPOSITORY, env!("CARGO_PKG_REPOSITORY"));
    assert!(zlog_gui::testing::registry::panels()
        .iter()
        .any(|p| p.id() == "update"));
}

fn listed() -> Vec<zlog_gui::testing::commands::Listed> {
    use zlog_gui::testing::commands as cmds;
    let core: Vec<cmds::CoreCommands> = cmds::CORE_GROUPS
        .iter()
        .map(|g| cmds::CoreCommands {
            group: *g,
            state: Default::default(),
            chosen: None,
        })
        .collect();
    let panels = zlog_gui::testing::registry::panels();
    cmds::list(&core, panels.iter().map(|p| (p.as_ref(), false)), "en")
}

#[test]
fn every_menu_command_can_be_put_on_the_toolbar() {
    init();
    use zlog_gui::testing::commands as cmds;
    let all = listed();
    let rows = cmds::toolbar_rows(&all, &[]);
    let mut count = 0;
    for l in &all {
        for c in l.commands.iter().filter(|c| c.menu) {
            let key = cmds::toolbar_key(l, c).expect(&c.id);
            assert!(rows.iter().any(|r| r.key == key), "{} {}", l.id, c.id);
            count += 1;
        }
    }
    assert!(count > 50, "{count}");
    let defaults: Vec<&str> = rows
        .iter()
        .filter(|r| r.default)
        .map(|r| r.key.as_str())
        .collect();
    assert_eq!(defaults, DEFAULT_TOOLBAR);
}

const DEFAULT_TOOLBAR: &[&str] = &[
    "file.open",
    "file.save",
    "call-candidates",
    "score",
    "multipliers",
    "scope",
    "map",
    "keyer",
    "cw-decoder",
    "link",
    "cluster",
];

#[test]
fn toolbar_default_set() {
    init();
    use zlog_gui::testing::commands::{list, toolbar_rows, CoreCommands, CORE_GROUPS};
    let core: Vec<CoreCommands> = CORE_GROUPS
        .iter()
        .map(|&group| CoreCommands {
            group,
            state: Default::default(),
            chosen: None,
        })
        .collect();
    let panels = zlog_gui::testing::registry::panels();
    let all = list(&core, panels.iter().map(|p| (p.as_ref(), false)), "en");
    let mut shown: Vec<String> = toolbar_rows(&all, &[])
        .into_iter()
        .filter(|r| r.default)
        .map(|r| r.key)
        .collect();
    shown.sort();
    assert_eq!(shown, DEFAULT_SHOWN, "{shown:?}");
}
const DEFAULT_SHOWN: &[&str] = &[
    "call-candidates",
    "cluster",
    "cw-decoder",
    "file.open",
    "file.save",
    "keyer",
    "link",
    "map",
    "multipliers",
    "scope",
    "score",
];

#[test]
fn no_two_default_keys_clash() {
    init();
    use zlog_gui::testing::commands::{command_infos, list, CoreCommands, CORE_GROUPS};
    use zlog_gui::testing::registry::entry_style;
    use zlog_ui::keys::default_clashes;
    let core: Vec<CoreCommands> = CORE_GROUPS
        .iter()
        .map(|&group| CoreCommands {
            group,
            state: Default::default(),
            chosen: None,
        })
        .collect();
    let panels = zlog_gui::testing::registry::panels();
    let all = list(&core, panels.iter().map(|p| (p.as_ref(), false)), "en");
    let styles: Vec<_> = ["zlog", "paper", "form"]
        .into_iter()
        .filter_map(entry_style)
        .collect();
    assert!(!styles.is_empty());
    for style in styles {
        let (infos, reserved) = command_infos(&all, &style.reserved_keys(), "en");
        let clashes = default_clashes(&infos, &reserved);
        assert!(clashes.is_empty(), "{}: {clashes:?}", style.id());
    }
}

#[test]
fn windows_and_backup_take_the_zlog_alt_keys() {
    init();
    use egui::{Key, KeyboardShortcut, Modifiers};
    let alt = |k| Some(KeyboardShortcut::new(Modifiers::ALT, k));
    let panels = zlog_gui::testing::registry::panels();
    let default = |panel: &str, id: &str| {
        panels
            .iter()
            .find(|p| p.id() == panel)
            .and_then(|p| p.commands("en").into_iter().find(|c| c.id == id))
            .and_then(|c| c.shortcut)
    };
    assert_eq!(default("backup", "backup"), alt(Key::B));
    assert_eq!(default("cluster", "window"), alt(Key::P));
    assert_eq!(default("rig", "window"), alt(Key::T));
    assert_eq!(default("link", "window"), alt(Key::Z));
}

#[test]
fn every_default_key_has_a_mac_menu_form() {
    init();
    use zlog_gui::testing::commands::{list, CoreCommands, CORE_GROUPS};
    let core: Vec<CoreCommands> = CORE_GROUPS
        .iter()
        .map(|&group| CoreCommands {
            group,
            state: Default::default(),
            chosen: None,
        })
        .collect();
    let panels = zlog_gui::testing::registry::panels();
    let all = list(&core, panels.iter().map(|p| (p.as_ref(), false)), "en");
    let mut seen = 0;
    let mut stack: Vec<&zlog_ui::Command> = all.iter().flat_map(|l| &l.commands).collect();
    while let Some(c) = stack.pop() {
        stack.extend(&c.children);
        let Some(k) = c.menu_key() else {
            continue;
        };
        seen += 1;
        let text = zlog_ui::keys::format(k);
        assert!(zlog_mac::parse(&text).is_some(), "{}: {text}", c.id);
        match zlog_mac::shown(k) {
            zlog_mac::Shown::Native(..) => assert!(zlog_mac::safe(k), "{}", c.id),
            zlog_mac::Shown::Hidden => assert!(!zlog_mac::safe(k), "{}", c.id),
        }
    }
    assert!(seen > 0);
}

#[test]
fn ellipsis_marks_exactly_the_commands_needing_input() {
    init();
    zlog_gui::testing::ellipsis::classify(DIALOG, PLAIN);
    zlog_gui::testing::ellipsis::lang_files(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.."
    )));
}
const DIALOG: &[&str] = &[
    "correct_nr/window",
    "file/file.export/file.export.0",
    "file/file.import/file.import.0",
    "file/file.open",
    "file/file.save_as",
    "file/file.save_as_zlo",
    "file/file.start_time",
    "file:ini/ini.export",
    "file:ini/ini.import",
    "file:toml/toml.export",
    "file:toml/toml.import",
    "guide/window",
    "jarl_member/window",
    "link/settings",
    "ops/window",
    "ports/window",
    "qso:correct_nr/correct",
    "rate/targets",
    "settings/settings.options",
    "soumu/window",
    "summary/upload",
    "summary/window",
    "update/window",
];
const PLAIN: &[&str] = &[
    "backup/backup",
    "call-candidates/take_hit",
    "call-candidates/window",
    "cluster/connect",
    "cluster/window",
    "cty/cty",
    "cw-decoder/window",
    "dc/post",
    "dc/post/discord",
    "dc/post/slack",
    "edit/delete",
    "edit/edit.clear",
    "edit/edit.cq_sp",
    "edit/edit.next_band",
    "edit/edit.next_mode",
    "edit/edit.next_power",
    "edit/insert",
    "edit/invalid",
    "edit/qso",
    "es/window",
    "file/file.exit",
    "file/file.export",
    "file/file.import",
    "file/file.save",
    "file/file.station",
    "ft8/window",
    "hist/window",
    "keyer/bank",
    "keyer/cq",
    "keyer/cq_hold",
    "keyer/cq_repeat",
    "keyer/keyboard",
    "keyer/speed_down",
    "keyer/speed_up",
    "keyer/stop",
    "keyer/window",
    "link-freq/window",
    "link/connect",
    "link/share",
    "link/window",
    "map/window",
    "multipliers/window",
    "qso:cluster/send-spot",
    "qso:soumu/look-up",
    "qso:this-station/check-call",
    "qsy/window",
    "qth/shown",
    "remote/connect",
    "remote/disconnect",
    "remote/stop",
    "remote/use.rig1",
    "remote/use.rig2",
    "rate-analysis/window",
    "rate/window",
    "rig/device.connect",
    "rig/device.disconnect",
    "rig/device.toggle_rig",
    "rig/window",
    "rtty/send",
    "rtty/stop",
    "rtty/window",
    "scope/memorise",
    "scope/window",
    "score/window",
    "search/window",
    "settings/settings.reopen",
    "so2r-queue/window",
    "so2r/both",
    "so2r/rx1",
    "so2r/rx2",
    "so2r/toggle_tx",
    "so2r/window",
    "this-station/window",
    "undo/redo",
    "undo/skip",
    "undo/undo",
    "voice/cq",
    "voice/cq_hold",
    "voice/cq_repeat",
    "voice/record",
    "voice/stop",
    "voice/window",
];
