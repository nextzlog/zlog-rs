//! Behaviour of the book with extensions that write back to the log.

use std::cell::RefCell;
use std::rc::Rc;

use zlog_core::{
    Band, Book, BookError, Command, Contest, ContestMode, Context, Extension, Link, Log, LogEvent,
    Observer, Qso, QsoId, QsoScore, Registry, ScoreSheet, Scoring, Station,
};

/// The smallest contest there is: no scoring at all. The core ships none of its own.
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

/// A package holding the observers and links of one test.
#[derive(Default)]
struct Package {
    observers: Vec<Box<dyn Observer>>,
    links: Vec<Box<dyn Link>>,
}

impl Extension for Package {
    fn name(&self) -> &str {
        "test"
    }
    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        std::mem::take(&mut self.observers)
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        std::mem::take(&mut self.links)
    }
}

fn observing(o: impl Observer + 'static) -> Registry {
    Registry::new().with(Package {
        observers: vec![Box::new(o)],
        ..Package::default()
    })
}

fn linking(l: impl Link + 'static) -> Registry {
    Registry::new().with(Package {
        links: vec![Box::new(l)],
        ..Package::default()
    })
}

fn qso(call: &str) -> Qso {
    Qso {
        callsign: call.into(),
        ..Qso::default()
    }
}

/// A zLink-like extension: QSOs from the network come in through `poll`,
/// QSOs typed here go out, and its own insertions are not echoed back.
struct Mirror {
    inbox: Rc<RefCell<Vec<Qso>>>,
    sent: Rc<RefCell<Vec<String>>>,
}

impl Observer for Mirror {
    fn name(&self) -> &str {
        "link"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if cx.caused_by_me() {
            return;
        }
        if let LogEvent::Inserted { index } = event {
            self.sent
                .borrow_mut()
                .push(cx.log().qsos[*index].callsign.clone());
        }
    }
}

impl Link for Mirror {
    fn poll(&mut self, cx: &mut Context<'_>) {
        for q in self.inbox.borrow_mut().drain(..) {
            cx.send(Command::Insert(q));
        }
    }
}

#[test]
fn link_receives_and_sends_without_echo() {
    let inbox = Rc::new(RefCell::new(Vec::new()));
    let sent = Rc::new(RefCell::new(Vec::new()));
    let link = Mirror {
        inbox: inbox.clone(),
        sent: sent.clone(),
    };
    let (mut book, _) = Book::open(Log::default(), Box::new(Nothing), "open", linking(link));

    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    inbox.borrow_mut().push(qso("JA2BBB"));
    book.poll().unwrap();

    let calls: Vec<_> = book
        .log()
        .qsos
        .iter()
        .map(|q| q.callsign.as_str())
        .collect();
    assert_eq!(calls, vec!["JA1AAA", "JA2BBB"]);
    assert_eq!(*sent.borrow(), vec!["JA1AAA"]);
}

/// An extension that inserts a QSO for every QSO inserted: the chain never ends.
struct Runaway;

impl Observer for Runaway {
    fn name(&self) -> &str {
        "runaway"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if let LogEvent::Inserted { .. } = event {
            cx.send(Command::Insert(qso("JA9ZZZ")));
        }
    }
}

#[test]
fn runaway_chain_is_stopped_and_named() {
    let (mut book, _) = Book::open(
        Log::default(),
        Box::new(Nothing),
        "open",
        observing(Runaway),
    );
    let err = book.execute(Command::Insert(qso("JA1AAA"))).unwrap_err();
    assert_eq!(err, BookError::Cascade("runaway".into()));
}

#[test]
fn update_and_delete_by_id() {
    let (mut book, _) = Book::open(Log::default(), Box::new(Nothing), "open", Registry::new());
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    book.execute(Command::Insert(qso("JA1BBB"))).unwrap();
    let id = QsoId::of(&book.log().qsos[0]);
    assert_ne!(id, QsoId::of(&book.log().qsos[1]));

    book.execute(Command::Update(id, qso("JA1AAB"))).unwrap();
    assert_eq!(book.log().qsos[0].callsign, "JA1AAB");
    assert_eq!(QsoId::of(&book.log().qsos[0]), id);
    assert_eq!(book.log().qsos[0].reserve3 % 100, 1);

    book.execute(Command::Delete(id)).unwrap();
    assert_eq!(book.log().qsos.len(), 1);
    assert_eq!(
        book.execute(Command::Delete(id)),
        Err(BookError::NotFound(id))
    );
}

