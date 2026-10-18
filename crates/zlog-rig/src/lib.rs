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
}

impl RigMode {
    pub fn to_log(self) -> Mode {
        match self {
            RigMode::Lsb | RigMode::Usb => Mode::Ssb,
            RigMode::Cw | RigMode::CwR => Mode::Cw,
            RigMode::Am => Mode::Am,
            RigMode::Fm => Mode::Fm,
            RigMode::Rtty | RigMode::RttyR => Mode::Rtty,
            RigMode::Data => Mode::Ft8,
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
    fn set_mode(&self, mode: RigMode) -> Vec<u8>;
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
            error: None,
            buf: Vec::new(),
        }
    }

    pub fn set_freq(&mut self, hz: u64) {
        let cmd = self.protocol.set_freq(self.state.vfo, hz);
        self.queue.push_back(cmd);
    }

    pub fn set_mode(&mut self, mode: RigMode) {
        let cmd = self.protocol.set_mode(mode);
        self.queue.push_back(cmd);
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
                Reply::Freq(Vfo::A, hz) => self.state.freq_a = Some(hz),
                Reply::Freq(Vfo::B, hz) => self.state.freq_b = Some(hz),
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
