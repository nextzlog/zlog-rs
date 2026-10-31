use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use zlog_core::{
    Band, Book, Command, Contest, ContestMode, Extension, Link, Log, Mismatch, Qso, QsoId,
    QsoScore, Registry, ScoreSheet, Scoring, Station,
};
use zlog_link::protocol::Message;
use zlog_link::session::{NetEvent, OnConnect, Transport};
use zlog_link::wire::{decode, encode, qso_to_text};
use zlog_link::{LogLink, Session};
use zlog_undo::{Refusal, Shared, Undo, UndoError};

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
struct Wire(Arc<Mutex<(Vec<NetEvent>, Vec<u8>)>>);

impl Wire {
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

impl Transport for Wire {
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

struct Peer {
    book: Book,
    history: Shared,
    wire: Wire,
    session: Arc<Mutex<Session>>,
    unread: Vec<usize>,
    logged: u32,
}

impl Peer {
    fn new() -> Self {
        let session = Arc::new(Mutex::new(Session::default()));
        let wire = Wire::default();
        {
            let mut s = session.lock().unwrap();
            s.settings.on_connect = OnConnect::ConnectOnly;
            s.connect(Box::new(wire.clone()));
        }
        let history = Shared::default();
        let registry = Registry::new()
            .with(Package(Some(LogLink::new(session.clone()))))
            .with(Undo(history.clone()));
        let (mut book, _) = Book::open(Log::default(), Box::new(Nothing), "nothing", registry);
        wire.0
            .lock()
            .unwrap()
            .0
            .push(NetEvent::Connected("server".into()));
        book.poll().unwrap();
        wire.heard();
        Peer {
            book,
            history,
            wire,
            session,
            unread: Vec::new(),
            logged: 0,
        }
    }

    fn get(&self, id: QsoId) -> Option<Qso> {
        self.book.find(id).map(|i| self.book.log().qsos[i].clone())
    }

    fn by_id(&self) -> BTreeMap<i32, Qso> {
        self.book
            .log()
            .qsos
            .iter()
            .map(|q| (q.reserve3 / 100, q.clone()))
            .collect()
    }
}

struct Flight {
    from: usize,
    id: i32,
    pending: HashSet<usize>,
}

struct Net {
    peers: Vec<Peer>,
    up: Vec<VecDeque<(String, usize)>>,
    down: Vec<VecDeque<(String, usize)>>,
    store: Vec<Qso>,
    flights: Vec<Option<Flight>>,
    conflicted: HashSet<i32>,
}

fn is_qso_command(m: &Message) -> Option<&Qso> {
    match m {
        Message::PutQso(q)
        | Message::EditQsoTo(q)
        | Message::DelQso(q)
        | Message::PutLog(q)
        | Message::PutLogEx(q) => Some(q),
        _ => None,
    }
}

impl Net {
    fn new(n: usize) -> Self {
        Net {
            peers: (0..n).map(|_| Peer::new()).collect(),
            up: vec![VecDeque::new(); n],
            down: vec![VecDeque::new(); n],
            store: Vec::new(),
            flights: Vec::new(),
            conflicted: HashSet::new(),
        }
    }

    fn fresh(&mut self, p: usize) -> Qso {
        let peer = &mut self.peers[p];
        peer.logged += 1;
        Qso {
            callsign: format!("JA{p}X{}", peer.logged),
            tx: p as u8 + 1,
            time: zlog_core::DelphiTime(46_000.0 + peer.logged as f64 / 1000.0),
            ..Qso::default()
        }
    }

    fn collect(&mut self, p: usize) -> usize {
        let mut n = 0;
        for line in self.peers[p].wire.heard() {
            let Some(m) = Message::parse(&line) else {
                continue;
            };
            let number = self.flights.len();
            if let Some(q) = is_qso_command(&m) {
                n += 1;
                let id = q.reserve3 / 100;
                let others = self.flights.iter().flatten();
                if others.into_iter().any(|f| f.id == id && f.from != p) {
                    self.conflicted.insert(id);
                }
                let pending = (0..self.peers.len()).filter(|&j| j != p).collect();
                self.flights.push(Some(Flight {
                    from: p,
                    id,
                    pending,
                }));
            } else {
                self.flights.push(None);
            }
            self.up[p].push_back((line, number));
        }
        n
    }

