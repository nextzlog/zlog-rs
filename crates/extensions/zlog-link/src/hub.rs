use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use zlog_core::{Band, Command, Log, Qso, QsoId};

use crate::freq::FreqList;
use crate::protocol::{self, Refused, Request};
use crate::session::ChatLine;
use crate::wire::fill_right;

pub type ClientId = u64;

pub const SLOTS: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    Refused(Refused),
    Truncated,
    Held,
    Duplicate(i32),
    TxClash { tx: i32, other: String },
    MergeTimeout,
    MergeBusy,
    Full,
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Send(ClientId, String),
    Apply(Box<Command>),
    Close(ClientId),
    Ask(ClientId),
    Note(ClientId, Note),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Peer {
    pub number: usize,
    pub addr: SocketAddr,
    pub pc_name: String,
    pub operator: String,
    pub band: Option<Band>,
    pub approved: bool,
    pub tls: bool,
    pub since: Instant,
    asked: bool,
    held: Vec<String>,
    tx: Option<i32>,
    checked: HashSet<i32>,
    sent: HashSet<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub merge_idle: Duration,
    pub merge_wait: Duration,
    pub name_wait: Duration,
    pub max_held: usize,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            merge_idle: Duration::from_secs(60),
            merge_wait: Duration::from_secs(30),
            name_wait: Duration::from_secs(2),
            max_held: 64,
        }
    }
}

pub struct Master<'a> {
    pub log: &'a Log,
    ids: HashMap<i32, i32>,
    latest: HashMap<i32, Qso>,
}

impl<'a> Master<'a> {
    pub fn new(log: &'a Log) -> Self {
        Master {
            log,
            ids: log
                .qsos
                .iter()
                .map(|q| (q.reserve3 / 100, q.reserve3))
                .collect(),
            latest: HashMap::new(),
        }
    }

    pub fn has(&self, q: &Qso) -> bool {
        self.ids.contains_key(&(q.reserve3 / 100))
    }

    fn put(&mut self, q: &Qso) {
        self.ids.insert(q.reserve3 / 100, q.reserve3);
        self.latest.insert(q.reserve3 / 100, q.clone());
    }

    fn stored(&self, id: i32) -> Option<Qso> {
        self.latest
            .get(&id)
            .cloned()
            .or_else(|| self.find(id * 100).cloned())
    }

    fn remove(&mut self, q: &Qso) {
        self.ids.remove(&(q.reserve3 / 100));
        self.latest.remove(&(q.reserve3 / 100));
    }

    fn find(&self, full: i32) -> Option<&'a Qso> {
        self.log
            .qsos
            .iter()
            .find(|q| q.reserve3 / 100 == full / 100)
    }
}

const CONSOLE_LINES: usize = 1000;

#[derive(Default)]
pub struct Hub {
    pub clients: BTreeMap<ClientId, Peer>,
    pub chat: Vec<ChatLine>,
    pub console: VecDeque<String>,
    pub freqs: FreqList,
    pub timing: Timing,
    pub clock: String,
    pub local_tx: Option<i32>,
    remembered: HashMap<IpAddr, usize>,
    merge: Option<(ClientId, Instant)>,
    waiting: VecDeque<(ClientId, Instant)>,
    queued: Vec<String>,
    renew: bool,
    writer: HashMap<i32, Option<ClientId>>,
}

impl Hub {
    pub fn merging(&self) -> Option<ClientId> {
        self.merge.map(|(id, _)| id)
    }

    pub fn approved(&self) -> usize {
        self.clients.values().filter(|p| p.approved).count()
    }

    pub fn take_renew(&mut self) -> bool {
        std::mem::take(&mut self.renew)
    }

    fn log(&mut self, line: String) {
        self.console.push_back(line);
        while self.console.len() > CONSOLE_LINES {
            self.console.pop_front();
        }
    }

