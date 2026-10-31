use std::net::SocketAddr;
use std::time::{Duration, Instant};

use zlog_core::{Band, Command, Log, Qso, QsoId};
use zlog_link::hub::{ClientId, Effect, Hub, Master, Note};
use zlog_link::protocol::{self, Refused};
use zlog_link::wire::qso_to_text;

fn qso(call: &str, id: i32) -> Qso {
    Qso {
        callsign: call.into(),
        band: Band::B14,
        reserve3: id,
        ..Qso::default()
    }
}

fn addr(n: u8) -> SocketAddr {
    format!("192.168.1.{n}:50000").parse().unwrap()
}

struct Net {
    hub: Hub,
    log: Log,
    now: Instant,
}

impl Net {
    fn new(qsos: Vec<Qso>) -> Self {
        let mut hub = Hub::default();
        hub.clock = "12:00".into();
        Net {
            hub,
            log: Log {
                qsos,
                ..Log::default()
            },
            now: Instant::now(),
        }
    }

    fn join(&mut self, id: ClientId, ip: u8) -> Vec<Effect> {
        let mut out = self.hub.join(id, addr(ip), false, self.now);
        let mut m = Master::new(&self.log);
        out.extend(self.hub.decide(id, true, &mut m, self.now));
        out
    }

    fn say(&mut self, id: ClientId, line: &str) -> Vec<Effect> {
        let mut m = Master::new(&self.log);
        self.hub.receive(id, line, &mut m, self.now)
    }
}

fn sent_to(effects: &[Effect], to: ClientId) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send(id, l) if *id == to => Some(l.clone()),
            _ => None,
        })
        .collect()
}

