use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use rustls::{ClientConnection, ServerConnection, StreamOwned};

use crate::crypto::Fingerprint;
use crate::guard::Rate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ending {
    #[default]
    Lf,
    CrLf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub addr: SocketAddr,
    pub name: String,
    pub fingerprint: Option<Fingerprint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKind {
    Connected,
    OutOfRange,
    Full,
    Accepted,
    PairRequested,
    Approved,
    Denied,
    Unknown,
    AuthFailed { failures: u32 },
    LockedOut { lockout: Duration },
    Locked,
    LineTooLong,
    RateLimited,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    pub time: SystemTime,
    pub addr: SocketAddr,
    pub name: String,
    pub kind: AuditKind,
}

pub(crate) type Fanout<E> = Arc<Mutex<Vec<Sender<E>>>>;

pub(crate) fn fan<E: Clone>(subs: &Fanout<E>, e: E) {
    subs.lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|s| s.send(e.clone()).is_ok());
}

pub(crate) fn listen<E>(subs: &Fanout<E>) -> Receiver<E> {
    let (tx, rx) = mpsc::channel();
    subs.lock().unwrap_or_else(|e| e.into_inner()).push(tx);
    rx
}

pub(crate) fn log(audit: &Fanout<Audit>, addr: SocketAddr, name: &str, kind: AuditKind) {
    fan(
        audit,
        Audit {
            time: SystemTime::now(),
            addr,
            name: name.into(),
            kind,
        },
    );
}

pub(crate) enum Stream {
    Plain(TcpStream),
    Server(Box<StreamOwned<ServerConnection, TcpStream>>),
    Client(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Stream {
    fn sock(&self) -> &TcpStream {
        match self {
            Stream::Plain(s) => s,
            Stream::Server(t) => &t.sock,
            Stream::Client(t) => &t.sock,
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Server(t) => t.read(buf),
            Stream::Client(t) => t.read(buf),
        }
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.write_all(bytes).and_then(|_| s.flush()),
            Stream::Server(t) => t.write_all(bytes).and_then(|_| t.flush()),
            Stream::Client(t) => t.write_all(bytes).and_then(|_| t.flush()),
        }
    }

    fn close(&mut self) {
        match self {
            Stream::Plain(_) => {}
            Stream::Server(t) => {
                t.conn.send_close_notify();
                let _ = t.flush();
            }
            Stream::Client(t) => {
                t.conn.send_close_notify();
                let _ = t.flush();
            }
        }
        let _ = self.sock().shutdown(Shutdown::Both);
    }
}

pub(crate) struct Slot(pub Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct LineConn {
    stream: Stream,
    pending: Vec<u8>,
    peer: PeerInfo,
    ending: Ending,
    max_line: usize,
    rate: Rate,
    audit: Option<Fanout<Audit>>,
    pub(crate) slot: Option<Slot>,
    closed: bool,
}

impl LineConn {
    pub(crate) fn new(stream: Stream, peer: PeerInfo, ending: Ending, max_line: usize) -> Self {
        LineConn {
            stream,
            pending: Vec::new(),
            peer,
            ending,
            max_line,
            rate: Rate::new(0),
            audit: None,
            slot: None,
            closed: false,
        }
    }

    pub(crate) fn guarded(mut self, per_sec: u32, audit: Fanout<Audit>) -> Self {
        self.rate = Rate::new(per_sec);
        self.audit = Some(audit);
        self
    }

    pub(crate) fn set_ending(&mut self, ending: Ending) {
        self.ending = ending;
    }

    pub(crate) fn set_name(&mut self, name: &str) {
        self.peer.name = name.into();
    }

    pub fn peer(&self) -> &PeerInfo {
        &self.peer
    }

    fn report(&self, kind: AuditKind) {
        if let Some(a) = &self.audit {
            log(a, self.peer.addr, &self.peer.name, kind);
        }
    }

    fn take(&mut self) -> io::Result<Option<Vec<u8>>> {
        let Some(i) = self.pending.iter().position(|&b| b == b'\n') else {
            if self.pending.len() > self.max_line {
                self.report(AuditKind::LineTooLong);
                return Err(io::ErrorKind::InvalidData.into());
            }
            return Ok(None);
        };
        let mut line: Vec<u8> = self.pending.drain(..=i).collect();
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.len() > self.max_line {
            self.report(AuditKind::LineTooLong);
            return Err(io::ErrorKind::InvalidData.into());
        }
        if !self.rate.allow() {
            self.report(AuditKind::RateLimited);
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Some(line))
    }

    pub fn read_line(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        if let Some(line) = self.take()? {
            return Ok(Some(line));
        }
        if self.closed {
            return Err(io::ErrorKind::NotConnected.into());
        }
        self.stream
            .sock()
            .set_read_timeout(Some(wait.max(Duration::from_millis(1))))?;
        let mut chunk = [0u8; 4096];
        match self.stream.read(&mut chunk) {
            Ok(0) => Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                self.pending.extend_from_slice(&chunk[..n]);
                self.take()
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    pub fn read_line_until(&mut self, wait: Duration) -> io::Result<Vec<u8>> {
        let start = std::time::Instant::now();
        loop {
            let left = wait.saturating_sub(start.elapsed());
            if left.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            if let Some(line) = self.read_line(left.min(Duration::from_millis(100)))? {
                return Ok(line);
            }
        }
    }

    pub fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        if self.closed {
            return Err(io::ErrorKind::NotConnected.into());
        }
        let mut bytes = Vec::with_capacity(line.len() + 2);
        bytes.extend(line.iter().filter(|b| !matches!(b, b'\r' | b'\n')));
        match self.ending {
            Ending::Lf => bytes.push(b'\n'),
            Ending::CrLf => bytes.extend_from_slice(b"\r\n"),
        }
        self.stream.write(&bytes)
    }

    pub fn shutdown(&mut self) {
        if !self.closed {
            self.closed = true;
            self.stream.close();
            self.report(AuditKind::Closed);
        }
    }

    pub(crate) fn exporter(&self, app: &str) -> Option<String> {
        crate::tls::pairing_code(app, |out, label| match &self.stream {
            Stream::Server(t) => t.conn.export_keying_material(out, label, None).is_ok(),
            Stream::Client(t) => t.conn.export_keying_material(out, label, None).is_ok(),
            Stream::Plain(_) => false,
        })
    }
}

impl Drop for LineConn {
    fn drop(&mut self) {
        self.shutdown();
    }
}
