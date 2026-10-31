use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zlog_core::{
    Band, Book, Command, Contest, ContestMode, Extension, Link, Log, Qso, QsoId, QsoScore,
    Registry, ScoreSheet, Scoring, Station,
};
use zlog_link::net::{Listener, Shared};
use zlog_link::session::{OnConnect, Tcp};
use zlog_link::{LogLink, ServerLink, Session};
use zlog_tls::{Ending, Limits, Peers, ServerConfig};

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

struct Package(Vec<Box<dyn Link>>);

impl Extension for Package {
    fn name(&self) -> &str {
        "test"
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        std::mem::take(&mut self.0)
    }
}

fn qso(call: &str, id: i32) -> Qso {
    Qso {
        callsign: call.into(),
        band: Band::B14,
        reserve3: id,
        ..Qso::default()
    }
}

fn book(qsos: Vec<Qso>, link: Box<dyn Link>) -> Book {
    let log = Log {
        qsos,
        ..Log::default()
    };
    Book::open(
        log,
        Box::new(Nothing),
        "nothing",
        Registry::new().with(Package(vec![link])),
    )
    .0
}

struct Server {
    net: Arc<Shared>,
    book: Book,
    port: u16,
}

fn server(qsos: Vec<Qso>, approve: bool) -> Server {
    server_with(qsos, approve, None)
}

fn server_with(qsos: Vec<Qso>, approve: bool, undo: Option<zlog_undo::Shared>) -> Server {
    let net = Arc::new(Shared::default());
    {
        let mut n = net.lock();
        n.settings.port = "0".into();
        n.settings.bind = "127.0.0.1".into();
        n.settings.approve = approve;
        n.start("nothing").unwrap();
    }
    let port = net.lock().listener.as_ref().unwrap().addr().port();
    let session = Arc::new(Mutex::new(Session::default()));
    let link: Box<dyn Link> = Box::new(ServerLink::new(net.clone(), session));
    let mut registry = Registry::new().with(Package(vec![link]));
    if let Some(h) = undo {
        registry = registry.with(zlog_undo::Undo(h));
    }
    let log = Log {
        qsos,
        ..Log::default()
    };
    let book = Book::open(log, Box::new(Nothing), "nothing", registry).0;
    Server { net, book, port }
}

struct Client {
    session: Arc<Mutex<Session>>,
    book: Book,
}

fn client(port: u16, pc: &str, qsos: Vec<Qso>, on_connect: OnConnect) -> Client {
    let session = Arc::new(Mutex::new(Session::default()));
    {
        let mut s = session.lock().unwrap();
        s.settings.pc_name = pc.into();
        s.settings.on_connect = on_connect;
        s.connect(Box::new(Tcp::open("127.0.0.1", port)));
    }
    let book = book(qsos, Box::new(LogLink::new(session.clone())));
    Client { session, book }
}

fn view(book: &Book) -> BTreeMap<i32, String> {
    book.log()
        .qsos
        .iter()
        .map(|q| (q.reserve3 / 100, q.callsign.clone()))
        .collect()
}

