use std::cell::RefCell;
use std::rc::Rc;

use zlog_core::{
    Band, Book, BookError, Command, Contest, ContestMode, Context, Extension, Link, Log, LogEvent,
    Observer, Qso, QsoId, QsoScore, Registry, ScoreSheet, Scoring, Station,
};

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
    assert_eq!(book.log().qsos[0].reserve3, 100_230_000);
    assert_ne!(QsoId::of(&book.log().qsos[1]), QsoId(1_002_300));
    assert_eq!(messages.len(), 1);
}

#[test]
fn same_id_other_call_gets_a_fresh_id_and_zero_ids_get_fresh_ones() {
    let with_id = |call: &str, id: i32| Qso {
        reserve3: id,
        ..qso(call)
    };
    let log = Log {
        qsos: vec![
            with_id("JA1AAA", 100_230_005),
            with_id("JA1BBB", 100_230_007),
            with_id("JA1CCC", 0),
            with_id("JA1CCC", 0),
        ],
        ..Log::default()
    };
    let (book, messages) = Book::open(log, Box::new(Nothing), "nothing", Registry::new());
    let q = &book.log().qsos;
    assert_eq!(q.len(), 4);
    assert_eq!(q[0].reserve3, 100_230_005);
    let ids: std::collections::HashSet<QsoId> = q.iter().map(QsoId::of).collect();
    assert_eq!(ids.len(), 4);
    assert!(q.iter().all(|q| q.reserve3 != 0));
    assert!(messages.is_empty());
    assert_eq!(book.find(QsoId(1_002_300)), Some(0));

    let (book, messages) = Book::open(
        Log::default(),
        Box::new(Nothing),
        "nothing",
        Registry::new(),
    );
    assert!(book.log().qsos.is_empty() && messages.is_empty());
}

struct Feed(Rc<RefCell<Vec<Command>>>);

impl Observer for Feed {
    fn name(&self) -> &str {
        "feed"
    }

    fn on_event(&mut self, _cx: &mut Context<'_>, _event: &LogEvent) {}
}

impl Link for Feed {
    fn poll(&mut self, cx: &mut Context<'_>) {
        for c in self.0.borrow_mut().drain(..) {
            cx.send(c);
        }
    }
}

struct Marker;

impl Observer for Marker {
    fn name(&self) -> &str {
        "marker"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if cx.caused_by_me() {
            return;
        }
        if let LogEvent::Inserted { index } = event {
            let q = &cx.log().qsos[*index];
            let marked = Qso {
                memo: "marked".into(),
                ..q.clone()
            };
            cx.send(Command::Update(QsoId::of(q), marked));
        }
    }
}

fn feeding(observer: impl Observer + 'static) -> (Book, Rc<RefCell<Vec<Command>>>) {
    let queue = Rc::new(RefCell::new(Vec::new()));
    let registry = Registry::new().with(Package {
        observers: vec![Box::new(observer)],
        links: vec![Box::new(Feed(queue.clone()))],
    });
    let (book, _) = Book::open(Log::default(), Box::new(Nothing), "open", registry);
    (book, queue)
}

#[test]
fn a_poll_may_send_far_more_commands_than_a_cascade_allows() {
    let (mut book, queue) = feeding(Marker);
    queue
        .borrow_mut()
        .extend((0..2000).map(|i| Command::Insert(qso(&format!("JA{i}")))));
    book.poll().unwrap();
    assert_eq!(book.log().qsos.len(), 2000);
    assert!(book.log().qsos.iter().all(|q| q.memo == "marked"));
}

#[test]
fn reactions_to_a_poll_are_still_bounded() {
    let (mut book, queue) = feeding(Runaway);
    queue.borrow_mut().push(Command::Insert(qso("JA1AAA")));
    let err = book.poll().unwrap_err();
    assert_eq!(err, BookError::Cascade("runaway".into()));
}

#[test]
fn an_update_from_a_link_keeps_the_senders_edit_counter() {
    let (mut book, queue) = feeding(Census(Rc::default()));
    let mut q = qso("JA1AAA");
    q.reserve3 = 100_230_000;
    book.execute(Command::Insert(q.clone())).unwrap();
    let id = QsoId::of(&q);

    q.reserve3 = 100_230_005;
    queue.borrow_mut().push(Command::Update(id, q.clone()));
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0].reserve3, 100_230_005);

    let other = Qso {
        reserve3: 0,
        ..q.clone()
    };
    queue.borrow_mut().push(Command::Update(id, other.clone()));
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0].reserve3, 100_230_006);
    book.execute(Command::Update(id, q)).unwrap();
    assert_eq!(book.log().qsos[0].reserve3, 100_230_007);
}