    fn to_all(&self, except: Option<ClientId>, line: &str) -> Vec<Effect> {
        self.clients
            .iter()
            .filter(|(id, p)| p.approved && Some(**id) != except)
            .map(|(id, _)| Effect::Send(*id, line.to_string()))
            .collect()
    }

    fn notice(&mut self, number: usize, text: &str) -> Vec<Effect> {
        let s = format!(
            "{} {} {text}",
            self.clock,
            fill_right(&number.to_string(), 3)
        );
        self.log(s.clone());
        self.chat.push(ChatLine {
            text: s.clone(),
            red: false,
        });
        self.to_all(None, &protocol::notice(&s))
    }

    fn slot(&mut self, ip: IpAddr) -> Option<usize> {
        let busy: HashSet<usize> = self.clients.values().map(|p| p.number).collect();
        let used: HashSet<usize> = self.remembered.values().copied().collect();
        let mine = self
            .remembered
            .get(&ip)
            .copied()
            .filter(|n| !busy.contains(n));
        let n = mine
            .or_else(|| (1..=SLOTS).find(|n| !busy.contains(n) && !used.contains(n)))
            .or_else(|| (1..=SLOTS).find(|n| !busy.contains(n)))?;
        self.remembered.insert(ip, n);
        Some(n)
    }

    pub fn join(&mut self, id: ClientId, addr: SocketAddr, tls: bool, now: Instant) -> Vec<Effect> {
        let Some(number) = self.slot(addr.ip()) else {
            return vec![Effect::Note(id, Note::Full), Effect::Close(id)];
        };
        self.clients.insert(
            id,
            Peer {
                number,
                addr,
                pc_name: String::new(),
                operator: String::new(),
                band: None,
                approved: false,
                tls,
                since: now,
                asked: false,
                held: Vec::new(),
                tx: None,
                checked: HashSet::new(),
                sent: HashSet::new(),
            },
        );
        Vec::new()
    }

    pub fn leave(&mut self, id: ClientId, now: Instant) -> Vec<Effect> {
        let Some(p) = self.clients.remove(&id) else {
            return Vec::new();
        };
        self.waiting.retain(|(w, _)| *w != id);
        self.freqs.forget(&p.pc_name);
        let mut out = Vec::new();
        if p.approved {
            let band = p.band.map(protocol::band_string).unwrap_or_default();
            let text = format!(
                "{band} client #{}({}) disconnected from network.",
                p.number,
                p.addr.ip()
            );
            out.extend(self.notice(p.number, text.trim_start()));
        }
        if self.merging() == Some(id) {
            out.extend(self.release(now));
        }
        out
    }

    pub fn decide(
        &mut self,
        id: ClientId,
        yes: bool,
        master: &mut Master<'_>,
        now: Instant,
    ) -> Vec<Effect> {
        let Some(p) = self.clients.get_mut(&id) else {
            return Vec::new();
        };
        if !yes {
            p.asked = true;
            return vec![Effect::Note(id, Note::Denied), Effect::Close(id)];
        }
        if p.approved {
            return Vec::new();
        }
        p.approved = true;
        let (number, ip, band) = (p.number, p.addr.ip(), p.band);
        let held = std::mem::take(&mut p.held);
        let mut out = vec![Effect::Note(id, Note::Approved)];
        out.extend(self.notice(
            number,
            &format!("New client #{number}({ip}) connected to network."),
        ));
        if let Some(b) = band {
            out.extend(self.band_set(id, b));
        }
        for line in held {
            out.extend(self.receive(id, &line, master, now));
        }
        out
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut out = Vec::new();
        for (id, p) in &mut self.clients {
            if !p.approved && !p.asked && now.duration_since(p.since) >= self.timing.name_wait {
                p.asked = true;
                out.push(Effect::Ask(*id));
            }
        }
        if let Some((holder, deadline)) = self.merge {
            if now >= deadline {
                out.push(Effect::Note(holder, Note::MergeTimeout));
                out.push(Effect::Close(holder));
                out.extend(self.release(now));
            }
        }
        while let Some(&(w, deadline)) = self.waiting.front() {
            if now < deadline {
                break;
            }
            self.waiting.pop_front();
            out.push(Effect::Note(w, Note::MergeBusy));
            out.push(Effect::Send(w, protocol::begin_merge_ng()));
        }
        out
    }

