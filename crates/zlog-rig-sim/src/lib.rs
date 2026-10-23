//! A simulated transceiver, so rig control can be developed and tested without a rig.
//!
//! [`SimRadio`] is the radio: VFOs, mode, power. [`SimTransport`] connects a
//! [`zlog_rig::Rig`] to it in place of a serial port, speaking the radio's side of
//! the protocol. The radio side is written here independently of `zlog-rig-cat`, so
//! a misreading of a protocol in one is not hidden by the same misreading in the other.
//!
//! The radio is shared through [`SimHandle`], so a test or a window can act as the
//! operator: turn the dial, change mode, switch the radio off. It can also echo every
//! frame (as the CI-V bus does), deliver replies in pieces, and announce changes
//! unasked (Icom transceive, Kenwood and Yaesu `AI1`).

use std::sync::{Arc, Mutex};

use zlog_rig::{RigMode, Transport, Vfo};

/// How the simulated radio talks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimProtocol {
    /// Icom CI-V with this address.
    Icom(u8),
    /// `;`-terminated text with this many frequency digits (11 Kenwood, 9 or 8 Yaesu).
    Text { digits: usize, yaesu: bool },
}

#[derive(Debug, Clone)]
pub struct SimRadio {
    pub protocol: SimProtocol,
    pub freq_a: u64,
    pub freq_b: u64,
    pub vfo: Vfo,
    pub mode: RigMode,
    /// A radio that is off answers nothing.
    pub power_on: bool,
    /// Echo every received byte back, as on a single-wire CI-V bus.
    pub echo: bool,
    /// Announce frequency and mode changes made at the radio.
    pub auto_info: bool,
    /// Deliver at most this many bytes per read (0 = all at once).
    pub chunk: usize,
    /// Commands received, for tests.
    pub received: Vec<Vec<u8>>,
}

impl SimRadio {
    pub fn new(protocol: SimProtocol) -> Self {
        SimRadio {
            protocol,
            freq_a: 7_012_000,
            freq_b: 7_100_000,
            vfo: Vfo::A,
            mode: RigMode::Cw,
            power_on: true,
            echo: false,
            auto_info: false,
            chunk: 0,
            received: Vec::new(),
        }
    }
}

/// The shared radio: one side is the transport, the other is the "operator".
#[derive(Clone)]
pub struct SimHandle(Arc<Mutex<Inner>>);

struct Inner {
    radio: SimRadio,
    out: Vec<u8>,
    inbuf: Vec<u8>,
}

impl SimHandle {
    pub fn new(radio: SimRadio) -> Self {
        SimHandle(Arc::new(Mutex::new(Inner {
            radio,
            out: Vec::new(),
            inbuf: Vec::new(),
        })))
    }

    pub fn transport(&self) -> SimTransport {
        SimTransport(self.clone())
    }

    /// Look at or change the radio; changes are not announced (use the methods below).
    pub fn with<R>(&self, f: impl FnOnce(&mut SimRadio) -> R) -> R {
        f(&mut self.0.lock().unwrap().radio)
    }

    /// The operator turns the dial of the VFO in use.
    pub fn tune(&self, hz: u64) {
        let mut g = self.0.lock().unwrap();
        match g.radio.vfo {
            Vfo::A => g.radio.freq_a = hz,
            Vfo::B => g.radio.freq_b = hz,
        }
        if g.radio.auto_info && g.radio.power_on {
            let msg = freq_message(&g.radio, g.radio.vfo, true);
            g.out.extend(msg);
        }
    }

    /// The operator switches to the other VFO (A/B key); not announced.
    pub fn select_vfo(&self, vfo: Vfo) {
        self.0.lock().unwrap().radio.vfo = vfo;
    }

    /// The operator presses a mode key.
    pub fn press_mode(&self, mode: RigMode) {
        let mut g = self.0.lock().unwrap();
        g.radio.mode = mode;
        if g.radio.auto_info && g.radio.power_on {
            let msg = mode_message(&g.radio, true);
            g.out.extend(msg);
        }
    }
}

pub struct SimTransport(SimHandle);

impl Transport for SimTransport {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut g = self.0 .0.lock().unwrap();
        if g.radio.echo {
            g.out.extend_from_slice(bytes);
        }
        g.inbuf.extend_from_slice(bytes);
        let inner = &mut *g;
        let (commands, rest) = split_commands(&inner.radio, &inner.inbuf);
        inner.inbuf = rest;
        for cmd in commands {
            inner.radio.received.push(cmd.clone());
            if inner.radio.power_on {
                let reply = answer(&mut inner.radio, &cmd);
                inner.out.extend(reply);
            }
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut Vec<u8>) -> std::io::Result<()> {
        let mut g = self.0 .0.lock().unwrap();
        let n = if g.radio.chunk == 0 {
            g.out.len()
        } else {
            g.radio.chunk.min(g.out.len())
        };
        buf.extend(g.out.drain(..n));
        Ok(())
    }
}