    fn relay(&mut self, p: usize) -> bool {
        let Some((line, number)) = self.up[p].pop_front() else {
            return false;
        };
        let m = Message::parse(&line).unwrap();
        let find = |store: &Vec<Qso>, q: &Qso| {
            store
                .iter()
                .position(|s| s.reserve3 / 100 == q.reserve3 / 100)
        };
        let others = |net: &mut Net, line: &str| {
            for j in 0..net.peers.len() {
                if j != p {
                    net.down[j].push_back((line.to_string(), number));
                }
            }
        };
        match &m {
            Message::PutQso(q) | Message::PutLog(q) => {
                if find(&self.store, q).is_none() {
                    self.store.push(q.clone());
                }
                others(self, &line);
            }
            Message::EditQsoTo(q) => {
                if let Some(i) = find(&self.store, q) {
                    self.store[i] = q.clone();
                }
                others(self, &line);
            }
            Message::DelQso(q) => {
                if let Some(i) = find(&self.store, q) {
                    self.store.remove(i);
                }
                others(self, &line);
            }
            Message::Other(t) if t.starts_with("BEGINMERGE") => {
                self.reply(p, "#ZLOG# BEGINMERGE-OK");
            }
            Message::Other(t) if t.starts_with("GETQSOIDS") => {
                let ids: String = self
                    .store
                    .iter()
                    .map(|q| format!("{} ", q.reserve3))
                    .collect();
                self.reply(p, &format!("#ZLOG# QSOIDS {ids}"));
                self.reply(p, "#ZLOG# ENDQSOIDS");
            }
            Message::Other(t) if t.starts_with("GETLOGQSOID") => {
                for full in t.split_whitespace().skip(1) {
                    let full: i32 = full.parse().unwrap();
                    if let Some(q) = self.store.iter().find(|q| q.reserve3 / 100 == full / 100) {
                        let line = format!("#ZLOG# PUTLOGEX {}", qso_to_text(q));
                        self.reply(p, &line);
                    }
                }
            }
            Message::Other(t) if t.starts_with("SENDRENEW") => self.reply(p, "#ZLOG# RENEW"),
            _ => {}
        }
        true
    }

    fn reply(&mut self, p: usize, line: &str) {
        let number = self.flights.len();
        self.flights.push(None);
        self.down[p].push_back((line.to_string(), number));
    }

    fn deliver(&mut self, p: usize, n: usize) {
        for _ in 0..n {
            let Some((line, number)) = self.down[p].pop_front() else {
                return;
            };
            self.peers[p].wire.say(&line);
            self.peers[p].unread.push(number);
        }
    }

    fn poll(&mut self, p: usize) {
        self.peers[p].book.poll().unwrap();
        for number in std::mem::take(&mut self.peers[p].unread) {
            if let Some(f) = &mut self.flights[number] {
                f.pending.remove(&p);
                if f.pending.is_empty() {
                    self.flights[number] = None;
                }
            }
        }
        assert_eq!(self.collect(p), 0, "echo from station {p}");
    }

    fn settle(&mut self) {
        loop {
            let mut moved = false;
            for p in 0..self.peers.len() {
                while self.relay(p) {
                    moved = true;
                }
            }
            for p in 0..self.peers.len() {
                if !self.down[p].is_empty() {
                    let n = self.down[p].len();
                    self.deliver(p, n);
                    moved = true;
                }
                self.poll(p);
            }
            if !moved && self.up.iter().all(|u| u.is_empty()) {
                break;
            }
        }
    }

