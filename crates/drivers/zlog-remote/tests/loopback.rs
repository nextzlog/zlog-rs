use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use haml::sim::so2r::SimCommand;
use haml::sim::{SimKeyer, SimSo2r};
use haml::{Command, Event, Mode, RigCaps, RigEvent, Vfo};
use zlog_keyer::KeyerDrv;
use zlog_remote::server::Listening;
use zlog_remote::{client, server, Remote, RemoteKeyerDrv, RemoteRigs, RemoteSo2rDrv, Settings};
use zlog_so2r::{Rx, So2rDrv};
use zlog_tls::{Identity, Peers, Token};
use zlog_ui::device::PortHub;
use zlog_ui::rig::{Exposed, RigSource};
use zlog_ui::tx::{Signal, TxCommand, TxEvent, TxHub};
use zlog_ui::Section;

const CAPS: RigCaps = RigCaps {
    xit: false,
    ..RigCaps::ALL
};

fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < Duration::from_secs(10), "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct Station {
    server: Listening,
    token: Token,
    rig: Exposed,
    tx: TxHub,
    sent: Arc<Mutex<Vec<String>>>,
    boxed: Arc<Mutex<Vec<SimCommand>>>,
}

fn station() -> Station {
    let rig = Exposed::default();
    rig.set(Some(("IC-7300".into(), CAPS)));
    let tx = TxHub::default();
    let keyer = SimKeyer::new(zlog_keyer::Timing {
        wpm: 60,
        ..Default::default()
    });
    let sent = keyer.sent.clone();
    tx.set_keyer(Some(Box::new(keyer)));
    let boxed = Arc::new(Mutex::new(Vec::new()));
    tx.set_so2r(Some(Box::new(SimSo2r::new(boxed.clone()))));
    let token = Token::generate();
    let server = lend(&token, [rig.clone(), Exposed::default()], &tx);
    Station {
        server,
        token,
        rig,
        tx,
        sent,
        boxed,
    }
}

fn lend(token: &Token, rigs: [Exposed; 2], tx: &TxHub) -> Listening {
    let settings = Settings {
        listen: true,
        listen_addr: "127.0.0.1".into(),
        listen_port: 0,
        token: Some(token.hash()),
        expose_rigs: [true, false],
        ..Settings::default()
    };
    let id = Identity::generate().unwrap();
    server::start(&settings, id, Peers::default(), rigs, tx).unwrap()
}

fn pinned(server: &Listening) -> Arc<Remote> {
    let remote = Arc::new(Remote::default());
    let addr = server.addr().to_string();
    remote.remember_server(&addr, server.tls.fingerprint().unwrap());
    remote
}

fn client_of(s: &Station) -> Arc<Remote> {
    let remote = pinned(&s.server);
    let addr = s.server.addr().to_string();
    client::connect(&remote, &addr, "JA1ZLO", s.token.as_str()).unwrap();
    remote
}

#[test]
fn a_new_client_pairs_once_with_the_same_code_on_both_sides() {
    let s = station();
    let remote = Arc::new(Remote::default());
    let addr = s.server.addr().to_string();
    let (r, a) = (remote.clone(), addr.clone());
    let pairing = std::thread::spawn(move || client::connect(&r, &a, "JA1ZLO", ""));
    let mut request = None;
    until("the server is asked", || {
        request = s.server.tls.requests().pop();
        request.is_some() && remote.lock().code.is_some()
    });
    let request = request.unwrap();
    assert_eq!(request.code, remote.lock().code);
    assert_eq!(request.name, "JA1ZLO");
    assert!(s.server.tls.decide(request.id, true));
    pairing.join().unwrap().unwrap();
    assert!(remote.lock().code.is_none());
    assert_eq!(remote.pin(&addr), s.server.tls.fingerprint());
    assert_eq!(s.server.tls.peers().list().len(), 1);
    client::disconnect(&remote);
    until("the slot is free", || s.server.peers().is_empty());
    client::connect(&remote, &addr, "JA1ZLO", "").unwrap();
    assert!(s.server.tls.requests().is_empty(), "known now");
}

