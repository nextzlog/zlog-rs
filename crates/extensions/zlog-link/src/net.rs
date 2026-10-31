use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use zlog_tls::discovery::{Announcer, Service};
use zlog_tls::{
    connect_tls, Audit, ClientConfig, ConnectError, Ending, Fingerprint, Identity, Limits,
    LineConn, Login, PeerInfo, Peers, Range, Request, Server, ServerConfig, Store, TokenHash,
};
use zlog_ui::Section;

use crate::hub::{ClientId, Hub, Note};
use crate::role::{self, Role};
use crate::session::{NetEvent, Transport};

pub use zlog_tls::PeerId;

pub const APP: &str = "zlog-link";
pub const SECTION: &str = "panel.link-net";
pub const IDENTITY: &str = "identity";
pub const CLIENTS: &str = "clients";
pub const SERVERS: &str = "servers";
pub const TOKEN: &str = "token_hash";
pub const CLIENT_TOKEN: &str = "client_token";
pub const MAX_LINE: usize = 1024;
pub const LINES_PER_SEC: u32 = 5000;
pub const APPROVAL: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub enum Event {
    Joined {
        id: ClientId,
        peer: PeerInfo,
        out: Sender<Out>,
    },
    Line {
        id: ClientId,
        bytes: Vec<u8>,
    },
    Left {
        id: ClientId,
    },
}

#[derive(Debug, Clone)]
pub enum Out {
    Line(Vec<u8>),
    Close,
}

fn serve(
    mut conn: LineConn,
    id: ClientId,
    rx: Receiver<Out>,
    tx: Sender<Event>,
    halt: Arc<AtomicBool>,
) {
    let left = |tx: &Sender<Event>| {
        let _ = tx.send(Event::Left { id });
        zlog_core::wake::wake();
    };
    loop {
        if halt.load(Ordering::SeqCst) {
            conn.shutdown();
            return left(&tx);
        }
        match conn.read_line(Duration::from_millis(20)) {
            Ok(Some(bytes)) => {
                let _ = tx.send(Event::Line { id, bytes });
                zlog_core::wake::wake();
            }
            Ok(None) => {}
            Err(_) => {
                conn.shutdown();
                return left(&tx);
            }
        }
        loop {
            match rx.try_recv() {
                Ok(Out::Line(bytes)) => {
                    if conn.write_line(&bytes).is_err() {
                        conn.shutdown();
                        return left(&tx);
                    }
                }
                Ok(Out::Close) | Err(TryRecvError::Disconnected) => {
                    conn.shutdown();
                    return left(&tx);
                }
                Err(TryRecvError::Empty) => break,
            }
        }
    }
}

pub struct Listener {
    tls: Arc<Server>,
    events: Receiver<Event>,
    outs: HashMap<ClientId, Sender<Out>>,
    halt: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub audit: Receiver<Audit>,
    pub secure: bool,
    _announcer: Option<Announcer>,
}

