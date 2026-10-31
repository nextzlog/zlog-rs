use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use zlog_core::{
    Band, Batch, Book, Command, Contest, ContestMode, Context, Extension, Link, Log, LogEvent,
    Observer, Qso, QsoId, QsoScore, Registry, ScoreSheet, Scoring, Station,
};
use zlog_webhook::config::Target;
use zlog_webhook::poster::Poster;
use zlog_webhook::{Config, Hub, Site};

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

struct Package(Arc<Hub>, Remote);

impl Extension for Package {
    fn name(&self) -> &str {
        "test"
    }
    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        vec![Box::new(Poster::new(self.0.clone()))]
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(self.1.clone())]
    }
}

fn qso(call: &str) -> Qso {
    Qso {
        callsign: call.into(),
        ..Qso::default()
    }
}

fn book() -> (Book, Arc<Hub>, Remote) {
    let hub = Arc::new(Hub::default());
    let on = Target {
        enabled: true,
        ..Target::default()
    };
    hub.state.lock().unwrap().config = Config {
        targets: [on, Target::default()],
        edits: true,
    };
    let remote = Remote::default();
    let registry = Registry::new().with(Package(hub.clone(), remote.clone()));
    let log = Log {
        qsos: vec![qso("JA1OPN")],
        ..Log::default()
    };
    let (book, _) = Book::open(log, Box::new(Nothing), "nothing", registry);
    (book, hub, remote)
}

fn posted(hub: &Hub) -> Vec<String> {
    posted_to(hub, Site::Discord)
}

fn posted_to(hub: &Hub, site: Site) -> Vec<String> {
    let mut s = hub.state.lock().unwrap();
    let mut lines = Vec::new();
    while let Some(batch) = s.outbox_mut(site).take(std::time::Instant::now()) {
        lines.extend(batch);
    }
    lines
}

#[test]
fn only_the_operators_own_qsos_are_posted() {
    let (mut book, hub, remote) = book();
    assert!(posted(&hub).is_empty());
    book.execute(Command::Insert(qso("JA1ZLO"))).unwrap();
    let lines = posted(&hub);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("JA1ZLO"));
    remote.0.borrow_mut().push(Command::Insert(qso("JA2LNK")));
    book.poll().unwrap();
    let batch = Batch {
        commands: vec![Command::Insert(qso("JA3EXT"))],
        tag: 7,
        ..Batch::default()
    };
    hub.state.lock().unwrap().config.edits = false;
    book.execute_if(batch).unwrap();
    assert!(posted(&hub).is_empty());
    hub.state.lock().unwrap().config.edits = true;
    let id = QsoId::of(&book.log().qsos[1]);
    let mut q = book.log().qsos[1].clone();
    q.callsign = "JA1ZLP".into();
    book.execute(Command::Update(id, q)).unwrap();
    book.execute(Command::Delete(id)).unwrap();
    assert_eq!(posted(&hub).len(), 2);
}

#[test]
fn undo_posts_a_restored_qso_as_it_posts_a_removed_one() {
    let (mut book, hub, _) = book();
    let id = QsoId::of(&book.log().qsos[0]);
    let gone = book.log().qsos[0].clone();
    book.execute(Command::Delete(id)).unwrap();
    assert_eq!(posted(&hub).len(), 1);
    let restore = Batch {
        commands: vec![Command::Insert(gone.clone())],
        tag: 7,
        ..Batch::default()
    };
    book.execute_if(restore).unwrap();
    let lines = posted(&hub);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("JA1OPN"));
    let undo_log = Batch {
        commands: vec![Command::Delete(QsoId::of(&book.log().qsos[0]))],
        tag: 8,
        ..Batch::default()
    };
    book.execute_if(undo_log).unwrap();
    assert_eq!(posted(&hub).len(), 1);
}

#[test]
fn nothing_of_an_import_is_posted() {
    let (mut book, hub, _) = book();
    book.begin_tagged_step(zlog_core::IMPORT);
    for n in 0..120 {
        book.execute(Command::Insert(qso(&format!("JA1A{n:03}"))))
            .unwrap();
    }
    book.end_step();
    assert_eq!(book.log().qsos.len(), 121);
    assert!(posted(&hub).is_empty());
}

#[test]
fn each_destination_switched_on_gets_the_line() {
    let (mut book, hub, _) = book();
    book.execute(Command::Insert(qso("JA1ZLO"))).unwrap();
    assert_eq!(posted(&hub).len(), 1);
    assert!(posted_to(&hub, Site::Slack).is_empty());
    hub.state.lock().unwrap().config.targets[Site::Slack.index()].enabled = true;
    book.execute(Command::Insert(qso("JA1<B>"))).unwrap();
    assert_eq!(posted(&hub), posted_to(&hub, Site::Slack));
    hub.state.lock().unwrap().config.targets[Site::Discord.index()].enabled = false;
    book.execute(Command::Insert(qso("JA1CCC"))).unwrap();
    assert!(posted(&hub).is_empty());
    assert_eq!(posted_to(&hub, Site::Slack).len(), 1);
}
