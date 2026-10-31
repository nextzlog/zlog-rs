use zlog_gui::testing::settings::Settings;

fn init() {
    zlog_gui::testing::registry::install(zlog_max::register);
}

#[test]
fn the_rig_commands_keep_their_shortcuts_and_names() {
    init();
    let mut s = Settings::default();
    s.section("bind")
        .insert("device/device.connect".into(), "Ctrl+R".into());
    s.section("bind")
        .insert("name:device/device.rig_window".into(), "Radio".into());
    s.section("bind")
        .insert("file/file.save".into(), "Ctrl+S".into());
    s.move_old_keys(&zlog_gui::testing::registry::migrations());
    let bind = s.section("bind");
    assert_eq!(bind["rig/device.connect"], "Ctrl+R");
    assert_eq!(bind["name:rig/window"], "Radio");
    assert_eq!(bind["file/file.save"], "Ctrl+S");
    assert!(!bind.contains_key("device/device.connect"));
}

#[test]
fn merged_check_windows_take_the_old_settings() {
    init();
    let mut s = Settings::default();
    s.section("panel.super-check")
        .insert("super_check_file".into(), "/m.scp".into());
    s.section("panel.partial-check")
        .insert("max".into(), "50".into());
    let w = s.section("window");
    w.insert("hidden_toolbars".into(), "partial-check,super-check".into());
    w.insert("shown_toolbars".into(), "check-multi,score".into());
    w.insert(
        "toolbar_order".into(),
        "check-call,score,check-multi,super-check".into(),
    );
    s.section("bind")
        .insert("partial-check/window".into(), "Ctrl+P".into());
    s.section("bind")
        .insert("name:check-call/window".into(), "Mine".into());
    s.move_old_keys(&zlog_gui::testing::registry::migrations());
    let c = s.section("panel.call-candidates").clone();
    assert_eq!(c["super_check_file"], "/m.scp");
    assert_eq!(c["max"], "50");
    assert!(!s.sections.contains_key("panel.super-check"));
    let w = s.section("window").clone();
    assert_eq!(w["hidden_toolbars"], "call-candidates");
    assert_eq!(w["shown_toolbars"], "score,this-station");
    assert_eq!(w["toolbar_order"], "this-station,score,call-candidates");
    assert_eq!(s.section("bind")["call-candidates/window"], "Ctrl+P");
    assert_eq!(s.section("bind")["name:this-station/window"], "Mine");
    let before = s.clone();
    s.move_old_keys(&zlog_gui::testing::registry::migrations());
    assert_eq!(s, before);
}

#[test]
fn renamed_windows_keep_their_settings_and_toolbars() {
    init();
    let mut s = Settings::parse_ini(
        "[panel.zserver]\nhost = h\n[panel.maplot]\nk = v\n[window]\nhidden_toolbars = score,maplot\nshown_toolbars = country\n",
    );
    s.move_old_keys(&zlog_gui::testing::registry::migrations());
    assert_eq!(s.section("panel.link")["host"], "h");
    assert_eq!(s.section("panel.map")["k"], "v");
    assert!(!s.sections.contains_key("panel.zserver"));
    assert_eq!(s.section("window")["hidden_toolbars"], "score,map");
    assert_eq!(s.section("window")["shown_toolbars"], "qth");
}

#[test]
fn pc_name_falls_back_to_the_old_zserver_key() {
    init();
    let mut s = Settings::parse_ini("[zserver]\npc-name = OLD\n");
    s.move_old_keys(&zlog_gui::testing::registry::migrations());
    assert_eq!(s.preferences().pc_name, "OLD");
    let mut tab = Settings::only("preferences", s.section("preferences"));
    assert_eq!(tab.preferences().pc_name, "OLD");
    let mut p = s.preferences();
    p.pc_name = "NEW".into();
    s.set_preferences(&p);
    assert_eq!(s.preferences().pc_name, "NEW");
}

#[test]
fn z_link_secrets_and_settings_survive_a_save_and_restart() {
    use egui_kittest::kittest::Queryable;
    use std::sync::{Arc, Mutex};
    use zlog_gui::testing::app::panels::{panel_sections, PanelSlot};
    use zlog_link::net::{Shared, SECTION};
    use zlog_link::{LinkNet, Session, ZLinkPanel};
    use zlog_ui::SettingsPage;
    init();
    let _env = zlog_testing::Env::new("link-secrets");
    let net = zlog_link::net::shared();
    let session = zlog_link::shared();
    let identity = net.lock().identity().unwrap();
    let ip: std::net::IpAddr = "192.168.1.7".parse().unwrap();
    net.lock().clients.add(zlog_tls_peer(ip), "PC7");

    let mut section = zlog_ui::Section::new();
    section.insert("server_port".into(), "5000".into());
    let page = zlog_link::panel::LinkOptions::default();
    let mut h = egui_kittest::Harness::builder()
        .with_accessibility_check(false)
        .build_ui_state(
            |ui, state: &mut (zlog_link::panel::LinkOptions, zlog_ui::Section)| {
                state.0.ui(ui, &mut state.1, "en")
            },
            (page, section),
        );
    h.get_by_label("New token").click();
    h.run();
    h.get_by_label("Token for a TLS server").click();
    h.run();
    h.get_by_label("Token for a TLS server")
        .type_text("SECRET42");
    h.run();
    let (page, section) = h.into_state();
    let token = page.shown_token().expect("shown once").to_string();
    assert_eq!(net.lock().client_token, "SECRET42");

    let mut settings = zlog_gui::testing::settings::Settings::default();
    *settings.section("panel.link") = section;
    let slot = |panel: Box<dyn zlog_ui::Panel>| PanelSlot {
        panel,
        open: false,
        was_open: false,
        title: String::new(),
    };
    let panels = [
        slot(Box::new(ZLinkPanel::with(session.clone(), net.clone()))),
        slot(Box::new(LinkNet::new(net.clone(), session.clone()))),
    ];
    assert!(panel_sections(&panels, &mut settings));
    settings.save().unwrap();

    let mut again = zlog_gui::testing::settings::Settings::load();
    let link = again.section("panel.link").clone();
    let secret = again.section(SECTION).clone();
    assert_eq!(link.get("server_port").map(String::as_str), Some("5000"));
    for k in ["token_hash", "client_token", "identity", "clients"] {
        assert!(!link.contains_key(k), "{k} leaked into panel.link");
        assert!(secret.contains_key(k), "{k} missing from {SECTION}");
    }
    assert!(!secret.contains_key("server_port"));

    let fresh = Arc::new(Shared::default());
    let fresh_session = Arc::new(Mutex::new(Session::default()));
    let mut panel = ZLinkPanel::with(fresh_session.clone(), fresh.clone());
    let mut hidden = LinkNet::new(fresh.clone(), fresh_session);
    zlog_ui::Panel::load(&mut hidden, &secret);
    zlog_ui::Panel::load(&mut panel, &link);
    let n = fresh.lock();
    assert!(n.token.unwrap().verify(&token), "token hash");
    assert_eq!(n.client_token, "SECRET42");
    assert_eq!(n.identity.as_ref(), Some(&identity));
    assert_eq!(n.clients.name(&zlog_tls_peer(ip)).as_deref(), Some("PC7"));
    assert_eq!(n.settings.port, "5000");
}

fn zlog_tls_peer(ip: std::net::IpAddr) -> zlog_link::net::PeerId {
    zlog_link::net::PeerId::Ip(ip)
}