#[test]
fn insert_before_a_qso_or_at_the_end() {
    let (mut book, _) = Book::open(Log::default(), Box::new(Nothing), "open", Registry::new());
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    book.execute(Command::Insert(qso("JA3CCC"))).unwrap();
    let id = QsoId::of(&book.log().qsos[1]);
    book.execute(Command::InsertBefore(id, qso("JA2BBB")))
        .unwrap();
    book.execute(Command::InsertBefore(QsoId(-1), qso("JA4DDD")))
        .unwrap();
    let calls: Vec<_> = book
        .log()
        .qsos
        .iter()
        .map(|q| q.callsign.as_str())
        .collect();
    assert_eq!(calls, vec!["JA1AAA", "JA2BBB", "JA3CCC", "JA4DDD"]);
}

#[test]
fn an_observer_editing_a_copy_advances_the_counter_of_local_qsos_only() {
    let (mut book, queue) = feeding(Marker);
    let mut q = qso("JA1AAA");
    q.reserve3 = 100_230_004;
    queue.borrow_mut().push(Command::Insert(q.clone()));
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0].memo, "marked");
    assert_eq!(book.log().qsos[0].reserve3, 100_230_004);
    q.callsign = "JA2BBB".into();
    q.reserve3 = 100_240_004;
    book.execute(Command::Insert(q)).unwrap();
    assert_eq!(book.log().qsos[1].memo, "marked");
    assert_eq!(book.log().qsos[1].reserve3, 100_240_005);
}

#[test]
fn an_update_from_a_link_with_the_same_counter_is_kept_exactly() {
    let (mut book, queue) = feeding(Census(Rc::default()));
    let mut q = qso("JA1AAA");
    q.reserve3 = 100_230_004;
    book.execute(Command::Insert(q.clone())).unwrap();
    q.memo = "from the server".into();
    queue
        .borrow_mut()
        .push(Command::Update(QsoId::of(&q), q.clone()));
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0], q);
}

#[test]
fn unknown_ids_from_a_link_are_ignored() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let (mut book, queue) = feeding(Events(events.clone()));
    events.borrow_mut().clear();
    queue.borrow_mut().extend([
        Command::Insert(qso("JA1AAA")),
        Command::Delete(QsoId(42)),
        Command::Update(QsoId(43), qso("JA9ZZZ")),
        Command::Insert(qso("JA2BBB")),
    ]);
    assert_eq!(book.poll(), Ok(Vec::new()));
    let calls: Vec<_> = book.log().qsos.iter().map(|q| q.callsign.clone()).collect();
    assert_eq!(calls, vec!["JA1AAA", "JA2BBB"]);
    let events = events.borrow();
    assert_eq!(events.len(), 2);
    assert!(events
        .iter()
        .all(|e| matches!(e, LogEvent::Inserted { .. })));
}

struct Blind;

impl Observer for Blind {
    fn name(&self) -> &str {
        "blind"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if let LogEvent::Inserted { index } = event {
            if cx.caused_by_me() {
                return;
            }
            let q = cx.log().qsos[*index].clone();
            cx.send(Command::Delete(QsoId(7)));
            cx.send(Command::Update(QsoId(8), q.clone()));
            let marked = Qso {
                memo: "marked".into(),
                ..q.clone()
            };
            cx.send(Command::Update(QsoId::of(&q), marked));
        }
    }
}