    fn differing(&self) -> Vec<i32> {
        let logs: Vec<_> = self.peers.iter().map(Peer::by_id).collect();
        let ids: HashSet<i32> = logs.iter().flat_map(|l| l.keys().copied()).collect();
        let mut out: Vec<i32> = ids
            .into_iter()
            .filter(|id| logs.iter().any(|l| l.get(id) != logs[0].get(id)))
            .collect();
        out.sort();
        out
    }

    fn assert_converged(&self) {
        assert_eq!(self.differing(), Vec::<i32>::new());
        let server: BTreeMap<i32, Qso> = self
            .store
            .iter()
            .map(|q| (q.reserve3 / 100, q.clone()))
            .collect();
        assert_eq!(server, self.peers[0].by_id(), "server log");
    }

    fn execute(&mut self, p: usize, c: Command) {
        self.peers[p].book.execute(c).unwrap();
        self.collect(p);
    }

    fn undo(&mut self, p: usize, redo: bool) -> Result<Vec<String>, UndoError> {
        let peer = &mut self.peers[p];
        let r = zlog_undo::undo(&peer.history, &mut peer.book, redo);
        self.collect(p);
        r
    }
}

fn edited(q: &Qso, memo: &str) -> Qso {
    Qso {
        memo: memo.into(),
        ..q.clone()
    }
}

fn first(net: &Net, p: usize) -> Qso {
    net.peers[p].book.log().qsos[0].clone()
}

fn sent(net: &Net, p: usize) -> Vec<String> {
    net.up[p].iter().map(|(l, _)| l.clone()).collect()
}

fn heard_kinds(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .map(|l| l.trim_start_matches("#ZLOG# ").split(' ').next().unwrap())
        .collect()
}

#[test]
fn undone_insert_goes_out_as_a_delete() {
    let mut net = Net::new(2);
    let q = net.fresh(0);
    net.execute(0, Command::Insert(q));
    net.settle();
    assert_eq!(net.peers[1].book.log().qsos.len(), 1);
    net.undo(0, false).unwrap();
    assert_eq!(heard_kinds(&sent(&net, 0)), ["DELQSO"]);
    net.settle();
    assert!(net.peers[1].book.log().qsos.is_empty());
    net.undo(0, true).unwrap();
    assert_eq!(heard_kinds(&sent(&net, 0)), ["PUTQSO"]);
    net.settle();
    net.assert_converged();
    assert_eq!(first(&net, 1).reserve3 % 100, 1);
}

#[test]
fn undo_of_a_qso_another_station_edited_is_refused() {
    let mut net = Net::new(2);
    let q = net.fresh(0);
    net.execute(0, Command::Insert(q));
    net.settle();
    let mine = first(&net, 0);
    net.execute(0, Command::Update(QsoId::of(&mine), edited(&mine, "A")));
    net.settle();
    let theirs = first(&net, 1);
    net.execute(1, Command::Update(QsoId::of(&theirs), edited(&theirs, "B")));
    net.settle();
    let log = net.peers[0].book.log().qsos.clone();
    let r = net.undo(0, false);
    assert_eq!(
        r,
        Err(UndoError::Refused(Refusal::Changed(Mismatch::Edited(
            QsoId::of(&mine)
        ))))
    );
    assert_eq!(net.peers[0].book.log().qsos, log);
    assert!(net.up[0].is_empty(), "nothing sent");
    assert!(net.peers[0].history.lock().unwrap().skip(false));
    assert!(net.undo(0, false).is_err());
    net.settle();
    net.assert_converged();
    assert_eq!(first(&net, 1).memo, "B");
}

#[test]
fn undone_delete_brings_the_id_back_and_merges_agree() {
    let mut net = Net::new(2);
    let q = net.fresh(0);
    net.execute(0, Command::Insert(q));
    net.settle();
    let q = first(&net, 0);
    net.execute(0, Command::Delete(QsoId::of(&q)));
    net.settle();
    assert!(net.peers[1].book.log().qsos.is_empty());
    net.undo(0, false).unwrap();
    net.settle();
    net.assert_converged();
    let back = first(&net, 1);
    assert_eq!(QsoId::of(&back), QsoId::of(&q));
    assert_eq!(back.reserve3, q.reserve3 + 1);

    let peer = &net.peers[1];
    peer.session.lock().unwrap().settings.on_connect = OnConnect::Merge;
    reconnect(&mut net, 1);
    let heard = sent(&net, 1);
    assert!(
        !heard
            .iter()
            .any(|l| l.contains("PUTLOG") || l.contains("EDITQSOTO")),
        "{heard:?}"
    );
    net.settle();
    net.assert_converged();
}

fn reconnect(net: &mut Net, p: usize) {
    let peer = &mut net.peers[p];
    peer.session
        .lock()
        .unwrap()
        .connect(Box::new(peer.wire.clone()));
    peer.wire
        .0
        .lock()
        .unwrap()
        .0
        .push(NetEvent::Connected("server".into()));
    net.poll(p);
    for _ in 0..4 {
        net.collect(p);
        while net.relay(p) {}
        let n = net.down[p].len();
        net.deliver(p, n);
        net.peers[p].book.poll().unwrap();
        net.peers[p].unread.clear();
    }
}

#[test]
fn remote_changes_between_an_edit_and_its_undo() {
    let mut net = Net::new(3);
    for p in 0..3 {
        let q = net.fresh(p);
        net.execute(p, Command::Insert(q));
    }
    net.settle();
    let mine = net.peers[0].book.log().qsos[0].clone();
    net.execute(0, Command::Update(QsoId::of(&mine), edited(&mine, "A")));
    let q = net.fresh(1);
    net.execute(1, Command::Insert(q));
    let own = net.peers[1].book.log().qsos.last().unwrap().clone();
    net.execute(1, Command::Delete(QsoId::of(&own)));
    let log = &net.peers[1].book.log().qsos;
    let other = log.iter().find(|q| q.tx == 3).unwrap().clone();
    net.execute(1, Command::Update(QsoId::of(&other), edited(&other, "B")));
    net.settle();
    net.undo(0, false).unwrap();
    net.settle();
    net.assert_converged();
    let here = net.peers[2].get(QsoId::of(&mine)).unwrap();
    assert_eq!(here.memo, "");
    assert_eq!(here.reserve3 % 100, 2);
    net.undo(0, true).unwrap();
    net.settle();
    net.assert_converged();
    assert_eq!(net.peers[2].get(QsoId::of(&mine)).unwrap().memo, "A");
    assert_eq!(net.peers[2].get(QsoId::of(&other)).unwrap().memo, "B");
}

#[test]
fn lines_that_arrive_while_an_undo_runs_are_read_after_it() {
    let mut net = Net::new(2);
    let q = net.fresh(0);
    net.execute(0, Command::Insert(q));
    net.settle();
    let mine = first(&net, 0);
    let id = QsoId::of(&mine);
    net.execute(0, Command::Update(id, edited(&mine, "A")));
    let theirs = first(&net, 1);
    net.execute(1, Command::Update(id, edited(&theirs, "B")));
    net.relay(1);
    net.deliver(0, 1);
    net.undo(0, false).unwrap();
    net.poll(0);
    net.settle();
    assert!(net.conflicted.contains(&id.0));
    assert_eq!(
        net.peers[0].get(id).unwrap().memo,
        "B",
        "station 1's edit read last"
    );
    assert_eq!(
        net.peers[1].get(id).unwrap().memo,
        "",
        "station 0's undo read last"
    );
}

#[test]
fn concurrent_edits_differ_without_undo_too() {
    let mut net = Net::new(2);
    let q = net.fresh(0);
    net.execute(0, Command::Insert(q));
    net.settle();
    let id = QsoId::of(&first(&net, 0));
    let (a, b) = (first(&net, 0), first(&net, 1));
    net.execute(0, Command::Update(id, edited(&a, "A")));
    net.execute(1, Command::Update(id, edited(&b, "B")));
    net.settle();
    assert_eq!(net.differing(), vec![id.0]);
    assert_eq!(net.peers[0].get(id).unwrap().memo, "B");
    assert_eq!(net.peers[1].get(id).unwrap().memo, "A");
}

#[test]
fn offline_undo_of_an_edit_is_merged_on_reconnect() {
    let mut net = Net::new(2);
    let q = net.fresh(0);
    net.execute(0, Command::Insert(q));
    let q = net.fresh(0);
    net.execute(0, Command::Insert(q));
    net.settle();
    let edit = net.peers[0].book.log().qsos[0].clone();
    net.execute(0, Command::Update(QsoId::of(&edit), edited(&edit, "A")));
    net.settle();
    net.peers[0].session.lock().unwrap().disconnect();
    net.undo(0, false).unwrap();
    assert!(net.up[0].is_empty(), "offline: nothing sent");
    net.peers[0].session.lock().unwrap().settings.on_connect = OnConnect::Merge;
    reconnect(&mut net, 0);
    net.settle();
    net.assert_converged();
    assert_eq!(net.peers[1].get(QsoId::of(&edit)).unwrap().memo, "");
    assert_eq!(
        net.peers[0].history.lock().unwrap().steps(),
        (0, 0),
        "steps forgotten after the merge"
    );
}

#[test]
fn offline_undo_of_an_insert_returns_on_merge_as_a_plain_delete_does() {
    for by_undo in [true, false] {
        let mut net = Net::new(2);
        let q = net.fresh(0);
        net.execute(0, Command::Insert(q));
        net.settle();
        let q = first(&net, 0);
        net.peers[0].session.lock().unwrap().disconnect();
        if by_undo {
            net.undo(0, false).unwrap();
        } else {
            net.execute(0, Command::Delete(QsoId::of(&q)));
        }
        assert!(net.peers[0].book.log().qsos.is_empty());
        net.peers[0].session.lock().unwrap().settings.on_connect = OnConnect::Merge;
        reconnect(&mut net, 0);
        net.settle();
        net.assert_converged();
        assert_eq!(
            net.peers[0].book.log().qsos.len(),
            1,
            "back from the server"
        );
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn same(a: &Qso, b: &Qso) -> bool {
    Qso {
        reserve3: a.reserve3 / 100,
        ..a.clone()
    } == Qso {
        reserve3: b.reserve3 / 100,
        ..b.clone()
    }
}

fn checked_step(net: &mut Net, p: usize, redo: bool, skip: bool) {
    let history = net.peers[p].history.clone();
    let step = history.lock().unwrap().next(redo).cloned();
    let before = net.peers[p].by_id();
    let steps = history.lock().unwrap().steps();
    if skip {
        let had = history.lock().unwrap().skip(redo);
        assert_eq!(had, step.is_some());
        assert_eq!(net.peers[p].by_id(), before, "a skip changes nothing");
        assert_eq!(net.collect(p), 0);
        return;
    }
    match net.undo(p, redo) {
        Err(UndoError::Refused(_)) => {
            assert_eq!(net.peers[p].by_id(), before, "refused: log unchanged");
            assert_eq!(history.lock().unwrap().steps(), steps, "refused: step kept");
            assert!(net.up[p].is_empty() || net.collect(p) == 0);
        }
        Err(e) => panic!("{e}"),
        Ok(_) => {
            let step = step.unwrap();
            let after = net.peers[p].by_id();
            let touched: HashSet<i32> = step.changes.iter().map(|c| c.id.0).collect();
            for c in &step.changes {
                let now = after.get(&c.id.0);
                match (&c.before, now) {
                    (None, None) => {}
                    (Some(b), Some(n)) => {
                        assert!(same(b, n), "restored exactly");
                        let was = before.get(&c.id.0).map_or(b.reserve3, |q| q.reserve3);
                        assert!(n.reserve3 > was || n.reserve3 % 100 == 99);
                    }
                    (b, n) => panic!("{:?}: wanted {b:?}, has {n:?}", c.id),
                }
            }
            for (id, q) in &before {
                if !touched.contains(id) {
                    assert_eq!(after.get(id), Some(q), "untouched QSOs stay");
                }
            }
        }
    }
}

#[derive(Default, Debug)]
struct Tally {
    undone: usize,
    refused: usize,
    conflicted: usize,
    diverged: usize,
}

fn random_run(seed: u64, undo: bool) -> Tally {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let n = 2 + (seed % 2) as usize;
    let mut net = Net::new(n);
    let mut tally = Tally::default();
    for round in 0..120 {
        let p = rng.below(n);
        match rng.below(100) {
            0..=39 => {
                let len = net.peers[p].book.log().qsos.len();
                let pick = (len > 0).then(|| net.peers[p].book.log().qsos[rng.below(len)].clone());
                let kind = rng.below(100);
                let kind = if undo || kind < 60 { kind } else { 50 };
                match (kind, pick) {
                    (0..=59, None) | (0..=29, _) => {
                        let q = net.fresh(p);
                        net.execute(p, Command::Insert(q));
                    }
                    (30..=44, Some(q)) => net.execute(p, Command::Delete(QsoId::of(&q))),
                    (45..=59, Some(q)) => {
                        let memo = format!("{p}.{round}");
                        net.execute(p, Command::Update(QsoId::of(&q), edited(&q, &memo)));
                    }
                    (60..=79, _) => {
                        let steps = |net: &Net| net.peers[p].history.lock().unwrap().steps();
                        let (u, _) = steps(&net);
                        checked_step(&mut net, p, false, false);
                        if steps(&net).0 < u {
                            tally.undone += 1;
                        } else if u > 0 {
                            tally.refused += 1;
                        }
                    }
                    (80..=91, _) => checked_step(&mut net, p, true, false),
                    _ => {
                        let redo = rng.below(2) == 0;
                        checked_step(&mut net, p, redo, true);
                    }
                }
            }
            40..=69 => {
                net.relay(p);
            }
            70..=89 => {
                let k = 1 + rng.below(3);
                net.deliver(p, k);
            }
            _ => net.poll(p),
        }
        for peer in &net.peers {
            let ids: HashSet<i32> = peer
                .book
                .log()
                .qsos
                .iter()
                .map(|q| q.reserve3 / 100)
                .collect();
            assert_eq!(
                ids.len(),
                peer.book.log().qsos.len(),
                "duplicate ID, seed {seed}"
            );
        }
    }
    net.settle();
    let differing = net.differing();
    for id in &differing {
        assert!(
            net.conflicted.contains(id),
            "seed {seed}: QSO {id} differs without concurrent edits"
        );
    }
    tally.conflicted = net.conflicted.len();
    tally.diverged = differing.len();
    tally
}

const REGRESSIONS: &[u64] = &[];

#[test]
fn random_interleavings_converge() {
    let runs: u64 = std::env::var("UNDO_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let mut with = Tally::default();
    let mut without = Tally::default();
    for seed in REGRESSIONS.iter().copied().chain(1..=runs) {
        let t = random_run(seed, true);
        with.undone += t.undone;
        with.refused += t.refused;
        with.conflicted += t.conflicted;
        with.diverged += t.diverged;
        let t = random_run(seed, false);
        without.conflicted += t.conflicted;
        without.diverged += t.diverged;
    }
    eprintln!("{runs} runs with undo: {with:?}");
    eprintln!("{runs} runs without undo: {without:?}");
    assert!(with.undone > runs as usize, "undos ran");
    assert!(with.refused > 0, "refusals happened");
    assert!(without.diverged > 0, "the race is there without undo");
}
