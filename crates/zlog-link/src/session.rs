//! The connection to Z-Server, shared by the panel and the link.
//!
//! zLog's `TZLinkForm` owns an ICS `TWSocket` (a plain TCP socket: despite the
//! "telnet" port name, zLog does no telnet option negotiation, so neither does this
//! client) and splits what it receives into lines at CR LF (`SetCommandBuffer`);
//! this client also ends a line at a bare LF (a CR without LF stays in the line).
//! Here a [`Transport`] moves bytes (TCP on threads, or memory in tests) and the
//! [`Session`] turns them into [`Message`]s for the link, keeping the chat, the
//! console and the station list for the panel.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zlog_core::Band;

use crate::freq::FreqList;
use crate::protocol::{self, Message};
use crate::wire;

/// Something that happened on a transport.
#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    /// Connected to this address.
    Connected(String),
    Data(Vec<u8>),
    /// Closed, with the error if it was not a normal close.
    Closed(Option<String>),
}

/// A byte stream to Z-Server. Neither method may block.
pub trait Transport: Send {
    fn send(&mut self, bytes: &[u8]);
    /// Events since the last call.
    fn poll(&mut self) -> Vec<NetEvent>;
    fn close(&mut self);
}

/// TCP, with a thread that connects and reads and a thread that writes.
pub struct Tcp {
    out: Sender<Vec<u8>>,
    events: Receiver<NetEvent>,
    stream: Arc<Mutex<Stream>>,
}

/// The connected stream, kept for [`Transport::close`], and whether close came first.
#[derive(Default)]
struct Stream {
    stream: Option<TcpStream>,
    closed: bool,
}

/// Keep a clone of the newly connected `s` for [`Transport::close`]; `None` when
/// close came while connecting, and the connection is shut down at once (else it
/// would stay open with nobody reading it).
fn attach(shared: &Mutex<Stream>, s: TcpStream) -> Option<TcpStream> {
    let mut st = shared.lock().unwrap();
    if st.closed {
        let _ = s.shutdown(Shutdown::Both);
        return None;
    }
    st.stream = s.try_clone().ok();
    Some(s)
}

/// The port number for zLog's port text: a number, or `telnet` (zLog's default).
pub fn port_number(port: &str) -> Option<u16> {
    match port.trim() {
        "" | "telnet" => Some(23),
        p => p.parse().ok(),
    }
}

impl Tcp {
    pub fn open(host: &str, port: u16) -> Tcp {
        let (out, out_rx) = channel::<Vec<u8>>();
        let (tx, events) = channel();
        let stream = Arc::new(Mutex::new(Stream::default()));
        let shared = stream.clone();
        let host = host.to_string();
        std::thread::spawn(move || {
            let addrs = match (host.as_str(), port).to_socket_addrs() {
                Ok(a) => a.collect::<Vec<_>>(),
                Err(e) => {
                    let _ = tx.send(NetEvent::Closed(Some(e.to_string())));
                    return;
                }
            };
            let mut last = String::from("no address");
            let Some(s) = addrs.iter().find_map(|a| {
                TcpStream::connect_timeout(a, Duration::from_secs(10))
                    .map_err(|e| last = e.to_string())
                    .ok()
            }) else {
                let _ = tx.send(NetEvent::Closed(Some(last)));
                return;
            };
            let _ = s.set_nodelay(true);
            let Some(mut s) = attach(&shared, s) else {
                return;
            };
            let addr = s.peer_addr().map_or(host.clone(), |a| a.to_string());
            let _ = tx.send(NetEvent::Connected(addr));
            if let Ok(mut w) = s.try_clone() {
                std::thread::spawn(move || {
                    for bytes in out_rx {
                        if w.write_all(&bytes).and_then(|_| w.flush()).is_err() {
                            break;
                        }
                    }
                });
            }
            let mut buf = [0u8; 2048];
            loop {
                match s.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(NetEvent::Closed(None));
                        break;
                    }
                    Ok(n) => {
                        let _ = tx.send(NetEvent::Data(buf[..n].to_vec()));
                    }
                    Err(e) => {
                        let _ = tx.send(NetEvent::Closed(Some(e.to_string())));
                        break;
                    }
                }
            }
        });
        Tcp {
            out,
            events,
            stream,
        }
    }
}