#[test]
fn an_ignored_reaction_does_not_drop_the_queued_ones() {
    let (mut book, _) = Book::open(Log::default(), Box::new(Nothing), "open", observing(Blind));
    assert_eq!(book.execute(Command::Insert(qso("JA1AAA"))), Ok(Vec::new()));
    assert_eq!(book.log().qsos.len(), 1);
    assert_eq!(book.log().qsos[0].memo, "marked");
    assert_eq!(
        book.execute(Command::Update(QsoId(9), qso("JA1AAA"))),
        Err(BookError::NotFound(QsoId(9)))
    );
}

struct Events(Rc<RefCell<Vec<LogEvent>>>);

impl Observer for Events {
    fn name(&self) -> &str {
        "events"
    }

    fn on_event(&mut self, _cx: &mut Context<'_>, event: &LogEvent) {
        self.0.borrow_mut().push(event.clone());
    }
}

#[test]
fn sort_by_time_is_stable_and_keeps_ids() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let at = |call: &str, t: f64| Qso {
        time: zlog_core::DelphiTime(t),
        ..qso(call)
    };
    let (mut book, _) = Book::open(
        Log::default(),
        Box::new(Nothing),
        "open",
        observing(Events(events.clone())),
    );
    for q in [at("C", 3.0), at("A1", 1.0), at("B", 2.0), at("A2", 1.0)] {
        book.execute(Command::Insert(q)).unwrap();
    }
    let ids: std::collections::HashSet<QsoId> = book.log().qsos.iter().map(QsoId::of).collect();
    events.borrow_mut().clear();
    book.execute(Command::SortByTime).unwrap();
    let calls: Vec<_> = book
        .log()
        .qsos
        .iter()
        .map(|q| q.callsign.as_str())
        .collect();
    assert_eq!(calls, vec!["A1", "A2", "B", "C"]);
    assert_eq!(*events.borrow(), vec![LogEvent::Sorted]);
    let after: std::collections::HashSet<QsoId> = book.log().qsos.iter().map(QsoId::of).collect();
    assert_eq!(ids, after);
}

#[test]
fn rescore_tells_no_one_and_changes_nothing() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let (mut book, _) = Book::open(
        Log::default(),
        Box::new(Nothing),
        "open",
        observing(Events(events.clone())),
    );
    book.execute(Command::Insert(qso("A"))).unwrap();
    let (log, changes) = (book.log().clone(), book.changes());
    events.borrow_mut().clear();
    book.execute(Command::Rescore).unwrap();
    assert!(events.borrow().is_empty());
    assert_eq!(book.changes(), changes);
    assert_eq!(book.log().qsos, log.qsos);
}

struct EntityFiller;

impl Observer for EntityFiller {
    fn name(&self) -> &str {
        "entity"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if cx.caused_by_me() {
            return;
        }
        let LogEvent::Inserted { index } = event else {
            return;
        };
        let qso = cx.log().qsos[*index].clone();
        if qso.entity.is_empty() {
            let mut updated = qso.clone();
            updated.entity = "XX".into();
            cx.send(Command::Update(QsoId::of(&qso), updated));
        }
        let message = format!("{}: XX", qso.callsign);
        cx.say(message);
    }
}

struct EntityExtension;

impl Extension for EntityExtension {
    fn name(&self) -> &str {
        "entity"
    }

    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        vec![Box::new(EntityFiller)]
    }
}

#[test]
fn entity_is_written_back_once() {
    let (mut book, _) = Book::open(
        Log::default(),
        Box::new(Nothing),
        "open",
        Registry::new().with(EntityExtension),
    );
    let messages = book
        .execute(Command::Insert(Qso {
            callsign: "K1ABC".into(),
            ..Qso::default()
        }))
        .unwrap();
    assert_eq!(book.log().qsos.len(), 1);
    assert_eq!(book.log().qsos[0].entity, "XX");
    assert_eq!(book.log().qsos[0].reserve3 % 100, 1, "edited exactly once");
    assert_eq!(messages, vec!["K1ABC: XX"]);
}