/// Whole commands in the input, and what is left over.
fn split_commands(radio: &SimRadio, input: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let end = match radio.protocol {
        SimProtocol::Icom(_) => 0xFD,
        SimProtocol::Text { .. } => b';',
    };
    let mut cmds = Vec::new();
    let mut rest = input;
    while let Some(i) = rest.iter().position(|b| *b == end) {
        cmds.push(rest[..=i].to_vec());
        rest = &rest[i + 1..];
    }
    (cmds, rest.to_vec())
}

// --- Icom radio side ------------------------------------------------------

fn bcd(hz: u64, bytes: usize) -> Vec<u8> {
    let digits: Vec<u8> = format!("{hz:0width$}", width = bytes * 2)
        .bytes()
        .map(|b| b - b'0')
        .collect();
    // Least significant pair first; each byte is (tens << 4) | units.
    digits.rchunks(2).map(|p| p[0] << 4 | p[1]).collect()
}

fn unbcd(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rev()
        .fold(0, |acc, b| acc * 100 + ((b >> 4) * 10 + (b & 0x0F)) as u64)
}

fn icom_mode_code(m: RigMode) -> u8 {
    match m {
        RigMode::Lsb => 0,
        RigMode::Usb | RigMode::Data => 1,
        RigMode::Am => 2,
        RigMode::Cw => 3,
        RigMode::Rtty => 4,
        RigMode::Fm => 5,
        RigMode::CwR => 7,
        RigMode::RttyR => 8,
        RigMode::Dv => 0x17,
        // A mode outside zLog's table (PSK on recent rigs).
        RigMode::Other => 0x12,
    }
}

fn icom_mode_of(c: u8) -> Option<RigMode> {
    [
        RigMode::Lsb,
        RigMode::Usb,
        RigMode::Am,
        RigMode::Cw,
        RigMode::Rtty,
        RigMode::Fm,
    ]
    .get(c as usize)
    .copied()
    .or(match c {
        7 => Some(RigMode::CwR),
        8 => Some(RigMode::RttyR),
        0x17 => Some(RigMode::Dv),
        _ => None,
    })
}

fn icom_frame(addr: u8, to: u8, body: &[u8]) -> Vec<u8> {
    let mut f = vec![0xFE, 0xFE, to, addr];
    f.extend_from_slice(body);
    f.push(0xFD);
    f
}

// --- text radio side ------------------------------------------------------

fn text_mode_code(m: RigMode, yaesu: bool) -> char {
    match m {
        RigMode::Lsb => '1',
        RigMode::Usb => '2',
        RigMode::Cw => '3',
        RigMode::Fm => '4',
        RigMode::Am => '5',
        RigMode::Rtty => '6',
        RigMode::CwR => '7',
        RigMode::RttyR => '9',
        RigMode::Data if yaesu => 'C',
        RigMode::Data => '2',
        // C4FM on Yaesu; Kenwood has no digital voice code, so it shows as FSK-R.
        RigMode::Dv if yaesu => 'E',
        RigMode::Dv => '9',
        RigMode::Other if yaesu => 'B',
        RigMode::Other => '0',
    }
}

fn text_mode_of(c: char) -> Option<RigMode> {
    Some(match c {
        '1' => RigMode::Lsb,
        '2' => RigMode::Usb,
        '3' => RigMode::Cw,
        '4' => RigMode::Fm,
        '5' => RigMode::Am,
        '6' => RigMode::Rtty,
        '7' => RigMode::CwR,
        '9' => RigMode::RttyR,
        'C' | '8' => RigMode::Data,
        'E' => RigMode::Dv,
        'B' | '0' => RigMode::Other,
        _ => return None,
    })
}

/// Frequency report; `unasked` marks a transceive broadcast (Icom command 00 to address 00).
fn freq_message(r: &SimRadio, vfo: Vfo, unasked: bool) -> Vec<u8> {
    let hz = if vfo == Vfo::A { r.freq_a } else { r.freq_b };
    match r.protocol {
        SimProtocol::Icom(addr) => {
            let (to, cmd) = if unasked { (0x00, 0x00) } else { (0xE0, 0x03) };
            let mut body = vec![cmd];
            body.extend(bcd(hz, if hz >= 10_000_000_000 { 6 } else { 5 }));
            icom_frame(addr, to, &body)
        }
        SimProtocol::Text { digits, .. } => {
            let cmd = if vfo == Vfo::A { "FA" } else { "FB" };
            format!("{cmd}{hz:0digits$};").into_bytes()
        }
    }
}

fn mode_message(r: &SimRadio, unasked: bool) -> Vec<u8> {
    match r.protocol {
        SimProtocol::Icom(addr) => {
            let (to, cmd) = if unasked { (0x00, 0x01) } else { (0xE0, 0x04) };
            icom_frame(addr, to, &[cmd, icom_mode_code(r.mode), 0x01])
        }
        SimProtocol::Text { yaesu, .. } => {
            let code = text_mode_code(r.mode, yaesu);
            if yaesu {
                format!("MD0{code};")
            } else {
                format!("MD{code};")
            }
            .into_bytes()
        }
    }
}

