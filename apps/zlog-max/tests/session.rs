use std::path::{Path, PathBuf};
use zlog_core::qso::QslState;
use zlog_core::{
    Band, Batch, Command, ContestMode, DelphiTime, Format, Log, LogFormat, Mode, Power, Qso, QsoId,
    Registry, Station,
};
use zlog_gui::testing::i18n::{tr, trf};
use zlog_gui::testing::session::*;
use zlog_gui::testing::settings::Preferences;

fn init() {
    zlog_gui::testing::registry::install(zlog_max::register);
}

fn registry() -> Registry {
    Registry::new()
        .with(zlog_cfg::Cfg)
        .with(zlog_rule_ja::Japan)
}

fn setup(dir: &Path) -> Setup {
    Setup {
        contest: 0,
        mode: ContestMode::Mix,
        station: Station {
            callsign: "JA1ZLO".into(),
            prov: "10".into(),
            ..Station::default()
        },
        contest_file: None,
        coeff: 1.0,
        path: dir.join("test.zlox"),
        use_contest_period: true,
        convert_old_other: false,
    }
}

#[test]
fn many_qsos_are_updated_with_one_save() {
    init();
    let dir = scratch("update_many");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    for call in ["JA1A", "JA1B"] {
        s.execute(Command::Insert(zlog_core::Qso {
            callsign: call.into(),
            nr_sent: "10".into(),
            ..zlog_core::Qso::default()
        }));
    }
    let list: Vec<(QsoId, Qso)> = s
        .log()
        .qsos
        .iter()
        .map(|q| {
            let mut new = q.clone();
            new.nr_sent = if q.callsign == "JA1A" {
                "10".into()
            } else {
                "11".into()
            };
            (QsoId::of(q), new)
        })
        .collect();
    let zlog_ui::EntryAction::Edit { batch, done } = zlog_ui::EntryAction::update_qsos(list) else {
        unreachable!()
    };
    s.edit(batch, done);
    assert_eq!(s.status.text, trf("status.qsos_updated", &[&1]));
    assert_eq!(s.log().qsos[1].nr_sent, "11");
    assert!(!s.unsaved());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_change_of_many_is_undone_at_once_and_saved() {
    init();
    use zlog_undo::Refusal;
    let dir = scratch("undo_many");
    let history = zlog_undo::Shared::default();
    let reg = registry().with(zlog_undo::Undo(history.clone()));
    let mut s = Session::open(setup(&dir), reg).unwrap();
    let blocked = |redo| history.lock().unwrap().blocked(redo);
    assert_eq!(blocked(false), Some(Refusal::Empty));
    for call in ["JA1A", "JA1B", "JA1C"] {
        s.execute(Command::Insert(zlog_core::Qso {
            callsign: call.into(),
            ..zlog_core::Qso::default()
        }));
    }
    let ids: Vec<QsoId> = s.log().qsos.iter().map(QsoId::of).collect();
    assert_eq!(s.change_band(&ids, Band::B21), 3);
    assert_eq!(blocked(false), None);
    let batch = |redo| history.lock().unwrap().batch(redo).unwrap();
    s.edit(batch(false), Some("undone".into()));
    assert!(s.log().qsos.iter().all(|q| q.band != Band::B21));
    assert!(!s.unsaved());
    assert_eq!(s.status.text, "undone");
    let h = history.lock().unwrap().skippable();
    assert_eq!(h, None, "nothing another station changed");
    s.edit(batch(true), None);
    assert!(s.log().qsos.iter().all(|q| q.band == Band::B21));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_log_that_cannot_be_written_stays_unsaved_and_says_why() {
    init();
    let dir = scratch("unwritable");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    s.execute(Command::Insert(zlog_core::Qso {
        callsign: "JA1ZLO".into(),
        ..zlog_core::Qso::default()
    }));
    let blocked = dir.join("blocked.zlox");
    std::fs::create_dir_all(&blocked).unwrap();
    let before = s.path.clone();
    assert!(s.try_save_as(blocked.clone()).is_err());
    assert_eq!(s.path, before, "still saved where it was");
    s.save_as(blocked.clone());
    assert_eq!(s.path, before, "menu Save As keeps the old file too");
    assert!(s.try_save_as(dir.join("elsewhere.zlox")).is_ok());
    assert!(!s.unsaved());
    std::fs::remove_dir_all(dir).unwrap();
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zlog-gui-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn qsos_from_a_link_are_saved() {
    init();
    use std::sync::{Arc, Mutex};
    use zlog_core::{Context, Extension, Link, LogEvent, Observer};
    type Inbox = Arc<Mutex<Vec<zlog_core::Qso>>>;
    struct Feed(Inbox);
    impl Observer for Feed {
        fn name(&self) -> &str {
            "feed"
        }
        fn on_event(&mut self, _: &mut Context<'_>, _: &LogEvent) {}
    }
    impl Link for Feed {
        fn poll(&mut self, cx: &mut Context<'_>) {
            for q in self.0.lock().unwrap().drain(..) {
                cx.send(Command::Insert(q));
            }
        }
    }
    struct Net(Option<Inbox>);
    impl Extension for Net {
        fn name(&self) -> &str {
            "net"
        }
        fn links(&mut self) -> Vec<Box<dyn Link>> {
            self.0
                .take()
                .map(|i| Box::new(Feed(i)) as Box<dyn Link>)
                .into_iter()
                .collect()
        }
    }
    let dir = scratch("linked");
    let inbox = Inbox::default();
    let mut s = Session::open(setup(&dir), registry().with(Net(Some(inbox.clone())))).unwrap();
    let t0 = s.log().start_time().0;
    inbox.lock().unwrap().push(zlog_core::Qso {
        callsign: "JA2AAA".into(),
        time: DelphiTime(t0 + 0.1),
        ..zlog_core::Qso::default()
    });
    s.poll();
    assert!(!s.unsaved());
    let s2 = Session::open(setup(&dir), registry()).unwrap();
    assert_eq!(s2.log().qsos.len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_check_before_enter_sends_sees_the_qso_logging_inserts() {
    init();
    let dir = scratch("entry_qso");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    s.entry.callsign = "JA2AAA".into();
    s.entry.nr_rcvd = "20H".into();
    let late = DelphiTime(t0 + 30.0);
    let q = s.entry_qso(late, None).unwrap();
    assert_eq!(q.invalid, s.out_of_period(&q));
    assert!(q.invalid, "the contest period is checked as when logging");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn close_saves_unsaved_changes() {
    init();
    let dir = scratch("close");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    s.execute_unsaved(Command::Insert(zlog_core::Qso {
        callsign: "JA2AAA".into(),
        time: DelphiTime(t0 + 0.1),
        ..zlog_core::Qso::default()
    }));
    assert!(s.unsaved());
    s.close();
    let s2 = Session::open(setup(&dir), registry()).unwrap();
    assert_eq!(s2.log().qsos.len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn change_mode_leaves_large_rsts() {
    init();
    let dir = scratch("bigrst");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    s.entry.band = Band::B7;
    s.entry.mode = Mode::Ssb;
    s.entry.callsign = "JA2AAA".into();
    s.entry.nr_rcvd = "20H".into();
    assert!(s.log_entry(DelphiTime(t0 + 0.1)).is_ok());
    let id = QsoId::of(&s.log().qsos[0]);
    s.change_qsos(&[id], |q| (q.rst_sent, q.rst_rcvd) = (6553, 59));
    s.change_mode(&[id], Mode::Cw);
    assert_eq!(s.log().qsos[0].rst_sent, 6553);
    assert_eq!(s.log().qsos[0].rst_rcvd, 599);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn log_save_and_reopen() {
    init();
    let dir = scratch("reopen");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    s.entry.band = Band::B7;
    s.entry.mode = Mode::Cw;
    s.entry.callsign = "JA2AAA".into();
    s.entry.nr_rcvd = "20H".into();
    assert!(s.log_entry(DelphiTime(t0 + 0.1)).is_ok());
    assert_eq!(s.entry.callsign, "");
    assert_eq!(s.sheet().score, 1);

    s.entry.callsign = "JA2BBB".into();
    s.entry.nr_rcvd = "99H".into();
    assert_eq!(
        s.log_entry(DelphiTime(t0 + 0.2)),
        Err(NotLogged::InvalidNumber)
    );
    assert_eq!(s.status.text, tr("status.invalid_number"));

    let s2 = Session::open(setup(&dir), registry()).unwrap();
    assert_eq!(s2.log().qsos.len(), 1);
    assert_eq!(s2.log().qsos[0].nr_sent, "10H");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn default_start_time_follows_the_contest_start_hour() {
    init();
    let now = DelphiTime::from_ymd_hms(2026, 6, 6, 15, 42, 10);
    let at = |t: DelphiTime| t.to_ymd_hms();
    assert_eq!(
        at(default_start_time(now, Some(21))),
        (2026, 6, 6, 21, 0, 0)
    );
    assert_eq!(at(default_start_time(now, Some(0))), (2026, 6, 7, 0, 0, 0));
    let end = DelphiTime::from_ymd_hms(2026, 6, 30, 23, 59, 59);
    assert_eq!(at(default_start_time(end, Some(0))), (2026, 7, 1, 0, 0, 0));
    assert_eq!(at(default_start_time(now, None)), (2026, 6, 6, 15, 0, 0));
    let midnight = DelphiTime::from_ymd_hms(2026, 6, 6, 0, 0, 0);
    assert_eq!(
        at(default_start_time(midnight, None)),
        (2026, 6, 6, 0, 0, 0)
    );
}

#[test]
fn new_log_gets_a_start_time_and_reopening_keeps_it() {
    init();
    let dir = scratch("start");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let start = s.log().start_time();
    assert_ne!(start.0, 0.0);
    assert_eq!(start.to_ymd_hms().3, 21);
    assert_eq!(s.period_hours(), 24);
    s.save();
    drop(s);
    let s2 = Session::open(setup(&dir), registry()).unwrap();
    assert_eq!(s2.log().start_time(), start);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn qso_outside_the_period_is_logged_invalid() {
    init();
    let dir = scratch("period");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    for (call, t) in [("JA2AAA", t0 - 0.01), ("JA2BBB", t0 + 1.01)] {
        s.entry.callsign = call.into();
        s.entry.nr_rcvd = "20H".into();
        s.log_entry(DelphiTime(t)).unwrap();
    }
    assert!(s.log().qsos.iter().all(|q| q.invalid));
    assert_eq!(s.sheet().score, 0);
    s.use_contest_period = false;
    s.entry.callsign = "JA2CCC".into();
    s.entry.nr_rcvd = "20H".into();
    s.log_entry(DelphiTime(t0 - 0.01)).unwrap();
    assert!(!s.log().qsos[2].invalid);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_row_logs_at_the_typed_time_and_sets_band_and_mode() {
    init();
    use zlog_ui::{EntryEnv, EntryField, EntryFields, EntryOps};
    let dir = scratch("row_time");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let (y, mo, d, ..) = s.log().start_time().to_ymd_hms();
    let mut view = zlog_gui::testing::main_window::View::default();
    let mut row = zlog_gui::testing::row::Row {
        session: &mut s,
        view: &mut view,
        hz: None,
    };
    assert!(row.caps().time && row.caps().period_refuses);
    row.set_band(Band::B14);
    row.set_mode(Mode::Ssb);
    assert_eq!((row.band(), row.mode()), (Band::B14, Mode::Ssb));
    assert_eq!(row.text(EntryField::RstSent), "59");
    row.set_text(EntryField::Call, "ja2aaa".into());
    row.set_text(EntryField::Number, "20H".into());
    row.set_text(EntryField::Time, "99:00".into());
    assert_eq!(row.log(), Err(NotLogged::InvalidTime));
    row.set_text(EntryField::Date, format!("{y}-{mo:02}-{d:02}"));
    row.set_text(EntryField::Time, "21:30".into());
    assert_eq!(row.log(), Ok(()));
    assert!(
        row.text(EntryField::Time).is_empty(),
        "cleared with the row"
    );
    let q = &s.log().qsos[0];
    assert_eq!(q.time.to_ymd_hms(), (y, mo, d, 21, 30, 0));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn refuse_keeps_a_qso_outside_the_period_out_of_the_log() {
    init();
    let dir = scratch("period_refuse");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    s.entry.callsign = "JA2AAA".into();
    s.entry.nr_rcvd = "20H".into();
    let late = DelphiTime(t0 + 1.01);
    assert_eq!(
        s.log_entry_with(late, None, Period::Refuse),
        Err(NotLogged::OutOfPeriod)
    );
    assert!(s.log().qsos.is_empty());
    assert_eq!(s.entry.callsign, "JA2AAA", "the entry is kept");
    let inside = DelphiTime(t0 + 0.1);
    assert!(s.log_entry_with(inside, None, Period::Refuse).is_ok());
    assert_eq!(s.log().qsos.len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn qso_before_the_suggested_start_is_flagged_and_correcting_the_start_fixes_it() {
    init();
    let dir = scratch("before_start");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    assert!(s.asks_start_time);
    let start = s.start_time();
    assert_eq!(start.to_ymd_hms().3, 21);
    s.entry.band = Band::B7;
    s.entry.mode = Mode::Cw;
    s.entry.callsign = "JA1ABC".into();
    s.entry.nr_rcvd = "12H".into();
    s.log_entry(DelphiTime(start.0 - 6.0 / 24.0)).unwrap();
    assert!(s.log().qsos[0].invalid);
    assert_eq!(s.sheet().score, 0);
    assert!(s.status.warning);
    assert_eq!(
        s.status.text,
        trf("status.logged_out_of_period", &[&"JA1ABC"])
    );
    let earlier = DelphiTime(start.0 - 1.0);
    assert_eq!(s.set_start_time(earlier, false), 0);
    assert!(s.log().qsos[0].invalid);
    assert!(!s.asks_start_time);
    assert_eq!(s.set_start_time(earlier, true), 1);
    assert!(!s.log().qsos[0].invalid);
    assert_eq!(s.sheet().score, 1);
    let s2 = Session::open(setup(&dir), registry()).unwrap();
    assert_eq!(s2.start_time(), earlier);
    assert!(!s2.asks_start_time);
    assert!(!s2.log().qsos[0].invalid);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn all_ja_12h_scores_one_point_and_one_multiplier() {
    init();
    let dir = scratch("allja_12h");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let now = now_for(s.log());
    s.set_start_time(DelphiTime(now.0 - 1.0), true);
    s.entry.band = Band::B7;
    s.entry.mode = Mode::Cw;
    s.entry.rst_sent = "599".into();
    s.entry.rst_rcvd = "599".into();
    s.entry.callsign = "JA1ABC".into();
    s.entry.nr_rcvd = "12H".into();
    s.log_entry(now).unwrap();
    let q = &s.log().qsos[0];
    assert!(!q.invalid && !q.dupe);
    assert_eq!((q.multi1.as_str(), q.points, q.new_multi1), ("12", 1, true));
    assert_eq!(q.nr_sent, "10H");
    assert!(!s.status.warning, "{:?}", s.status);
    assert_eq!(s.sheet().score, 1);
    assert_eq!(s.sheet().total.multi, 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn external_qsos_are_scored_and_checked_for_dupes() {
    init();
    let dir = scratch("external");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let now = now_for(s.log());
    s.set_start_time(DelphiTime(now.0 - 1.0), true);
    s.entry.callsign = "JA9TYPED".into();
    let q = Qso {
        time: now,
        callsign: "JA1ABC".into(),
        band: Band::B7,
        mode: Mode::Ft8,
        nr_rcvd: "12H".into(),
        ..Qso::default()
    };
    s.log_external(q.clone());
    let logged = &s.log().qsos[0];
    assert_eq!(logged.multi1, "12");
    assert!(!logged.dupe && !s.status.warning, "{:?}", s.status);
    s.log_external(q);
    assert!(s.log().qsos[1].dupe && s.status.warning);
    assert_eq!(s.entry.callsign, "JA9TYPED");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn grid_menu_changes_band_mode_power_and_invalid() {
    init();
    let dir = scratch("grid_menu");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.start_time().0 + 0.1;
    s.entry.band = Band::B7;
    s.entry.mode = Mode::Cw;
    for (i, call) in ["JA2AAA", "JA2BBB"].into_iter().enumerate() {
        s.entry.callsign = call.into();
        s.entry.nr_rcvd = "20H".into();
        s.log_entry(DelphiTime(t0 + i as f64 * 0.01)).unwrap();
    }
    let ids: Vec<QsoId> = s.log().qsos.iter().map(QsoId::of).collect();
    assert_eq!(s.change_band(&ids, Band::B14), 2);
    assert!(s.log().qsos.iter().all(|q| q.band == Band::B14));
    assert_eq!(s.change_mode(&ids[..1], Mode::Ssb), 1);
    let q = &s.log().qsos[0];
    assert_eq!((q.mode, q.rst_sent, q.rst_rcvd), (Mode::Ssb, 59, 59));
    s.change_mode(&ids[..1], Mode::Cw);
    assert_eq!(s.log().qsos[0].rst_sent, 599);
    assert_eq!(s.change_power(&ids[1..], Power::P001), 1);
    assert_eq!(s.log().qsos[1].power, Power::P001);
    assert_eq!(s.change_power(&ids[1..], Power::P001), 0);
    let score = s.sheet().score;
    let toggle = |q: &mut Qso| q.invalid = !q.invalid;
    assert_eq!(s.change_qsos(&ids[..1], toggle), 1);
    assert!(s.log().qsos[0].invalid);
    assert_eq!(s.sheet().score, score - 1);
    s.change_qsos(&ids[..1], toggle);
    assert!(!s.log().qsos[0].invalid);
    assert!(s.execute(Command::Delete(ids[0])));
    assert_eq!(s.change_band(&ids, Band::B21), 1);
    let s2 = Session::open(setup(&dir), registry()).unwrap();
    assert_eq!(s2.log().qsos[0].band, Band::B21);
    std::fs::remove_dir_all(dir).unwrap();
}

struct Lines;

impl zlog_core::LogFormat for Lines {
    fn name(&self) -> &str {
        "lines"
    }
    fn extensions(&self) -> &[&str] {
        &["txt"]
    }
    fn can_import(&self) -> bool {
        true
    }
    fn import(&self, bytes: &[u8]) -> Result<Vec<Qso>, String> {
        let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, l)| Qso {
                callsign: l.trim().into(),
                nr_rcvd: "20H".into(),
                reserve3: (i as i32 + 1) * 100,
                ..Qso::default()
            })
            .collect())
    }
}

struct Refuser;

impl zlog_core::Observer for Refuser {
    fn name(&self) -> &str {
        "refuser"
    }
    fn on_event(&mut self, cx: &mut zlog_core::Context<'_>, event: &zlog_core::LogEvent) {
        if let zlog_core::LogEvent::Inserted { index } = event {
            if cx.log().qsos[*index].callsign == "LOOP" {
                cx.send(Command::Insert(Qso {
                    callsign: "LOOP".into(),
                    ..Qso::default()
                }));
            }
        }
    }
}

struct Testing;

impl zlog_core::Extension for Testing {
    fn name(&self) -> &str {
        "testing"
    }
    fn formats(&self) -> Vec<Box<dyn LogFormat>> {
        vec![Box::new(Lines)]
    }
    fn observers(&mut self) -> Vec<Box<dyn zlog_core::Observer>> {
        vec![Box::new(Refuser)]
    }
}

fn import_registry() -> Registry {
    registry().with(Testing)
}

fn lines_format(s: &Session) -> usize {
    s.formats()
        .iter()
        .position(|f| f.name() == "lines")
        .unwrap()
}

#[test]
fn import_keeps_and_saves_qsos_whose_chain_failed() {
    init();
    let dir = scratch("import-refused");
    let mut s = Session::open(setup(&dir), import_registry()).unwrap();
    let file = dir.join("in.txt");
    std::fs::write(&file, "JA2AAA\nLOOP\nJA3BBB\n").unwrap();
    s.import(lines_format(&s), &file);
    assert!(s.status.warning, "{:?}", s.status);
    assert!(s.status.text.starts_with(&trf("status.imported", &[&3])));
    assert!(!s.unsaved());
    let s2 = Session::open(setup(&dir), registry()).unwrap();
    assert_eq!(s2.log().qsos.len(), s.log().qsos.len());
    assert!(s2.log().qsos.iter().any(|q| q.callsign == "LOOP"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn import_status_keeps_a_save_failure() {
    init();
    let dir = scratch("import-save-failed");
    let mut s = Session::open(setup(&dir), import_registry()).unwrap();
    let file = dir.join("in.txt");
    std::fs::write(&file, "JA2AAA\nLOOP\n").unwrap();
    s.path = dir.join("missing").join("test.zlox");
    s.import(lines_format(&s), &file);
    assert!(s.status.warning);
    let failed = trf("status.save_failed", &[&""]);
    assert!(s.status.text.contains(failed.trim_end()), "{:?}", s.status);
    assert!(s.unsaved());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn entry_whose_chain_failed_is_logged_once() {
    init();
    let dir = scratch("entry-chain-failed");
    let mut s = Session::open(setup(&dir), import_registry()).unwrap();
    let t0 = s.log().start_time().0;
    s.entry.callsign = "LOOP".into();
    s.entry.nr_rcvd = "20H".into();
    assert!(s
        .log_entry_with(DelphiTime(t0 + 0.1), None, Period::Warn)
        .is_ok());
    assert!(s.status.warning, "the failure is reported");
    assert!(
        s.entry.callsign.is_empty(),
        "Enter again would log it twice"
    );
    assert!(!s.unsaved());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_import_is_one_step() {
    init();
    let dir = scratch("import-step");
    let history = zlog_undo::Shared::default();
    let reg = import_registry().with(zlog_undo::Undo(history.clone()));
    let mut s = Session::open(setup(&dir), reg).unwrap();
    let file = dir.join("in.txt");
    std::fs::write(&file, "JA2AAA\nJA3BBB\nJA4CCC\n").unwrap();
    s.import(lines_format(&s), &file);
    let h = history.lock().unwrap();
    assert_eq!(h.steps(), (1, 0));
    assert_eq!(h.next(false).unwrap().tag, Some(zlog_core::IMPORT));
    assert_eq!(h.next(false).unwrap().changes.len(), 3);
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn import_without_refusals_is_info() {
    init();
    let dir = scratch("import-ok");
    let mut s = Session::open(setup(&dir), import_registry()).unwrap();
    let file = dir.join("in.txt");
    std::fs::write(&file, "JA2AAA\n\nJA3BBB\n").unwrap();
    s.import(lines_format(&s), &file);
    assert_eq!(s.status, Status::info(trf("status.imported", &[&2])));
    assert_eq!(s.log().qsos.len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn import_saves_only_when_something_was_added() {
    init();
    let dir = scratch("import-none");
    let mut s = Session::open(setup(&dir), import_registry()).unwrap();
    let file = dir.join("in.txt");
    std::fs::write(&file, "").unwrap();
    s.import(lines_format(&s), &file);
    assert_eq!(s.status, Status::info(trf("status.imported", &[&0])));
    assert!(!s.path.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn import_of_a_missing_file_fails() {
    init();
    let dir = scratch("import-missing");
    let mut s = Session::open(setup(&dir), import_registry()).unwrap();
    s.import(lines_format(&s), &dir.join("absent.txt"));
    assert!(s.status.warning);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rig_frequency_is_recorded_in_khz() {
    init();
    let dir = scratch("freq");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    let log = |s: &mut Session, call: &str, hz: Option<u64>| {
        s.entry.callsign = call.into();
        s.entry.nr_rcvd = "20H".into();
        s.log_entry_with(DelphiTime(t0 + 0.1), hz, Period::Warn)
            .unwrap();
        s.log().qsos.last().unwrap().freq.clone()
    };
    assert!(s.record_freq, "RecordFreqInMemo defaults to on");
    assert_eq!(log(&mut s, "JA2AAA", Some(7_012_345)), "7012.3");
    assert_eq!(log(&mut s, "JA2BBB", None), "");
    assert_eq!(
        log(&mut s, "JA2CCC", Some(0)),
        "",
        "no frequency from the rig"
    );
    s.record_freq = false;
    assert_eq!(log(&mut s, "JA2DDD", Some(7_012_345)), "");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn khz_string_as_zlog() {
    init();
    assert_eq!(khz_string(7_012_345), "7012.3");
    assert_eq!(khz_string(14_000_000), "14000.0");
    assert_eq!(khz_string(999), "0.9");
    assert_eq!(khz_string(10_489_550_000), "10489550.0");
}

#[test]
fn pedition_reaches_dv() {
    init();
    let dir = scratch("pedi");
    let reg = Registry::new().with(zlog_rule_dx::World);
    let index = reg
        .contests()
        .iter()
        .position(|c| c.build(ContestMode::Mix).name().contains("Pedition"))
        .expect("Pedition contest");
    let mut st = setup(&dir);
    st.contest = index;
    let mut s = Session::open(st, reg).unwrap();
    s.entry.band = Band::B7;
    s.entry.mode = Mode::Ft8;
    s.cycle_mode(false);
    assert_eq!(s.entry.mode, Mode::Dv);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn qsos_outside_the_period_are_told_to_the_panels() {
    init();
    use zlog_ui::PeriodCheck;
    let dir = scratch("edit-period");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    let mut q = Qso {
        time: DelphiTime(t0 - 0.5),
        callsign: "JA2AAA".into(),
        band: Band::B7,
        ..Qso::default()
    };
    assert!(s.outside(&q));
    q.time = DelphiTime(t0 + 0.1);
    assert!(!s.outside(&q));
    s.use_contest_period = false;
    q.time = DelphiTime(t0 - 0.5);
    assert!(!s.outside(&q));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn open_takes_the_contest_period_setting() {
    init();
    let dir = scratch("period-setting");
    let mut st = setup(&dir);
    st.use_contest_period = false;
    let s = Session::open(st, registry()).unwrap();
    assert!(!s.use_contest_period);
    assert_eq!(s.period_hours(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

fn one_qso_log(mode: Mode) -> Log {
    let mut log = Log::default();
    log.qsos.push(Qso {
        callsign: "JA2AAA".into(),
        nr_rcvd: "20H".into(),
        band: Band::B7,
        mode,
        ..Qso::default()
    });
    log
}

#[test]
fn out_of_range_bytes_are_reported() {
    init();
    let dir = scratch("bad-bytes");
    let st = setup(&dir);
    let mut bytes = one_qso_log(Mode::Cw).to_bytes(Format::Zlox);
    bytes[zlog_core::zlo::ZLOX_RECORD + 92] = 200;
    std::fs::write(&st.path, bytes).unwrap();
    let s = Session::open(st, registry()).unwrap();
    assert_eq!(s.status, Status::warning(trf("status.bad_bytes", &[&1])));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn old_other_is_asked_for_zlo_only_and_converted_to_zlox() {
    init();
    let dir = scratch("old-other");
    let zlo = dir.join("old.zlo");
    one_qso_log(Mode::Ft4).save(&zlo, Format::Zlo).unwrap();
    assert!(asks_old_other(&zlo));
    let zlox = dir.join("new.zlox");
    one_qso_log(Mode::Ft4).save(&zlox, Format::Zlox).unwrap();
    assert!(!asks_old_other(&zlox), "only .ZLO files are asked about");
    one_qso_log(Mode::Cw)
        .save(dir.join("cw.zlo"), Format::Zlo)
        .unwrap();
    assert!(!asks_old_other(&dir.join("cw.zlo")));
    assert!(!asks_old_other(&dir.join("absent.zlo")));

    let mut st = setup(&dir);
    st.path = zlo.clone();
    let s = Session::open(st.clone(), registry()).unwrap();
    assert_eq!((s.log().qsos[0].mode, s.format), (Mode::Ft4, Format::Zlo));
    drop(s);
    st.convert_old_other = true;
    let s = Session::open(st, registry()).unwrap();
    assert_eq!(s.log().qsos[0].mode, Mode::Other);
    assert_eq!(
        (s.path.clone(), s.format),
        (dir.join("old.zlox"), Format::Zlox)
    );
    assert!(s.path.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn utc_hhmm_from_the_log_zone() {
    init();
    let t = DelphiTime::from_ymd_hms(2026, 6, 6, 9, 5, 30);
    assert_eq!(utc_hhmm(t, None), "0905");
    assert_eq!(utc_hhmm(t, Some(540)), "0005");
    assert_eq!(utc_hhmm(t, Some(600)), "2305");
}

#[test]
fn time_replaces_d_in_place() {
    init();
    assert_eq!(with_time("10$D", "1307"), "101307");
    assert_eq!(with_time("$DM", "1307"), "1307M");
    assert_eq!(with_time("001", "1307"), "0011307");
}

#[test]
fn bartg_sends_serial_and_utc_time() {
    init();
    let dir = scratch("bartg");
    let reg = Registry::new().with(zlog_rule_dx::World);
    let index = reg
        .contests()
        .iter()
        .position(|c| c.build(ContestMode::Rtty).name() == "BARTG HF RTTY")
        .expect("BARTG");
    let mut st = setup(&dir);
    st.contest = index;
    st.mode = ContestMode::Rtty;
    let mut s = Session::open(st, reg).unwrap();
    s.entry.band = Band::B14;
    s.entry.mode = Mode::Rtty;
    s.entry.callsign = "G3AAA".into();
    s.entry.nr_rcvd = "001".into();
    let t = DelphiTime::from_ymd_hms(2026, 3, 21, 13, 7, 0);
    s.log_entry(t).unwrap();
    let q = &s.log().qsos[0];
    assert_eq!(q.nr_sent, "0011307");
    assert_eq!(q.serial, 1, "the serial is kept (main.pas Q.Serial)");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dialogs_act_on_the_qso_not_the_row() {
    init();
    let dir = scratch("by_id");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    for (i, call) in ["JA1AAA", "JA2BBB"].into_iter().enumerate() {
        s.entry.callsign = call.into();
        s.entry.nr_rcvd = "20H".into();
        s.log_entry(DelphiTime(t0 + 0.1 * (i + 1) as f64)).unwrap();
    }
    let edit = QsoId::of(&s.log().qsos[1]);
    let mut edited = s.log().qsos[1].clone();
    edited.memo = "QSB".into();
    let gone = QsoId::of(&s.log().qsos[0]);
    assert!(s.execute(Command::Delete(gone)));
    let batch = |c: Command| Batch {
        commands: vec![c],
        ..Batch::default()
    };
    s.edit(batch(Command::Update(edit, edited)), None);
    assert_eq!(s.log().qsos.len(), 1);
    assert_eq!(s.log().qsos[0].callsign, "JA2BBB");
    assert_eq!(s.log().qsos[0].memo, "QSB");
    s.edit(batch(Command::Delete(gone)), None);
    assert!(s.status.warning);
    assert_eq!(s.log().qsos.len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dupe_is_reported_on_space() {
    init();
    let dir = scratch("dupe");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0;
    s.entry.callsign = "JA2AAA".into();
    s.entry.nr_rcvd = "20H".into();
    s.log_entry(DelphiTime(t0 + 0.1)).unwrap();
    s.entry.callsign = "ja2aaa".into();
    assert!(s.check_call());
    assert!(s.status.warning);
    std::fs::remove_dir_all(dir).unwrap();
}

fn log_at(s: &mut Session, call: &str, band: Band, t: f64) {
    s.entry.callsign = call.into();
    s.entry.nr_rcvd = "20H".into();
    s.entry.band = band;
    s.log_entry(DelphiTime(t)).unwrap();
}

#[test]
fn logging_sets_the_qsy_violation() {
    init();
    let dir = scratch("qsy");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0 + 0.1;
    let min = 1.0 / 1440.0;
    log_at(&mut s, "JA2AAA", Band::B7, t0);
    log_at(&mut s, "JA2BBB", Band::B14, t0 + min);
    assert!(!s.log().qsos[1].qsy_violation, "rules are off by default");
    s.qsy_rule.countdown_minutes = Some(10);
    log_at(&mut s, "JA2CCC", Band::B21, t0 + 2.0 * min);
    assert!(s.log().qsos[2].qsy_violation);
    log_at(&mut s, "JA2DDD", Band::B21, t0 + 30.0 * min);
    assert!(!s.log().qsos[3].qsy_violation);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn logging_stores_the_pc_name() {
    init();
    let dir = scratch("pcname");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let t0 = s.log().start_time().0 + 0.1;
    log_at(&mut s, "JA2AAA", Band::B7, t0);
    assert_eq!(s.log().qsos[0].pc_name, "");
    s.pc_name = "PC1".into();
    log_at(&mut s, "JA2BBB", Band::B7, t0);
    assert_eq!(s.log().qsos[1].pc_name, "PC1");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn preferences_reach_the_log_and_the_saved_file() {
    init();
    let dir = scratch("prefs");
    let mut s = Session::open(setup(&dir), registry()).unwrap();
    let mut p = Preferences {
        qsl_default: QslState::PseQsl,
        pc_name: "PC9".into(),
        ..Preferences::default()
    };
    p.set_countdown(true);
    s.apply_preferences(&p);
    assert_eq!(s.qsy_rule.countdown_minutes, Some(10));
    assert_eq!(s.log().qsy_rule, s.qsy_rule);
    let t0 = s.log().start_time().0 + 0.1;
    log_at(&mut s, "JA2AAA", Band::B7, t0);
    assert_eq!(s.log().qsos[0].pc_name, "PC9");
    let path = dir.join("prefs.zlo");
    s.save_as(path.clone());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes, s.log().to_bytes_with(Format::Zlo, QslState::PseQsl));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_contest_file_opens_with_a_warning_for_a_missing_dat() {
    init();
    let dir = scratch("contest-file");
    let cfg = dir.join("MY.CFG");
    std::fs::write(&cfg, "#My Contest\ndat my.dat\n").unwrap();
    let st = Setup {
        contest_file: Some(cfg),
        ..setup(&dir)
    };
    let s = Session::open(
        st,
        Registry::new()
            .with(zlog_cfg::Cfg)
            .with(zlog_rule_ja::Japan),
    )
    .unwrap();
    assert_eq!(s.rules().name(), "My Contest");
    assert!(s.status.text.contains("my.dat"), "{}", s.status.text);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_dupe_is_an_invalid_number_where_the_scoring_script_says_so() {
    init();
    zlog_lua::install();
    zlog_cfg::script::host::place(zlog_cfg::script::host::Placement::Child {
        program: env!("CARGO_BIN_EXE_zlog-max").into(),
        args: vec![zlog_cfg::script::host::SUBCOMMAND.into()],
    });
    zlog_cfg::script::host::patience(std::time::Duration::from_secs(30));
    let dir = scratch("dupe-script");
    let plain = dir.join("PLAIN.CFG");
    std::fs::write(&plain, "#Plain Contest\nundefmulti on\n").unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/extensions/zlog-lua/tests/fixtures");
    for f in ["sample.cfg", "sample.dat"] {
        std::fs::copy(fixtures.join(f), dir.join(f)).unwrap();
    }
    for (file, refused) in [(dir.join("sample.cfg"), true), (plain, false)] {
        let _ = std::fs::remove_file(dir.join("test.zlox"));
        let st = Setup {
            contest_file: Some(file),
            ..setup(&dir)
        };
        let mut s = Session::open(st, registry()).unwrap();
        let t0 = s.log().start_time().0;
        let log = |s: &mut Session| {
            s.entry.callsign = "JA1AAA".into();
            s.entry.band = Band::B7;
            s.entry.mode = Mode::Cw;
            s.entry.nr_rcvd = "10T".into();
            s.log_entry_with(DelphiTime(t0 + 0.1), None, Period::Warn)
        };
        assert_eq!(log(&mut s), Ok(()), "{}", s.rules().name());
        if refused {
            assert_eq!(s.log().qsos[0].points, 3);
            assert_eq!(log(&mut s), Err(NotLogged::InvalidNumber));
            assert_eq!(s.log().qsos.len(), 1);
        } else {
            assert_eq!(log(&mut s), Ok(()));
            assert!(s.log().qsos[1].dupe);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_used_period_of_length_zero_ends_now() {
    init();
    struct OpenPeriod;
    impl zlog_core::Extension for OpenPeriod {
        fn name(&self) -> &str {
            "open-period"
        }
        fn contests(&self) -> Vec<Box<dyn zlog_core::ContestDef>> {
            let cfg = "#Open Period\nuseperiod on\nperiod 0\n";
            vec![Box::new(zlog_cfg::from_cfg(cfg, |_| None))]
        }
    }
    let dir = scratch("open-period");
    let s = Session::open(setup(&dir), Registry::new().with(OpenPeriod)).unwrap();
    assert!(s.rules().uses_period());
    assert_eq!(s.period_hours(), 0);
    let start = s.log().start_time().0;
    assert!(start > 0.0, "a used period suggests a start time");
    let at = |t: f64| Qso {
        time: DelphiTime(t),
        ..Qso::default()
    };
    assert!(!s.out_of_period(&at(start)));
    assert!(s.out_of_period(&at(start - 1.0 / 24.0)), "before the start");
    assert!(s.out_of_period(&at(now_for(s.log()).0 + 1.0)), "after now");
    assert!(zlog_ui::PeriodCheck::outside(&s, &at(start - 1.0)));
    std::fs::remove_dir_all(dir).unwrap();
}
