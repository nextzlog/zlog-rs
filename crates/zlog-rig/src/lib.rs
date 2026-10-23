//! Rig control: the device interface for transceivers.
//!
//! - [`Transport`]: bytes in and out (a serial port, or the simulator).
//! - [`Protocol`]: builds requests and parses replies of one CAT family; no I/O.
//! - [`Rig`]: one rig, polling it with one request in flight at a time (as zLog does,
//!   the next poll goes out after the reply or a timeout) and keeping its state.
//! - [`sync`]: zLog's rules between the rig and the entry row.
//!
//! Time is passed in, never read from the clock, so tests can run the rig and the
//! simulator step by step.

pub mod sync;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use zlog_core::Mode;

/// Operating mode as rigs know it; [`RigMode::to_log`] maps it to the log's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RigMode {
    Lsb,
    Usb,
    Cw,
    CwR,
    Am,
    Fm,
    Rtty,
    RttyR,
    /// SSB with the data flag set (FT8 and other digital modes).
    Data,
    /// Digital voice (Icom DV, mode code 17; `TICOM.ExecuteCommand`).
    Dv,
    /// A mode code zLog does not know (`mOther`): the entry row goes to Other.
    Other,
}

impl RigMode {
    /// The name shown to the operator, upper case as rigs and zLog write modes
    /// (`ModeString`: CW, SSB, FM, AM, RTTY, Other, DV).
    pub fn label(self) -> &'static str {
        match self {
            RigMode::Lsb => "LSB",
            RigMode::Usb => "USB",
            RigMode::Cw => "CW",
            RigMode::CwR => "CW-R",
            RigMode::Am => "AM",
            RigMode::Fm => "FM",
            RigMode::Rtty => "RTTY",
            RigMode::RttyR => "RTTY-R",
            RigMode::Data => "DATA",
            RigMode::Dv => "DV",
            RigMode::Other => "Other",
        }
    }

    pub fn to_log(self) -> Mode {
        match self {
            RigMode::Lsb | RigMode::Usb => Mode::Ssb,
            RigMode::Cw | RigMode::CwR => Mode::Cw,
            RigMode::Am => Mode::Am,
            RigMode::Fm => Mode::Fm,
            RigMode::Rtty | RigMode::RttyR => Mode::Rtty,
            RigMode::Data => Mode::Ft8,
            RigMode::Dv => Mode::Dv,
            RigMode::Other => Mode::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vfo {
    #[default]
    A,
    B,
}

/// What a rig last reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RigState {
    pub freq_a: Option<u64>,
    pub freq_b: Option<u64>,
    pub vfo: Vfo,
    pub mode: Option<RigMode>,
}

impl RigState {
    /// Frequency of the VFO in use, in hertz.
    pub fn freq(&self) -> Option<u64> {
        match self.vfo {
            Vfo::A => self.freq_a,
            Vfo::B => self.freq_b,
        }
    }
}

/// A piece of information parsed from the rig's replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Freq(Vfo, u64),
    Mode(RigMode),
    Vfo(Vfo),
    /// The rig accepted a command.
    Ok,
    /// The rig refused a command.
    Rejected,
}

/// Bytes to and from a rig. Reads never block: they return what has arrived.
pub trait Transport: Send {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn read(&mut self, buf: &mut Vec<u8>) -> std::io::Result<()>;
}

/// One CAT family: requests as bytes, replies parsed from bytes. No I/O.
pub trait Protocol: Send {
    /// The next status request; families that need several (frequency, then mode)
    /// return them in turn.
    fn poll_request(&mut self) -> Vec<u8>;
    fn set_freq(&self, vfo: Vfo, hz: u64) -> Vec<u8>;
    /// The mode command for the VFO in use; empty when the family has no command for
    /// the mode (then nothing is sent).
    fn set_mode(&self, vfo: Vfo, mode: RigMode) -> Vec<u8>;
    /// Whether the rig answers a set command (Icom replies `FB`; Kenwood and Yaesu stay
    /// silent). When it does not, the next request goes out without waiting.
    fn answers_sets(&self) -> bool;
    /// Take bytes received so far; return the replies completed by them.
    /// Partial replies stay buffered until their end arrives.
    fn feed(&mut self, bytes: &[u8]) -> Vec<Reply>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RigError {
    Io(String),
    /// No reply within the timeout (the rig is off or unplugged).
    NoReply,
}

/// One rig being controlled.
pub struct Rig {
    protocol: Box<dyn Protocol>,
    transport: Box<dyn Transport>,
    pub state: RigState,
    /// Commands from the operator, sent before the next poll.
    queue: VecDeque<Vec<u8>>,
    waiting_since: Option<Instant>,
    next_poll: Option<Instant>,
    pub interval: Duration,
    pub timeout: Duration,
    /// Transverter offset in hertz (`_freqoffset`): added to what the rig reports and
    /// taken off what it is told, so [`Rig::state`] holds the real frequency. zLog keeps
    /// the dial frequency in its `FreqMem` (`TRig.UpdateFreqMem` stores `_currentfreq`);
    /// here the band memory holds real frequencies, which is the same once the offset
    /// is applied both ways.
    pub freq_offset: u64,
    pub error: Option<RigError>,
    buf: Vec<u8>,
}

impl Rig {
    /// zLog polls every 150 ms at 9600 bps or slower, 75 ms when faster.
    pub fn new(protocol: Box<dyn Protocol>, transport: Box<dyn Transport>, baud: u32) -> Self {
        let interval = if baud <= 9600 { 150 } else { 75 };
        Rig {
            protocol,
            transport,
            state: RigState::default(),
            queue: VecDeque::new(),
            waiting_since: None,
            next_poll: None,
            interval: Duration::from_millis(interval),
            timeout: Duration::from_millis(1000),
            freq_offset: 0,
            error: None,
            buf: Vec::new(),
        }
    }

