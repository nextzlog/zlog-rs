use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zlog_core::{Command, Context, Link, LogEvent, Observer, Origin};

use crate::hub::{Effect, Master, Note};
use crate::net::{Ask, Event, Net, Shared};
use crate::protocol::{self, Request};
use crate::session::Session;
use crate::wire;

pub struct ServerLink {
    net: Arc<Shared>,
    session: Arc<Mutex<Session>>,
    inbox: VecDeque<Event>,
}

impl ServerLink {
    pub fn new(net: Arc<Shared>, session: Arc<Mutex<Session>>) -> Self {
        ServerLink {
            net,
            session,
            inbox: VecDeque::new(),
        }
    }
}

fn clock() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

fn run(
    n: &mut Net,
    effects: Vec<Effect>,
    commands: &mut Vec<Command>,
    master: &mut Master<'_>,
    now: Instant,
) -> bool {
    let mut sent = false;
    let mut todo: VecDeque<Effect> = effects.into();
    while let Some(e) = todo.pop_front() {
        match e {
            Effect::Send(id, line) => {
                if let Some(l) = &n.listener {
                    l.send(id, wire::encode(&line));
                }
            }
            Effect::Apply(c) => {
                commands.push(*c);
                sent = true;
            }
            Effect::Close(id) => {
                if let Some(l) = &n.listener {
                    l.close(id);
                }
            }
            Effect::Ask(id) => {
                let Some(p) = n.hub.clients.get(&id) else {
                    continue;
                };
                let (addr, name) = (p.addr, p.pc_name.clone());
                if !n.settings.approve || n.known(addr, &name) {
                    todo.extend(n.hub.decide(id, true, master, now));
                } else if !n.asks.iter().any(|a| a.id == id) {
                    n.asks.push(Ask { id, addr, name });
                }
            }
            Effect::Note(id, note) => {
                let who = n
                    .hub
                    .clients
                    .get(&id)
                    .map(|p| format!("#{} {} ({})", p.number, p.pc_name, p.addr.ip()))
                    .unwrap_or_default();
                if let Note::TxClash { tx, other } = &note {
                    let text = crate::texts().format("en", "wire.tx_clash", &[tx, other]);
                    if let Some(l) = &n.listener {
                        l.send(id, wire::encode(&protocol::notice(&format!("!{text}"))));
                    }
                }
                n.notes.push((who, note));
            }
        }
    }
    sent
}

impl Observer for ServerLink {
    fn name(&self) -> &str {
        "Z-Server"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        if cx.caused_by_me() || cx.root_origin() != Origin::User {
            return;
        }
        let line = match event {
            LogEvent::Inserted { index } => protocol::put_qso(&cx.log().qsos[*index]),
            LogEvent::Updated { index, .. } => protocol::edit_qso_to(&cx.log().qsos[*index]),
            LogEvent::Deleted { qso, .. } => protocol::del_qso(qso),
            _ => return,
        };
        let mut n = self.net.lock();
        if n.listener.is_none() {
            return;
        }
        let effects = n.hub.local(line);
        let mut commands = Vec::new();
        let mut master = Master::new(cx.log());
        run(&mut n, effects, &mut commands, &mut master, Instant::now());
    }
}

impl Link for ServerLink {
    fn poll(&mut self, cx: &mut Context<'_>) {
        let now = Instant::now();
        let mut commands = Vec::new();
        let freqs = {
            let mut n = self.net.lock();
            let Some(l) = n.listener.as_mut() else {
                self.inbox.clear();
                return;
            };
            self.inbox.extend(l.poll());
            let audit: Vec<_> = l.audit.try_iter().collect();
            n.audit.extend(audit);
            n.hub.clock = clock();
            let mut master = Master::new(cx.log());
            let mut sent = false;
            for (id, yes) in std::mem::take(&mut n.decisions) {
                n.asks.retain(|a| a.id != id);
                if yes {
                    if let Some(p) = n.hub.clients.get(&id) {
                        n.clients.add(zlog_tls::PeerId::Ip(p.addr.ip()), &p.pc_name);
                    }
                }
                let effects = n.hub.decide(id, yes, &mut master, now);
                sent |= run(&mut n, effects, &mut commands, &mut master, now);
            }
            while let Some(e) = self.inbox.pop_front() {
                let effects = match e {
                    Event::Joined { id, peer, .. } => {
                        let mut effects =
                            n.hub.join(id, peer.addr, peer.fingerprint.is_some(), now);
                        if peer.fingerprint.is_some() {
                            effects.extend(n.hub.decide(id, true, &mut master, now));
                        }
                        effects
                    }
                    Event::Line { id, bytes } => {
                        let text = wire::decode(&bytes);
                        let reads = Request::parse(&text).is_ok_and(|r| r.reads_log());
                        if reads && sent {
                            self.inbox.push_front(Event::Line { id, bytes });
                            zlog_core::wake::wake();
                            break;
                        }
                        n.hub.receive(id, &text, &mut master, now)
                    }
                    Event::Left { id } => {
                        n.asks.retain(|a| a.id != id);
                        n.hub.leave(id, now)
                    }
                };
                sent |= run(&mut n, effects, &mut commands, &mut master, now);
            }
            let effects = n.hub.tick(now);
            run(&mut n, effects, &mut commands, &mut master, now);
            if n.hub.take_renew() {
                commands.push(Command::SortByTime);
            }
            n.hub.freqs.clone()
        };
        for c in commands {
            cx.send(c);
        }
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if s.freqs != freqs {
            s.freqs = freqs;
        }
    }
}
