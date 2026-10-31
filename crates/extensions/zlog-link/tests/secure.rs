use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zlog_core::{
    Band, Book, Contest, ContestMode, Extension, Link, Log, Qso, QsoScore, Registry, ScoreSheet,
    Scoring, Station,
};
use zlog_link::net::{Pairing, SecureTcp, Shared, StartError};
use zlog_link::role::{self, Role};
use zlog_link::session::OnConnect;
use zlog_link::{LogLink, ServerLink, Session};

static SERIAL: Mutex<()> = Mutex::new(());

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

fn wait(what: &str, mut step: impl FnMut() -> bool) {
    let start = Instant::now();
    while !step() {
        assert!(start.elapsed() < Duration::from_secs(20), "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn tls_pairing_then_known_then_pair_again_when_forgotten() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let net = Arc::new(Shared::default());
    {
        let mut n = net.lock();
        n.settings.port = "0".into();
        n.settings.bind = "127.0.0.1".into();
        n.settings.tls = true;
        n.start("nothing").unwrap();
        assert_eq!(n.secure(), Some(true));
    }
    let port = net.lock().listener.as_ref().unwrap().addr().port();
    let addr = format!("127.0.0.1:{port}");
    let qso = Qso {
        callsign: "JA1AAA".into(),
        reserve3: 100_000_100,
        ..Qso::default()
    };
    let mut server = book(
        vec![qso],
        Box::new(ServerLink::new(
            net.clone(),
            Arc::new(Mutex::new(Session::default())),
        )),
    );
    let mut plain = TcpStream::connect(&addr).unwrap();
    let _ = plain.write_all(b"#ZLOG# PCNAME OLD\r\n#ZLOG# SENDLOG\r\n");
    plain
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut buf = [0u8; 512];
    let got = plain.read(&mut buf).unwrap_or(0);
    assert!(
        !String::from_utf8_lossy(&buf[..got]).contains("ZLOG"),
        "TLS refuses plaintext"
    );

    let mine = Arc::new(Shared::default());
    let session = Arc::new(Mutex::new(Session::default()));
    let connect = |pairing: Pairing| {
        let mut s = session.lock().unwrap();
        s.settings.pc_name = "PC1".into();
        s.settings.on_connect = OnConnect::Download;
        s.connect(Box::new(SecureTcp::open(
            mine.clone(),
            &addr,
            "PC1",
            pairing,
        )));
        s.secure = true;
    };
    let mut client = book(Vec::new(), Box::new(LogLink::new(session.clone())));
    let approve = |server: &mut Book| {
        let mut code = None;
        wait("pairing request", || {
            server.poll().unwrap();
            let r = net.lock().listener.as_ref().unwrap().requests();
            code = r.first().map(|r| (r.id, r.code.clone()));
            code.is_some() && mine.lock().code.is_some()
        });
        let (id, theirs) = code.unwrap();
        assert_eq!(
            theirs,
            mine.lock().code.clone(),
            "both sides show the same code"
        );
        net.lock().listener.as_ref().unwrap().decide(id, true);
    };
    connect(Pairing::Auto);
    approve(&mut server);
    wait("download over TLS", || {
        server.poll().unwrap();
        client.poll().unwrap();
        client.log().qsos.len() == 1
    });
    assert!(mine.lock().pin(&addr).is_some(), "the server is pinned");
    session.lock().unwrap().disconnect();

    connect(Pairing::Auto);
    wait("known login", || {
        server.poll().unwrap();
        client.poll().unwrap();
        session.lock().unwrap().connected()
    });
    session.lock().unwrap().disconnect();

    let forgotten: Vec<_> = net.lock().clients.list();
    for p in forgotten {
        net.lock().clients.remove(&p.id);
    }
    connect(Pairing::Auto);
    wait("asked to pair again", || {
        server.poll().unwrap();
        client.poll().unwrap();
        mine.lock().repair.is_some()
    });
    assert_eq!(mine.lock().repair.as_deref(), Some(addr.as_str()));
    assert!(
        !session.lock().unwrap().connected(),
        "never re-paired silently"
    );
    mine.lock().repair = None;
    connect(Pairing::Again);
    approve(&mut server);
    wait("paired again", || {
        server.poll().unwrap();
        client.poll().unwrap();
        session.lock().unwrap().connected()
    });
    session.lock().unwrap().disconnect();
    net.lock().stop();
}

#[test]
fn client_and_server_roles_lock_each_other_both_ways() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = target.local_addr().unwrap().port();
    let mut s = Session::default();
    s.settings.host = "127.0.0.1".into();
    s.settings.port = port.to_string();
    let net = Shared::default();
    {
        let mut n = net.lock();
        n.settings.port = "0".into();
        n.settings.bind = "127.0.0.1".into();
    }

    s.connect_tcp().unwrap();
    assert_eq!(role::active(), Role::Client);
    assert_eq!(net.lock().start("x"), Err(StartError::Role(Role::Client)));
    assert!(!net.lock().running());
    s.disconnect();
    assert_eq!(role::active(), Role::Off);

    net.lock().start("x").unwrap();
    assert_eq!(s.connect_tcp(), Err("role-busy"));
    assert!(!s.active());
    net.lock().stop();
    assert_eq!(role::active(), Role::Off);

    s.settings.role = Role::Server;
    s.connect_tcp().unwrap();
    assert_eq!(
        s.settings.role,
        Role::Client,
        "joining needs no role choice"
    );
    s.disconnect();
    assert_eq!(role::active(), Role::Off);
}

#[test]
fn a_privileged_port_explains_why_it_cannot_listen() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let busy = TcpListener::bind("127.0.0.1:0").unwrap();
    let net = Shared::default();
    let mut n = net.lock();
    n.settings.bind = "127.0.0.1".into();
    n.settings.port = busy.local_addr().unwrap().port().to_string();
    let Err(e) = n.start("x") else {
        panic!("the port is taken")
    };
    assert!(matches!(e, StartError::Bind { .. }));
    assert_eq!(role::active(), Role::Off, "released after a failure");
    let text = zlog_link::panel::start_problem(
        &StartError::Bind {
            port: 23,
            error: std::io::ErrorKind::PermissionDenied,
            text: "Permission denied".into(),
        },
        "en",
    );
    assert!(text.contains("23") && text.contains("1024"), "{text}");
    n.settings.port = "x".into();
    assert!(matches!(n.start("x"), Err(StartError::Settings(_))));
}
