use std::sync::{Arc, Mutex};

use zlog_core::{
    Band, Book, Command, Contest, ContestMode, Context, Extension, Link, Log, LogEvent, Mode,
    Observer, Qso, QsoId, QsoScore, Registry, ScoreSheet, Scoring, Station,
};
use zlog_link::session::{NetEvent, OnConnect, Transport};
use zlog_link::wire::{decode, encode, qso_to_text};
use zlog_link::{LogLink, Session, ZLinkPanel};
use zlog_ui::View;

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
struct Server(Arc<Mutex<(Vec<NetEvent>, Vec<u8>)>>);

impl Server {
    fn say(&self, line: &str) {
        let bytes = encode(&format!("{line}\r\n"));
        self.0.lock().unwrap().0.push(NetEvent::Data(bytes));
    }

    fn heard(&self) -> Vec<String> {
        let bytes = std::mem::take(&mut self.0.lock().unwrap().1);
        decode(&bytes)
            .split("\r\n")
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()
    }
}

impl Transport for Server {
    fn send(&mut self, bytes: &[u8]) {
        self.0.lock().unwrap().1.extend_from_slice(bytes);
    }
    fn poll(&mut self) -> Vec<NetEvent> {
        std::mem::take(&mut self.0.lock().unwrap().0)
    }
    fn close(&mut self) {}
}

struct Package(Option<LogLink>);

impl Extension for Package {
    fn name(&self) -> &str {
        "test"
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(self.0.take().unwrap())]
    }
}

fn qso(call: &str, id: i32) -> Qso {
    Qso {
        callsign: call.into(),
        band: Band::B14,
        operator: "JA1ZLO".into(),
        reserve3: id,
        ..Qso::default()
    }
}

fn setup(log: Log, on_connect: OnConnect) -> (Book, Server, Arc<Mutex<Session>>) {
    let session = Arc::new(Mutex::new(Session::default()));
    let server = Server::default();
    {
        let mut s = session.lock().unwrap();
        s.settings.pc_name = "MAC".into();
        s.settings.on_connect = on_connect;
        s.connect(Box::new(server.clone()));
    }
    let registry = Registry::new().with(Package(Some(LogLink::new(session.clone()))));
    let (book, _) = Book::open(log, Box::new(Nothing), "nothing", registry);
    server
        .0
        .lock()
        .unwrap()
        .0
        .push(NetEvent::Connected("server".into()));
    (book, server, session)
}

fn setup_with_observer(on_connect: OnConnect) -> (Book, Server) {
    let session = Arc::new(Mutex::new(Session::default()));
    let server = Server::default();
    {
        let mut s = session.lock().unwrap();
        s.settings.pc_name = "MAC".into();
        s.settings.on_connect = on_connect;
        s.connect(Box::new(server.clone()));
    }
    let registry = Registry::new()
        .with(Package(Some(LogLink::new(session))))
        .with(EntityExtension);
    let (book, _) = Book::open(Log::default(), Box::new(Nothing), "nothing", registry);
    server
        .0
        .lock()
        .unwrap()
        .0
        .push(NetEvent::Connected("server".into()));
    (book, server)
}

#[test]
fn observer_reactions_go_out_for_local_qsos_only() {
    let (mut book, server) = setup_with_observer(OnConnect::ConnectOnly);
    book.poll().unwrap();
    server.heard();

    let remote = qso("DL1AA", 200_010_002);
    server.say(&format!("#ZLOG# PUTQSO {}", qso_to_text(&remote)));
    book.poll().unwrap();
    let here = &book.log().qsos[0];
    assert!(!here.entity.is_empty(), "the observer ran");
    assert_eq!(here.reserve3, remote.reserve3, "counter of the sender kept");
    assert!(server.heard().is_empty(), "no echo");

    book.execute(Command::Insert(qso("W1AW", 0))).unwrap();
    let local = book.log().qsos[1].clone();
    assert!(!local.entity.is_empty());
    let heard = server.heard();
    assert_eq!(heard.len(), 3, "{heard:?}");
    assert!(heard[1].starts_with("#ZLOG# PUTQSO "));
    assert_eq!(
        heard[2],
        format!("#ZLOG# EDITQSOTO {}", qso_to_text(&local))
    );
    assert_eq!(local.reserve3 % 100, 1);
}