#[test]
fn poll_changes_reports_whether_the_log_changed() {
    let inbox = Rc::new(RefCell::new(Vec::new()));
    let sent = Rc::new(RefCell::new(Vec::new()));
    let link = Mirror {
        inbox: inbox.clone(),
        sent,
    };
    let (mut book, _) = Book::open(Log::default(), Box::new(Nothing), "open", linking(link));
    let idle = book.poll_changes();
    assert_eq!(idle.result, Ok(Vec::new()));
    assert!(!idle.changed);
    inbox.borrow_mut().push(qso("JA2BBB"));
    let before = book.changes();
    let polled = book.poll_changes();
    assert!(polled.changed);
    assert_eq!(book.changes(), before + 1);
    assert!(!book.poll_changes().changed);
}

#[test]
fn an_unmatched_end_step_is_ignored() {
    let (mut book, _) = Book::open(
        Log::default(),
        Box::new(Nothing),
        "nothing",
        Registry::new(),
    );
    book.end_step();
    book.execute(Command::Insert(Qso::default())).unwrap();
    book.begin_step();
    book.end_step();
    assert_eq!(book.log().qsos.len(), 1);
}

#[test]
fn a_fresh_id_is_found_when_every_serial_of_its_salt_is_taken() {
    let log = Log {
        qsos: (0..10000)
            .map(|s| Qso {
                callsign: format!("JA{s}"),
                reserve3: s * 10_000,
                ..Qso::default()
            })
            .collect(),
        ..Log::default()
    };
    let (mut book, _) = Book::open(log, Box::new(Nothing), "nothing", Registry::new());
    book.execute(Command::Insert(Qso::default())).unwrap();
    let ids: std::collections::HashSet<QsoId> = book.log().qsos.iter().map(QsoId::of).collect();
    assert_eq!(ids.len(), 10001);
}

#[test]
fn a_qso_sharing_an_id_is_deleted_by_its_new_id() {
    let with_id = |call: &str, id: i32| Qso {
        reserve3: id,
        ..qso(call)
    };
    let log = Log {
        qsos: vec![with_id("JA1AAA", 10_010_100), with_id("JA2BBB", 10_010_100)],
        ..Log::default()
    };
    let open = |log: Log| Book::open(log, Box::new(Nothing), "nothing", Registry::new()).0;
    let mut book = open(log.clone());
    let second = QsoId::of(&book.log().qsos[1]);
    assert_ne!(second, QsoId::of(&book.log().qsos[0]));
    assert_eq!(QsoId::of(&open(log).log().qsos[1]), second);
    book.execute(Command::Delete(second)).unwrap();
    assert_eq!(book.log().qsos.len(), 1);
    assert_eq!(book.log().qsos[0].callsign, "JA1AAA");
}

#[test]
fn a_fresh_id_never_returns_a_taken_one() {
    let log = Log {
        qsos: (0..1_000_000)
            .map(|n| Qso {
                reserve3: (n % 10000) * 10_000 + (n / 10000) * 100,
                ..Qso::default()
            })
            .collect(),
        ..Log::default()
    };
    let (mut book, _) = Book::open(log, Box::new(Nothing), "nothing", Registry::new());
    let err = book.execute(Command::Insert(qso("JA1AAA"))).unwrap_err();
    assert_eq!(err, BookError::NoFreeId(0));
    assert_eq!(book.log().qsos.len(), 1_000_000);
    let other = Qso {
        tx: 1,
        ..qso("JA1AAA")
    };
    book.execute(Command::Insert(other)).unwrap();
    assert_eq!(book.log().qsos[1_000_000].reserve3 / 100_000_000, 1);
}

struct RejectsBad;