impl Listener {
    pub fn start(
        config: ServerConfig,
        peers: Peers,
        announce: Option<Service>,
    ) -> io::Result<Listener> {
        let secure = config.tls.is_some();
        let tls = Arc::new(Server::bind(config, peers)?);
        let audit = tls.audit();
        let (tx, events) = channel();
        let halt = Arc::new(AtomicBool::new(false));
        let (server, stop) = (tls.clone(), halt.clone());
        let thread = std::thread::Builder::new()
            .name("zlog-link-accept".into())
            .spawn(move || {
                let mut next: ClientId = 0;
                while !stop.load(Ordering::SeqCst) {
                    let Some(conn) = server.accept(Duration::from_millis(100)) else {
                        continue;
                    };
                    next += 1;
                    let id = next;
                    let peer = conn.peer().clone();
                    let (out, rx) = channel();
                    let (tx2, stop2) = (tx.clone(), stop.clone());
                    let _ = tx.send(Event::Joined { id, peer, out });
                    zlog_core::wake::wake();
                    let _ = std::thread::Builder::new()
                        .name("zlog-link-client".into())
                        .spawn(move || serve(conn, id, rx, tx2, stop2));
                }
            })?;
        let _announcer = match announce {
            Some(s) => Announcer::start(&s).ok(),
            None => None,
        };
        Ok(Listener {
            tls,
            events,
            outs: HashMap::new(),
            halt,
            thread: Some(thread),
            audit,
            secure,
            _announcer,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.tls.addr()
    }

    pub fn requests(&self) -> Vec<Request> {
        self.tls.requests()
    }

    pub fn decide(&self, id: u64, approve: bool) {
        self.tls.decide(id, approve);
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let events: Vec<Event> = self.events.try_iter().collect();
        for e in &events {
            match e {
                Event::Joined { id, out, .. } => {
                    self.outs.insert(*id, out.clone());
                }
                Event::Left { id } => {
                    self.outs.remove(id);
                }
                Event::Line { .. } => {}
            }
        }
        events
    }

    pub fn send(&self, id: ClientId, bytes: Vec<u8>) {
        if let Some(o) = self.outs.get(&id) {
            let _ = o.send(Out::Line(bytes));
        }
    }

    pub fn close(&self, id: ClientId) {
        if let Some(o) = self.outs.get(&id) {
            let _ = o.send(Out::Close);
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.halt.store(true, Ordering::SeqCst);
        for o in self.outs.values() {
            let _ = o.send(Out::Close);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerSettings {
    pub port: String,
    pub bind: String,
    pub tls: bool,
    pub allowed: String,
    pub approve: bool,
    pub max_conns: usize,
    pub merge_secs: u64,
    pub announce: bool,
    pub name: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        ServerSettings::load(&Section::new())
    }
}

fn get<T: std::str::FromStr>(s: &Section, key: &str, default: T) -> T {
    s.get(key)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn text(s: &Section, key: &str, default: &str) -> String {
    s.get(key).cloned().unwrap_or_else(|| default.into())
}

pub fn private_ranges() -> String {
    Range::private()
        .iter()
        .map(Range::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl ServerSettings {
    pub fn load(s: &Section) -> Self {
        ServerSettings {
            port: text(s, "server_port", "telnet"),
            bind: text(s, "server_bind", "0.0.0.0"),
            tls: get(s, "server_tls", false),
            allowed: text(s, "server_allowed", &private_ranges()),
            approve: get(s, "server_approve", true),
            max_conns: get(s, "server_max", crate::hub::SLOTS),
            merge_secs: get(s, "server_merge_secs", 60),
            announce: get(s, "server_announce", false),
            name: text(s, "server_name", ""),
        }
    }

    pub fn ranges(&self) -> Result<Vec<Range>, String> {
        self.allowed
            .split([',', ' ', ';'])
            .filter(|r| !r.trim().is_empty())
            .map(str::parse)
            .collect()
    }

    pub fn name(&self) -> String {
        match self.name.trim() {
            "" => "zLog".into(),
            n => n.into(),
        }
    }

    pub fn service(&self, contest: &str, port: u16) -> Service {
        Service {
            kind: APP.into(),
            name: self.name(),
            port,
            txt: vec![
                ("name".into(), self.name()),
                ("contest".into(), contest.into()),
                ("tls".into(), u8::from(self.tls).to_string()),
            ],
        }
    }

    pub fn endpoint(&self) -> Result<SocketAddr, String> {
        let port = crate::session::port_number(&self.port).ok_or("bad-port")?;
        (self.bind.trim(), port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or_else(|| "bad-bind".into())
    }
}

pub fn fields(language: &str) -> Vec<zlog_ui::device::SettingField> {
    use zlog_ui::device::{SettingField, SettingKind};
    let t = |k: &str| crate::texts().get(language, k).to_string();
    let flag = |key: &'static str, default: &str| {
        SettingField::new(key, t(key), SettingKind::Bool).default_value(default)
    };
    let line = |key: &'static str, default: &str| {
        SettingField::new(key, t(key), SettingKind::Text).default_value(default)
    };
    vec![
        line("server_port", "telnet"),
        line("server_bind", "0.0.0.0"),
        flag("server_tls", "false"),
        line("server_allowed", &private_ranges()),
        flag("server_approve", "true"),
        SettingField::new(
            "server_max",
            t("server_max"),
            SettingKind::Int { min: 1, max: 32 },
        )
        .default_value(crate::hub::SLOTS.to_string()),
        SettingField::new(
            "server_merge_secs",
            t("server_merge_secs"),
            SettingKind::Int { min: 10, max: 3600 },
        )
        .default_value("60"),
        flag("server_announce", "false"),
        line("server_name", ""),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub id: ClientId,
    pub addr: SocketAddr,
    pub name: String,
}

#[derive(Default)]
pub struct Net {
    pub settings: ServerSettings,
    pub listener: Option<Listener>,
    pub hub: Hub,
    pub error: Option<String>,
    pub identity: Option<Identity>,
    pub clients: Peers,
    pub servers: Peers,
    pub code: Option<String>,
    pub repair: Option<String>,
    pub asks: Vec<Ask>,
    pub decisions: Vec<(ClientId, bool)>,
    pub notes: Vec<(String, Note)>,
    pub audit: Vec<Audit>,
    pub client_token: String,
    pub token: Option<TokenHash>,
    pub loaded: bool,
}

pub struct Shared(Mutex<Net>);

impl Shared {
    pub fn lock(&self) -> MutexGuard<'_, Net> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for Shared {
    fn default() -> Self {
        Shared(Mutex::new(Net::default()))
    }
}

pub fn shared() -> Arc<Shared> {
    static NET: OnceLock<Arc<Shared>> = OnceLock::new();
    NET.get_or_init(Arc::default).clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    Role(Role),
    Settings(String),
    Bind {
        port: u16,
        error: io::ErrorKind,
        text: String,
    },
}

impl Net {
    pub fn running(&self) -> bool {
        self.listener.is_some()
    }

    pub fn secure(&self) -> Option<bool> {
        self.listener.as_ref().map(|l| l.secure)
    }

    pub fn identity(&mut self) -> Result<Identity, String> {
        if let Some(id) = &self.identity {
            return Ok(id.clone());
        }
        let id = Identity::generate()?;
        self.identity = Some(id.clone());
        Ok(id)
    }

    pub fn load(&mut self, section: &Section) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.identity = section.load(IDENTITY).and_then(|t| Identity::from_text(&t));
        self.clients
            .replace(&zlog_tls::load_peers(section, CLIENTS));
        self.servers
            .replace(&zlog_tls::load_peers(section, SERVERS));
        self.token = zlog_tls::load_token_hash(section, TOKEN);
        self.client_token = text(section, CLIENT_TOKEN, "");
    }

    pub fn save(&self, section: &mut Section) {
        if let Some(id) = &self.identity {
            section.save(IDENTITY, id.to_text());
        }
        section.save(CLIENTS, self.clients.to_text());
        section.save(SERVERS, self.servers.to_text());
        let token = self.token.map(|t| t.to_string()).unwrap_or_default();
        section.insert(TOKEN.into(), token);
        section.insert(CLIENT_TOKEN.into(), self.client_token.clone());
    }

    pub fn start(&mut self, contest: &str) -> Result<(), StartError> {
        if self.listener.is_some() {
            return Ok(());
        }
        let s = self.settings.clone();
        let bind = s.endpoint().map_err(StartError::Settings)?;
        let allowed = s.ranges().map_err(StartError::Settings)?;
        let tls = match s.tls {
            true => Some(self.identity().map_err(StartError::Settings)?),
            false => None,
        };
        role::claim(Role::Server).map_err(StartError::Role)?;
        let config = ServerConfig {
            tls,
            token: self.token.filter(|_| s.tls),
            allowed,
            approve_new: s.tls,
            ending: Ending::CrLf,
            limits: Limits {
                max_conns: s.max_conns.clamp(1, crate::hub::SLOTS),
                max_line: MAX_LINE,
                lines_per_sec: LINES_PER_SEC,
                approval: APPROVAL,
                ..Limits::default()
            },
            ..ServerConfig::new(APP, bind)
        };
        let announce = s.announce.then(|| s.service(contest, bind.port()));
        match Listener::start(config, self.clients.clone(), announce) {
            Ok(l) => {
                let local_tx = self.hub.local_tx;
                self.hub = Hub::default();
                self.hub.timing.merge_idle = Duration::from_secs(s.merge_secs.max(10));
                self.hub.local_tx = local_tx;
                self.listener = Some(l);
                self.error = None;
                Ok(())
            }
            Err(e) => {
                role::release(Role::Server);
                Err(StartError::Bind {
                    port: bind.port(),
                    error: e.kind(),
                    text: e.to_string(),
                })
            }
        }
    }

    pub fn stop(&mut self) {
        if self.listener.take().is_some() {
            role::release(Role::Server);
        }
        self.hub.clients.clear();
        self.asks.clear();
        self.decisions.clear();
    }

    pub fn say(&mut self, shown: String, line: &str, red: bool) {
        self.hub
            .chat
            .push(crate::session::ChatLine { text: shown, red });
        self.broadcast(line);
    }

    pub fn broadcast(&self, line: &str) {
        if let Some(l) = &self.listener {
            for (id, p) in &self.hub.clients {
                if p.approved {
                    l.send(*id, crate::wire::encode(line));
                }
            }
        }
    }

    pub fn pin(&self, addr: &str) -> Option<Fingerprint> {
        self.servers.list().into_iter().find_map(|p| match p.id {
            PeerId::Cert(fp) if p.name == addr => Some(fp),
            _ => None,
        })
    }

    pub fn remember_server(&self, addr: &str, fp: Fingerprint) {
        for p in self.servers.list() {
            if p.name == addr {
                self.servers.remove(&p.id);
            }
        }
        self.servers.add(PeerId::Cert(fp), addr);
    }

    pub fn known(&self, addr: SocketAddr, name: &str) -> bool {
        self.clients
            .name(&PeerId::Ip(addr.ip()))
            .is_some_and(|n| n == name.trim())
    }
}

pub struct SecureTcp {
    out: Sender<Out>,
    events: Receiver<NetEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pairing {
    Auto,
    Again,
}

fn login(net: &Shared, addr: &str, name: &str, pairing: Pairing) -> Result<LineConn, String> {
    let (identity, pin, token) = {
        let mut n = net.lock();
        (
            n.identity()?,
            n.pin(addr),
            n.client_token.trim().to_string(),
        )
    };
    let config = ClientConfig {
        identity: Some(identity),
        ending: Ending::CrLf,
        max_line: MAX_LINE * 2,
        ..ClientConfig::new(APP, name)
    };
    let hs = connect_tls(addr, &config, pin.filter(|_| pairing == Pairing::Auto))
        .map_err(|e| e.to_string())?;
    let fp = hs.fingerprint();
    let how = match (pin.is_some() && pairing == Pairing::Auto, token.is_empty()) {
        (true, true) => Login::Known,
        (true, false) => Login::Token(token),
        (false, _) => {
            net.lock().code = Some(hs.code().into());
            zlog_core::wake::wake();
            Login::Pair
        }
    };
    let pairing = how == Login::Pair;
    let got = hs.login(how, APPROVAL);
    net.lock().code = None;
    match got {
        Ok(conn) => {
            if pairing {
                net.lock().remember_server(addr, fp);
            }
            Ok(conn)
        }
        Err(ConnectError::NotPaired) => {
            net.lock().repair = Some(addr.into());
            zlog_core::wake::wake();
            Err(ConnectError::NotPaired.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

impl SecureTcp {
    pub fn open(net: Arc<Shared>, addr: &str, name: &str, pairing: Pairing) -> SecureTcp {
        let (out, rx) = channel::<Out>();
        let (tx, events) = channel();
        let (addr, name) = (addr.to_string(), name.to_string());
        std::thread::spawn(move || {
            let say = |e: NetEvent| {
                let _ = tx.send(e);
                zlog_core::wake::wake();
            };
            if zlog_net::offline() && !is_loopback(&addr) {
                return say(NetEvent::Closed(Some("offline".into())));
            }
            let mut conn = match login(&net, &addr, &name, pairing) {
                Ok(c) => c,
                Err(e) => return say(NetEvent::Closed(Some(e))),
            };
            say(NetEvent::Connected(conn.peer().addr.to_string()));
            loop {
                match conn.read_line(Duration::from_millis(20)) {
                    Ok(Some(mut line)) => {
                        line.extend_from_slice(b"\r\n");
                        say(NetEvent::Data(line));
                    }
                    Ok(None) => {}
                    Err(e) => return say(NetEvent::Closed(Some(e.to_string()))),
                }
                loop {
                    match rx.try_recv() {
                        Ok(Out::Line(bytes)) => {
                            for l in bytes.split(|b| *b == b'\n') {
                                let l = l.strip_suffix(b"\r").unwrap_or(l);
                                if !l.is_empty() && conn.write_line(l).is_err() {
                                    return say(NetEvent::Closed(None));
                                }
                            }
                        }
                        Ok(Out::Close) | Err(TryRecvError::Disconnected) => {
                            conn.shutdown();
                            return;
                        }
                        Err(TryRecvError::Empty) => break,
                    }
                }
            }
        });
        SecureTcp { out, events }
    }
}

fn is_loopback(addr: &str) -> bool {
    addr.to_socket_addrs()
        .map(|mut a| a.all(|a| a.ip().is_loopback()))
        .unwrap_or(false)
}

impl Transport for SecureTcp {
    fn send(&mut self, bytes: &[u8]) {
        let _ = self.out.send(Out::Line(bytes.to_vec()));
    }

    fn poll(&mut self) -> Vec<NetEvent> {
        self.events.try_iter().collect()
    }

    fn close(&mut self) {
        let _ = self.out.send(Out::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_tls::discovery::{decode_txt, encode_txt};

    #[test]
    fn the_announcement_carries_name_contest_and_tls() {
        let mut s = ServerSettings::default();
        assert_eq!((s.port.as_str(), s.tls, s.approve), ("telnet", false, true));
        assert_eq!(s.endpoint().unwrap().port(), 23);
        assert!(s
            .ranges()
            .unwrap()
            .iter()
            .any(|r| r.contains("192.168.1.9".parse().unwrap())));
        assert!(!s
            .ranges()
            .unwrap()
            .iter()
            .any(|r| r.contains("8.8.8.8".parse().unwrap())));
        s.name = "JA1ZLO multi".into();
        s.tls = true;
        let svc = s.service("ALLJA1", 23);
        assert_eq!(svc.kind, APP);
        let entries: Vec<String> = encode_txt(&svc.txt)
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let back = decode_txt(&entries);
        let get = |k: &str| back.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
        assert_eq!(get("name"), Some("JA1ZLO multi"));
        assert_eq!(get("contest"), Some("ALLJA1"));
        assert_eq!(get("tls"), Some("1"));
    }

    #[test]
    fn every_server_field_has_a_text_and_defaults_round_trip() {
        for lang in ["en", "ja"] {
            let f = fields(lang);
            assert!(f.iter().all(|f| !f.label.is_empty() && f.label != f.key));
            let mut section = Section::new();
            for f in &f {
                section.insert(f.key.into(), f.default.clone());
            }
            assert_eq!(ServerSettings::load(&section), ServerSettings::default());
        }
    }

    #[test]
    fn approved_plaintext_peers_are_known_by_address_and_name() {
        let n = Net::default();
        let a: SocketAddr = "192.168.1.5:4000".parse().unwrap();
        n.clients.add(PeerId::Ip(a.ip()), "PC5");
        assert!(n.known(a, "PC5"));
        assert!(
            !n.known(a, "PC6"),
            "another PC on that address is asked again"
        );
        let mut section = Section::new();
        n.save(&mut section);
        let mut m = Net::default();
        m.load(&section);
        assert!(m.known(a, "PC5"));
    }
}
