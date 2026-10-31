use std::cell::RefCell;
use std::rc::Rc;

use zlog_core::{
    Band, Book, Command, Contest, ContestMode, Context, Extension, Link, Log, LogEvent, Mismatch,
    Observer, Qso, QsoId, QsoScore, Registry, ScoreSheet, Scoring, Station,
};
use zlog_undo::{Refusal, Shared, Undo, UndoError, MAX_STEPS};

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
            qsos: (0..log.qsos.len())
                .map(|i| QsoScore {
                    points: (i % 3) as u8,
                    ..QsoScore::default()
                })
                .collect(),
            sheet: ScoreSheet::default(),
        }
    }
}

#[derive(Clone, Default)]
struct Remote(Rc<RefCell<Vec<Command>>>);

impl Observer for Remote {
    fn name(&self) -> &str {
        "remote"
    }
    fn on_event(&mut self, _cx: &mut Context<'_>, _event: &LogEvent) {}
}

impl Link for Remote {
    fn poll(&mut self, cx: &mut Context<'_>) {
        for c in self.0.borrow_mut().drain(..) {
            cx.send(c);
        }
    }
}

struct Filler;

impl Observer for Filler {
    fn name(&self) -> &str {
        "filler"
    }
    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if let LogEvent::Inserted { index } = event {
            let q = &cx.log().qsos[*index];
            if q.entity.is_empty() {
                let mut q = q.clone();
                q.entity = "JA".into();
                cx.send(Command::Update(QsoId::of(&q), q));
            }
        }
    }
}

struct Package(Remote, bool);

impl Extension for Package {
    fn name(&self) -> &str {
        "test"
    }
    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        if self.1 {
            vec![Box::new(Filler)]
        } else {
            Vec::new()
        }
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(self.0.clone())]
    }
}

struct Undoable {
    book: Book,
    history: Shared,
}

impl Undoable {
    fn undo(&mut self) -> Result<Vec<String>, UndoError> {
        zlog_undo::undo(&self.history, &mut self.book, false)
    }

    fn redo(&mut self) -> Result<Vec<String>, UndoError> {
        zlog_undo::undo(&self.history, &mut self.book, true)
    }

    fn steps(&self) -> (usize, usize) {
        self.history.lock().unwrap().steps()
    }

    fn skip(&mut self, redo: bool) -> bool {
        self.history.lock().unwrap().skip(redo)
    }

    fn execute(&mut self, command: Command) -> Result<Vec<String>, zlog_core::BookError> {
        self.book.execute(command)
    }

    fn poll(&mut self) -> Result<Vec<String>, zlog_core::BookError> {
        self.book.poll()
    }

    fn begin_step(&mut self) {
        self.book.begin_step()
    }

    fn end_step(&mut self) {
        self.book.end_step()
    }

    fn close(&mut self) -> Vec<String> {
        self.book.close()
    }
}

impl std::ops::Deref for Undoable {
    type Target = Book;
    fn deref(&self) -> &Book {
        &self.book
    }
}

fn book_with(filler: bool) -> (Undoable, Remote) {
    let remote = Remote::default();
    let history = Shared::default();
    let registry = Registry::new()
        .with(Package(remote.clone(), filler))
        .with(Undo(history.clone()));
    let (book, _) = Book::open(Log::default(), Box::new(Nothing), "nothing", registry);
    (Undoable { book, history }, remote)
}

fn book() -> (Undoable, Remote) {
    book_with(false)
}

fn qso(call: &str) -> Qso {
    Qso {
        callsign: call.into(),
        ..Qso::default()
    }
}

fn calls(book: &Book) -> Vec<&str> {
    book.log()
        .qsos
        .iter()
        .map(|q| q.callsign.as_str())
        .collect()
}

fn get(book: &Book, id: QsoId) -> Qso {
    book.log().qsos[book.find(id).unwrap()].clone()
}

fn edited(book: &Book, id: QsoId, memo: &str) -> Qso {
    let mut q = get(book, id);
    q.memo = memo.into();
    q
}

