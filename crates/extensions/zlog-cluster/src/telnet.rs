use std::io::{ErrorKind, Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::parse::is_login_prompt;

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

#[derive(Default)]
pub struct Filter {
    state: State,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum State {
    #[default]
    Data,
    Iac,
    Option(u8),
    Sub,
    SubIac,
}

impl Filter {
    pub fn feed(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut text = Vec::with_capacity(bytes.len());
        let mut reply = Vec::new();
        for &b in bytes {
            self.state = match (self.state, b) {
                (State::Data, IAC) => State::Iac,
                (State::Data, b) => {
                    text.push(b);
                    State::Data
                }
                (State::Iac, IAC) => {
                    text.push(IAC);
                    State::Data
                }
                (State::Iac, WILL | WONT | DO | DONT) => State::Option(b),
                (State::Iac, SB) => State::Sub,
                (State::Iac, _) => State::Data,
                (State::Option(verb), option) => {
                    match verb {
                        WILL => reply.extend([IAC, DONT, option]),
                        DO => reply.extend([IAC, WONT, option]),
                        _ => {}
                    }
                    State::Data
                }
                (State::Sub, IAC) => State::SubIac,
                (State::Sub, _) => State::Sub,
                (State::SubIac, SE) => State::Data,
                (State::SubIac, _) => State::Sub,
            };
        }
        (text, reply)
    }
}

#[derive(Default)]
pub struct Lines {
    pending: Vec<u8>,
}

impl Lines {
    pub fn push(&mut self, text: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(text);
        let mut out = Vec::new();
        while let Some(p) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=p).collect();
            out.push(decode(&line).trim_end_matches(['\r', '\n']).to_string());
        }
        out
    }

    pub fn partial(&self) -> String {
        decode(&self.pending)
    }
}

pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connected,
    Line(String),
    Closed(String),
}

pub struct Connection {
    to_thread: Sender<String>,
    from_thread: Receiver<Event>,
}

impl Connection {
    pub fn open(host: &str, port: u16, login: Option<String>) -> Self {
        let (to_thread, commands) = channel();
        let (events, from_thread) = channel();
        let address = format!("{host}:{port}");
        thread::spawn(move || {
            let reason = run(&address, login, &commands, &events)
                .err()
                .unwrap_or_default();
            let _ = events.send(Event::Closed(reason));
            zlog_core::wake::wake();
        });
        Connection {
            to_thread,
            from_thread,
        }
    }

    pub fn send(&self, line: &str) {
        let _ = self.to_thread.send(line.to_string());
    }