    pub fn local(&mut self, line: String) -> Vec<Effect> {
        if let Ok(Request::EditQsoTo(q)) = Request::parse(&line) {
            self.writer.insert(q.reserve3 / 100, None);
        }
        if self.merge.is_some() {
            self.queued.push(line);
            return Vec::new();
        }
        self.to_all(None, &line)
    }

    fn release(&mut self, now: Instant) -> Vec<Effect> {
        self.merge = None;
        if let Some((next, _)) = self.waiting.pop_front() {
            self.merge = Some((next, now + self.timing.merge_idle));
            return vec![Effect::Send(next, protocol::begin_merge_ok())];
        }
        let queued = std::mem::take(&mut self.queued);
        queued.iter().flat_map(|l| self.to_all(None, l)).collect()
    }

    fn band_set(&mut self, id: ClientId, b: Band) -> Vec<Effect> {
        let Some(p) = self.clients.get(&id) else {
            return Vec::new();
        };
        let text = format!(
            "client #{}({}) set to {}.",
            p.number,
            p.addr.ip(),
            protocol::band_string(b)
        );
        self.notice(p.number, &text)
    }

    fn tx_clash(&mut self, id: ClientId, q: &Qso, live: bool) -> Vec<Effect> {
        let Some(p) = self.clients.get(&id) else {
            return Vec::new();
        };
        let own = live || (!p.pc_name.is_empty() && q.pc_name == p.pc_name);
        if p.tx.is_some() || !own {
            return Vec::new();
        }
        let tx = q.reserve3 / 100_000_000;
        let other = if self.local_tx == Some(tx) {
            Some("Z-Server".to_string())
        } else {
            self.clients
                .iter()
                .find(|(o, c)| **o != id && c.tx == Some(tx))
                .map(|(_, c)| format!("#{} {}", c.number, c.pc_name))
        };
        if let Some(p) = self.clients.get_mut(&id) {
            p.tx = Some(tx);
        }
        match other {
            Some(other) => vec![Effect::Note(id, Note::TxClash { tx, other })],
            None => Vec::new(),
        }
    }

    pub fn receive(
        &mut self,
        id: ClientId,
        line: &str,
        master: &mut Master<'_>,
        now: Instant,
    ) -> Vec<Effect> {
        let clipped = protocol::clip(line);
        let mut notes = Vec::new();
        if clipped.is_some() {
            notes.push(Effect::Note(id, Note::Truncated));
        }
        let line = clipped.as_deref().unwrap_or(line);
        let request = match Request::parse(line) {
            Ok(r) => r,
            Err(Refused::Empty) => return notes,
            Err(e) => {
                notes.push(Effect::Note(id, Note::Refused(e)));
                return notes;
            }
        };
        let mut out = self.handle(id, line, request, master, now);
        notes.append(&mut out);
        notes
    }