#[test]
fn insert_is_undone_and_redone() {
    let (mut book, _) = book();
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    let id = QsoId::of(&book.log().qsos[0]);
    assert_eq!(book.steps(), (1, 0));
    book.undo().unwrap();
    assert!(book.log().qsos.is_empty());
    assert_eq!(book.steps(), (0, 1));
    book.redo().unwrap();
    let q = get(&book, id);
    assert_eq!(q.callsign, "JA1AAA");
    assert_eq!(q.reserve3 % 100, 1);
    assert_eq!(book.steps(), (1, 0));
}

#[test]
fn undone_update_ends_with_a_higher_edit_counter() {
    let (mut book, _) = book();
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    let id = QsoId::of(&book.log().qsos[0]);
    book.execute(Command::Update(id, edited(&book, id, "new")))
        .unwrap();
    assert_eq!(get(&book, id).reserve3 % 100, 1);
    book.undo().unwrap();
    let q = get(&book, id);
    assert_eq!(q.memo, "");
    assert_eq!(q.reserve3 % 100, 2);
    book.redo().unwrap();
    let q = get(&book, id);
    assert_eq!(q.memo, "new");
    assert_eq!(q.reserve3 % 100, 3);
}

#[test]
fn undone_delete_puts_the_qso_back_in_place_with_its_id() {
    let (mut book, _) = book();
    for c in ["A", "B", "C"] {
        book.execute(Command::Insert(qso(c))).unwrap();
    }
    let id = QsoId::of(&book.log().qsos[1]);
    let r3 = get(&book, id).reserve3;
    book.execute(Command::Delete(id)).unwrap();
    book.undo().unwrap();
    assert_eq!(calls(&book), ["A", "B", "C"]);
    assert_eq!(get(&book, id).reserve3, r3 + 1);
    book.redo().unwrap();
    assert_eq!(calls(&book), ["A", "C"]);
}

#[test]
fn an_edit_of_many_is_one_step() {
    let (mut book, _) = book();
    for i in 0..50 {
        book.execute(Command::Insert(qso(&format!("JA{i}"))))
            .unwrap();
    }
    let ids: Vec<QsoId> = book.log().qsos.iter().map(QsoId::of).collect();
    book.begin_step();
    for &id in &ids {
        book.execute(Command::Update(id, edited(&book, id, "batch")))
            .unwrap();
    }
    book.end_step();
    assert_eq!(book.steps(), (51, 0));
    book.undo().unwrap();
    assert!(book.log().qsos.iter().all(|q| q.memo.is_empty()));
    book.redo().unwrap();
    assert!(book.log().qsos.iter().all(|q| q.memo == "batch"));
}

#[test]
fn a_step_touched_by_another_station_is_refused_until_skipped() {
    let (mut book, remote) = book();
    book.execute(Command::Insert(qso("OLD"))).unwrap();
    let old = QsoId::of(&book.log().qsos[0]);
    book.execute(Command::Update(old, edited(&book, old, "mine")))
        .unwrap();
    book.execute(Command::Insert(qso("NEW"))).unwrap();
    let new = QsoId::of(&book.log().qsos[1]);
    let mut theirs = edited(&book, new, "theirs");
    theirs.reserve3 += 1;
    remote.0.borrow_mut().push(Command::Update(new, theirs));
    book.poll().unwrap();
    assert_eq!(book.steps(), (3, 0), "received edits are no steps");

    let log = book.log().qsos.clone();
    for _ in 0..2 {
        assert_eq!(
            book.undo(),
            Err(UndoError::Refused(Refusal::Changed(Mismatch::Edited(new))))
        );
        assert_eq!(book.log().qsos, log, "refused: nothing changed");
        assert_eq!(book.steps(), (3, 0), "the step stays");
    }
    assert!(book.skip(false));
    assert_eq!(book.log().qsos, log);
    book.undo().unwrap();
    assert_eq!(get(&book, old).memo, "");
    assert_eq!(get(&book, new).memo, "theirs");
    book.undo().unwrap();
    assert_eq!(calls(&book), ["NEW"]);
    assert_eq!(book.undo(), Err(UndoError::Refused(Refusal::Empty)));
}