/// Records what it saw when the contest was opened.
struct Census(Rc<RefCell<Vec<String>>>);

impl Observer for Census {
    fn name(&self) -> &str {
        "census"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if let LogEvent::Opened { .. } = event {
            let calls = cx.log().qsos.iter().map(|q| q.callsign.clone());
            self.0.borrow_mut().extend(calls);
        }
    }
}

#[test]
fn qsos_already_in_the_file_are_seen_on_open() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = Log {
        qsos: vec![qso("JA1AAA"), qso("JA2BBB")],
        ..Log::default()
    };
    Book::open(
        log,
        Box::new(Nothing),
        "nothing",
        observing(Census(seen.clone())),
    );
    assert_eq!(*seen.borrow(), vec!["JA1AAA", "JA2BBB"]);
}

/// A broken contest that forgets the last QSO.
struct ShortCount;

impl Contest for ShortCount {
    fn name(&self) -> &str {
        "short"
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
        let n = log.qsos.len().saturating_sub(1);
        Scoring {
            qsos: vec![
                QsoScore {
                    points: 9,
                    ..QsoScore::default()
                };
                n
            ],
            sheet: ScoreSheet::default(),
        }
    }
}

#[test]
fn a_wrong_scoring_result_is_rejected() {
    let (mut book, _) = Book::open(
        Log::default(),
        Box::new(ShortCount),
        "short",
        Registry::new(),
    );
    let err = book.execute(Command::Insert(qso("JA1AAA"))).unwrap_err();
    assert!(matches!(err, BookError::Scoring(_)));
    assert_eq!(book.log().qsos[0].points, 0, "the log keeps its old scores");
}

#[test]
fn scores_stored_in_the_file_are_recomputed_on_open() {
    let stale = Qso {
        callsign: "JA1AAA".into(),
        points: 99,
        dupe: true,
        new_multi1: true,
        ..Qso::default()
    };
    let (book, _) = Book::open(
        Log {
            qsos: vec![stale],
            ..Log::default()
        },
        Box::new(Nothing),
        "nothing",
        Registry::new(),
    );
    let q = &book.log().qsos[0];
    assert_eq!((q.points, q.dupe, q.new_multi1), (0, false, false));
}

#[test]
fn qsos_without_ids_get_them_on_open() {
    let log = Log {
        qsos: vec![qso("JA1AAA"), qso("JA1BBB"), qso("JA1CCC")],
        ..Log::default()
    };
    let (mut book, _) = Book::open(log, Box::new(Nothing), "nothing", Registry::new());
    let ids: Vec<QsoId> = book.log().qsos.iter().map(QsoId::of).collect();
    assert!(ids.iter().all(|id| id.0 != 0));
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    book.execute(Command::Delete(ids[1])).unwrap();
    let calls: Vec<_> = book
        .log()
        .qsos
        .iter()
        .map(|q| q.callsign.as_str())
        .collect();
    assert_eq!(calls, vec!["JA1AAA", "JA1CCC"]);
}

#[test]
fn the_edit_counter_stops_at_99() {
    let mut q = qso("JA1AAA");
    q.reserve3 = 10_000 * 7 + 99;
    let (mut book, _) = Book::open(
        Log {
            qsos: vec![q],
            ..Log::default()
        },
        Box::new(Nothing),
        "nothing",
        Registry::new(),
    );
    let id = QsoId::of(&book.log().qsos[0]);
    book.execute(Command::Update(id, qso("JA1AAB"))).unwrap();
    assert_eq!(book.log().qsos[0].reserve3 % 100, 99);
}

#[test]
fn a_qso_saved_twice_is_read_once() {
    let with_id = |call: &str, id: i32| Qso {
        reserve3: id,
        ..qso(call)
    };
    let log = Log {
        qsos: vec![
            with_id("JA1AAA", 100_230_000),
            with_id("JA1AAA", 100_230_001),
            with_id("JA1CCC", 100_230_000),
        ],
        ..Log::default()
    };
    let (book, messages) = Book::open(log, Box::new(Nothing), "nothing", Registry::new());
    let calls: Vec<_> = book
        .log()
        .qsos
        .iter()
        .map(|q| q.callsign.as_str())
        .collect();
    assert_eq!(calls, vec!["JA1AAA", "JA1CCC"]);
    assert_ne!(
        QsoId::of(&book.log().qsos[0]),
        QsoId::of(&book.log().qsos[1])
    );
    assert_eq!(messages.len(), 1);
}
