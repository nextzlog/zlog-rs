use super::*;
use crate::host::fake::FakeSpawner;
use crate::host::State;
use crate::protocol::ToHost;
use std::time::{Duration, Instant};
use zlog_core::{Book, ContestDef, ContestMode, Extension, Log, Registry};

struct Ext(Arc<Mutex<Manager>>);

impl Extension for Ext {
    fn name(&self) -> &str {
        "zylo"
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(ZyloLink::new(self.0.clone()))]
    }
}

fn name() -> String {
    format!("p.{}", crate::library::EXTENSION)
}

type Seen = Arc<Mutex<Vec<ToHelper>>>;

fn book(spawner: FakeSpawner) -> (Book, Arc<Mutex<Manager>>, Seen) {
    book_with(spawner, Nothing)
}

struct Nothing;

impl Extension for Nothing {
    fn name(&self) -> &str {
        "nothing"
    }
}

fn book_with(spawner: FakeSpawner, other: impl Extension) -> (Book, Arc<Mutex<Manager>>, Seen) {
    let seen = spawner.seen.clone();
    let mut m = Manager::new(Arc::new(spawner));
    m.enabled.insert(name());
    let m = Arc::new(Mutex::new(m));
    let contest = zlog_cfg::from_cfg("#TEST\n", |_| None).build(ContestMode::Mix);
    let registry = Registry::new().with(Ext(m.clone())).with(other);
    let (book, _) = Book::open(Log::default(), contest, "test", registry);
    (book, m, seen)
}

fn qso(call: &str) -> Qso {
    Qso {
        callsign: call.into(),
        ..Qso::default()
    }
}

fn poll_until(book: &mut Book, mut done: impl FnMut(&Book) -> bool) -> Vec<String> {
    let end = Instant::now() + Duration::from_secs(5);
    let mut said = Vec::new();
    while !done(book) {
        assert!(Instant::now() < end, "timed out");
        said.extend(book.poll().unwrap_or_default());
        std::thread::sleep(Duration::from_millis(5));
    }
    said
}

fn poll_a_while(book: &mut Book) -> Vec<String> {
    let mut said = Vec::new();
    for _ in 0..20 {
        said.extend(book.poll().unwrap_or_default());
        std::thread::sleep(Duration::from_millis(5));
    }
    said
}

fn inserts(seen: &Seen) -> Vec<u8> {
    seen.lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            ToHelper::Insert { depth, .. } => Some(*depth),
            _ => None,
        })
        .collect()
}

#[test]
fn opening_attaches_and_changes_reach_the_plugin() {
    let (mut book, _, seen) = book(FakeSpawner::new(|_| vec![]));
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    let id = QsoId::of(&book.log().qsos[0]);
    let mut edited = book.log().qsos[0].clone();
    edited.callsign = "JA1BBB".into();
    book.execute(Command::Update(id, edited)).unwrap();
    book.execute(Command::Delete(id)).unwrap();
    book.execute(Command::SortByTime).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while seen.lock().unwrap().len() < 8 {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(2));
    }
    std::thread::sleep(Duration::from_millis(20));
    let kinds: Vec<&str> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|m| match m {
            ToHelper::Hello { .. } => "hello",
            ToHelper::Launch { .. } => "launch",
            ToHelper::Offset { .. } => "offset",
            ToHelper::Attach { .. } => "attach",
            ToHelper::Insert { .. } => "insert",
            ToHelper::Delete { .. } => "delete",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        ["hello", "launch", "offset", "attach", "insert", "delete", "insert", "delete"]
    );
}

#[test]
fn undo_of_an_edit_reaches_the_plugin_as_delete_then_insert() {
    let history = zlog_undo::Shared::default();
    let (mut book, _, seen) = book_with(
        FakeSpawner::new(|_| vec![]),
        zlog_undo::Undo(history.clone()),
    );
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    let id = QsoId::of(&book.log().qsos[0]);
    let mut edited = book.log().qsos[0].clone();
    edited.callsign = "JA1BBB".into();
    book.execute(Command::Update(id, edited)).unwrap();
    zlog_undo::undo(&history, &mut book, false).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    let changes = |seen: &Seen| {
        seen.lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                ToHelper::Insert { record, .. } => Some((true, crate::record::from_record(record))),
                ToHelper::Delete { record, .. } => {
                    Some((false, crate::record::from_record(record)))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    while changes(&seen).len() < 5 {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(2));
    }
    let undo = &changes(&seen)[3..];
    assert_eq!((undo[0].0, undo[0].1.callsign.as_str()), (false, "JA1BBB"));
    assert_eq!((undo[1].0, undo[1].1.callsign.as_str()), (true, "JA1AAA"));
    assert_eq!(undo[1].1.reserve3 % 100, 2);
}

#[test]
fn insert_in_insert_stays_bounded() {
    let (mut book, _, seen) = book(FakeSpawner::new(|m| match m {
        ToHelper::Insert { depth, record } => vec![ToHost::Insert {
            depth: depth + 1,
            record: *record,
        }],
        _ => vec![],
    }));
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    let mut said = poll_until(&mut book, |b| b.log().qsos.len() == 1 + MAX_DEPTH as usize);
    said.extend(poll_a_while(&mut book));
    assert_eq!(book.log().qsos.len(), 1 + MAX_DEPTH as usize);
    assert_eq!(inserts(&seen), (0..MAX_DEPTH).collect::<Vec<_>>());
    assert_eq!(said.len(), 1, "{said:?}");
}

#[test]
fn commits_edit_fields_by_id_and_are_not_told_to_plugins() {
    let (mut book, _, seen) = book(FakeSpawner::new(|m| match m {
        ToHelper::Insert { depth: 0, record } => {
            let mut q = crate::record::from_record(record);
            q.memo = "from plugin".into();
            q.callsign = "IGNORED".into();
            vec![ToHost::Commit {
                depth: 1,
                record: to_record(&q),
                id: QsoId::of(&q).0,
                mask: crate::record::commit::NOTE,
            }]
        }
        _ => vec![],
    }));
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    poll_until(&mut book, |b| b.log().qsos[0].memo == "from plugin");
    assert_eq!(book.log().qsos[0].callsign, "JA1AAA");
    poll_a_while(&mut book);
    assert_eq!(inserts(&seen), vec![0]);
}

#[test]
fn a_crashing_plugin_is_disabled_with_a_warning() {
    let (mut book, m, _) = book(FakeSpawner::new(|m| match m {
        ToHelper::Insert { .. } => panic!("plugin crashed"),
        _ => vec![],
    }));
    book.execute(Command::Insert(qso("JA1AAA"))).unwrap();
    let mut said = poll_until(&mut book, |_| {
        matches!(m.lock().unwrap().state(&name()), Some(State::Disabled(_)))
    });
    said.extend(poll_a_while(&mut book));
    assert!(said.iter().any(|s| s.contains(&name())), "{said:?}");
    book.execute(Command::Insert(qso("JA1BBB"))).unwrap();
    assert_eq!(book.log().qsos.len(), 2);
}

#[test]
fn toasts_reach_the_status_bar() {
    let (mut book, _, _) = book(FakeSpawner::new(|m| match m {
        ToHelper::Attach { .. } => vec![ToHost::Notify {
            text: "hello".into(),
        }],
        _ => vec![],
    }));
    let mut said = Vec::new();
    let end = Instant::now() + Duration::from_secs(5);
    while !said.iter().any(|s: &String| s.ends_with("hello")) {
        assert!(Instant::now() < end);
        said.extend(book.poll().unwrap());
        std::thread::sleep(Duration::from_millis(5));
    }
}