    pub fn poll(&self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            match self.from_thread.try_recv() {
                Ok(e) => out.push(e),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

fn run(
    address: &str,
    mut login: Option<String>,
    commands: &Receiver<String>,
    events: &Sender<Event>,
) -> Result<(), String> {
    let mut stream = zlog_net::connect_address(address, Duration::from_secs(15))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| e.to_string())?;
    let _ = events.send(Event::Connected);
    zlog_core::wake::wake();
    let mut filter = Filter::default();
    let mut lines = Lines::default();
    let mut prompt = false;
    let mut buf = [0u8; 4096];
    loop {
        loop {
            match commands.try_recv() {
                Ok(line) => stream
                    .write_all(format!("{line}\r\n").as_bytes())
                    .map_err(|e| e.to_string())?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
        match stream.read(&mut buf) {
            Ok(0) => return Err("closed by the server".into()),
            Ok(n) => {
                let (text, reply) = filter.feed(&buf[..n]);
                if !reply.is_empty() {
                    stream.write_all(&reply).map_err(|e| e.to_string())?;
                }
                let got = lines.push(&text);
                for line in &got {
                    prompt |= is_login_prompt(line);
                }
                if !got.is_empty() {
                    got.into_iter()
                        .for_each(|l| drop(events.send(Event::Line(l))));
                    zlog_core::wake::wake();
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.to_string()),
        }
        if login.is_some() && (prompt || is_login_prompt(&lines.partial())) {
            let call = login.take().unwrap_or_default();
            thread::sleep(Duration::from_millis(500));
            stream
                .write_all(format!("{call}\r\n").as_bytes())
                .map_err(|e| e.to_string())?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_runs_never_reach_a_remote_cluster() {
        zlog_net::set_offline(true);
        let (_tx, rx) = channel();
        let (etx, _erx) = channel();
        let before = zlog_net::remote_connects();
        let r = run("dxc.example.org:7300", None, &rx, &etx);
        zlog_net::set_offline(false);
        assert_eq!(r.unwrap_err(), "offline");
        assert_eq!(zlog_net::remote_connects(), before);
    }

    #[test]
    fn negotiation_is_refused_and_stripped() {
        let mut f = Filter::default();
        let (text, reply) = f.feed(&[b'a', IAC, DO, 1, b'b', IAC, WILL, 3, IAC, IAC]);
        assert_eq!(text, vec![b'a', b'b', IAC]);
        assert_eq!(reply, vec![IAC, WONT, 1, IAC, DONT, 3]);
    }

    fn prompt_after(chunks: &[&[u8]]) -> bool {
        let mut lines = Lines::default();
        let mut prompt = false;
        for chunk in chunks {
            for line in lines.push(chunk) {
                prompt |= is_login_prompt(&line);
            }
        }
        prompt || is_login_prompt(&lines.partial())
    }

    #[test]
    fn login_prompt_with_crlf_is_seen() {
        assert!(prompt_after(&[b"Welcome\r\nPlease enter your call:\r\n"]));
    }

    #[test]
    fn login_prompt_without_line_end_is_seen() {
        assert!(prompt_after(&[b"Welcome\r\nlogin: "]));
    }

    #[test]
    fn login_prompt_split_across_reads_is_seen() {
        assert!(prompt_after(&[b"Please enter yo", b"ur call:\r", b"\n"]));
    }

    #[test]
    fn no_prompt_in_plain_text() {
        assert!(!prompt_after(&[b"DX de JA1ABC: 7010.0 JA2XYZ\r\n", b""]));
        assert!(!prompt_after(&[]));
    }

    #[test]
    fn lines_split_and_keep_partial() {
        let mut l = Lines::default();
        assert!(l.push(b"").is_empty());
        assert_eq!(l.push(b"a\r\n\nb"), vec!["a", ""]);
        assert_eq!(l.partial(), "b");
        assert_eq!(l.push(b"c\n"), vec!["bc"]);
        assert_eq!(l.partial(), "");
    }

    #[test]
    fn lines_tolerate_invalid_utf8() {
        let mut l = Lines::default();
        assert_eq!(l.push(&[0xff, b'x', b'\n']), vec!["\u{fffd}x"]);
    }

    #[test]
    fn lines_decode_shift_jis() {
        let mut bytes = b"DX de JA1ZLO: 7010.0 JA1YAA ".to_vec();
        bytes.extend([0x93, 0x8c, 0x8b, 0x9e]);
        bytes.extend(b"\r\n");
        let mut l = Lines::default();
        assert_eq!(
            l.push(&bytes),
            vec!["DX de JA1ZLO: 7010.0 JA1YAA \u{6771}\u{4eac}"]
        );
        assert_eq!(l.push(&[0xb1, 0xb2]), Vec::<String>::new());
        assert_eq!(l.partial(), "\u{ff71}\u{ff72}");
    }

    #[test]
    fn lines_keep_utf8() {
        let mut l = Lines::default();
        assert_eq!(
            l.push("\u{6771}\u{4eac}\n".as_bytes()),
            vec!["\u{6771}\u{4eac}"]
        );
    }

    #[test]
    fn decode_edge_cases() {
        assert_eq!(decode(b""), "");
        assert_eq!(decode(b"CQ"), "CQ");
        assert_eq!(decode(&[b'A', 0x93]), "A\u{fffd}");
    }

    #[test]
    fn subnegotiation_across_reads() {
        let mut f = Filter::default();
        let (t1, _) = f.feed(&[b'x', IAC, SB, 24, 1]);
        let (t2, r2) = f.feed(&[IAC, SE, b'y']);
        assert_eq!(t1, b"x");
        assert_eq!(t2, b"y");
        assert!(r2.is_empty());
    }
}