fn calls(book: &Book) -> Vec<&str> {
    book.log()
        .qsos
        .iter()
        .map(|q| q.callsign.as_str())
        .collect()
}

#[test]
fn qsos_go_both_ways_without_echo() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    assert_eq!(
        server.heard(),
        vec!["#ZLOG# OPERATOR ", "#ZLOG# PCNAME MAC"]
    );

    let remote = qso("JA2BBB", 200_010_000);
    server.say(&format!("#ZLOG# PUTQSO {}", qso_to_text(&remote)));
    server.say(&format!("#ZLOG# PUTQSO {}", qso_to_text(&remote)));
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["JA2BBB"]);
    assert_eq!(book.log().qsos[0].reserve3, 200_010_000);
    assert!(server.heard().is_empty(), "no echo");

    book.execute(Command::Insert(qso("JA1AAA", 0))).unwrap();
    let heard = server.heard();
    assert_eq!(heard[0], "#ZLOG# BAND 4");
    let local = book.log().qsos[1].clone();
    assert_eq!(heard[1], format!("#ZLOG# PUTQSO {}", qso_to_text(&local)));

    let mut edited = remote.clone();
    edited.nr_rcvd = "13M".into();
    edited.reserve3 += 3;
    server.say(&format!("#ZLOG# EDITQSOTO {}", qso_to_text(&edited)));
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0], edited);
    assert!(server.heard().is_empty());

    let id = zlog_core::QsoId::of(&local);
    let mut changed = local.clone();
    changed.memo = "QSB".into();
    book.execute(Command::Update(id, changed)).unwrap();
    let after = book.log().qsos[1].clone();
    assert_eq!(after.reserve3, local.reserve3 + 1);
    assert_eq!(
        server.heard(),
        vec![format!("#ZLOG# EDITQSOTO {}", qso_to_text(&after))]
    );

    server.say(&format!("#ZLOG# DELQSO {}", qso_to_text(&edited)));
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["JA1AAA"]);
    assert!(server.heard().is_empty());
    book.execute(Command::Delete(id)).unwrap();
    assert_eq!(
        server.heard(),
        vec![format!("#ZLOG# DELQSO {}", qso_to_text(&after))]
    );
}

#[test]
fn merge_at_connect() {
    let log = Log {
        qsos: vec![
            qso("SAME", 100_010_000),
            qso("OLDHERE", 100_020_001),
            qso("NEWHERE", 100_030_005),
            qso("ONLYHERE", 100_040_000),
        ],
        ..Log::default()
    };
    let (mut book, server, _) = setup(log, OnConnect::Auto);
    book.poll().unwrap();
    let heard = server.heard();
    assert_eq!(heard[0], "#ZLOG# BAND 4");
    assert_eq!(heard[3], "#ZLOG# BEGINMERGE");

    server.say("#ZLOG# BEGINMERGE-OK");
    book.poll().unwrap();
    assert_eq!(server.heard(), vec!["#ZLOG# GETQSOIDS"]);

    server.say("#ZLOG# QSOIDS 100010000 100020003 100030002 ");
    server.say("#ZLOG# QSOIDS 300050000 ");
    server.say("#ZLOG# ENDQSOIDS");
    book.poll().unwrap();
    let q = &book.log().qsos;
    assert_eq!(
        server.heard(),
        vec![
            format!("#ZLOG# EDITQSOTO {}", qso_to_text(&q[2])),
            format!("#ZLOG# PUTLOG {}", qso_to_text(&q[3])),
            "#ZLOG# RENEW".to_string(),
            "#ZLOG# GETLOGQSOID 100020003 300050000 ".to_string(),
            "#ZLOG# SENDRENEW".to_string(),
            "#ZLOG# ENDMERGE".to_string(),
        ]
    );

    let mut newer = qso("OLDHERE", 100_020_003);
    newer.memo = "newer".into();
    server.say(&format!("#ZLOG# PUTLOGEX {}", qso_to_text(&newer)));
    server.say(&format!(
        "#ZLOG# PUTLOGEX {}",
        qso_to_text(&qso("ONLYTHERE", 300_050_000))
    ));
    server.say("#ZLOG# RENEW");
    book.poll().unwrap();
    assert_eq!(
        calls(&book),
        vec!["SAME", "OLDHERE", "NEWHERE", "ONLYHERE", "ONLYTHERE"]
    );
    assert_eq!(book.log().qsos[1].memo, "newer");
    assert_eq!(book.log().qsos[1].reserve3, 100_020_003);
    assert!(server.heard().is_empty());
}