    /// Tune the VFO in use to a real frequency (the transverter offset is taken off).
    pub fn set_freq(&mut self, hz: u64) {
        let dial = hz.saturating_sub(self.freq_offset);
        let cmd = self.protocol.set_freq(self.state.vfo, dial);
        self.queue.push_back(cmd);
    }

    /// Set the mode of the VFO in use; modes the family has no command for are skipped.
    pub fn set_mode(&mut self, mode: RigMode) {
        let cmd = self.protocol.set_mode(self.state.vfo, mode);
        if !cmd.is_empty() {
            self.queue.push_back(cmd);
        }
    }

    /// Run the rig up to `now`; returns true when its state changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let before = self.state;
        self.buf.clear();
        if let Err(e) = self.transport.read(&mut self.buf) {
            self.error = Some(RigError::Io(e.to_string()));
        }
        let replies = self.protocol.feed(&self.buf);
        if !replies.is_empty() {
            self.waiting_since = None;
            self.next_poll = Some(now + self.interval);
            self.error = None;
        }
        for r in replies {
            match r {
                Reply::Freq(Vfo::A, hz) => {
                    self.state.freq_a = Some(hz.saturating_add(self.freq_offset))
                }
                Reply::Freq(Vfo::B, hz) => {
                    self.state.freq_b = Some(hz.saturating_add(self.freq_offset))
                }
                Reply::Mode(m) => self.state.mode = Some(m),
                Reply::Vfo(v) => self.state.vfo = v,
                Reply::Ok | Reply::Rejected => {}
            }
        }
        if let Some(since) = self.waiting_since {
            if now.duration_since(since) >= self.timeout {
                self.error = Some(RigError::NoReply);
                self.waiting_since = None;
                self.next_poll = Some(now + self.interval);
            }
        }
        // One request in flight at a time; operator commands go first.
        if self.waiting_since.is_none() && self.next_poll.is_none_or(|t| now >= t) {
            let (request, answered) = match self.queue.pop_front() {
                Some(cmd) => (cmd, self.protocol.answers_sets()),
                None => (self.protocol.poll_request(), true),
            };
            match self.transport.write(&request) {
                Ok(()) if answered => self.waiting_since = Some(now),
                // A silent set command: the next request may follow at once.
                Ok(()) => self.next_poll = None,
                Err(e) => self.error = Some(RigError::Io(e.to_string())),
            }
        }
        self.state != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_are_shown_in_capitals() {
        let all = [
            RigMode::Lsb,
            RigMode::Usb,
            RigMode::Cw,
            RigMode::CwR,
            RigMode::Am,
            RigMode::Fm,
            RigMode::Rtty,
            RigMode::RttyR,
            RigMode::Data,
            RigMode::Dv,
        ];
        for m in all {
            assert_eq!(m.label(), m.label().to_uppercase(), "{m:?}");
        }
        assert_eq!(RigMode::Am.label(), "AM");
        assert_eq!(RigMode::Rtty.label(), "RTTY");
    }
    use std::sync::{Arc, Mutex};

    /// A protocol that answers nothing itself; records what the rig writes.
    struct Echo;

    impl Protocol for Echo {
        fn poll_request(&mut self) -> Vec<u8> {
            b"?".to_vec()
        }
        fn set_freq(&self, vfo: Vfo, hz: u64) -> Vec<u8> {
            format!("{vfo:?}{hz}").into_bytes()
        }
        fn set_mode(&self, vfo: Vfo, mode: RigMode) -> Vec<u8> {
            if mode == RigMode::Data {
                Vec::new()
            } else {
                format!("{vfo:?}{mode:?}").into_bytes()
            }
        }
        fn answers_sets(&self) -> bool {
            false
        }
        fn feed(&mut self, bytes: &[u8]) -> Vec<Reply> {
            let s = String::from_utf8_lossy(bytes);
            s.split(';')
                .filter_map(|r| r.parse().ok().map(|hz| Reply::Freq(Vfo::A, hz)))
                .collect()
        }
    }

