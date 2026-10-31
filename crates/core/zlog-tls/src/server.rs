use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustls::{ServerConnection, StreamOwned};

use crate::crypto::{Fingerprint, TokenHash};
use crate::guard::{Guard, Limits, Range, Verdict};
use crate::line::{
    listen, log, Audit, AuditKind, Ending, Fanout, LineConn, PeerInfo, Slot, Stream,
};
use crate::peers::{PeerId, Peers};
use crate::tls::Identity;

pub const AUTH: &str = "AUTH";

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub app: String,
    pub bind: SocketAddr,
    pub tls: Option<Identity>,
    pub token: Option<TokenHash>,
    pub allowed: Vec<Range>,
    pub approve_new: bool,
    pub handshake: bool,
    pub ending: Ending,
    pub limits: Limits,
}

impl ServerConfig {
    pub fn new(app: &str, bind: SocketAddr) -> Self {
        ServerConfig {
            app: app.into(),
            bind,
            tls: None,
            token: None,
            allowed: Range::private(),
            approve_new: true,
            handshake: false,
            ending: Ending::Lf,
            limits: Limits::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub addr: SocketAddr,
    pub name: String,
    pub code: Option<String>,
    pub fingerprint: Option<Fingerprint>,
}

struct Shared {
    config: ServerConfig,
    tls: Option<Arc<rustls::ServerConfig>>,
    token: Mutex<Option<TokenHash>>,
    peers: Peers,
    guard: Guard,
    audit: Fanout<Audit>,
    conns: Arc<AtomicUsize>,
    ready: Mutex<Sender<LineConn>>,
    pending: Mutex<Vec<(Request, Sender<bool>)>>,
    next: AtomicU64,
    halt: AtomicBool,
}

pub struct Server {
    addr: SocketAddr,
    fingerprint: Option<Fingerprint>,
    shared: Arc<Shared>,
    ready: Mutex<Receiver<LineConn>>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn bind(config: ServerConfig, peers: Peers) -> io::Result<Server> {
        let tls = config
            .tls
            .as_ref()
            .map(|id| id.server_config().map(Arc::new))
            .transpose()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let listener = TcpListener::bind(config.bind)?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let (tx, rx) = mpsc::channel();
        let fingerprint = config.tls.as_ref().map(Identity::fingerprint);
        let shared = Arc::new(Shared {
            token: Mutex::new(config.token),
            config,
            tls,
            peers,
            guard: Guard::default(),
            audit: Fanout::default(),
            conns: Arc::default(),
            ready: Mutex::new(tx),
            pending: Mutex::default(),
            next: AtomicU64::new(1),
            halt: AtomicBool::new(false),
        });
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("zlog-tls-accept".into())
            .spawn(move || accept_loop(&listener, &s))?;
        Ok(Server {
            addr,
            fingerprint,
            shared,
            ready: Mutex::new(rx),
            thread: Some(thread),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn fingerprint(&self) -> Option<Fingerprint> {
        self.fingerprint
    }

    pub fn peers(&self) -> &Peers {
        &self.shared.peers
    }

    pub fn accept(&self, wait: Duration) -> Option<LineConn> {
        let rx = self.ready.lock().unwrap_or_else(|e| e.into_inner());
        rx.recv_timeout(wait).ok()
    }

    pub fn requests(&self) -> Vec<Request> {
        self.shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(r, _)| r.clone())
            .collect()
    }

    pub fn decide(&self, id: u64, approve: bool) -> bool {
        let mut p = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let Some(i) = p.iter().position(|(r, _)| r.id == id) else {
            return false;
        };
        let (_, tx) = p.remove(i);
        tx.send(approve).is_ok()
    }

    pub fn set_token(&self, token: Option<TokenHash>) {
        *self.shared.token.lock().unwrap_or_else(|e| e.into_inner()) = token;
    }

    pub fn audit(&self) -> Receiver<Audit> {
        listen(&self.shared.audit)
    }

    pub fn connections(&self) -> usize {
        self.shared.conns.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.halt.store(true, Ordering::SeqCst);
        self.shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn accept_loop(listener: &TcpListener, s: &Arc<Shared>) {
    while !s.halt.load(Ordering::SeqCst) {
        let (stream, addr) = match listener.accept() {
            Ok(x) => x,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(_) => continue,
        };
        if !Range::allows(&s.config.allowed, addr.ip()) {
            log(&s.audit, addr, "", AuditKind::OutOfRange);
            continue;
        }
        if s.conns.fetch_add(1, Ordering::SeqCst) >= s.config.limits.max_conns {
            s.conns.fetch_sub(1, Ordering::SeqCst);
            log(&s.audit, addr, "", AuditKind::Full);
            continue;
        }
        let slot = Slot(s.conns.clone());
        let s = s.clone();
        let _ = std::thread::Builder::new()
            .name("zlog-tls-handshake".into())
            .spawn(move || {
                log(&s.audit, addr, "", AuditKind::Connected);
                if let Some(mut conn) = handshake(stream, addr, &s) {
                    conn.slot = Some(slot);
                    let _ = s.ready.lock().unwrap_or_else(|e| e.into_inner()).send(conn);
                }
            });
    }
}

enum Method {
    Implicit,
    Hello,
    Known,
    Pair,
    Token(String),
}

fn parse(line: &[u8]) -> Option<(Method, String)> {
    let line = String::from_utf8_lossy(line);
    let rest = line.strip_prefix(AUTH)?.strip_prefix(' ')?;
    let (method, rest) = rest.split_once(' ').unwrap_or((rest, ""));
    Some(match method {
        "hello" => (Method::Hello, rest.into()),
        "known" => (Method::Known, rest.into()),
        "pair" => (Method::Pair, rest.into()),
        "token" => {
            let (token, name) = rest.split_once(' ').unwrap_or((rest, ""));
            (Method::Token(token.into()), name.into())
        }
        _ => return None,
    })
}

fn say(conn: &mut LineConn, word: &str) {
    let _ = conn.write_line(format!("{AUTH} {word}").as_bytes());
}

fn tls_stream(stream: TcpStream, config: &Arc<rustls::ServerConfig>) -> Option<Stream> {
    let conn = ServerConnection::new(config.clone()).ok()?;
    let mut tls = StreamOwned::new(conn, stream);
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock).ok()?;
    }
    Some(Stream::Server(Box::new(tls)))
}

fn handshake(stream: TcpStream, addr: SocketAddr, s: &Shared) -> Option<LineConn> {
    let limits = &s.config.limits;
    stream.set_nonblocking(false).ok()?;
    stream.set_nodelay(true).ok()?;
    stream.set_read_timeout(Some(limits.handshake)).ok()?;
    stream.set_write_timeout(Some(limits.handshake)).ok()?;
    let (stream, fingerprint) = match &s.tls {
        Some(config) => {
            let st = tls_stream(stream, config)?;
            let Stream::Server(t) = &st else {
                return None;
            };
            let fp = t
                .conn
                .peer_certificates()
                .and_then(|c| c.first())
                .map(|c| Fingerprint::of(c));
            (st, Some(fp?))
        }
        None => (Stream::Plain(stream), None),
    };
    let peer = PeerInfo {
        addr,
        name: String::new(),
        fingerprint,
    };
    let mut conn = LineConn::new(stream, peer, Ending::Lf, limits.max_line);
    let talks = s.tls.is_some() || s.config.handshake;
    if s.guard.locked(addr.ip()) {
        log(&s.audit, addr, "", AuditKind::Locked);
        if talks {
            say(&mut conn, "locked");
        }
        return None;
    }
    let (method, name) = match talks {
        true => parse(&conn.read_line_until(limits.handshake).ok()?)?,
        false => (Method::Implicit, String::new()),
    };
    let id = match fingerprint {
        Some(fp) => PeerId::Cert(fp),
        None => PeerId::Ip(addr.ip()),
    };
    let name = s.peers.name(&id).filter(|n| !n.is_empty()).unwrap_or(name);
    conn.set_name(&name);
    let ok = match method {
        Method::Token(token) => check_token(&mut conn, &token, addr.ip(), s),
        Method::Pair if s.tls.is_some() => approve(&mut conn, id, s),
        Method::Pair => false,
        Method::Known | Method::Hello | Method::Implicit
            if s.peers.knows(&id) || !s.config.approve_new =>
        {
            true
        }
        Method::Known => {
            log(&s.audit, addr, &name, AuditKind::Unknown);
            say(&mut conn, "unknown");
            false
        }
        Method::Hello | Method::Implicit => approve(&mut conn, id, s),
    };
    if !ok {
        return None;
    }
    if talks {
        say(&mut conn, "ok");
    }
    log(&s.audit, addr, &name, AuditKind::Accepted);
    conn.set_ending(s.config.ending);
    Some(conn.guarded(limits.lines_per_sec, s.audit.clone()))
}

fn check_token(conn: &mut LineConn, token: &str, ip: IpAddr, s: &Shared) -> bool {
    let want = *s.token.lock().unwrap_or_else(|e| e.into_inner());
    let addr = conn.peer().addr;
    let name = conn.peer().name.clone();
    if want.is_some_and(|h| h.verify(token)) {
        s.guard.succeed(ip);
        return true;
    }
    match s.guard.fail(ip, &s.config.limits) {
        Verdict::Delay(delay, failures) => {
            log(&s.audit, addr, &name, AuditKind::AuthFailed { failures });
            std::thread::sleep(delay);
            say(conn, "denied");
        }
        Verdict::Lock(lockout) => {
            let failures = s.config.limits.attempts;
            log(&s.audit, addr, &name, AuditKind::AuthFailed { failures });
            log(&s.audit, addr, &name, AuditKind::LockedOut { lockout });
            say(conn, "locked");
        }
    }
    false
}

fn approve(conn: &mut LineConn, id: PeerId, s: &Shared) -> bool {
    let code = conn.exporter(&s.config.app);
    let peer = conn.peer().clone();
    let (tx, rx) = mpsc::channel();
    let request = Request {
        id: s.next.fetch_add(1, Ordering::SeqCst),
        addr: peer.addr,
        name: peer.name.clone(),
        code,
        fingerprint: peer.fingerprint,
    };
    s.pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((request.clone(), tx));
    log(&s.audit, peer.addr, &peer.name, AuditKind::PairRequested);
    if s.tls.is_some() || s.config.handshake {
        say(conn, "wait");
    }
    let start = Instant::now();
    let decided = loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(d) => break d,
            Err(RecvTimeoutError::Disconnected) => break false,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if s.halt.load(Ordering::SeqCst) || start.elapsed() > s.config.limits.approval {
            break false;
        }
    };
    s.pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(r, _)| r.id != request.id);
    if decided {
        s.peers.add(id, &peer.name);
        log(&s.audit, peer.addr, &peer.name, AuditKind::Approved);
    } else {
        log(&s.audit, peer.addr, &peer.name, AuditKind::Denied);
        if s.tls.is_some() || s.config.handshake {
            say(conn, "denied");
        }
    }
    decided
}