#[test]
fn download_replaces_the_log_in_one_poll() {
    let log = Log {
        qsos: (0..40)
            .map(|i| qso("LOCAL", 100_000_000 + i * 10_000))
            .collect(),
        ..Log::default()
    };
    let (mut book, server, session) = setup(log, OnConnect::Download);
    book.poll().unwrap();
    assert!(book.log().qsos.is_empty());
    server.heard();
    book.poll().unwrap();
    assert_eq!(server.heard(), vec!["#ZLOG# SENDLOG"]);

    for i in 0..50 {
        let q = qso("REMOTE", 200_000_000 + i * 10_000);
        server.say(&format!("#ZLOG# PUTLOG {}", qso_to_text(&q)));
    }
    server.say("#ZLOG# RENEW");
    book.poll().unwrap();
    assert!(session.lock().unwrap().inbox_is_empty());
    assert_eq!(book.log().qsos.len(), 50);
    assert!(server.heard().is_empty());

    server.say("#ZLOG# SENDLOG");
    book.poll().unwrap();
    let heard = server.heard();
    assert_eq!(heard.len(), 51);
    assert_eq!(heard[50], "#ZLOG# RENEW");
}

#[test]
fn chat_arrives_in_the_session() {
    let (mut book, server, session) = setup(Log::default(), OnConnect::ConnectOnly);
    server.say("#ZLOG# PUTMESSAGE !21:05      7> QRV 7MHz?");
    let said = book.poll().unwrap();
    assert_eq!(said, vec!["21:05      7> QRV 7MHz?"]);
    let s = session.lock().unwrap();
    assert!(s.chat[0].red);
}

struct Picky;

impl Contest for Picky {
    fn name(&self) -> &str {
        "picky"
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
        let bad = log.qsos.last().is_some_and(|q| q.callsign == "BAD");
        Scoring {
            qsos: vec![QsoScore::default(); log.qsos.len() - bad as usize],
            sheet: ScoreSheet::default(),
        }
    }
}

fn put(server: &Server, command: &str, q: &Qso) {
    server.say(&format!("#ZLOG# {command} {}", qso_to_text(q)));
}

fn push(server: &Server, e: NetEvent) {
    server.0.lock().unwrap().0.push(e);
}

fn timed(call: &str, id: i32, hour: u32) -> Qso {
    Qso {
        time: zlog_core::DelphiTime::from_ymd_hms(2026, 10, 21, hour, 0, 0),
        ..qso(call, id)
    }
}

#[test]
fn edit_with_unchanged_reserve3_keeps_it() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    server.heard();
    let remote = qso("JA2BBB", 200_010_004);
    put(&server, "PUTQSO", &remote);
    book.poll().unwrap();
    let mut edited = remote.clone();
    edited.memo = "same counter".into();
    put(&server, "EDITQSOTO", &edited);
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0], edited);
    edited.memo = "again".into();
    put(&server, "PUTLOGEX", &edited);
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0].reserve3, 200_010_004);
    assert_eq!(book.log().qsos[0].memo, "again");
    let mut older = edited.clone();
    older.reserve3 = 200_010_001;
    put(&server, "EDITQSOTO", &older);
    book.poll().unwrap();
    assert_eq!(book.log().qsos[0].reserve3, 200_010_001);
    assert!(server.heard().is_empty());
}

#[test]
fn edits_and_deletes_of_unknown_qsos_are_ignored() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    put(&server, "EDITQSOTO", &qso("NOBODY", 900_000_000));
    put(&server, "DELQSO", &qso("NOBODY", 900_000_000));
    put(&server, "LOCKQSO", &qso("NOBODY", 900_000_000));
    put(&server, "PUTQSO", &qso("", 900_010_000));
    server.say("#ZLOG# PUTQSO garbage");
    book.poll().unwrap();
    assert!(book.log().qsos.is_empty());
    put(&server, "PUTLOG", &qso("", 900_030_000));
    book.poll().unwrap();
    assert!(book.log().qsos.is_empty());
    put(&server, "PUTLOGEX", &qso("NEW", 900_020_000));
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["NEW"]);
}

