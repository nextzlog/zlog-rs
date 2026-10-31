use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use zlog_tls::discovery::{decode_txt, encode_txt, service_type, Announcer, Browser, Service};
use zlog_tls::{
    connect_plain, connect_tls, load_identity, load_peers, AuditKind, ClientConfig, ConnectError,
    Ending, Identity, Limits, LineConn, Login, PeerId, Peers, Range, Server, ServerConfig, Token,
};

const APP: &str = "test";
const WAIT: Duration = Duration::from_secs(10);

fn until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(t) = f() {
            return t;
        }
        assert!(start.elapsed() < WAIT, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn local() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

fn fast() -> Limits {
    Limits {
        delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(4),
        ..Limits::default()
    }
}

fn tls_server(token: Option<&Token>, limits: Limits) -> Server {
    let config = ServerConfig {
        tls: Some(Identity::generate().unwrap()),
        token: token.map(Token::hash),
        limits,
        ..ServerConfig::new(APP, local())
    };
    Server::bind(config, Peers::default()).unwrap()
}

fn client(name: &str) -> ClientConfig {
    ClientConfig {
        identity: Some(Identity::generate().unwrap()),
        ..ClientConfig::new(APP, name)
    }
}

fn pair_in_background(
    addr: SocketAddr,
    config: &ClientConfig,
) -> (
    String,
    std::thread::JoinHandle<Result<LineConn, ConnectError>>,
) {
    let hs = connect_tls(addr, config, None).unwrap();
    let code = hs.code().to_string();
    (
        code,
        std::thread::spawn(move || hs.login(Login::Pair, WAIT)),
    )
}

fn echo(server: &Server, conn: &mut LineConn) {
    let mut far = until("accepted", || server.accept(Duration::from_millis(50)));
    conn.write_line(b"CQ TEST").unwrap();
    assert_eq!(far.read_line_until(WAIT).unwrap(), b"CQ TEST");
    far.write_line(b"73").unwrap();
    assert_eq!(conn.read_line_until(WAIT).unwrap(), b"73");
}

#[test]
fn pairing_shows_the_same_code_on_both_sides_and_remembers_the_peer() {
    let server = tls_server(None, fast());
    let me = client("JA1ZLO");
    let (code, login) = pair_in_background(server.addr(), &me);
    assert_eq!(code.len(), 6);
    assert!(code.bytes().all(|b| b.is_ascii_digit()));
    let request = until("asked", || server.requests().pop());
    assert_eq!(request.code.as_deref(), Some(code.as_str()));
    assert_eq!(request.name, "JA1ZLO");
    let fp = me.identity.as_ref().unwrap().fingerprint();
    assert_eq!(request.fingerprint, Some(fp));
    assert!(server.decide(request.id, true));
    let mut conn = login.join().unwrap().unwrap();
    assert_eq!(conn.peer().fingerprint, server.fingerprint());
    echo(&server, &mut conn);
    assert!(server.peers().knows(&PeerId::Cert(fp)));
    assert_eq!(server.peers().list()[0].name, "JA1ZLO");

    let pin = server.fingerprint();
    let again = connect_tls(server.addr(), &me, pin).unwrap();
    let mut conn = again.login(Login::Known, WAIT).unwrap();
    assert!(server.requests().is_empty(), "no second approval");
    echo(&server, &mut conn);

    assert!(server.peers().remove(&PeerId::Cert(fp)));
    let gone = connect_tls(server.addr(), &me, pin).unwrap();
    assert!(matches!(
        gone.login(Login::Known, WAIT),
        Err(ConnectError::NotPaired)
    ));
}

#[test]
fn a_denied_pairing_is_refused_and_forgotten() {
    let server = tls_server(None, fast());
    let audit = server.audit();
    let (_, login) = pair_in_background(server.addr(), &client("X"));
    let request = until("asked", || server.requests().pop());
    assert!(server.decide(request.id, false));
    assert!(matches!(login.join().unwrap(), Err(ConnectError::Denied)));
    assert!(server.peers().list().is_empty());
    let kinds: Vec<_> = audit.try_iter().map(|a| a.kind).collect();
    assert!(kinds.contains(&AuditKind::PairRequested), "{kinds:?}");
    assert!(kinds.contains(&AuditKind::Denied), "{kinds:?}");
}

#[test]
fn a_man_in_the_middle_shows_different_codes() {
    let server = tls_server(None, fast());
    let mitm = tls_server(None, fast());
    let victim = client("VICTIM");
    let (victim_code, victim_login) = pair_in_background(mitm.addr(), &victim);
    let mitm_request = until("the relay is asked", || mitm.requests().pop());
    assert_eq!(mitm_request.code.as_deref(), Some(victim_code.as_str()));
    let (relay_code, relay_login) = pair_in_background(server.addr(), &client("VICTIM"));
    let request = until("the server is asked", || server.requests().pop());
    assert_eq!(request.code.as_deref(), Some(relay_code.as_str()));
    assert_ne!(
        request.code.as_deref(),
        Some(victim_code.as_str()),
        "the operator sees a code that differs from the one on the victim"
    );
    assert!(server.decide(request.id, false));
    assert!(mitm.decide(mitm_request.id, false));
    assert!(relay_login.join().unwrap().is_err());
    assert!(victim_login.join().unwrap().is_err());
}

#[test]
fn known_or_token_logins_need_a_pinned_server() {
    let token = Token::generate();
    let server = tls_server(Some(&token), fast());
    let me = client("A");
    let fp = server.fingerprint().unwrap();
    let hs = connect_tls(server.addr(), &me, None).unwrap();
    assert_eq!(hs.fingerprint(), fp);
    match hs.login(Login::Token(token.to_string()), WAIT) {
        Err(ConnectError::Unknown(found)) => assert_eq!(found, fp),
        r => panic!("{:?}", r.err()),
    }
    let other = Identity::generate().unwrap().fingerprint();
    match connect_tls(server.addr(), &me, Some(other)) {
        Err(ConnectError::Mismatch { pinned, found }) => assert_eq!((pinned, found), (other, fp)),
        r => panic!("{:?}", r.err().map(|e| e.to_string())),
    }
    assert!(matches!(
        connect_tls(server.addr(), &ClientConfig::new(APP, "B"), Some(fp)),
        Err(ConnectError::NoIdentity)
    ));
}

#[test]
fn a_token_logs_in_without_pairing_and_is_checked_by_hash() {
    let token = Token::generate();
    let server = tls_server(Some(&token), fast());
    let pin = server.fingerprint();
    let me = client("VPN");
    let spaced = token.as_str().to_lowercase().replace('-', " ");
    let hs = connect_tls(server.addr(), &me, pin).unwrap();
    let mut conn = hs.login(Login::Token(spaced), WAIT).unwrap();
    echo(&server, &mut conn);
    assert!(
        server.peers().list().is_empty(),
        "a token login is not paired"
    );
    let wrong = connect_tls(server.addr(), &me, pin).unwrap();
    assert!(matches!(
        wrong.login(Login::Token("guess".into()), WAIT),
        Err(ConnectError::Denied)
    ));
    let fresh = Token::generate();
    server.set_token(Some(fresh.hash()));
    let old = connect_tls(server.addr(), &me, pin).unwrap();
    assert!(matches!(
        old.login(Login::Token(token.to_string()), WAIT),
        Err(ConnectError::Denied)
    ));
    let a = Token::generate();
    assert_ne!(a, fresh);
    assert_eq!(a.as_str().replace('-', "").len(), 32);
    let stored: zlog_tls::TokenHash = a.hash().to_string().parse().unwrap();
    assert!(stored.verify(a.as_str()) && !stored.verify(fresh.as_str()));
}

#[test]
fn repeated_failures_lock_the_address_out_until_it_expires() {
    let token = Token::generate();
    let server = tls_server(
        Some(&token),
        Limits {
            attempts: 3,
            lockout: Duration::from_millis(400),
            ..fast()
        },
    );
    let audit = server.audit();
    let pin = server.fingerprint();
    let me = client("BAD");
    let try_token =
        |t: &str| connect_tls(server.addr(), &me, pin)?.login(Login::Token(t.into()), WAIT);
    for _ in 0..2 {
        assert!(matches!(try_token("guess"), Err(ConnectError::Denied)));
    }
    assert!(matches!(try_token("guess"), Err(ConnectError::Locked)));
    assert!(
        matches!(try_token(token.as_str()), Err(ConnectError::Locked)),
        "even the right token waits"
    );
    let kinds: Vec<_> = audit.try_iter().map(|a| a.kind).collect();
    assert!(kinds.contains(&AuditKind::AuthFailed { failures: 2 }));
    assert!(kinds
        .iter()
        .any(|k| matches!(k, AuditKind::LockedOut { .. })));
    assert!(kinds.contains(&AuditKind::Locked));
    std::thread::sleep(Duration::from_millis(500));
    assert!(try_token(token.as_str()).is_ok());
}

fn plain(config: ServerConfig) -> Server {
    Server::bind(config, Peers::default()).unwrap()
}

#[test]
fn a_plain_link_carries_shift_jis_lines_with_crlf() {
    let server = plain(ServerConfig {
        approve_new: false,
        ending: Ending::CrLf,
        ..ServerConfig::new(APP, local())
    });
    let config = ClientConfig {
        ending: Ending::CrLf,
        ..ClientConfig::new(APP, "")
    };
    let mut conn = connect_plain(server.addr(), &config, None).unwrap();
    let mut far = until("accepted", || server.accept(Duration::from_millis(50)));
    let sjis = [0x93, 0xfa, 0x96, 0x7b, b'\r'];
    conn.write_line(&sjis).unwrap();
    assert_eq!(far.read_line_until(WAIT).unwrap(), &sjis[..4]);
    far.write_line(b"PUTQSO").unwrap();
    assert_eq!(conn.read_line_until(WAIT).unwrap(), b"PUTQSO");
    assert_eq!(server.connections(), 1);
    drop(far);
    until("the slot is free", || {
        (server.connections() == 0).then_some(())
    });
}

#[test]
fn a_new_plain_peer_waits_for_approval_by_address_and_name() {
    let server = plain(ServerConfig {
        handshake: true,
        ..ServerConfig::new(APP, local())
    });
    let config = ClientConfig::new(APP, "JA1YAA");
    let addr = server.addr();
    let c = config.clone();
    let login = std::thread::spawn(move || connect_plain(addr, &c, Some(WAIT)));
    let request = until("asked", || server.requests().pop());
    assert_eq!(request.name, "JA1YAA");
    assert_eq!(request.code, None);
    assert!(request.addr.ip().is_loopback());
    server.decide(request.id, true);
    assert!(login.join().unwrap().is_ok());
    assert!(server
        .peers()
        .knows(&PeerId::Ip("127.0.0.1".parse().unwrap())));
    assert!(
        connect_plain(addr, &config, Some(WAIT)).is_ok(),
        "known now"
    );
}

#[test]
fn addresses_out_of_range_and_extra_connections_are_dropped() {
    let server = plain(ServerConfig {
        allowed: vec!["10.0.0.0/8".parse().unwrap()],
        approve_new: false,
        ..ServerConfig::new(APP, local())
    });
    let audit = server.audit();
    let mut conn = connect_plain(server.addr(), &ClientConfig::new(APP, ""), None).unwrap();
    assert!(conn.read_line_until(WAIT).is_err());
    assert!(audit.recv_timeout(WAIT).unwrap().kind == AuditKind::OutOfRange);

    let server = plain(ServerConfig {
        approve_new: false,
        limits: Limits {
            max_conns: 1,
            ..Limits::default()
        },
        ..ServerConfig::new(APP, local())
    });
    let audit = server.audit();
    let config = ClientConfig::new(APP, "");
    let _first = connect_plain(server.addr(), &config, None).unwrap();
    let _kept = until("accepted", || server.accept(Duration::from_millis(50)));
    let mut second = connect_plain(server.addr(), &config, None).unwrap();
    assert!(second.read_line_until(WAIT).is_err());
    until("full", || {
        audit
            .try_iter()
            .find(|a| a.kind == AuditKind::Full)
            .map(|_| ())
    });
}

#[test]
fn long_lines_and_floods_close_the_link() {
    let server = plain(ServerConfig {
        approve_new: false,
        limits: Limits {
            max_line: 8,
            lines_per_sec: 3,
            ..Limits::default()
        },
        ..ServerConfig::new(APP, local())
    });
    let audit = server.audit();
    let config = ClientConfig::new(APP, "");
    let mut conn = connect_plain(server.addr(), &config, None).unwrap();
    let mut far = until("accepted", || server.accept(Duration::from_millis(50)));
    conn.write_line(b"0123456789").unwrap();
    assert!(far.read_line_until(WAIT).is_err());
    let mut conn = connect_plain(server.addr(), &config, None).unwrap();
    let mut far = until("accepted", || server.accept(Duration::from_millis(50)));
    for _ in 0..5 {
        conn.write_line(b"QRL?").unwrap();
    }
    let got: Vec<_> = (0..5).map(|_| far.read_line_until(WAIT)).collect();
    assert_eq!(got.iter().filter(|r| r.is_ok()).count(), 3);
    let kinds: Vec<_> = audit.try_iter().map(|a| a.kind).collect();
    assert!(kinds.contains(&AuditKind::LineTooLong), "{kinds:?}");
    assert!(kinds.contains(&AuditKind::RateLimited), "{kinds:?}");
}

#[test]
fn ranges_default_to_private_networks() {
    let private = Range::private();
    for ip in [
        "192.168.1.5",
        "10.1.2.3",
        "172.31.0.1",
        "127.0.0.1",
        "::1",
        "fe80::1",
    ] {
        assert!(Range::allows(&private, ip.parse().unwrap()), "{ip}");
    }
    for ip in ["8.8.8.8", "172.32.0.1", "2001:db8::1"] {
        assert!(!Range::allows(&private, ip.parse().unwrap()), "{ip}");
    }
    let mapped = "::ffff:192.168.0.1".parse().unwrap();
    assert!(Range::allows(&private, mapped));
    let r: Range = "0.0.0.0/0".parse().unwrap();
    assert!(r.contains("1.2.3.4".parse().unwrap()));
    assert_eq!(r.to_string(), "0.0.0.0/0");
    assert!("10.0.0.0/33".parse::<Range>().is_err());
}

#[test]
fn identity_token_and_peers_persist_through_a_store() {
    let mut store: BTreeMap<String, String> = BTreeMap::new();
    let id = load_identity(&mut store, "identity").unwrap();
    assert_eq!(load_identity(&mut store, "identity").unwrap(), id);
    assert_eq!(
        Identity::from_text(&id.to_text()).unwrap().fingerprint(),
        id.fingerprint()
    );
    let peers = Peers::default();
    peers.add(PeerId::Cert(id.fingerprint()), "JA1ZLO; club");
    peers.add(PeerId::Ip("192.168.0.9".parse().unwrap()), "");
    store.insert("peers".into(), peers.to_text());
    let back = load_peers(&store, "peers");
    assert_eq!(back.list(), peers.list());
    assert_eq!(back.list()[0].name, "JA1ZLO club");
}

#[test]
fn txt_fields_are_cleaned_and_bounded() {
    let fields = vec![
        ("Name".to_string(), "JA1ZLO\nstation".to_string()),
        ("contest".to_string(), "x".repeat(400)),
        ("=".to_string(), "dropped".to_string()),
        ("tls".to_string(), "1".to_string()),
    ];
    let txt = encode_txt(&fields);
    assert_eq!(txt.len(), 3);
    assert_eq!(txt[0], ("name".into(), "JA1ZLOstation".into()));
    assert_eq!(txt[1].1.len(), 255 - "contest=".len());
    let entries: Vec<String> = txt.iter().map(|(k, v)| format!("{k}={v}")).collect();
    assert_eq!(decode_txt(&entries), txt);
    assert_eq!(service_type("zLog Remote!"), "_zlogremote._tcp.local.");
}

#[test]
fn discovery_finds_an_announced_service() {
    let service = Service {
        kind: "zlog-test".into(),
        name: "JA1ZLO test".into(),
        port: 6500,
        txt: vec![
            ("tls".into(), "1".into()),
            ("contest".into(), "ALLJA1".into()),
        ],
    };
    let _a = Announcer::start(&service).unwrap();
    let mut b = Browser::start("zlog-test").unwrap();
    let found = until("found", || {
        b.poll().into_iter().find(|f| f.name == service.name)
    });
    assert_eq!(found.port, 6500);
    assert_eq!(found.get("contest"), Some("ALLJA1"));
    assert!(!found.endpoints().is_empty());
}