fn settle(
    server: &mut Server,
    clients: &mut [&mut Client],
    done: impl Fn(&Server, &[&mut Client]) -> bool,
) {
    let start = Instant::now();
    let mut calm = 0;
    while calm < 20 {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no convergence: {:?}",
            view(&server.book)
        );
        server.book.poll().unwrap();
        for c in clients.iter_mut() {
            c.book.poll().unwrap();
        }
        calm = if done(server, clients) { calm + 1 } else { 0 };
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn same(server: &Server, clients: &[&mut Client]) -> bool {
    let want = view(&server.book);
    clients
        .iter()
        .all(|c| view(&c.book) == want && c.session.lock().unwrap().connected())
}

#[test]
fn merge_download_and_live_changes_converge() {
    let a = qso("JA1AAA", 100_000_100);
    let mut s = server(vec![a.clone()], false);
    let mut c1 = client(
        s.port,
        "PC1",
        vec![qso("JA2BBB", 200_000_100)],
        OnConnect::Merge,
    );
    let mut c2 = client(s.port, "PC2", Vec::new(), OnConnect::Auto);
    settle(&mut s, &mut [&mut c1, &mut c2], |s, c| {
        same(s, c) && view(&s.book).len() == 2
    });
    assert_eq!(s.net.lock().hub.approved(), 2);
    assert_eq!(s.net.lock().hub.merging(), None, "both merges ended");

    let tx = |call: &str, tx: u8| Qso { tx, ..qso(call, 0) };
    s.book.execute(Command::Insert(tx("JA0SRV", 3))).unwrap();
    c1.book.execute(Command::Insert(tx("JA1ONE", 4))).unwrap();
    let mut edited = c2
        .book
        .log()
        .qsos
        .iter()
        .find(|q| q.callsign == "JA1AAA")
        .unwrap()
        .clone();
    edited.callsign = "JA1AAZ".into();
    c2.book
        .execute(Command::Update(QsoId::of(&a), edited))
        .unwrap();
    settle(&mut s, &mut [&mut c1, &mut c2], |s, c| {
        same(s, c) && view(&s.book).len() == 4
    });
    assert!(view(&s.book).values().any(|c| c == "JA1AAZ"));

    let srv = s
        .book
        .log()
        .qsos
        .iter()
        .find(|q| q.callsign == "JA0SRV")
        .unwrap()
        .clone();
    s.book.execute(Command::Delete(QsoId::of(&srv))).unwrap();
    let two = qso("JA2BBB", 200_000_100);
    c2.book.execute(Command::Delete(QsoId::of(&two))).unwrap();
    settle(&mut s, &mut [&mut c1, &mut c2], |s, c| {
        same(s, c) && view(&s.book).len() == 2
    });
}

#[test]
fn near_simultaneous_edits_end_with_one_version_everywhere() {
    let b = qso("JA2BBB", 200_000_100);
    let mut s = server(vec![b.clone()], false);
    let mut c1 = client(s.port, "PC1", Vec::new(), OnConnect::Download);
    let mut c2 = client(s.port, "PC2", Vec::new(), OnConnect::Download);
    settle(&mut s, &mut [&mut c1, &mut c2], |s, c| {
        same(s, c) && c.iter().all(|c| c.book.log().qsos.len() == 1)
    });
    for (c, call) in [(&mut c1, "JA2ONE"), (&mut c2, "JA2TWO")] {
        let mut q = c.book.log().qsos[0].clone();
        q.callsign = call.into();
        c.book.execute(Command::Update(QsoId::of(&b), q)).unwrap();
    }
    let mut q = s.book.log().qsos[0].clone();
    q.callsign = "JA2SRV".into();
    s.book.execute(Command::Update(QsoId::of(&b), q)).unwrap();
    settle(&mut s, &mut [&mut c1, &mut c2], same);
    let full = |b: &Book| b.log().qsos[0].clone();
    assert_eq!(full(&c1.book), full(&s.book));
    assert_eq!(full(&c2.book), full(&s.book));
}

#[test]
fn a_local_edit_during_a_client_merge_reaches_everybody_after_it() {
    let a = qso("JA1AAA", 100_000_100);
    let mut s = server(vec![a.clone()], false);
    let mut c1 = client(s.port, "PC1", Vec::new(), OnConnect::Download);
    settle(&mut s, &mut [&mut c1], |s, c| {
        same(s, c) && c[0].book.log().qsos.len() == 1
    });
    let mut raw = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    raw.write_all(b"#ZLOG# PCNAME RAW\r\n#ZLOG# BEGINMERGE\r\n\r\n")
        .unwrap();
    let start = Instant::now();
    while s.net.lock().hub.merging().is_none() {
        assert!(start.elapsed() < Duration::from_secs(10));
        s.book.poll().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut q = a.clone();
    q.callsign = "JA1LOC".into();
    s.book.execute(Command::Update(QsoId::of(&a), q)).unwrap();
    for _ in 0..20 {
        s.book.poll().unwrap();
        c1.book.poll().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        c1.book.log().qsos[0].callsign,
        "JA1AAA",
        "held during the merge"
    );
    raw.write_all(b"#ZLOG# ENDMERGE\r\n").unwrap();
    settle(&mut s, &mut [&mut c1], |s, c| {
        same(s, c) && c[0].book.log().qsos[0].callsign == "JA1LOC"
    });
    let mut lines = BufReader::new(raw.try_clone().unwrap());
    raw.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut seen = Vec::new();
    while !seen
        .iter()
        .any(|l: &String| l.starts_with("#ZLOG# EDITQSOTO"))
    {
        let mut l = String::new();
        lines.read_line(&mut l).unwrap();
        seen.push(l);
    }
    assert!(seen.iter().any(|l| l.starts_with("#ZLOG# BEGINMERGE-OK")));
}

fn guarded(limits: Limits, allowed: &str) -> Listener {
    let config = ServerConfig {
        allowed: vec![allowed.parse().unwrap()],
        approve_new: false,
        ending: Ending::CrLf,
        limits,
        ..ServerConfig::new("zlog-link", "127.0.0.1:0".parse().unwrap())
    };
    Listener::start(config, Peers::default(), None).unwrap()
}

fn closed(stream: &mut TcpStream) -> bool {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut buf = [0u8; 64];
    loop {
        match std::io::Read::read(stream, &mut buf) {
            Ok(0) | Err(_) => return true,
            Ok(_) => {}
        }
    }
}

#[test]
fn plaintext_guards_refuse_outsiders_floods_and_long_lines() {
    let out = guarded(Limits::default(), "10.0.0.0/8");
    let mut s = TcpStream::connect(out.addr()).unwrap();
    assert!(closed(&mut s), "outside the allowed range");
    let a = out.audit.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(a.kind, zlog_tls::AuditKind::OutOfRange);

    let limits = Limits {
        lines_per_sec: 50,
        max_line: 300,
        ..Limits::default()
    };
    let mut l = guarded(limits, "127.0.0.0/8");
    let mut flood = TcpStream::connect(l.addr()).unwrap();
    let burst: String = (0..200).map(|_| "#ZLOG# PUTMESSAGE x\r\n").collect();
    let _ = flood.write_all(burst.as_bytes());
    assert!(closed(&mut flood), "flood");
    let mut long = TcpStream::connect(l.addr()).unwrap();
    let _ = long.write_all(format!("#ZLOG# {}\r\n", "x".repeat(400)).as_bytes());
    assert!(closed(&mut long), "too long");
    let start = Instant::now();
    let mut kinds = Vec::new();
    while kinds.len() < 2 && start.elapsed() < Duration::from_secs(5) {
        l.poll();
        kinds.extend(l.audit.try_iter().map(|a| a.kind).filter(|k| {
            matches!(
                k,
                zlog_tls::AuditKind::RateLimited | zlog_tls::AuditKind::LineTooLong
            )
        }));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        kinds.contains(&zlog_tls::AuditKind::RateLimited),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&zlog_tls::AuditKind::LineTooLong),
        "{kinds:?}"
    );
}

#[test]
fn a_new_plaintext_client_gets_nothing_until_approved_by_address_and_name() {
    let mut s = server(vec![qso("JA1AAA", 100_000_100)], true);
    let mut raw = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    raw.write_all(
        b"#ZLOG# BAND 2\r\n#ZLOG# PCNAME PC9\r\n#ZLOG# SENDLOG\r\n#ZLOG# GETFILE zlog.ini\r\n",
    )
    .unwrap();
    let start = Instant::now();
    while s.net.lock().asks.is_empty() {
        assert!(start.elapsed() < Duration::from_secs(10));
        s.book.poll().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let ask = s.net.lock().asks[0].clone();
    assert_eq!(ask.name, "PC9");
    raw.set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let mut buf = [0u8; 256];
    assert!(
        std::io::Read::read(&mut raw, &mut buf).is_err(),
        "nothing before approval"
    );
    s.net.lock().decisions.push((ask.id, true));
    raw.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(raw.try_clone().unwrap());
    let mut got = Vec::new();
    while !got.iter().any(|l: &String| l.starts_with("#ZLOG# RENEW")) {
        s.book.poll().unwrap();
        let mut l = String::new();
        if reader.read_line(&mut l).is_ok() {
            got.push(l);
        }
    }
    assert!(
        got.iter()
            .any(|l| l.starts_with("#ZLOG# PUTLOG ZLOGQSODATA:")),
        "{got:?}"
    );
    assert!(!got.iter().any(|l| l.contains("FILE")), "no file transfer");
    assert!(
        s.net.lock().known("127.0.0.1:1".parse().unwrap(), "PC9"),
        "remembered"
    );
    let notes: Vec<_> = s.net.lock().notes.iter().map(|(_, n)| n.clone()).collect();
    assert!(
        notes.contains(&zlog_link::hub::Note::Refused(
            zlog_link::protocol::Refused::File
        )),
        "{notes:?}"
    );
}

#[test]
fn server_undo_and_redo_reach_the_clients() {
    let a = qso("JA1AAA", 100_000_100);
    let history = zlog_undo::Shared::default();
    let mut s = server_with(vec![a.clone()], false, Some(history.clone()));
    let mut c1 = client(s.port, "PC1", Vec::new(), OnConnect::Download);
    settle(&mut s, &mut [&mut c1], |s, c| {
        same(s, c) && c[0].book.log().qsos.len() == 1
    });
    let mut q = a.clone();
    q.callsign = "JA1BAD".into();
    s.book.execute(Command::Update(QsoId::of(&a), q)).unwrap();
    settle(&mut s, &mut [&mut c1], |s, c| {
        same(s, c) && c[0].book.log().qsos[0].callsign == "JA1BAD"
    });
    zlog_undo::undo(&history, &mut s.book, false).unwrap();
    settle(&mut s, &mut [&mut c1], |s, c| {
        same(s, c) && c[0].book.log().qsos[0].callsign == "JA1AAA"
    });
    zlog_undo::undo(&history, &mut s.book, true).unwrap();
    settle(&mut s, &mut [&mut c1], |s, c| {
        same(s, c) && c[0].book.log().qsos[0].callsign == "JA1BAD"
    });
}

#[test]
fn the_server_station_frequency_reaches_the_clients() {
    let mut s = server(Vec::new(), false);
    let mut c1 = client(s.port, "PC1", Vec::new(), OnConnect::ConnectOnly);
    settle(&mut s, &mut [&mut c1], |s, _| {
        s.net.lock().hub.approved() == 1
    });
    let line = zlog_link::protocol::freq(
        Band::B7,
        Band::B7,
        false,
        7_025_000,
        zlog_core::Mode::Cw,
        true,
        "12:00:00",
        "SERVER",
    );
    s.net.lock().broadcast(&line);
    settle(&mut s, &mut [&mut c1], |_, c| {
        c[0].session
            .lock()
            .unwrap()
            .freqs
            .lines()
            .iter()
            .any(|l| l.contains("[SERVER]"))
    });
}