#[test]
fn a_failing_command_does_not_lose_the_rest_of_the_download() {
    let session = Arc::new(Mutex::new(Session::default()));
    let server = Server::default();
    {
        let mut s = session.lock().unwrap();
        s.settings.on_connect = OnConnect::ConnectOnly;
        s.connect(Box::new(server.clone()));
    }
    let registry = Registry::new().with(Package(Some(LogLink::new(session.clone()))));
    let (mut book, _) = Book::open(Log::default(), Box::new(Picky), "picky", registry);
    push(&server, NetEvent::Connected("server".into()));
    book.poll().unwrap();
    put(&server, "PUTLOG", &qso("FIRST", 100_010_000));
    put(&server, "PUTLOG", &qso("BAD", 100_020_000));
    put(&server, "PUTLOG", &qso("AFTER", 100_030_000));
    put(&server, "PUTLOG", &qso("LAST", 100_040_000));
    assert!(book.poll().is_err());
    assert_eq!(calls(&book), vec!["FIRST", "BAD", "AFTER", "LAST"]);
    assert!(session.lock().unwrap().inbox_is_empty());
}

#[test]
fn renew_sorts_the_log_by_time() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    server.heard();
    put(&server, "PUTLOG", &timed("C", 100_030_000, 3));
    put(&server, "PUTLOG", &timed("A", 100_010_000, 1));
    put(&server, "PUTLOG", &timed("B", 100_020_000, 2));
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["C", "A", "B"]);
    server.say("#ZLOG# RENEW");
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["A", "B", "C"]);
    put(&server, "PUTLOG", &timed("Z", 100_040_000, 0));
    server.say("#ZLOG# RENEW ");
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["Z", "A", "B", "C"]);
    assert!(server.heard().is_empty());
}

#[test]
fn insert_before_named_qso() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    put(&server, "PUTLOG", &qso("A", 100_010_000));
    put(&server, "PUTLOG", &qso("C", 100_030_000));
    let mut b = qso("B", 100_020_000);
    b.reserve2 = 100_030_005;
    put(&server, "INSQSO", &b);
    let mut d = qso("D", 100_040_000);
    d.reserve2 = 999_990_000;
    put(&server, "INSQSO", &d);
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["A", "B", "C", "D"]);
}

#[test]
fn insqsoat_waits_for_renew() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    server.heard();
    put(&server, "PUTLOG", &qso("A", 100_010_000));
    put(&server, "PUTLOG", &qso("C", 100_030_000));
    let mut b = qso("B", 100_020_000);
    b.reserve2 = 100_030_005;
    put(&server, "INSQSOAT", &b);
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["A", "C"]);
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["A", "C"]);
    server.say("#ZLOG# RENEW");
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["A", "B", "C"]);
    server.say("#ZLOG# RENEW");
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["A", "B", "C"]);
    assert!(server.heard().is_empty());
}

#[test]
fn insqsoat_queue_runs_before_a_later_edit() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    let mut b = qso("B", 100_020_000);
    b.reserve2 = 999_990_000;
    put(&server, "INSQSOAT", &b);
    put(&server, "INSQSOAT", &b);
    put(&server, "PUTQSO", &qso("A", 100_010_000));
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["A"]);
    let mut edited = b.clone();
    edited.memo = "edited".into();
    put(&server, "EDITQSOTO", &edited);
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["A", "B"]);
    assert_eq!(book.log().qsos[1].memo, "edited");
}

#[test]
fn lines_split_anywhere_and_shift_jis_memo_arrives() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    let mut q = qso("JA3CCC", 300_010_000);
    q.memo = "大阪 ソ".into();
    q.operator = "髙橋".into();
    let bytes = encode(&format!("#ZLOG# PUTQSO {}\r\n", qso_to_text(&q)));
    for b in &bytes {
        push(&server, NetEvent::Data(vec![*b]));
        book.poll().unwrap();
    }
    assert_eq!(book.log().qsos.len(), 1);
    assert_eq!(book.log().qsos[0].memo, "大阪 ソ");
    assert_eq!(book.log().qsos[0].operator, "髙橋");
    let text = format!(
        "#ZLOG# PUTQSO {}\n#ZLOG# PUTQSO {}",
        qso_to_text(&qso("LF", 300_020_000)),
        qso_to_text(&qso("PARTIAL", 300_030_000))
    );
    push(&server, NetEvent::Data(encode(&text)));
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["JA3CCC", "LF"]);
    push(&server, NetEvent::Closed(None));
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["JA3CCC", "LF", "PARTIAL"]);
}

