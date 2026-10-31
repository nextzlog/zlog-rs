use std::cell::RefCell;
use std::rc::Rc;

use zlog_core::{
    Band, Batch, BatchError, Book, Command, Contest, ContestMode, Context, Extension, Link, Log,
    LogEvent, Mismatch, Observer, Origin, Qso, QsoId, QsoScore, Registry, ScoreSheet, Scoring,
    Station, Step,
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
struct Seen(Rc<RefCell<(Vec<Step>, Vec<Origin>)>>);

impl Observer for Seen {
    fn name(&self) -> &str {
        "seen"
    }
    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if *event == LogEvent::Resynced {
            self.0.borrow_mut().1.push(cx.origin());
        }
    }
    fn on_step(&mut self, _log: &Log, step: &Step) {
        self.0.borrow_mut().0.push(step.clone());
    }
}

#[derive(Clone, Default)]
struct Remote(Rc<RefCell<(Vec<Command>, bool)>>);

impl Observer for Remote {
    fn name(&self) -> &str {
        "remote"
    }
    fn on_event(&mut self, _cx: &mut Context<'_>, _event: &LogEvent) {}
}

impl Link for Remote {
    fn poll(&mut self, cx: &mut Context<'_>) {
        let mut r = self.0.borrow_mut();
        for c in r.0.drain(..) {
            cx.send(c);
        }
        if std::mem::take(&mut r.1) {
            cx.resync();
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

struct Package(Remote, Seen);

impl Extension for Package {
    fn name(&self) -> &str {
        "test"
    }
    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        vec![Box::new(Filler), Box::new(self.1.clone())]
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(self.0.clone())]
    }
}

fn book() -> (Book, Remote, Seen) {
    let (remote, seen) = (Remote::default(), Seen::default());
    let registry = Registry::new().with(Package(remote.clone(), seen.clone()));
    let (book, _) = Book::open(Log::default(), Box::new(Nothing), "nothing", registry);
    (book, remote, seen)
}

fn qso(call: &str) -> Qso {
    Qso {
        callsign: call.into(),
        ..Qso::default()
    }
}

fn steps(seen: &Seen) -> Vec<Step> {
    seen.0.borrow().0.clone()
}

#[test]
fn each_command_of_the_operator_is_a_step_with_its_reactions() {
    let (mut book, _, seen) = book();
    book.execute(Command::Insert(qso("A"))).unwrap();
    let s = steps(&seen);
    assert_eq!(s.len(), 1);
    let c = &s[0].changes[0];
    assert_eq!((c.before.as_ref(), s[0].tag), (None, None));
    assert_eq!(
        c.after.as_ref().unwrap().entity,
        "JA",
        "the filler's update"
    );
    assert_eq!(c.id, QsoId::of(&book.log().qsos[0]));
}

#[test]
fn nested_steps_are_one_and_a_sort_or_nothing_is_none() {
    let (mut book, _, seen) = book();
    book.begin_step();
    book.begin_step();
    for c in ["A", "B"] {
        book.execute(Command::Insert(qso(c))).unwrap();
    }
    book.end_step();
    assert!(steps(&seen).is_empty(), "the outermost counts");
    book.end_step();
    assert_eq!(steps(&seen).len(), 1);
    assert_eq!(steps(&seen)[0].changes.len(), 2);
    book.execute(Command::SortByTime).unwrap();
    book.begin_step();
    book.end_step();
    assert_eq!(steps(&seen).len(), 1);
}

#[test]
fn changes_received_from_a_link_are_no_steps() {
    let (mut book, remote, seen) = book();
    remote.0.borrow_mut().0.push(Command::Insert(qso("R")));
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0].entity, "JA");
    assert!(steps(&seen).is_empty());
}

#[test]
fn a_batch_runs_as_one_tagged_step_when_the_log_is_as_expected() {
    let (mut book, remote, seen) = book();
    for c in ["A", "B"] {
        book.execute(Command::Insert(qso(c))).unwrap();
    }
    let (a, b) = (book.log().qsos[0].clone(), book.log().qsos[1].clone());
    remote.0.borrow_mut().0.push(Command::InsertBefore(
        QsoId::of(&a),
        Qso {
            reserve3: 900_000_000,
            ..qso("C")
        },
    ));
    book.poll().unwrap();
    assert_ne!(book.log().qsos[1].points, a.points);
    let batch = Batch {
        expect: vec![(QsoId::of(&a), Some(a.clone())), (QsoId(7), None)],
        commands: vec![
            Command::Delete(QsoId::of(&a)),
            Command::Delete(QsoId::of(&b)),
        ],
        tag: 42,
    };
    book.execute_if(batch).unwrap();
    assert_eq!(book.log().qsos.len(), 1);
    let last = steps(&seen).pop().unwrap();
    assert_eq!((last.tag, last.changes.len()), (Some(42), 2));
}

#[test]
fn a_batch_on_a_log_that_differs_changes_nothing() {
    let (mut book, remote, seen) = book();
    book.execute(Command::Insert(qso("A"))).unwrap();
    let a = book.log().qsos[0].clone();
    let id = QsoId::of(&a);
    let delete = |expect| Batch {
        expect,
        commands: vec![Command::Delete(id)],
        tag: 1,
    };
    let mut theirs = Qso {
        memo: "theirs".into(),
        ..a.clone()
    };
    theirs.reserve3 += 1;
    remote.0.borrow_mut().0.push(Command::Update(id, theirs));
    book.poll().unwrap();
    let log = book.log().qsos.clone();
    let refused = |book: &mut Book, expect| match book.execute_if(delete(expect)) {
        Err(BatchError::Refused(m)) => m,
        r => panic!("{r:?}"),
    };
    assert_eq!(
        refused(&mut book, vec![(id, Some(a.clone()))]),
        Mismatch::Edited(id)
    );
    assert_eq!(refused(&mut book, vec![(id, None)]), Mismatch::Taken(id));
    assert_eq!(book.log().qsos, log);
    assert_eq!(steps(&seen).len(), 1, "no step");
    remote.0.borrow_mut().0.push(Command::Delete(id));
    book.poll().unwrap();
    assert_eq!(
        refused(&mut book, vec![(id, Some(a))]),
        Mismatch::Deleted(id)
    );
}

#[test]
fn a_resync_is_told_after_the_commands_of_the_poll() {
    let (mut book, remote, seen) = book();
    book.execute(Command::Insert(qso("A"))).unwrap();
    let id = QsoId::of(&book.log().qsos[0]);
    *remote.0.borrow_mut() = (vec![Command::Delete(id)], true);
    book.poll().unwrap();
    assert!(book.log().qsos.is_empty());
    assert_eq!(seen.0.borrow().1, [Origin::Member(2)]);
    book.poll().unwrap();
    assert_eq!(seen.0.borrow().1.len(), 1, "once");
}