#[test]
fn a_token_for_an_unknown_server_asks_to_trust_its_certificate() {
    let s = station();
    let remote = Arc::new(Remote::default());
    let addr = s.server.addr().to_string();
    assert!(client::connect(&remote, &addr, "X", s.token.as_str()).is_err());
    let (at, fp) = remote.lock().trust.clone().unwrap();
    assert_eq!(
        (at.as_str(), Some(fp)),
        (addr.as_str(), s.server.tls.fingerprint())
    );
    remote.remember_server(&addr, fp);
    assert!(client::connect(&remote, &addr, "X", "wrong").is_err());
    assert!(client::connect(&remote, &addr, "X", s.token.as_str()).is_ok());
}

#[test]
fn a_wrong_token_or_no_address_is_refused() {
    let s = station();
    let remote = pinned(&s.server);
    let addr = s.server.addr().to_string();
    assert!(client::connect(&remote, &addr, "X", "QRM").is_err());
    assert!(client::connect(&remote, " ", "X", s.token.as_str()).is_err());
    assert!(remote.client().is_none());
    let rigs = RemoteRigs::new(remote.clone());
    assert!(rigs.ports().is_empty());
    assert!(rigs.open("remote:1 RIG1").unwrap().is_err());
    assert!(rigs.open("COM3").is_none(), "a local port is not ours");
    assert!(RemoteKeyerDrv::new(remote.clone())
        .open(&Section::new(), &PortHub::new())
        .is_err());
}

#[test]
fn the_remote_rig_is_listed_with_its_caps_and_driven_both_ways() {
    let s = station();
    let remote = client_of(&s);
    let rigs = RemoteRigs::new(remote.clone());
    assert_eq!(rigs.ports(), ["remote:1 RIG1 IC-7300"]);
    let mut rig = rigs.open("remote:1 RIG1 IC-7300").unwrap().unwrap();
    assert_eq!(rig.caps(), CAPS);
    rig.send(Command::SetFreq(Vfo::A, 7_012_000), Instant::now());
    let mut got = Vec::new();
    until("the command reached the lent rig", || {
        got.extend(s.rig.take());
        !got.is_empty()
    });
    assert_eq!(got, [Command::SetFreq(Vfo::A, 7_012_000)]);
    s.rig
        .publish(RigEvent::Event(Event::Freq(Vfo::A, 7_012_000)));
    s.rig.publish(RigEvent::Event(Event::Mode(Mode::Cw)));
    until("the rig state came back", || {
        rig.tick(Instant::now());
        rig.state().freq() == Some(7_012_000) && rig.state().mode == Some(Mode::Cw)
    });
}

#[test]
fn f_key_cw_goes_through_both_arbiters_to_the_radio_the_box_was_told() {
    let s = station();
    let remote = client_of(&s);
    let local = TxHub::default();
    let keyer = RemoteKeyerDrv::new(remote.clone())
        .open(&Section::new(), &PortHub::new())
        .unwrap();
    let so2r = RemoteSo2rDrv::new(remote.clone())
        .open(&Section::new(), &PortHub::new())
        .unwrap();
    local.set_keyer(Some(keyer));
    local.set_so2r(Some(so2r));
    let events = s.tx.subscribe();
    local.send(TxCommand::Focus(2));
    local.send(TxCommand::Listen(Rx::Both));
    local.send(TxCommand::Send {
        id: zlog_ui::tx::ticket(),
        radio: 2,
        signal: Signal::Cw("CQ TEST".into()),
    });
    until("keyed on the lending station", || {
        s.sent.lock().unwrap().contains(&"CQ TEST".to_string())
    });
    until("finished here", || {
        local.tick();
        !local.busy()
    });
    let seen: Vec<_> = events.try_iter().collect();
    assert!(
        seen.iter()
            .any(|e| matches!(e, TxEvent::Started { radio: 2, .. })),
        "{seen:?}"
    );
    let boxed = s.boxed.lock().unwrap().clone();
    assert!(boxed.contains(&SimCommand::Tx(2)) && boxed.contains(&SimCommand::Rx(Rx::Both)));
}

