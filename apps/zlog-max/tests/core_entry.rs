use zlog_gui::testing::main_window::View;
use zlog_gui::testing::row::Row;
use zlog_gui::testing::session::Session;
use zlog_ui::{EntryEnv, EntryField, EntryFields, EntryOps, Setup, TabState};

fn session(dir: &std::path::Path) -> Session {
    zlog_gui::testing::registry::install(zlog_std::register);
    let registry = zlog_gui::testing::registry::registry();
    let contest = registry
        .contests()
        .iter()
        .position(|c| c.id() == "base")
        .expect("base contest");
    let setup = Setup {
        contest,
        contest_file: None,
        mode: zlog_core::ContestMode::Mix,
        station: zlog_core::Station {
            callsign: "JA1ZLO".into(),
            ..Default::default()
        },
        coeff: 1.0,
        path: dir.join("entry.zlox"),
        use_contest_period: false,
        convert_old_other: false,
    };
    Session::open(setup, registry).expect("session")
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("zlog-entry-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn edited(s: &Session) -> bool {
    s.tab.edited(&s.entry.callsign)
}

#[test]
fn tab_marks_the_call_and_editing_it_is_seen_by_e() {
    let dir = temp("1");
    let mut s = session(&dir);
    let mut v = View::default();
    let mut row = Row {
        session: &mut s,
        view: &mut v,
        hz: None,
    };
    row.set_text(EntryField::Call, "ja1yaa".into());
    row.tab_sent();
    assert_eq!(row.session.tab, TabState::Sent("JA1YAA".into()));
    assert!(!edited(row.session));
    row.set_text(EntryField::Call, "JA1YAB".into());
    assert!(edited(row.session));
    row.clear();
    assert_eq!(row.session.tab, TabState::Normal);
    row.set_text(EntryField::Call, "JA1YAA".into());
    row.tab_sent();
    row.log().unwrap();
    assert_eq!(row.session.tab, TabState::Normal, "logging starts anew");
}

#[test]
fn alt_o_asks_the_frame_for_the_operator_role() {
    let dir = temp("2");
    let mut s = session(&dir);
    let mut v = View::default();
    Row {
        session: &mut s,
        view: &mut v,
        hz: None,
    }
    .run_role("operator");
    assert_eq!(v.run_role.as_deref(), Some("operator"));
}

#[test]
fn a_forced_qso_is_logged_and_marked() {
    let dir = temp("3");
    let mut s = session(&dir);
    let mut v = View::default();
    let mut row = Row {
        session: &mut s,
        view: &mut v,
        hz: None,
    };
    row.set_text(EntryField::Call, "JA1YAA".into());
    row.log_forced().unwrap();
    let q = s.log().qsos.last().unwrap();
    assert_eq!((q.callsign.as_str(), q.forced), ("JA1YAA", true));
    assert!(s.entry.callsign.is_empty());
}

#[test]
fn set_power_sets_it_directly() {
    let dir = temp("4");
    let mut s = session(&dir);
    let mut v = View::default();
    let mut row = Row {
        session: &mut s,
        view: &mut v,
        hz: None,
    };
    row.set_power(zlog_core::Power::P005);
    assert_eq!(row.power(), zlog_core::Power::P005);
}
