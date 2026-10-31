use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zlog_core::Band;

use crate::freq::FreqList;
use crate::protocol::{self, Message};
use crate::wire;

#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    Connected(String),
    Data(Vec<u8>),
    Closed(Option<String>),
}

pub trait Transport: Send {
    fn send(&mut self, bytes: &[u8]);
    fn poll(&mut self) -> Vec<NetEvent>;
    fn close(&mut self);
}

pub struct Tcp {
    out: Sender<Vec<u8>>,
    events: Receiver<NetEvent>,
    stream: Arc<Mutex<Stream>>,
}

#[derive(Default)]
struct Stream {
    stream: Option<TcpStream>,
    closed: bool,
}

fn attach(shared: &Mutex<Stream>, s: TcpStream) -> Option<TcpStream> {
    let mut st = shared.lock().unwrap();
    if st.closed {
        let _ = s.shutdown(Shutdown::Both);
        return None;
    }
    st.stream = s.try_clone().ok();
    Some(s)
}

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
            let s = match zlog_net::connect(&host, port, Duration::from_secs(10)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(NetEvent::Closed(Some(e)));
                    zlog_core::wake::wake();
                    return;
                }
            };
            let _ = s.set_nodelay(true);
            let Some(mut s) = attach(&shared, s) else {
                return;
            };
            let addr = s.peer_addr().map_or(host.clone(), |a| a.to_string());
            let _ = tx.send(NetEvent::Connected(addr));
            zlog_core::wake::wake();
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
                        zlog_core::wake::wake();
                        break;
                    }
                    Ok(n) => {
                        let _ = tx.send(NetEvent::Data(buf[..n].to_vec()));
                        zlog_core::wake::wake();
                    }
                    Err(e) => {
                        let _ = tx.send(NetEvent::Closed(Some(e.to_string())));
                        zlog_core::wake::wake();
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

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnConnect {
    #[default]
    Auto,
    Merge,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Prompt {
    #[default]
    Band,
    PcName,
    Operator,
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Settings {
    pub host: String,
    pub port: String,
    pub pc_name: String,
    pub line_break: LineBreak,
    pub on_connect: OnConnect,
    pub prompt: Prompt,
    pub role: crate::role::Role,
    pub tls: bool,
    pub recent: Vec<String>,
    pub auto_connect: bool,
    pub auto_start: bool,
}

impl Settings {
    pub fn address(&self) -> String {
        format!(
            "{}:{}",
            self.host.trim(),
            port_number(&self.port).unwrap_or(23)
        )
    }

    pub fn remember(&mut self) {
        let a = self.address();
        self.recent.retain(|r| *r != a);
        self.recent.insert(0, a);
        self.recent.truncate(8);
    }
}

pub const MAX_LINE: usize = 2048;

pub const LARGE_DELETION: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatLine {
    pub text: String,
    pub red: bool,
}

#[derive(Debug, Default)]
pub struct Merge {
    pub order: Vec<i32>,
    pub ids: HashMap<i32, i32>,
}

const CONSOLE_LINES: usize = 1000;

#[derive(Default)]
pub struct Session {
    pub settings: Settings,
    pub state: State,
    transport: Option<Box<dyn Transport>>,
    buffer: Vec<u8>,
    pub(crate) inbox: VecDeque<Message>,
    pub console: VecDeque<String>,
    pub chat: Vec<ChatLine>,
    pub freqs: FreqList,
    pub(crate) status: Vec<String>,
    pub(crate) init_pending: bool,
    pub(crate) merge: Option<Merge>,
    pub(crate) clearing: bool,
    pub entry_band: Option<Band>,
    pub entry_operator: String,
    pub(crate) band_sent: Option<Band>,
    pub(crate) operator_sent: Option<String>,
    by_user: bool,
    claimed: bool,
    pub secure: bool,
    pub backup: Option<std::path::PathBuf>,
    pub backed_up: Option<std::path::PathBuf>,
    pub error: Option<String>,
}

impl Session {
    pub fn connect_tcp(&mut self) -> Result<(), &'static str> {
        let port = port_number(&self.settings.port).ok_or("bad-port")?;
        if self.settings.host.trim().is_empty() {
            return Err("no-host");
        }
        self.claim()?;
        let tcp = Tcp::open(self.settings.host.trim(), port);
        self.connect(Box::new(tcp));
        self.claimed = true;
        self.secure = false;
        self.settings.remember();
        Ok(())
    }

    pub fn connect_secure(
        &mut self,
        net: Arc<crate::net::Shared>,
        pairing: crate::net::Pairing,
    ) -> Result<(), &'static str> {
        port_number(&self.settings.port).ok_or("bad-port")?;
        if self.settings.host.trim().is_empty() {
            return Err("no-host");
        }
        self.claim()?;
        let addr = self.settings.address();
        let name = self.settings.pc_name.clone();
        let t = crate::net::SecureTcp::open(net, &addr, &name, pairing);
        self.connect(Box::new(t));
        self.claimed = true;
        self.secure = true;
        self.settings.remember();
        Ok(())
    }

    fn claim(&mut self) -> Result<(), &'static str> {
        use crate::role::{self, Role};
        role::claim(Role::Client).map_err(|_| "role-busy")?;
        self.settings.role = Role::Client;
        Ok(())
    }

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

    pub fn inbox_is_empty(&self) -> bool {
        self.inbox.is_empty() && !self.clearing
    }

    pub fn active(&self) -> bool {
        self.transport.is_some()
    }

    pub fn connected(&self) -> bool {
        self.state == State::Connected
    }

    pub fn write(&mut self, line: &str) {
        if !self.connected() {
            return;
        }
        let lb = self.settings.line_break.code();
        if let Some(t) = &mut self.transport {
            t.send(&wire::encode(&format!("{line}{lb}")));
        }
    }

    pub(crate) fn send_operator(&mut self) {
        let op = self.entry_operator.clone();
        self.write(&protocol::operator(&op));
        self.operator_sent = Some(op);
    }

    pub(crate) fn send_band(&mut self, band: Band) {
        self.write(&protocol::band(band));
        self.band_sent = Some(band);
    }

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
        if std::mem::take(&mut self.claimed) {
            crate::role::release(crate::role::Role::Client);
        }
        self.init_pending = false;
        self.merge = None;
        self.clearing = false;
    }

    fn split_lines(&mut self) {
        while let Some(i) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=i).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.len() > MAX_LINE {
                self.log(format!("dropped a line of {} bytes", line.len()));
                continue;
            }
            let text = wire::decode(&line);
            self.receive(&text);
        }
        if self.buffer.len() > MAX_LINE {
            self.log(format!("dropped a line of {} bytes", self.buffer.len()));
            self.buffer.clear();
        }
    }

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

    pub fn lock_qso(&mut self, q: &zlog_core::Qso) {
        if self.connected() {
            self.write(&protocol::lock_qso(q));
        }
    }

    pub fn unlock_qso(&mut self, q: &zlog_core::Qso) {
        if self.connected() {
            self.write(&protocol::unlock_qso(q));
        }
    }

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
        pipe.0.lock().unwrap().0 = vec![NetEvent::Data(b"\n".to_vec())];
        s.pump();
        assert_eq!(s.inbox.pop_front(), Some(Message::SendLog));
    }

    #[test]
    fn prompts() {
        use zlog_core::Band;
        let t = |p: Prompt, op| p.text(Band::B7, "PC1", op, "JA1ZLO");
        assert_eq!(t(Prompt::Band, "JA1YWX"), "7");
        assert_eq!(t(Prompt::PcName, "JA1YWX"), "PC1");
        assert_eq!(t(Prompt::Operator, "JA1YWX"), "JA1YWX");
        assert_eq!(t(Prompt::Operator, ""), "JA1ZLO");
        assert_eq!(t(Prompt::Call, "JA1YWX"), "JA1ZLO");
        assert_eq!(
            Prompt::ALL.map(Prompt::key),
            ["band", "pc-name", "operator", "call"]
        );
    }

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
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let open = Mutex::new(Stream::default());
        assert!(attach(&open, client).is_some());
        assert!(open.lock().unwrap().stream.is_some());
    }

    #[test]
    fn data_from_the_server_wakes_the_window() {
        if !zlog_core::wake::alone(concat!(
            module_path!(),
            "::data_from_the_server_wakes_the_window"
        )) {
            return;
        }
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let woken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        zlog_core::wake::set(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let past = |n: usize| {
            let end = std::time::Instant::now() + Duration::from_secs(3);
            while woken.load(std::sync::atomic::Ordering::SeqCst) <= n
                && std::time::Instant::now() < end
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            woken.load(std::sync::atomic::Ordering::SeqCst) > n
        };
        let mut t = Tcp::open("127.0.0.1", server.local_addr().unwrap().port());
        let (mut peer, _) = server.accept().unwrap();
        assert!(past(0), "connected");
        let n = woken.load(std::sync::atomic::Ordering::SeqCst);
        peer.write_all(b"hello\r\n").unwrap();
        assert!(past(n), "data");
        let events = t.poll();
        assert!(
            matches!(events.first(), Some(NetEvent::Connected(_))),
            "{events:?}"
        );
        t.close();
    }

    #[test]
    fn offline_opens_never_reach_a_remote_server() {
        zlog_net::set_offline(true);
        let before = zlog_net::remote_connects();
        let mut t = Tcp::open("zserver.example.org", 23);
        let end = std::time::Instant::now() + Duration::from_secs(5);
        let mut events = Vec::new();
        while events.is_empty() && std::time::Instant::now() < end {
            events = t.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        zlog_net::set_offline(false);
        assert!(
            matches!(events.first(), Some(NetEvent::Closed(Some(e))) if e == "offline"),
            "{events:?}"
        );
        assert_eq!(zlog_net::remote_connects(), before);
    }

    #[test]
    fn ports() {
        assert_eq!(port_number("telnet"), Some(23));
        assert_eq!(port_number("1234"), Some(1234));
        assert_eq!(port_number("x"), None);
    }
}