impl Transport for Tcp {
    fn send(&mut self, bytes: &[u8]) {
        let _ = self.out.send(bytes.to_vec());
    }

    fn poll(&mut self) -> Vec<NetEvent> {
        self.events.try_iter().collect()
    }

    fn close(&mut self) {
        let mut st = self.stream.lock().unwrap();
        st.closed = true;
        if let Some(s) = st.stream.take() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

/// `TELNETlinebreak` of zlog.ini: what ends each line sent (`LineBreakCode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineBreak {
    #[default]
    CrLf,
    Cr,
    Lf,
}

impl LineBreak {
    pub const ALL: [LineBreak; 3] = [LineBreak::CrLf, LineBreak::Cr, LineBreak::Lf];

    pub fn code(self) -> &'static str {
        match self {
            LineBreak::CrLf => "\r\n",
            LineBreak::Cr => "\r",
            LineBreak::Lf => "\n",
        }
    }

    /// The number zlog.ini stores.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// What to do with the log right after connecting (`TZServerInquiry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnConnect {
    /// What zLog's dialog proposes: download into an empty log, else merge.
    #[default]
    Auto,
    /// `BEGINMERGE`: exchange the QSOs either side lacks.
    Merge,
    /// Clear this log and load the server's (`LoadLogFromZLink`).
    Download,
    ConnectOnly,
}

impl OnConnect {
    pub const ALL: [OnConnect; 4] = [
        OnConnect::Auto,
        OnConnect::Merge,
        OnConnect::Download,
        OnConnect::ConnectOnly,
    ];

    pub fn key(self) -> &'static str {
        match self {
            OnConnect::Auto => "auto",
            OnConnect::Merge => "merge",
            OnConnect::Download => "download",
            OnConnect::ConnectOnly => "connect-only",
        }
    }
}

/// What precedes `> ` in a chat line (`TChatForm.GetPrompt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Prompt {
    #[default]
    Band,
    PcName,
    Operator,
    /// The station callsign (zLog `CALL`).
    Call,
}

impl Prompt {
    pub const ALL: [Prompt; 4] = [Prompt::Band, Prompt::PcName, Prompt::Operator, Prompt::Call];

    pub fn key(self) -> &'static str {
        match self {
            Prompt::Band => "band",
            Prompt::PcName => "pc-name",
            Prompt::Operator => "operator",
            Prompt::Call => "call",
        }
    }

    /// The text before `> ` (`TChatForm.GetPrompt`): the operator falls back to the
    /// station callsign when none is set.
    pub fn text(
        self,
        band: zlog_core::Band,
        pc_name: &str,
        operator: &str,
        mycall: &str,
    ) -> String {
        match self {
            Prompt::Band => band.label(),
            Prompt::PcName => pc_name,
            Prompt::Operator if operator.is_empty() => mycall,
            Prompt::Operator => operator,
            Prompt::Call => mycall,
        }
        .to_string()
    }
}

/// The `[Z-Link]` settings of zlog.ini this client uses.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Settings {
    pub host: String,
    /// A number or `telnet`.
    pub port: String,
    pub pc_name: String,
    pub line_break: LineBreak,
    pub on_connect: OnConnect,
    pub prompt: Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

/// A line of the chat window.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatLine {
    pub text: String,
    /// Sent with `!`: zLog shows it in red in the status bar.
    pub red: bool,
}

/// The server's QSO IDs during a merge (`FMergeTempList`): full IDs (with edit
/// counter) by ID without counter, in the order received.
#[derive(Debug, Default)]
pub struct Merge {
    pub order: Vec<i32>,
    pub ids: HashMap<i32, i32>,
}