fn applied(effects: &[Effect]) -> Vec<Command> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Apply(c) => Some((**c).clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn delphi_connect_waits_for_approval_before_any_log_data() {
    let mut n = Net::new(vec![qso("JA1AAA", 100_000_100)]);
    n.join(1, 1);
    assert!(n.hub.join(2, addr(2), false, n.now).is_empty());
    let mut out = Vec::new();
    for l in [
        "#ZLOG# BAND 2",
        "#ZLOG# OPERATOR JA1ZLO",
        "#ZLOG# PCNAME PC2",
    ] {
        out.extend(n.say(2, l));
    }
    assert_eq!(
        out,
        [Effect::Ask(2)],
        "the PC name comes first, then approval"
    );
    assert!(n.say(2, "#ZLOG# BEGINMERGE").is_empty(), "held");
    assert!(
        n.say(2, "").is_empty(),
        "the empty line after BEGINMERGE is dropped"
    );
    assert!(n
        .say(1, "#ZLOG# PUTMESSAGE hi")
        .iter()
        .all(|e| !matches!(e, Effect::Send(2, _))));
    let mut m = Master::new(&n.log);
    let out = n.hub.decide(2, true, &mut m, n.now);
    let to2 = sent_to(&out, 2);
    assert_eq!(
        to2,
        [
            "#ZLOG# PUTMESSAGE 12:00 2   New client #2(192.168.1.2) connected to network.",
            "#ZLOG# PUTMESSAGE 12:00 2   client #2(192.168.1.2) set to 7 MHz.",
            "#ZLOG# BEGINMERGE-OK",
        ]
    );
    assert_eq!(sent_to(&out, 1).len(), 2, "notices go to everybody");
    let p = &n.hub.clients[&2];
    assert_eq!(
        (p.pc_name.as_str(), p.operator.as_str(), p.band),
        ("PC2", "JA1ZLO", Some(Band::B7))
    );
}

#[test]
fn a_refused_client_is_closed_and_a_silent_one_is_asked_later() {
    let mut n = Net::new(Vec::new());
    n.hub.join(1, addr(1), false, n.now);
    let mut m = Master::new(&n.log);
    assert_eq!(
        n.hub.decide(1, false, &mut m, n.now),
        [Effect::Note(1, Note::Denied), Effect::Close(1)]
    );
    n.hub.join(2, addr(2), false, n.now);
    assert!(n.hub.tick(n.now).is_empty());
    assert_eq!(n.hub.tick(n.now + Duration::from_secs(3)), [Effect::Ask(2)]);
    assert!(
        n.hub.tick(n.now + Duration::from_secs(4)).is_empty(),
        "asked once"
    );
}

#[test]
fn old_merge_trace_from_a_delphi_client() {
    let mut log: Vec<Qso> = (1..=25)
        .map(|i| qso("JA1AAA", 100_000_000 + i * 10_000))
        .collect();
    log[0].reserve3 += 1;
    let mut n = Net::new(log);
    n.join(1, 1);
    n.join(2, 2);
    assert_eq!(
        sent_to(&n.say(1, "#ZLOG# BEGINMERGE"), 1),
        ["#ZLOG# BEGINMERGE-OK"]
    );
    let ids = sent_to(&n.say(1, "#ZLOG# GETQSOIDS"), 1);
    assert_eq!(ids.len(), 3);
    assert!(ids[0].starts_with("#ZLOG# QSOIDS 100010001 100020000 "));
    assert!(ids[0].ends_with(" 100200000 "));
    assert_eq!(
        ids[1],
        "#ZLOG# QSOIDS 100210000 100220000 100230000 100240000 100250000 "
    );
    assert_eq!(ids[2], "#ZLOG# ENDQSOIDS");
    let mine = qso("JA2BBB", 200_000_100);
    let out = n.say(1, &protocol::put_log(&mine));
    assert_eq!(applied(&out), [Command::Insert(mine.clone())]);
    assert_eq!(
        sent_to(&out, 2),
        [protocol::put_log(&mine)],
        "relayed unchanged"
    );
    assert!(sent_to(&out, 1).is_empty(), "never echoed");
    let mut newer = n.log.qsos[1].clone();
    newer.reserve3 += 1;
    newer.callsign = "JA1AAB".into();
    let out = n.say(1, &protocol::edit_qso_to(&newer));
    assert_eq!(applied(&out), [Command::Update(QsoId(1_000_200), newer)]);
    let out = n.say(1, "#ZLOG# RENEW");
    assert_eq!(sent_to(&out, 2), ["#ZLOG# RENEW"]);
    assert!(n.hub.take_renew() && !n.hub.take_renew());
    let out = n.say(1, "#ZLOG# GETLOGQSOID 100010001 999999999 ");
    let got = sent_to(&out, 1);
    assert_eq!(got.len(), 2);
    assert!(got[0].starts_with("#ZLOG# PUTLOGEX ZLOGQSODATA:"));
    assert_eq!(got[1], "#ZLOG# RENEW");
    assert_eq!(sent_to(&n.say(1, "#ZLOG# SENDRENEW"), 1), ["#ZLOG# RENEW"]);
    assert_eq!(n.hub.merging(), Some(1));
    n.say(1, "#ZLOG# ENDMERGE");
    assert_eq!(n.hub.merging(), None);
}

#[test]
fn qso_changes_follow_zserver() {
    let anchor = qso("JA1AAA", 100_000_100);
    let mut n = Net::new(vec![anchor.clone()]);
    n.join(1, 1);
    n.join(2, 2);
    let out = n.say(1, &protocol::put_qso(&anchor));
    assert_eq!(out[0], Effect::Note(1, Note::Duplicate(anchor.reserve3)));
    assert!(applied(&out).is_empty());
    assert_eq!(sent_to(&out, 2).len(), 1, "zServer relays it anyway");
    let ghost = qso("JA9ZZZ", 900_000_100);
    let out = n.say(1, &protocol::edit_qso_to(&ghost));
    assert!(applied(&out).is_empty(), "unknown ID");
    assert_eq!(sent_to(&out, 2).len(), 1);
    let mut ins = qso("JA3CCC", 300_000_100);
    ins.reserve2 = anchor.reserve3;
    let out = n.say(2, &format!("#ZLOG# INSQSO {}", qso_to_text(&ins)));
    assert_eq!(
        applied(&out),
        [Command::InsertBefore(QsoId(1_000_001), ins)]
    );
    let mut m = Master::new(&n.log);
    let line = format!("#ZLOG# EXDELQSO {}", qso_to_text(&anchor));
    let out = n.hub.receive(2, &line, &mut m, n.now);
    assert_eq!(applied(&out), [Command::Delete(QsoId(1_000_001))]);
    let out = n.hub.receive(1, &protocol::del_qso(&anchor), &mut m, n.now);
    assert!(applied(&out).is_empty(), "already gone in this poll");
}

#[test]
fn sendlog_band_who_console_and_relays() {
    let mut n = Net::new(Vec::new());
    n.join(1, 1);
    n.join(2, 2);
    assert!(
        n.say(1, "#ZLOG# SENDLOG").is_empty(),
        "nothing for an empty log"
    );
    let out = n.say(1, "#ZLOG# BAND 4");
    assert_eq!(
        sent_to(&out, 2),
        ["#ZLOG# PUTMESSAGE 12:00 1   client #1(192.168.1.1) set to 14 MHz."]
    );
    let out = n.say(2, "#ZLOG# BAND 4");
    assert!(sent_to(&out, 2).contains(&"#ZLOG# PUTMESSAGE Band already in use!".to_string()));
    assert!(
        sent_to(&n.say(1, "#ZLOG# BAND 4"), 2).is_empty(),
        "announced once"
    );
    n.say(1, "#ZLOG# OPERATOR JA1ZLO");
    n.say(2, "#ZLOG# BAND 0");
    n.say(2, "#ZLOG# OPERATOR JH1XXX");
    assert_eq!(
        sent_to(&n.say(1, "#ZLOG# WHO"), 1),
        [
            "#ZLOG# PUTMESSAGE 1.9 MHz  JH1XXX",
            "#ZLOG# PUTMESSAGE 14 MHz   JA1ZLO"
        ]
    );
    let out = n.say(1, "#ZLOG# SPOT2 JA1ZLO 7025.0");
    assert_eq!(sent_to(&out, 2), ["#ZLOG# SPOT2 JA1ZLO 7025.0"]);
    let console = sent_to(&n.say(2, "#ZLOG# GETCONSOLE"), 2);
    assert!(
        console.iter().all(|l| l.starts_with("#ZLOG# PUTMESSAGE ")),
        "{console:?}"
    );
    let n2 = n.log.qsos.len();
    n.log.qsos.push(qso("JA1AAA", 100_000_100));
    let got = sent_to(&n.say(2, "#ZLOG# SENDLOG"), 2);
    assert_eq!(got.len(), n2 + 2);
    assert_eq!(got.last().unwrap(), "#ZLOG# RENEW");
    assert!(n.say(2, "#ZLOG# EDITQSOFROM x").is_empty());
}

#[test]
fn malformed_long_and_file_lines_are_refused_not_relayed() {
    let mut n = Net::new(Vec::new());
    n.join(1, 1);
    n.join(2, 2);
    let cases = [
        ("PUTQSO x", Refused::Header),
        ("#ZLOG# PUTQSO junk", Refused::Malformed),
        ("#ZLOG# GETLOGQSOID 1 x", Refused::Malformed),
        ("#ZLOG# GETFILE zlog.ini", Refused::File),
        ("#ZLOG# PUTFILE a,1,1", Refused::File),
        ("#ZLOG# FILEDATA QUJD", Refused::File),
        ("#ZLOG# PUTMESSAGE a\u{7}b", Refused::Control),
    ];
    for (line, why) in cases {
        assert_eq!(
            n.say(1, line),
            [Effect::Note(1, Note::Refused(why))],
            "{line}"
        );
    }
    let long = format!("#ZLOG# PUTMESSAGE {}", "x".repeat(300));
    let edge = format!("#ZLOG# PUTMESSAGE {}", "x".repeat(255));
    let out = n.say(1, &long);
    assert_eq!(out[0], Effect::Note(1, Note::Truncated));
    assert_eq!(
        sent_to(&out, 2),
        std::slice::from_ref(&edge),
        "cut like Delphi, then accepted"
    );
    assert_eq!(sent_to(&n.say(1, &edge), 2), [edge]);
    let mut q = qso("JA1AAA", 0);
    q.reserve3 = 0;
    assert_eq!(
        n.say(1, &protocol::put_qso(&q)),
        [Effect::Note(1, Note::Refused(Refused::Malformed))],
        "no ID"
    );
}

#[test]
fn the_merge_lock_queues_times_out_and_is_released_on_disconnect() {
    let mut n = Net::new(Vec::new());
    n.join(1, 1);
    n.join(2, 2);
    n.join(3, 3);
    n.say(1, "#ZLOG# BEGINMERGE");
    assert!(
        sent_to(&n.say(2, "#ZLOG# BEGINMERGE"), 2).is_empty(),
        "waits"
    );
    let out = n.say(1, "#ZLOG# ENDMERGE");
    assert_eq!(sent_to(&out, 2), ["#ZLOG# BEGINMERGE-OK"]);
    assert_eq!(n.hub.merging(), Some(2));
    n.say(3, "#ZLOG# BEGINMERGE");
    let later = n.now + Duration::from_secs(31);
    let out = n.hub.tick(later);
    assert!(out.contains(&Effect::Note(3, Note::MergeBusy)));
    assert_eq!(
        sent_to(&out, 3),
        ["#ZLOG# BEGINMERGE-NG"],
        "sent once, never a hang"
    );
    assert!(n.hub.tick(later).is_empty());
    let stalled = n.now + Duration::from_secs(61);
    let out = n.hub.tick(stalled);
    assert_eq!(
        &out[..2],
        [Effect::Note(2, Note::MergeTimeout), Effect::Close(2)]
    );
    assert_eq!(n.hub.merging(), None);
    n.say(1, "#ZLOG# BEGINMERGE");
    let out = n.hub.leave(1, n.now);
    assert_eq!(n.hub.merging(), None, "released when the holder leaves");
    assert!(sent_to(&out, 3)[0].ends_with("client #1(192.168.1.1) disconnected from network."));
}

#[test]
fn local_changes_wait_for_a_running_merge() {
    let mut n = Net::new(Vec::new());
    n.join(1, 1);
    n.join(2, 2);
    let line = protocol::put_qso(&qso("JA1ZLO", 100_000_100));
    assert_eq!(
        sent_to(&n.hub.local(line.clone()), 2),
        std::slice::from_ref(&line)
    );
    n.say(1, "#ZLOG# BEGINMERGE");
    assert!(n.hub.local(line.clone()).is_empty(), "queued");
    let out = n.say(1, "#ZLOG# ENDMERGE");
    assert_eq!(sent_to(&out, 1), std::slice::from_ref(&line));
    assert_eq!(sent_to(&out, 2), [line]);
}

#[test]
fn tx_number_clashes_are_reported() {
    let mut n = Net::new(Vec::new());
    n.hub.local_tx = Some(1);
    n.join(1, 1);
    n.join(2, 2);
    let out = n.say(1, &protocol::put_qso(&qso("JA1AAA", 100_010_000)));
    assert!(out.contains(&Effect::Note(
        1,
        Note::TxClash {
            tx: 1,
            other: "Z-Server".into()
        }
    )));
    let mut q = qso("JA2AAA", 200_010_000);
    q.pc_name = "PC2".into();
    n.say(2, "#ZLOG# PCNAME PC2");
    let out = n.say(2, &protocol::put_log(&q));
    assert!(!out
        .iter()
        .any(|e| matches!(e, Effect::Note(_, Note::TxClash { .. }))));
    n.join(3, 3);
    n.say(3, "#ZLOG# PCNAME PC3");
    let out = n.say(3, &protocol::put_qso(&qso("JA3AAA", 200_020_000)));
    assert!(out.contains(&Effect::Note(
        3,
        Note::TxClash {
            tx: 2,
            other: "#2 PC2".into()
        }
    )));
    let theirs = qso("JA9AAA", 300_030_000);
    let out = n.say(3, &protocol::put_log(&theirs));
    assert!(
        !out.iter()
            .any(|e| matches!(e, Effect::Note(_, Note::TxClash { .. }))),
        "not its own"
    );
}

#[test]
fn slots_keep_the_address_and_run_out_after_32() {
    let mut n = Net::new(Vec::new());
    n.join(1, 1);
    n.join(2, 2);
    n.hub.leave(1, n.now);
    n.join(3, 3);
    assert_eq!(n.hub.clients[&3].number, 3, "a never used slot first");
    n.join(4, 1);
    assert_eq!(n.hub.clients[&4].number, 1, "the old slot of that address");
    for id in 5..=33 {
        n.join(id, id as u8 + 10);
    }
    assert_eq!(n.hub.clients.len(), 32);
    let out = n.hub.join(99, addr(200), false, n.now);
    assert_eq!(out, [Effect::Note(99, Note::Full), Effect::Close(99)]);
}

#[test]
fn new_merge_commands_are_answered() {
    let a = qso("JA1AAA", 100_000_100);
    let b = qso("JA1BBB", 100_010_100);
    let mut n = Net::new(vec![a.clone(), b.clone()]);
    n.join(1, 1);
    n.say(1, "#ZLOG# NEWBEGINMERGE");
    let out = n.say(1, &format!("#ZLOG# CHECKQSOIDS {} 555500000 ", a.reserve3));
    assert_eq!(sent_to(&out, 1), ["#ZLOG# CHECKQSOIDS 555500000 "]);
    let out = n.say(1, "#ZLOG# GETUNREGQSOS");
    assert_eq!(
        sent_to(&out, 1),
        [protocol::put_log_ex(&b), "#ZLOG# NEWENDMERGE".into()]
    );
    let out = n.say(1, &format!("#ZLOG# EXGETQSOIDS {}", a.reserve3));
    assert_eq!(
        sent_to(&out, 1),
        [
            format!("#ZLOG# QSOIDS {} ", b.reserve3),
            "#ZLOG# ENDQSOIDS".into()
        ]
    );
    let out = n.say(1, &format!("#ZLOG# EXSENDLOG {}", b.reserve3));
    assert_eq!(sent_to(&out, 1), [protocol::put_log_ex(&a)]);
}

#[test]
fn racing_edits_keep_the_first_and_tell_the_late_sender() {
    let b = qso("JA2BBB", 200_000_100);
    let mut n = Net::new(vec![b.clone()]);
    n.join(1, 1);
    n.join(2, 2);
    let mut m = Master::new(&n.log);
    let (mut one, mut two) = (b.clone(), b.clone());
    one.reserve3 += 1;
    one.callsign = "JA2ONE".into();
    two.reserve3 += 1;
    two.callsign = "JA2TWO".into();
    let out = n
        .hub
        .receive(1, &protocol::edit_qso_to(&one), &mut m, n.now);
    assert_eq!(applied(&out).len(), 1);
    let out = n
        .hub
        .receive(2, &protocol::edit_qso_to(&two), &mut m, n.now);
    assert!(applied(&out).is_empty());
    assert_eq!(
        sent_to(&out, 2),
        [protocol::edit_qso_to(&one)],
        "the late sender gets the kept one"
    );
    assert!(sent_to(&out, 1).is_empty(), "not relayed");
    let mut three = one.clone();
    three.reserve3 += 1;
    let out = n
        .hub
        .receive(2, &protocol::edit_qso_to(&three), &mut m, n.now);
    assert_eq!(applied(&out).len(), 1, "a newer edit wins");
    let stale = b.clone();
    let out = n
        .hub
        .receive(1, &protocol::edit_qso_to(&stale), &mut m, n.now);
    assert!(applied(&out).is_empty(), "an older edit never overwrites");
}
