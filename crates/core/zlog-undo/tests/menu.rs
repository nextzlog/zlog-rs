use std::sync::{Arc, Mutex};

use zlog_core::{
    Band, Book, Command, Contest, ContestMode, Context, Extension, Link, Log, LogEvent, Observer,
    Qso, QsoId, QsoScore, Registry, ScoreSheet, Scoring, Station,
};
use zlog_ui::{EntryAction, Panel};
use zlog_undo::{Shared, Undo, UndoMenu};

struct Nothing;

impl Contest for Nothing {
    fn name(&self) -> &str {
        "nothing"
    }
    fn band_range(&self) -> (Band, Band) {
        (Band::B1_9, Band::HIGHEST)
    }
    fn contest_mode(&self) -> ContestMode {
        ContestMode::All
    }
    fn sent_number(&self, _band: Band, _power: zlog_core::Power, _station: &Station) -> String {
        String::new()
    }
    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }
    fn score(&mut self, log: &Log) -> Scoring {
        Scoring {
            qsos: vec![QsoScore::default(); log.qsos.len()],
            sheet: ScoreSheet::default(),
        }
    }
}

fn book() -> (Book, Shared, UndoMenu) {
    let history = Shared::default();
    let registry = Registry::new().with(Undo(history.clone()));
    let (book, _) = Book::open(Log::default(), Box::new(Nothing), "nothing", registry);
    let menu = UndoMenu::new(history.clone());
    (book, history, menu)
}

fn qso(call: &str) -> Qso {
    Qso {
        callsign: call.into(),
        ..Qso::default()
    }
}

fn labels(menu: &UndoMenu, language: &str) -> [String; 2] {
    let c = menu.commands(language);
    [c[0].menu_text().to_string(), c[1].menu_text().to_string()]
}

fn undo(book: &mut Book, history: &Shared, redo: bool) {
    zlog_undo::undo(history, book, redo).unwrap();
}

#[test]
fn labels_name_the_callsign_and_what_was_done() {
    let (mut book, history, menu) = book();
    assert_eq!(labels(&menu, "ja"), ["元に戻す", "やり直し"]);
    assert_eq!(labels(&menu, "en"), ["Undo", "Redo"]);
    book.execute(Command::Insert(qso("JA1ZLO"))).unwrap();
    assert_eq!(labels(&menu, "ja")[0], "元に戻す: JA1ZLO の記録");
    assert_eq!(labels(&menu, "en")[0], "Undo: log JA1ZLO");
    undo(&mut book, &history, false);
    assert_eq!(labels(&menu, "ja"), ["元に戻す", "やり直し: JA1ZLO の記録"]);
    undo(&mut book, &history, true);
    let id = QsoId::of(&book.log().qsos[0]);
    let mut q = book.log().qsos[0].clone();
    q.callsign = "JA1ZLP".into();
    book.execute(Command::Update(id, q)).unwrap();
    assert_eq!(labels(&menu, "ja")[0], "元に戻す: JA1ZLP の訂正");
    undo(&mut book, &history, false);
    assert_eq!(labels(&menu, "ja")[1], "やり直し: JA1ZLP の訂正");
    assert_eq!(labels(&menu, "en")[1], "Redo: edit of JA1ZLP");
    undo(&mut book, &history, true);
    book.execute(Command::Delete(id)).unwrap();
    assert_eq!(labels(&menu, "ja")[0], "元に戻す: JA1ZLP の削除");
    undo(&mut book, &history, false);
    assert_eq!(labels(&menu, "en")[1], "Redo: deletion of JA1ZLP");
}

#[test]
fn a_step_of_many_counts_the_others_and_mixed_kinds_are_a_change() {
    let (mut book, history, menu) = book();
    for i in 0..50 {
        book.execute(Command::Insert(qso(&format!("JA1Z{i:02}"))))
            .unwrap();
    }
    book.begin_step();
    for q in book.log().qsos.clone() {
        let id = QsoId::of(&q);
        book.execute(Command::Update(
            id,
            Qso {
                memo: "x".into(),
                ..q
            },
        ))
        .unwrap();
    }
    book.end_step();
    assert_eq!(labels(&menu, "ja")[0], "元に戻す: JA1Z00 ほか 49 件の訂正");
    assert_eq!(labels(&menu, "en")[0], "Undo: edit of JA1Z00 and 49 more");
    undo(&mut book, &history, false);
    assert_eq!(labels(&menu, "ja")[1], "やり直し: JA1Z00 ほか 49 件の訂正");
    assert_eq!(labels(&menu, "en")[1], "Redo: edit of JA1Z00 and 49 more");
    book.begin_step();
    book.execute(Command::Insert(qso("JA1ZLO"))).unwrap();
    book.execute(Command::Delete(QsoId::of(&book.log().qsos[0])))
        .unwrap();
    book.end_step();
    assert_eq!(labels(&menu, "ja")[0], "元に戻す: JA1ZLO ほか 1 件の変更");
    assert_eq!(labels(&menu, "en")[0], "Undo: change of JA1ZLO and 1 more");
}