    fn handle(
        &mut self,
        id: ClientId,
        line: &str,
        request: Request,
        master: &mut Master<'_>,
        now: Instant,
    ) -> Vec<Effect> {
        let Some(p) = self.clients.get_mut(&id) else {
            return Vec::new();
        };
        if !p.approved {
            match request {
                Request::Band(b) => p.band = band_of(&b).or(p.band),
                Request::Operator(o) => p.operator = o,
                Request::PcName(n) => {
                    p.pc_name = n;
                    if !p.asked {
                        p.asked = true;
                        return vec![Effect::Ask(id)];
                    }
                }
                _ if p.held.len() >= self.timing.max_held => {
                    return vec![Effect::Note(id, Note::Held)]
                }
                _ => p.held.push(line.to_string()),
            }
            return Vec::new();
        }
        if let Some((holder, deadline)) = &mut self.merge {
            if *holder == id {
                *deadline = now + self.timing.merge_idle;
            }
        }
        let relay = |hub: &Hub| hub.to_all(Some(id), line);
        let mut out = Vec::new();
        match request {
            Request::Freq(s) => {
                self.freqs.take(&s);
                out.extend(relay(self));
            }
            Request::Band(b) => {
                let Some(b) = band_of(&b) else {
                    return out;
                };
                let Some(p) = self.clients.get_mut(&id) else {
                    return out;
                };
                let first = p.band.is_none();
                p.band = Some(b);
                let taken = self
                    .clients
                    .iter()
                    .any(|(o, c)| *o != id && c.approved && c.band == Some(b));
                if taken {
                    out.push(Effect::Send(id, protocol::notice("Band already in use!")));
                }
                if first {
                    out.extend(self.band_set(id, b));
                }
            }
            Request::Operator(o) => {
                if let Some(p) = self.clients.get_mut(&id) {
                    p.operator = o;
                }
            }
            Request::PcName(n) => {
                if let Some(p) = self.clients.get_mut(&id) {
                    p.pc_name = n;
                }
            }
            Request::Chat(text) => {
                let (text, red) = match text.strip_prefix('!') {
                    Some(t) => (t.to_string(), true),
                    None => (text, false),
                };
                self.log(text.clone());
                self.chat.push(ChatLine { text, red });
                out.extend(relay(self));
            }
            Request::PutQso(q) | Request::PutLog(q) => {
                let live = line.as_bytes().get(7..14) == Some(b"PUTQSO ");
                out.extend(self.tx_clash(id, &q, live));
                if let Some(p) = self.clients.get_mut(&id) {
                    p.sent.insert(q.reserve3 / 100);
                }
                if master.has(&q) {
                    out.push(Effect::Note(id, Note::Duplicate(q.reserve3)));
                } else {
                    master.put(&q);
                    out.push(Effect::Apply(Box::new(Command::Insert(q))));
                }
                out.extend(relay(self));
            }
            Request::EditQsoTo(q) => {
                let key = q.reserve3 / 100;
                let Some(&stored) = master.ids.get(&key) else {
                    out.extend(relay(self));
                    return out;
                };
                let other = self.writer.get(&key).is_some_and(|w| *w != Some(id));
                if q.reserve3 < stored || (q.reserve3 == stored && other) {
                    if let Some(s) = master.stored(key).filter(|s| *s != q) {
                        out.push(Effect::Send(id, protocol::edit_qso_to(&s)));
                    }
                    return out;
                }
                self.writer.insert(key, Some(id));
                master.put(&q);
                out.push(Effect::Apply(Box::new(Command::Update(QsoId::of(&q), q))));
                out.extend(relay(self));
            }
            Request::DelQso(q) => {
                if master.has(&q) {
                    master.remove(&q);
                    out.push(Effect::Apply(Box::new(Command::Delete(QsoId::of(&q)))));
                }
                out.extend(relay(self));
            }
            Request::InsQso(q) => {
                if !master.has(&q) {
                    master.put(&q);
                    let before = QsoId(q.reserve2 / 100);
                    out.push(Effect::Apply(Box::new(Command::InsertBefore(before, q))));
                }
                out.extend(relay(self));
            }
            Request::Renew => {
                self.renew = true;
                out.extend(relay(self));
            }
            Request::SendRenew => out.push(Effect::Send(id, protocol::renew_after_log())),
            Request::SendLog => {
                let qsos = &master.log.qsos;
                out.extend(qsos.iter().map(|q| Effect::Send(id, protocol::put_log(q))));
                if !qsos.is_empty() {
                    out.push(Effect::Send(id, protocol::renew_after_log()));
                }
            }
            Request::GetQsoIds => {
                let ids: Vec<i32> = master.log.qsos.iter().map(|q| q.reserve3).collect();
                out.extend(
                    protocol::qso_ids(&ids)
                        .into_iter()
                        .map(|l| Effect::Send(id, l)),
                );
            }
            Request::GetLogQsoId(list) => {
                for full in list {
                    if let Some(q) = master.find(full) {
                        out.push(Effect::Send(id, protocol::put_log_ex(q)));
                    }
                }
                out.push(Effect::Send(id, protocol::renew_after_log()));
            }
            Request::BeginMerge => match self.merge {
                None => {
                    self.merge = Some((id, now + self.timing.merge_idle));
                    out.push(Effect::Send(id, protocol::begin_merge_ok()));
                }
                Some((holder, _)) if holder == id => {
                    out.push(Effect::Send(id, protocol::begin_merge_ok()))
                }
                Some(_) => {
                    if !self.waiting.iter().any(|(w, _)| *w == id) {
                        self.waiting.push_back((id, now + self.timing.merge_wait));
                    }
                }
            },
            Request::EndMerge => {
                if self.merging() == Some(id) {
                    out.extend(self.release(now));
                }
            }
            Request::ExGetQsoIds(list) => {
                let known: HashSet<i32> = list.iter().map(|f| f / 100).collect();
                let ids: Vec<i32> = master
                    .log
                    .qsos
                    .iter()
                    .filter(|q| !known.contains(&(q.reserve3 / 100)))
                    .map(|q| q.reserve3)
                    .collect();
                out.extend(
                    protocol::qso_ids(&ids)
                        .into_iter()
                        .map(|l| Effect::Send(id, l)),
                );
            }
            Request::ExSendLog(list) => {
                let known: HashSet<i32> = list.iter().map(|f| f / 100).collect();
                for q in &master.log.qsos {
                    if !known.contains(&(q.reserve3 / 100)) {
                        out.push(Effect::Send(id, protocol::put_log_ex(q)));
                    }
                }
            }
            Request::NewBeginMerge => {
                if let Some(p) = self.clients.get_mut(&id) {
                    p.checked.clear();
                    p.sent.clear();
                }
            }
            Request::CheckQsoIds(list) => {
                let Some(p) = self.clients.get_mut(&id) else {
                    return out;
                };
                let mut lacking = Vec::new();
                for full in list {
                    if master.ids.contains_key(&(full / 100)) {
                        p.checked.insert(full / 100);
                    } else {
                        lacking.push(full);
                    }
                }
                out.push(Effect::Send(id, protocol::check_qso_ids(&lacking)));
            }
            Request::GetUnregQsos => {
                let Some(p) = self.clients.get(&id) else {
                    return out;
                };
                for q in &master.log.qsos {
                    let i = q.reserve3 / 100;
                    if !p.checked.contains(&i) && !p.sent.contains(&i) {
                        out.push(Effect::Send(id, protocol::put_log_ex(q)));
                    }
                }
                out.push(Effect::Send(id, protocol::new_end_merge()));
            }
            Request::Who => {
                let mut bands: Vec<(Band, String)> = self
                    .clients
                    .values()
                    .filter(|c| c.approved)
                    .filter_map(|c| c.band.map(|b| (b, c.operator.clone())))
                    .collect();
                bands.sort_by_key(|(b, _)| *b);
                bands.dedup_by_key(|(b, _)| *b);
                for (b, op) in bands {
                    let text = format!("{}{op}", fill_right(&protocol::band_string(b), 9));
                    out.push(Effect::Send(id, protocol::notice(&text)));
                }
            }
            Request::GetConsole => {
                let skip = self.console.len().saturating_sub(100);
                for l in self.console.iter().skip(skip) {
                    out.push(Effect::Send(id, protocol::notice(l)));
                }
            }
            Request::Ignored => {}
            Request::Relay => out.extend(relay(self)),
        }
        out
    }
}

fn band_of(s: &str) -> Option<Band> {
    let n: u8 = s.trim().parse().ok()?;
    Band::from_byte(n).filter(|b| *b <= Band::B10G || *b == Band::Unknown)
}