#[test]
fn local_shift_jis_qso_goes_out_in_shift_jis() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    server.heard();
    let mut q = qso("JA1AAA", 0);
    q.memo = "東京".into();
    book.execute(Command::Insert(q)).unwrap();
    let raw = server.0.lock().unwrap().1.clone();
    assert!(raw.windows(4).any(|w| w == [0x93, 0x8C, 0x8B, 0x9E]));
    assert!(raw.ends_with(b"\r\n"));
}

#[test]
fn nothing_is_sent_while_disconnected() {
    let (mut book, server, session) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    server.heard();
    session.lock().unwrap().disconnect();
    book.execute(Command::Insert(qso("JA1AAA", 0))).unwrap();
    assert!(server.heard().is_empty());
}

#[test]
fn merge_end_to_end_with_empty_server() {
    let log = Log {
        qsos: vec![qso("ONE", 100_010_000), qso("TWO", 100_020_000)],
        ..Log::default()
    };
    let (mut book, server, _) = setup(log, OnConnect::Merge);
    book.poll().unwrap();
    server.heard();
    server.say("#ZLOG# BEGINMERGE-OK");
    server.say("#ZLOG# ENDQSOIDS");
    book.poll().unwrap();
    let q = &book.log().qsos;
    assert_eq!(
        server.heard(),
        vec![
            "#ZLOG# GETQSOIDS".to_string(),
            format!("#ZLOG# PUTLOG {}", qso_to_text(&q[0])),
            format!("#ZLOG# PUTLOG {}", qso_to_text(&q[1])),
            "#ZLOG# RENEW".to_string(),
            "#ZLOG# ENDMERGE".to_string(),
        ]
    );
}

#[test]
fn merge_of_empty_log_asks_for_everything() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::Merge);
    book.poll().unwrap();
    assert_eq!(server.heard().last().unwrap(), "#ZLOG# BEGINMERGE");
    server.say("#ZLOG# BEGINMERGE-OK");
    server.say("#ZLOG# QSOIDS 100010003 100010009 100020000 ");
    server.say("#ZLOG# ENDQSOIDS");
    book.poll().unwrap();
    assert_eq!(
        server.heard(),
        vec![
            "#ZLOG# GETQSOIDS",
            "#ZLOG# GETLOGQSOID 100010003 100020000 ",
            "#ZLOG# SENDRENEW",
            "#ZLOG# ENDMERGE",
        ]
    );
    let mut a = timed("A", 100_010_003, 5);
    a.memo = "Ｓｈｉｆｔ＿ＪＩＳ".into();
    put(&server, "PUTLOGEX", &a);
    put(&server, "PUTLOGEX", &timed("B", 100_020_000, 4));
    server.say("#ZLOG# RENEW");
    book.poll().unwrap();
    assert_eq!(calls(&book), vec!["B", "A"]);
    assert_eq!(book.log().qsos[1].memo, "Ｓｈｉｆｔ＿ＪＩＳ");
    assert!(server.heard().is_empty());
}

#[test]
fn merge_replies_outside_a_merge_are_harmless() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    server.heard();
    server.say("#ZLOG# QSOIDS 1 2 3 ");
    server.say("#ZLOG# ENDQSOIDS");
    server.say("#ZLOG# BEGINMERGE-NG");
    book.poll().unwrap();
    assert!(server.heard().is_empty());
    assert!(book.log().qsos.is_empty());
}

#[test]
fn sendlog_of_empty_log_is_only_renew() {
    let (mut book, server, _) = setup(Log::default(), OnConnect::ConnectOnly);
    book.poll().unwrap();
    server.heard();
    server.say("#ZLOG# SENDLOG");
    book.poll().unwrap();
    assert_eq!(server.heard(), vec!["#ZLOG# RENEW"]);
}

struct Entry {
    log: Log,
    station: Station,
    sheet: ScoreSheet,
    band: Band,
    cq: bool,
    operator: String,
    pc_name: String,
    hz: Option<u64>,
}

impl Entry {
    fn new() -> Self {
        Entry {
            log: Log::default(),
            station: Station::default(),
            sheet: ScoreSheet::default(),
            band: Band::B7,
            cq: false,
            operator: String::new(),
            pc_name: "MAC".into(),
            hz: None,
        }
    }