impl Contest for RejectsBad {
    fn name(&self) -> &str {
        "bad"
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
        let bad = log.qsos.iter().any(|q| q.callsign == "BAD");
        Scoring {
            qsos: vec![QsoScore::default(); if bad { 0 } else { log.qsos.len() }],
            sheet: ScoreSheet::default(),
        }
    }
}

struct Chain;

impl Observer for Chain {
    fn name(&self) -> &str {
        "chain"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        match event {
            LogEvent::Inserted { index } if cx.log().qsos[*index].callsign != "BAD" => {
                cx.send(Command::Insert(qso("JA9ZZZ")))
            }
            LogEvent::Resynced => cx.send(Command::Insert(qso("BAD"))),
            _ => {}
        }
    }
}

struct Resyncing(Rc<RefCell<Vec<Command>>>);

impl Observer for Resyncing {
    fn name(&self) -> &str {
        "resync"
    }

    fn on_event(&mut self, _cx: &mut Context<'_>, _event: &LogEvent) {}
}

impl Link for Resyncing {
    fn poll(&mut self, cx: &mut Context<'_>) {
        for c in self.0.borrow_mut().drain(..) {
            cx.send(c);
        }
        cx.resync();
    }
}

#[test]
fn a_poll_reports_its_first_error_when_the_resync_fails_too() {
    let inbox = Rc::new(RefCell::new(vec![Command::Insert(qso("JA1AAA"))]));
    let registry = Registry::new().with(Package {
        observers: vec![Box::new(Chain)],
        links: vec![Box::new(Resyncing(inbox))],
    });
    let (mut book, _) = Book::open(Log::default(), Box::new(RejectsBad), "bad", registry);
    let polled = book.poll_changes();
    assert_eq!(polled.result, Err(BookError::Cascade("chain".into())));
    assert!(polled.changed);
    assert!(book.log().qsos.iter().any(|q| q.callsign == "BAD"));
}

#[test]
fn a_failing_batch_changes_nothing() {
    let (mut book, _) = Book::open(Log::default(), Box::new(Nothing), "x", Registry::new());
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    let id = QsoId::of(&book.log().qsos[0]);
    let before = (book.log().qsos.clone(), book.changes());
    let batch = |commands| zlog_core::Batch {
        expect: Vec::new(),
        commands,
        tag: 7,
    };
    let failing = [
        vec![Command::Insert(qso("JA2BBB")), Command::Delete(QsoId(42))],
        vec![Command::Delete(id), Command::Update(id, qso("JA1AAB"))],
        vec![Command::Delete(id), Command::Delete(id)],
    ];
    for commands in failing {
        let err = book.execute_if(batch(commands)).unwrap_err();
        assert!(matches!(
            err,
            zlog_core::BatchError::Book(BookError::NotFound(_))
        ));
        assert_eq!((book.log().qsos.clone(), book.changes()), before);
    }
    let fine = vec![
        Command::Insert(Qso {
            reserve3: 5_550_000,
            ..qso("JA3CCC")
        }),
        Command::Update(QsoId(55_500), qso("JA3CCD")),
        Command::Delete(id),
    ];
    book.execute_if(batch(fine)).unwrap();
    let calls: Vec<_> = book
        .log()
        .qsos
        .iter()
        .map(|q| q.callsign.as_str())
        .collect();
    assert_eq!(calls, vec!["JA3CCD"]);
}

#[test]
fn a_batch_inside_a_step_is_refused() {
    let (mut book, _) = Book::open(Log::default(), Box::new(Nothing), "x", Registry::new());
    book.begin_step();
    let batch = zlog_core::Batch {
        expect: Vec::new(),
        commands: vec![Command::Insert(qso("JA1AAA"))],
        tag: 7,
    };
    let err = book.execute_if(batch).unwrap_err();
    assert_eq!(err, zlog_core::BatchError::Book(BookError::Nested));
    book.end_step();
    assert!(book.log().qsos.is_empty());
}