/// zLog keeps the last 1000 console lines (`AddConsole`).
const CONSOLE_LINES: usize = 1000;

/// The connection and everything the panel shows.
#[derive(Default)]
pub struct Session {
    pub settings: Settings,
    pub state: State,
    transport: Option<Box<dyn Transport>>,
    buffer: Vec<u8>,
    /// Commands for the link, which needs the log to carry them out.
    pub(crate) inbox: VecDeque<Message>,
    pub console: VecDeque<String>,
    pub chat: Vec<ChatLine>,
    /// The running frequencies, kept across connections as zLog keeps them until
    /// cleared.
    pub freqs: FreqList,
    /// Messages for the status bar, taken by the link.
    pub(crate) status: Vec<String>,
    /// Set on connect: the link sends band, operator and PC name and starts the merge.
    pub(crate) init_pending: bool,
    pub(crate) merge: Option<Merge>,
    /// Download chosen: the link empties the log, then sends `SENDLOG`.
    pub(crate) clearing: bool,
    /// Band of the entry row, as the panel last saw it.
    pub entry_band: Option<Band>,
    /// Operator of the entry row (`CurrentQSO.Operator`), as the panel last saw it.
    pub entry_operator: String,
    /// Band last sent with `BAND`.
    pub(crate) band_sent: Option<Band>,
    /// Operator last sent with `OPERATOR`.
    pub(crate) operator_sent: Option<String>,
    by_user: bool,
    /// The last connection error, for the panel.
    pub error: Option<String>,
}