#[test]
fn a_step_whose_qso_was_deleted_or_taken_is_refused() {
    let (mut book, remote) = book();
    book.execute(Command::Insert(qso("A"))).unwrap();
    let a = get(&book, QsoId::of(&book.log().qsos[0]));
    let id = QsoId::of(&a);
    book.execute(Command::Update(id, edited(&book, id, "x")))
        .unwrap();
    remote.0.borrow_mut().push(Command::Delete(id));
    book.poll().unwrap();
    assert_eq!(
        book.undo(),
        Err(UndoError::Refused(Refusal::Changed(Mismatch::Deleted(id))))
    );

    let (mut book, remote) = self::book();
    book.execute(Command::Insert(qso("A"))).unwrap();
    let a = book.log().qsos[0].clone();
    let id = QsoId::of(&a);
    book.execute(Command::Delete(id)).unwrap();
    remote.0.borrow_mut().push(Command::Insert(a));
    book.poll().unwrap();
    let log = book.log().qsos.clone();
    assert_eq!(
        book.undo(),
        Err(UndoError::Refused(Refusal::Changed(Mismatch::Taken(id))))
    );
    assert_eq!(book.log().qsos, log);
}

#[test]
fn scores_changed_by_later_qsos_do_not_refuse() {
    let (mut book, remote) = book();
    book.execute(Command::Insert(qso("A"))).unwrap();
    book.execute(Command::Insert(qso("B"))).unwrap();
    let b = QsoId::of(&book.log().qsos[1]);
    book.execute(Command::Update(b, edited(&book, b, "x")))
        .unwrap();
    let points = get(&book, b).points;
    remote.0.borrow_mut().push(Command::InsertBefore(
        QsoId::of(&book.log().qsos[0]),
        Qso {
            reserve3: 900_000_000,
            ..qso("C")
        },
    ));
    book.poll().unwrap();
    assert_ne!(get(&book, b).points, points);
    book.undo().unwrap();
    assert_eq!(get(&book, b).memo, "");
}

#[test]
fn steps_survive_a_sort() {
    let (mut book, _) = book();
    for (c, t) in [("LATE", 2.0), ("EARLY", 1.0)] {
        let mut q = qso(c);
        q.time.0 = t;
        book.execute(Command::Insert(q)).unwrap();
    }
    let late = QsoId::of(&book.log().qsos[0]);
    book.execute(Command::Update(late, edited(&book, late, "x")))
        .unwrap();
    book.execute(Command::SortByTime).unwrap();
    assert_eq!(book.steps(), (3, 0), "a sort is no step");
    book.undo().unwrap();
    assert_eq!(get(&book, late).memo, "");
    book.undo().unwrap();
    assert_eq!(calls(&book), ["LATE"]);
}

#[test]
fn reactions_to_the_operator_belong_to_the_step() {
    let (mut book, remote) = book_with(true);
    book.execute(Command::Insert(qso("A"))).unwrap();
    assert_eq!(book.log().qsos[0].entity, "JA");
    assert_eq!(book.steps(), (1, 0));
    book.undo().unwrap();
    assert!(book.log().qsos.is_empty());
    remote.0.borrow_mut().push(Command::Insert(Qso {
        reserve3: 900_000_000,
        ..qso("R")
    }));
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0].entity, "JA");
    assert_eq!(book.steps(), (0, 1), "the redo step stays");
}

#[test]
fn a_new_edit_clears_redo() {
    let (mut book, _) = book();
    book.execute(Command::Insert(qso("A"))).unwrap();
    book.undo().unwrap();
    assert_eq!(book.steps(), (0, 1));
    book.execute(Command::Insert(qso("B"))).unwrap();
    assert_eq!(book.steps(), (1, 0));
    assert_eq!(book.redo(), Err(UndoError::Refused(Refusal::Empty)));
}

#[test]
fn history_is_bounded_and_cleared_on_close() {
    let (mut book, _) = book();
    for i in 0..MAX_STEPS + 5 {
        book.execute(Command::Insert(qso(&format!("JA{i}"))))
            .unwrap();
    }
    assert_eq!(book.steps(), (MAX_STEPS, 0));
    book.close();
    assert_eq!(book.steps(), (0, 0));
}