#[test]
fn losing_the_client_stops_all_tx_on_the_lending_station() {
    let s = station();
    let remote = client_of(&s);
    let events = s.tx.subscribe();
    let c = remote.client().unwrap();
    c.send(&TxCommand::Send {
        id: zlog_ui::tx::ticket(),
        radio: 1,
        signal: Signal::Voice,
    })
    .unwrap();
    until("PTT on", || {
        s.tx.tick();
        s.tx.transmitting() == Some(1)
    });
    until("one client", || s.server.peers().len() == 1);
    client::disconnect(&remote);
    until("all TX stopped", || {
        s.tx.tick();
        s.tx.transmitting().is_none()
    });
    assert!(events.try_iter().any(|e| e == TxEvent::Stopped));
    assert!(remote.client().is_none());
}

#[test]
fn a_rig_or_keyer_lent_later_reaches_the_client_and_devices_reopen() {
    let rig = Exposed::default();
    let tx = TxHub::default();
    let token = Token::generate();
    let server = lend(&token, [rig.clone(), Exposed::default()], &tx);
    let before = zlog_ui::device::reopened();
    let remote = pinned(&server);
    client::connect(&remote, &server.addr().to_string(), "X", token.as_str()).unwrap();
    assert!(
        zlog_ui::device::reopened() > before,
        "devices chosen reopen on connect"
    );
    let rigs = RemoteRigs::new(remote.clone());
    assert_eq!(rigs.ports(), ["remote:1 RIG1"]);
    assert!(rigs.owns("remote:1 RIG1") && !rigs.owns("COM3"));
    assert!(remote.lock().listing.as_ref().unwrap().tx.is_empty());
    rig.set(Some(("IC-7300".into(), CAPS)));
    tx.set_keyer(Some(Box::new(SimKeyer::new(Default::default()))));
    let c = remote.client().unwrap();
    until("told of the change", || c.changed());
    let epoch = zlog_ui::device::reopened();
    assert!(client::relist(&remote));
    assert!(
        zlog_ui::device::reopened() > epoch,
        "devices reopen on a new listing"
    );
    assert_eq!(rigs.ports(), ["remote:1 RIG1 IC-7300"]);
    let mut opened = rigs.open("remote:1 RIG1 IC-7300").unwrap().unwrap();
    assert_eq!(opened.caps(), CAPS);
    opened.tick(Instant::now());
    assert_eq!(remote.lock().listing.as_ref().unwrap().tx, ["keyer"]);
}

#[test]
fn a_server_that_forgot_this_client_asks_to_pair_again() {
    let s = station();
    let remote = Arc::new(Remote::default());
    let addr = s.server.addr().to_string();
    let pair = |again: bool| {
        let (r, a) = (remote.clone(), addr.clone());
        let t = std::thread::spawn(move || match again {
            true => client::pair_again(&r, &a, "JA1ZLO"),
            false => client::connect(&r, &a, "JA1ZLO", ""),
        });
        let mut request = None;
        until("the server is asked", || {
            request = s.server.tls.requests().pop();
            request.is_some()
        });
        assert!(s.server.tls.decide(request.unwrap().id, true));
        t.join().unwrap().unwrap();
    };
    pair(false);
    client::disconnect(&remote);
    until("the slot is free", || s.server.peers().is_empty());
    for p in s.server.tls.peers().list() {
        s.server.tls.peers().remove(&p.id);
    }
    let e = client::connect(&remote, &addr, "JA1ZLO", "").unwrap_err();
    assert!(e.contains(&addr), "{e}");
    assert_eq!(
        remote.lock().repair.as_deref(),
        Some(addr.as_str()),
        "asked, not removed"
    );
    assert!(
        remote.pin(&addr).is_some(),
        "the pin is kept until the user agrees"
    );
    assert!(
        s.server.tls.requests().is_empty(),
        "never re-paired silently"
    );
    pair(true);
    assert!(remote.lock().repair.is_none());
    assert_eq!(s.server.tls.peers().list().len(), 1);
    assert!(remote.client().is_some());
}