impl Session {
    /// Connect with TCP to the host and port in the settings.
    pub fn connect_tcp(&mut self) -> Result<(), &'static str> {
        let port = port_number(&self.settings.port).ok_or("bad-port")?;
        if self.settings.host.trim().is_empty() {
            return Err("no-host");
        }
        let tcp = Tcp::open(self.settings.host.trim(), port);
        self.connect(Box::new(tcp));
        Ok(())
    }

    /// Start a session on any transport.
    pub fn connect(&mut self, transport: Box<dyn Transport>) {
        self.disconnect();
        self.transport = Some(transport);
        self.state = State::Connecting;
        self.error = None;
        self.by_user = false;
        self.buffer.clear();
    }

    pub fn disconnect(&mut self) {
        if let Some(mut t) = self.transport.take() {
            t.close();
            self.by_user = true;
            self.closed(None);
        }
    }

    /// Whether every received QSO command has been applied.
    pub fn inbox_is_empty(&self) -> bool {
        self.inbox.is_empty() && !self.clearing
    }

    pub fn connected(&self) -> bool {
        self.state == State::Connected
    }

    /// `WriteData`: send one line with the configured line break, in Shift_JIS.
    /// Nothing is sent while not connected, as in zLog.
    pub fn write(&mut self, line: &str) {
        if !self.connected() {
            return;
        }
        let lb = self.settings.line_break.code();
        if let Some(t) = &mut self.transport {
            t.send(&wire::encode(&format!("{line}{lb}")));
        }
    }

    /// `SendOperator`: the operator of the entry row (`CurrentQSO.Operator`), which
    /// may be empty.
    pub(crate) fn send_operator(&mut self) {
        let op = self.entry_operator.clone();
        self.write(&protocol::operator(&op));
        self.operator_sent = Some(op);
    }

    /// `SendBand`.
    pub(crate) fn send_band(&mut self, band: Band) {
        self.write(&protocol::band(band));
        self.band_sent = Some(band);
    }

    /// Take what the transport received. Chat, rig status and commands this client
    /// does not act on are handled here; QSO commands wait for the link.
    pub fn pump(&mut self) {
        let events = match &mut self.transport {
            Some(t) => t.poll(),
            None => return,
        };
        for e in events {
            match e {
                NetEvent::Connected(addr) => {
                    self.state = State::Connected;
                    self.init_pending = true;
                    self.band_sent = None;
                    self.operator_sent = None;
                    self.log(format!("connected to {addr}"));
                }
                NetEvent::Data(bytes) => {
                    self.buffer.extend(bytes);
                    self.split_lines();
                }
                NetEvent::Closed(error) => {
                    self.transport = None;
                    self.closed(error);
                }
            }
        }
    }

    fn closed(&mut self, error: Option<String>) {
        // A line left without its line break is still a command (`SetCommandBuffer(True)`).
        if !self.buffer.is_empty() {
            self.buffer.push(b'\n');
            self.split_lines();
        }
        if let Some(e) = &error {
            self.log(e.clone());
        }
        if self.state != State::Disconnected {
            self.log("disconnected...".into());
        }
        if !self.by_user && self.error.is_none() {
            self.error = Some(error.unwrap_or_else(|| "closed".into()));
        }
        self.state = State::Disconnected;
        self.init_pending = false;
        self.merge = None;
        self.clearing = false;
    }

    /// zLog splits at CR LF only; this client also takes a bare LF (departure, kept:
    /// it loses nothing from a zLog peer, whose lines all end in CR LF, and reads
    /// servers or tools that end lines in LF alone).
    fn split_lines(&mut self) {
        while let Some(i) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=i).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let text = wire::decode(&line);
            self.receive(&text);
        }
    }

    /// One received line (`CommProcess` and the part of `ProcessCommand` that needs
    /// no log).
    pub(crate) fn receive(&mut self, line: &str) {
        if !line.contains("FILEDATA") {
            self.log(line.to_string());
        }
        let Some(m) = Message::parse(line) else {
            return;
        };
        match m {
            Message::Freq(s) => self.freqs.take(&s),
            Message::PutMessage { text, red } => {
                self.status.push(text.clone());
                self.chat.push(ChatLine { text, red });
            }
            Message::BeginMergeNg | Message::Other(_) => {}
            m => self.inbox.push_back(m),
        }
    }

    fn log(&mut self, line: String) {
        self.console.push_back(line);
        if self.console.len() > CONSOLE_LINES {
            self.console.pop_front();
        }
    }

    /// `TZLinkForm.LockQSO`: tell the others a QSO is being edited here.
    pub fn lock_qso(&mut self, q: &zlog_core::Qso) {
        if self.connected() {
            self.write(&protocol::lock_qso(q));
        }
    }

    /// `TZLinkForm.UnLockQSO`: the edit has ended. zLog declares it but never calls it
    /// (the edit that follows arrives as `EDITQSOTO`); sending it when the dialog
    /// closes releases the lock at the other stations (departure).
    pub fn unlock_qso(&mut self, q: &zlog_core::Qso) {
        if self.connected() {
            self.write(&protocol::unlock_qso(q));
        }
    }

    /// Send a chat line (`TChatForm.SendMessage`) and show it here.
    pub fn send_chat(&mut self, time_hhnn: &str, prompt: &str, text: &str) {
        if let Some((shown, line)) = protocol::chat(time_hhnn, prompt, text) {
            let red = text.starts_with('!');
            self.chat.push(ChatLine { text: shown, red });
            self.write(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A transport whose other end is the test.
    #[derive(Clone, Default)]
    struct Pipe(Arc<Mutex<(Vec<NetEvent>, Vec<u8>)>>);

    impl Transport for Pipe {
        fn send(&mut self, bytes: &[u8]) {
            self.0.lock().unwrap().1.extend_from_slice(bytes);
        }
        fn poll(&mut self) -> Vec<NetEvent> {
            std::mem::take(&mut self.0.lock().unwrap().0)
        }
        fn close(&mut self) {}
    }

    #[test]
    fn lines_are_split_and_decoded() {
        let pipe = Pipe::default();
        let mut s = Session::default();
        s.connect(Box::new(pipe.clone()));
        let mut data = b"#ZLOG# PUTMESSAGE 12:00     14> ".to_vec();
        data.extend(wire::encode("こんにちは\r\n#ZLOG# REN"));
        pipe.0.lock().unwrap().0 = vec![
            NetEvent::Connected("server".into()),
            NetEvent::Data(data),
            NetEvent::Data(b"EW\r\n".to_vec()),
        ];
        s.pump();
        assert!(s.connected());
        assert_eq!(s.chat[0].text, "12:00     14> こんにちは");
        assert_eq!(s.inbox.pop_front(), Some(Message::Renew));
        s.write("#ZLOG# BAND 2");
        assert_eq!(pipe.0.lock().unwrap().1, b"#ZLOG# BAND 2\r\n");
    }

    /// K4: a bare LF ends a line too (a departure from zLog, kept on purpose); an
    /// empty line and a lone CR are harmless.
    #[test]
    fn bare_lf_ends_a_line() {
        let pipe = Pipe::default();
        let mut s = Session::default();
        s.connect(Box::new(pipe.clone()));
        pipe.0.lock().unwrap().0 = vec![
            NetEvent::Connected("server".into()),
            NetEvent::Data(b"#ZLOG# RENEW\n\r\n\n#ZLOG# SENDLOG\r".to_vec()),
        ];
        s.pump();
        assert_eq!(s.inbox.pop_front(), Some(Message::Renew));
        assert_eq!(s.inbox.pop_front(), None);
        // The CR alone does not end the line.
        pipe.0.lock().unwrap().0 = vec![NetEvent::Data(b"\n".to_vec())];
        s.pump();
        assert_eq!(s.inbox.pop_front(), Some(Message::SendLog));
    }

    /// N6: the CALL prompt, and the operator falling back to the callsign.
    #[test]
    fn prompts() {
        use zlog_core::Band;
        let t = |p: Prompt, op| p.text(Band::B7, "PC1", op, "JA1ZLO");
        assert_eq!(t(Prompt::Band, "JR8PPG"), "7");
        assert_eq!(t(Prompt::PcName, "JR8PPG"), "PC1");
        assert_eq!(t(Prompt::Operator, "JR8PPG"), "JR8PPG");
        assert_eq!(t(Prompt::Operator, ""), "JA1ZLO");
        assert_eq!(t(Prompt::Call, "JR8PPG"), "JA1ZLO");
        assert_eq!(
            Prompt::ALL.map(Prompt::key),
            ["band", "pc-name", "operator", "call"]
        );
    }

    /// N8: lock lines go out only while connected.
    #[test]
    fn lock_needs_connection() {
        let q = zlog_core::Qso::default();
        let mut s = Session::default();
        s.lock_qso(&q);
        s.unlock_qso(&q);
        let pipe = Pipe::default();
        s.connect(Box::new(pipe.clone()));
        pipe.0.lock().unwrap().0 = vec![NetEvent::Connected("server".into())];
        s.pump();
        s.lock_qso(&q);
        s.unlock_qso(&q);
        let sent = String::from_utf8_lossy(&pipe.0.lock().unwrap().1).to_string();
        let lines: Vec<&str> = sent.lines().filter(|l| l.contains("LOCKQSO")).collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("#ZLOG# LOCKQSO "));
        assert!(lines[1].starts_with("#ZLOG# UNLOCKQSO "));
    }

    #[test]
    fn close_while_connecting_shuts_the_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        let shared = Mutex::new(Stream {
            stream: None,
            closed: true,
        });
        assert!(attach(&shared, client).is_none());
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut buf = [0u8; 8];
        assert_eq!(peer.read(&mut buf).unwrap(), 0);
        // Not closed: kept for close.
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let open = Mutex::new(Stream::default());
        assert!(attach(&open, client).is_some());
        assert!(open.lock().unwrap().stream.is_some());
    }

    #[test]
    fn ports() {
        assert_eq!(port_number("telnet"), Some(23));
        assert_eq!(port_number("1234"), Some(1234));
        assert_eq!(port_number("x"), None);
    }
}