    fn follow(&self, panel: &mut ZLinkPanel) {
        let view = View {
            log: &self.log,
            contest: &Nothing,
            sheet: &self.sheet,
            callsign: "",
            station: &self.station,
            number: "",
            memo: "",
            rst_sent: "599",
            power: zlog_core::Power::P010,
            band: self.band,
            mode: Mode::Cw,
            language: "en",
            spots: &[],
            hz: self.hz,
            rig: 1,
            cq: self.cq,
            operator: &self.operator,
            period_hours: 0,
            pc_name: &self.pc_name,
            log_path: "",
            focus: None,
            editing: None,
            selected: None,
            period: &(),
            tab: Default::default(),
            changes: 0,
        };
        panel.follow(&view);
    }
}

#[test]
fn operator_of_the_entry_row_is_announced_on_change() {
    let (mut book, server, session) = setup(Log::default(), OnConnect::ConnectOnly);
    let mut panel = ZLinkPanel::new(session.clone());
    let mut entry = Entry::new();
    entry.operator = "JA1XYZ".into();
    entry.station.operator = "STATIONOP".into();
    entry.follow(&mut panel);
    book.poll().unwrap();
    assert_eq!(
        server.heard(),
        vec![
            "#ZLOG# BAND 2",
            "#ZLOG# OPERATOR JA1XYZ",
            "#ZLOG# PCNAME MAC"
        ]
    );
    entry.follow(&mut panel);
    assert!(server.heard().is_empty(), "unchanged: not sent again");
    entry.operator = "JA1ZLO".into();
    entry.follow(&mut panel);
    assert_eq!(server.heard(), vec!["#ZLOG# OPERATOR JA1ZLO"]);
    entry.operator.clear();
    entry.follow(&mut panel);
    assert_eq!(server.heard(), vec!["#ZLOG# OPERATOR "]);
    entry.band = Band::B14;
    entry.follow(&mut panel);
    assert_eq!(server.heard(), vec!["#ZLOG# BAND 4"]);
}

#[test]
fn rig_status_carries_cq_and_multi_station() {
    let (mut book, server, session) = setup(Log::default(), OnConnect::ConnectOnly);
    let mut panel = ZLinkPanel::new(session.clone());
    let mut entry = Entry::new();
    entry.follow(&mut panel);
    book.poll().unwrap();
    server.heard();
    entry.follow(&mut panel);
    assert!(server.heard().is_empty());
    entry.hz = Some(7_010_000);
    entry.cq = true;
    entry.station.tx_nr = 1;
    entry.follow(&mut panel);
    let heard = server.heard();
    assert_eq!(heard.len(), 1);
    assert!(
        heard[0].starts_with("#ZLOG# FREQ 30 7        7010.0 CW   CQ "),
        "{}",
        heard[0]
    );
    assert!(heard[0].ends_with("  [MAC]"));
    entry.follow(&mut panel);
    entry.cq = false;
    entry.follow(&mut panel);
    assert!(server.heard().is_empty());
}

#[test]
fn rig_status_of_a_single_station() {
    let (mut book, server, session) = setup(Log::default(), OnConnect::ConnectOnly);
    let mut panel = ZLinkPanel::new(session.clone());
    let mut entry = Entry::new();
    entry.follow(&mut panel);
    book.poll().unwrap();
    server.heard();
    entry.hz = Some(7_010_000);
    entry.follow(&mut panel);
    let heard = server.heard();
    assert!(
        heard[0].starts_with("#ZLOG# FREQ 2  7        7010.0 CW   SP "),
        "{heard:?}"
    );
    let lines = session.lock().unwrap().freqs.lines();
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].starts_with("7        7010.0 CW   SP "),
        "{lines:?}"
    );
}

#[test]
fn pc_name_comes_from_the_view() {
    let (mut book, server, session) = setup(Log::default(), OnConnect::ConnectOnly);
    let mut panel = ZLinkPanel::new(session.clone());
    let mut entry = Entry::new();
    entry.pc_name = "PC2".into();
    entry.follow(&mut panel);
    book.poll().unwrap();
    assert!(server.heard().contains(&"#ZLOG# PCNAME PC2".to_string()));
    assert_eq!(session.lock().unwrap().settings.pc_name, "PC2");
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