    /// Bytes written by the rig, and bytes waiting for it to read.
    type Lines = (Vec<Vec<u8>>, Vec<u8>);

    #[derive(Clone, Default)]
    struct Wire(Arc<Mutex<Lines>>);

    impl Transport for Wire {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            self.0.lock().unwrap().0.push(bytes.to_vec());
            Ok(())
        }
        fn read(&mut self, buf: &mut Vec<u8>) -> std::io::Result<()> {
            buf.append(&mut self.0.lock().unwrap().1);
            Ok(())
        }
    }

    fn rig(baud: u32) -> (Wire, Rig) {
        let w = Wire::default();
        (w.clone(), Rig::new(Box::new(Echo), Box::new(w), baud))
    }

    #[test]
    fn polling_interval_follows_the_speed() {
        for (baud, ms) in [
            (1200, 150),
            (4800, 150),
            (9600, 150),
            (19200, 75),
            (115200, 75),
        ] {
            assert_eq!(rig(baud).1.interval, Duration::from_millis(ms), "{baud}");
        }
        assert_eq!(rig(9600).1.timeout, Duration::from_millis(1000));
    }

    #[test]
    fn next_poll_waits_for_the_interval_after_a_reply() {
        let (w, mut r) = rig(19200);
        let t = Instant::now();
        r.tick(t);
        assert_eq!(w.0.lock().unwrap().0.len(), 1);
        w.0.lock().unwrap().1.extend(b"7012000;");
        r.tick(t + Duration::from_millis(10));
        // The reply came in; the next poll waits 75 ms.
        r.tick(t + Duration::from_millis(80));
        assert_eq!(w.0.lock().unwrap().0.len(), 1);
        r.tick(t + Duration::from_millis(85));
        assert_eq!(w.0.lock().unwrap().0.len(), 2);
    }

    #[test]
    fn modes_without_a_command_are_not_sent() {
        let (w, mut r) = rig(9600);
        r.set_mode(RigMode::Data);
        r.set_mode(RigMode::Cw);
        let t = Instant::now();
        r.tick(t);
        r.tick(t);
        assert_eq!(w.0.lock().unwrap().0[0], b"ACw");
    }

    #[test]
    fn set_commands_go_to_the_vfo_in_use() {
        let (w, mut r) = rig(9600);
        r.state.vfo = Vfo::B;
        r.set_freq(14_025_000);
        r.set_mode(RigMode::Usb);
        let t = Instant::now();
        r.tick(t);
        r.tick(t);
        let sent = w.0.lock().unwrap().0.clone();
        assert_eq!(sent[..2], [b"B14025000".to_vec(), b"BUsb".to_vec()]);
    }

    #[test]
    fn transverter_offset_both_ways() {
        let (w, mut r) = rig(9600);
        r.freq_offset = 1_150_000_000;
        let t = Instant::now();
        r.set_freq(1_294_050_000);
        r.tick(t);
        assert_eq!(w.0.lock().unwrap().0[0], b"A144050000");
        w.0.lock().unwrap().1.extend(b"144050000;");
        r.tick(t);
        assert_eq!(r.state.freq(), Some(1_294_050_000));
        // A huge offset saturates instead of overflowing.
        r.freq_offset = u64::MAX - 10;
        w.0.lock().unwrap().1.extend(b"144050000;");
        r.tick(t);
        assert_eq!(r.state.freq(), Some(u64::MAX));
        r.freq_offset = 1_150_000_000;
        // An offset above the frequency asked for does not wrap.
        r.set_freq(1_000);
        r.tick(t + Duration::from_secs(1));
        assert_eq!(w.0.lock().unwrap().0.last().unwrap(), b"A0");
    }
}

/// A real serial port.
#[cfg(feature = "serial")]
pub mod serial {
    use std::io::{Read, Write};
    use std::time::Duration;

    pub use serialport::{available_ports, DataBits, StopBits};

    pub struct SerialTransport(Box<dyn serialport::SerialPort>);

    impl SerialTransport {
        pub fn open(path: &str, baud: u32, stop_bits: StopBits) -> Result<Self, String> {
            serialport::new(path, baud)
                .stop_bits(stop_bits)
                .data_bits(DataBits::Eight)
                .timeout(Duration::from_millis(1))
                .open()
                .map(SerialTransport)
                .map_err(|e| e.to_string())
        }
    }

    impl super::Transport for SerialTransport {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            self.0.write_all(bytes)
        }

        fn read(&mut self, buf: &mut Vec<u8>) -> std::io::Result<()> {
            let n = self.0.bytes_to_read()? as usize;
            if n > 0 {
                let mut chunk = vec![0; n];
                let got = self.0.read(&mut chunk)?;
                buf.extend_from_slice(&chunk[..got]);
            }
            Ok(())
        }
    }
}
