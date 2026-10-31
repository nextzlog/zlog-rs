use std::fmt;
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::{ClientConnection, StreamOwned};

use crate::crypto::Fingerprint;
use crate::line::{Ending, LineConn, PeerInfo, Stream};
use crate::server::AUTH;
use crate::tls::{server_name, Identity};

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub app: String,
    pub name: String,
    pub identity: Option<Identity>,
    pub ending: Ending,
    pub max_line: usize,
    pub timeout: Duration,
}

impl ClientConfig {
    pub fn new(app: &str, name: &str) -> Self {
        ClientConfig {
            app: app.into(),
            name: name.into(),
            identity: None,
            ending: Ending::Lf,
            max_line: 1 << 16,
            timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    Known,
    Pair,
    Token(String),
}

#[derive(Debug)]
pub enum ConnectError {
    Unknown(Fingerprint),
    Mismatch {
        pinned: Fingerprint,
        found: Fingerprint,
    },
    NotPaired,
    Denied,
    Locked,
    NoIdentity,
    Io(io::Error),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectError::Unknown(fp) => write!(f, "unknown server certificate {fp}"),
            ConnectError::Mismatch { pinned, found } => write!(
                f,
                "the server certificate {found} does not match the pinned {pinned}"
            ),
            ConnectError::NotPaired => f.write_str("the server does not know this station"),
            ConnectError::Denied => f.write_str("the server refused"),
            ConnectError::Locked => f.write_str("too many failures, locked out for now"),
            ConnectError::NoIdentity => f.write_str("no identity for TLS"),
            ConnectError::Io(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ConnectError {}

impl From<io::Error> for ConnectError {
    fn from(e: io::Error) -> Self {
        ConnectError::Io(e)
    }
}

fn open(addr: impl ToSocketAddrs, timeout: Duration) -> io::Result<TcpStream> {
    let mut last = io::Error::from(io::ErrorKind::AddrNotAvailable);
    for a in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => {
                s.set_nodelay(true)?;
                s.set_read_timeout(Some(timeout))?;
                s.set_write_timeout(Some(timeout))?;
                return Ok(s);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn info(stream: &TcpStream, fingerprint: Option<Fingerprint>) -> io::Result<PeerInfo> {
    Ok(PeerInfo {
        addr: stream.peer_addr()?,
        name: String::new(),
        fingerprint,
    })
}

fn answer(conn: &mut LineConn, wait: Duration) -> Result<(), ConnectError> {
    let start = Instant::now();
    loop {
        let left = wait.saturating_sub(start.elapsed());
        let line = match conn.read_line_until(left) {
            Ok(l) => l,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Err(ConnectError::Denied),
            Err(e) => return Err(e.into()),
        };
        let line = String::from_utf8_lossy(&line);
        match line.strip_prefix(AUTH).map(str::trim) {
            Some("ok") => return Ok(()),
            Some("wait") => {}
            Some("denied") => return Err(ConnectError::Denied),
            Some("locked") => return Err(ConnectError::Locked),
            Some("unknown") => return Err(ConnectError::NotPaired),
            _ => return Err(io::Error::from(io::ErrorKind::InvalidData).into()),
        }
    }
}

pub fn connect_plain(
    addr: impl ToSocketAddrs,
    config: &ClientConfig,
    hello: Option<Duration>,
) -> Result<LineConn, ConnectError> {
    let stream = open(addr, config.timeout)?;
    let peer = info(&stream, None)?;
    let mut conn = LineConn::new(Stream::Plain(stream), peer, Ending::Lf, config.max_line);
    if let Some(wait) = hello {
        conn.write_line(format!("{AUTH} hello {}", config.name).as_bytes())?;
        answer(&mut conn, wait)?;
    }
    conn.set_ending(config.ending);
    Ok(conn)
}

pub struct Handshake {
    conn: LineConn,
    fingerprint: Fingerprint,
    code: String,
    pinned: bool,
    config: ClientConfig,
}

pub fn connect_tls(
    addr: impl ToSocketAddrs,
    config: &ClientConfig,
    pin: Option<Fingerprint>,
) -> Result<Handshake, ConnectError> {
    let invalid = |e: rustls::Error| io::Error::new(io::ErrorKind::InvalidData, e);
    let identity = config.identity.as_ref().ok_or(ConnectError::NoIdentity)?;
    let stream = open(addr, config.timeout)?;
    let (pinning, tls) = identity.client_config(pin).map_err(invalid)?;
    let conn = ClientConnection::new(Arc::new(tls), server_name()).map_err(invalid)?;
    let mut tls = StreamOwned::new(conn, stream);
    while tls.conn.is_handshaking() {
        if let Err(e) = tls.conn.complete_io(&mut tls.sock) {
            return Err(match (pin, pinning.seen()) {
                (Some(pinned), Some(found)) if pinned != found => {
                    ConnectError::Mismatch { pinned, found }
                }
                _ => ConnectError::Io(e),
            });
        }
    }
    let found = pinning
        .seen()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    let peer = info(&tls.sock, Some(found))?;
    let conn = LineConn::new(
        Stream::Client(Box::new(tls)),
        peer,
        Ending::Lf,
        config.max_line,
    );
    let code = conn
        .exporter(&config.app)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    Ok(Handshake {
        conn,
        fingerprint: found,
        code,
        pinned: pin == Some(found),
        config: config.clone(),
    })
}

impl Handshake {
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn login(mut self, login: Login, wait: Duration) -> Result<LineConn, ConnectError> {
        let name = &self.config.name;
        let line = match &login {
            Login::Pair => format!("{AUTH} pair {name}"),
            _ if !self.pinned => return Err(ConnectError::Unknown(self.fingerprint)),
            Login::Known => format!("{AUTH} known {name}"),
            Login::Token(t) => {
                let token: String = t.chars().filter(|c| !c.is_whitespace()).collect();
                format!("{AUTH} token {token} {name}")
            }
        };
        self.conn.write_line(line.as_bytes())?;
        answer(&mut self.conn, wait)?;
        self.conn.set_ending(self.config.ending);
        Ok(self.conn)
    }
}