#[test]
fn a_blocked_step_says_why_and_only_it_can_be_skipped() {
    let (mut book, history, mut menu, remote) = linked();
    let enabled =
        |menu: &UndoMenu| -> Vec<bool> { menu.commands("en").iter().map(|c| c.enabled).collect() };
    assert_eq!(enabled(&menu), [false, false, false]);
    let c = menu.commands("ja");
    assert_eq!(
        c[0].disabled_reason.as_deref(),
        Some("元に戻す変更はありません")
    );
    book.execute(Command::Insert(qso("JA1ZLO"))).unwrap();
    assert_eq!(enabled(&menu), [true, false, false]);
    let c = &menu.commands("en")[0];
    assert!(c.section == zlog_ui::MenuSection::Undo && c.row_key.is_none() && c.shortcut.is_none());
    let mut theirs = book.log().qsos[0].clone();
    theirs.memo = "theirs".into();
    theirs.reserve3 += 1;
    remote
        .0
        .lock()
        .unwrap()
        .push(Command::Update(QsoId::of(&theirs), theirs));
    book.poll().unwrap();
    assert_eq!(enabled(&menu), [false, false, true]);
    let c = menu.commands("ja");
    assert_eq!(
        c[0].disabled_reason.as_deref(),
        Some("元に戻せません: JA1ZLO は他局が編集済み (「この操作を飛ばす」で続行)")
    );
    let log = book.log().qsos.clone();
    menu.run_command("skip");
    assert_eq!(book.log().qsos, log, "a skip changes nothing");
    let actions = menu.take_actions();
    let [EntryAction::Say(said)] = &actions[..] else {
        panic!("{actions:?}");
    };
    assert_eq!(said.text, "Action skipped; the log is unchanged");
    assert_eq!(history.lock().unwrap().steps(), (0, 0));
    book.execute(Command::Insert(qso("JA1ZZZ"))).unwrap();
    menu.run_command("undo");
    let actions = menu.take_actions();
    let [EntryAction::Edit { batch, done }] = &actions[..] else {
        panic!("{actions:?}");
    };
    assert_eq!(done.as_deref(), Some("Undone: log JA1ZZZ"));
    book.execute_if(batch.clone()).unwrap();
    assert_eq!(book.log().qsos.len(), 1);
    assert_eq!(history.lock().unwrap().steps(), (0, 1));
}

#[derive(Clone, Default)]
struct Remote(Arc<Mutex<Vec<Command>>>);

impl Observer for Remote {
    fn name(&self) -> &str {
        "remote"
    }
    fn on_event(&mut self, _cx: &mut Context<'_>, _event: &LogEvent) {}
}

impl Link for Remote {
    fn poll(&mut self, cx: &mut Context<'_>) {
        for c in self.0.lock().unwrap().drain(..) {
            cx.send(c);
        }
    }
}

struct Network(Remote);

impl Extension for Network {
    fn name(&self) -> &str {
        "network"
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(self.0.clone())]
    }
}

fn linked() -> (Book, Shared, UndoMenu, Remote) {
    let (history, remote) = (Shared::default(), Remote::default());
    let registry = Registry::new()
        .with(Network(remote.clone()))
        .with(Undo(history.clone()));
    let (book, _) = Book::open(Log::default(), Box::new(Nothing), "nothing", registry);
    let menu = UndoMenu::new(history.clone());
    (book, history, menu, remote)
}

#[test]
fn an_import_is_undone_and_redone_in_one_step() {
    let (mut book, history, menu) = book();
    book.execute(Command::Insert(qso("JA1ZLO"))).unwrap();
    book.begin_tagged_step(zlog_core::IMPORT);
    for n in 0..120 {
        book.execute(Command::Insert(qso(&format!("JA1A{n:03}"))))
            .unwrap();
    }
    book.end_step();
    assert_eq!(labels(&menu, "ja")[0], "元に戻す: インポート 120 件");
    assert_eq!(labels(&menu, "en")[0], "Undo: import of 120 QSOs");
    undo(&mut book, &history, false);
    assert_eq!(book.log().qsos.len(), 1);
    assert_eq!(
        labels(&menu, "ja"),
        ["元に戻す: JA1ZLO の記録", "やり直し: インポート 120 件"]
    );
    undo(&mut book, &history, true);
    assert_eq!(book.log().qsos.len(), 121);
    assert_eq!(labels(&menu, "ja")[0], "元に戻す: インポート 120 件");
}

#[test]
fn two_queued_undos_keep_the_redo() {
    let (mut book, history, _menu) = book();
    book.execute(Command::Insert(qso("JA1ZLO"))).unwrap();
    let first = history.lock().unwrap().batch(false).unwrap();
    let second = history.lock().unwrap().batch(false).unwrap();
    book.execute_if(first).unwrap();
    assert!(book.log().qsos.is_empty());
    assert!(book.execute_if(second).is_err());
    assert_eq!(history.lock().unwrap().steps(), (0, 1));
}