/// Status answer: Kenwood `IF` (frequency, mode and VFO in use), Yaesu `IF` (VFO A)
/// and `OI` (VFO B), laid out as the manufacturers' references give them.
fn info_message(r: &SimRadio, vfo: Vfo) -> Vec<u8> {
    let hz = if vfo == Vfo::A { r.freq_a } else { r.freq_b };
    match r.protocol {
        SimProtocol::Text {
            yaesu: false,
            digits,
        } => {
            let code = text_mode_code(r.mode, false);
            let v = if r.vfo == Vfo::A { '0' } else { '1' };
            // IF, frequency, 5 spaces, RIT offset, RIT, XIT, memory, TX, mode, VFO,
            // scan, split, tone, tone number, shift.
            format!("IF{hz:0digits$}     +0000000000{code}{v}000000;").into_bytes()
        }
        SimProtocol::Text {
            yaesu: true,
            digits,
        } => {
            let cmd = if vfo == Vfo::A { "IF" } else { "OI" };
            let code = text_mode_code(r.mode, true);
            // Memory channel, frequency, clarifier, RX/TX clarifier, mode, VFO/memory,
            // CTCSS, tone, shift.
            format!("{cmd}001{hz:0digits$}+000000{code}00000;").into_bytes()
        }
        SimProtocol::Icom(_) => Vec::new(),
    }
}

/// The radio's answer to one whole command.
fn answer(r: &mut SimRadio, cmd: &[u8]) -> Vec<u8> {
    match r.protocol {
        SimProtocol::Icom(addr) => {
            // FE FE to from cmd data.. FD
            if cmd.len() < 6 || cmd[0] != 0xFE || cmd[1] != 0xFE || cmd[2] != addr {
                return Vec::new();
            }
            let body = &cmd[4..cmd.len() - 1];
            let nak = icom_frame(addr, 0xE0, &[0xFA]);
            let ok = icom_frame(addr, 0xE0, &[0xFB]);
            match body.first() {
                Some(0x03) => freq_message(r, r.vfo, false),
                Some(0x04) => mode_message(r, false),
                Some(0x05) if body.len() >= 6 => {
                    let hz = unbcd(&body[1..]);
                    match r.vfo {
                        Vfo::A => r.freq_a = hz,
                        Vfo::B => r.freq_b = hz,
                    }
                    ok
                }
                Some(0x06) => match body.get(1).and_then(|c| icom_mode_of(*c)) {
                    Some(m) => {
                        r.mode = m;
                        ok
                    }
                    None => nak,
                },
                _ => nak,
            }
        }
        SimProtocol::Text { digits, yaesu } => {
            let text = String::from_utf8_lossy(cmd);
            let body = text.trim_end_matches(';');
            match body {
                "FA" => freq_message(r, Vfo::A, false),
                "FB" => freq_message(r, Vfo::B, false),
                "MD" if !yaesu => mode_message(r, false),
                "MD0" | "MD1" if yaesu => mode_message(r, false),
                "IF" => info_message(r, if yaesu { Vfo::A } else { r.vfo }),
                "OI" if yaesu => info_message(r, Vfo::B),
                // Yaesu VFO in use: FT (9 digits) answers 0/1, FR (8 digits) 0 or 2; FR on
                // the 9-digit FTDX101 answers the mute of main and sub, 01 or 10.
                "FT" if yaesu && digits == 9 => {
                    format!("FT{};", if r.vfo == Vfo::A { 0 } else { 1 }).into_bytes()
                }
                "FR" if yaesu && digits == 8 => {
                    format!("FR{};", if r.vfo == Vfo::A { 0 } else { 2 }).into_bytes()
                }
                "FR" if yaesu => {
                    format!("FR{};", if r.vfo == Vfo::A { "01" } else { "10" }).into_bytes()
                }
                "AI1" => {
                    r.auto_info = true;
                    Vec::new()
                }
                "AI0" => {
                    r.auto_info = false;
                    Vec::new()
                }
                _ if (body.starts_with("FA") || body.starts_with("FB"))
                    && body.len() == 2 + digits =>
                {
                    match body[2..].parse() {
                        Ok(hz) if body.starts_with("FA") => {
                            r.freq_a = hz;
                            Vec::new()
                        }
                        Ok(hz) => {
                            r.freq_b = hz;
                            Vec::new()
                        }
                        Err(_) => b"?;".to_vec(),
                    }
                }
                _ if body.starts_with("MD") => {
                    let code = body.chars().last().and_then(text_mode_of);
                    // Yaesu names the receiver: 0 main (VFO A), 1 sub (VFO B); a mode for
                    // the VFO not in use is refused here, to catch a wrong P1.
                    let p1 = if r.vfo == Vfo::A { "0" } else { "1" };
                    let p1_ok = !yaesu || body.get(2..3) == Some(p1);
                    let well_formed = p1_ok && body.len() == if yaesu { 4 } else { 3 };
                    match code {
                        Some(m) if well_formed => {
                            r.mode = m;
                            Vec::new()
                        }
                        _ => b"?;".to_vec(),
                    }
                }
                _ => b"?;".to_vec(),
            }
        }
    }
}
