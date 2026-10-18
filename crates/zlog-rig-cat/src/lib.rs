//! CAT protocols of the common transceiver families, as [`zlog_rig::Protocol`]s.
//!
//! - [`Icom`]: CI-V frames `FE FE <to> <from> <cmd> [data] FD`, BCD frequencies.
//! - [`TextCat`]: the `;`-terminated text commands of Kenwood, Elecraft and Yaesu,
//!   which differ in the number of frequency digits and in mode codes.
//!
//! Commands follow zLog (`URigCtrlIcom.pas`, `URigCtrlKenwood.pas`,
//! `URigCtrlYaesu.pas`) and the manufacturers' CAT references.

use zlog_rig::{Protocol, Reply, RigMode, Vfo};

/// Address zLog uses for itself on the CI-V bus.
pub const ICOM_CONTROLLER: u8 = 0xE0;

/// Icom CI-V.
pub struct Icom {
    pub address: u8,
    buf: Vec<u8>,
    /// Polls alternate between frequency and mode.
    poll_mode: bool,
}

impl Icom {
    pub fn new(address: u8) -> Self {
        Icom {
            address,
            buf: Vec::new(),
            poll_mode: false,
        }
    }

    fn frame(&self, body: &[u8]) -> Vec<u8> {
        let mut f = vec![0xFE, 0xFE, self.address, ICOM_CONTROLLER];
        f.extend_from_slice(body);
        f.push(0xFD);
        f
    }
}

/// Frequency in hertz as CI-V BCD, least significant byte first (5 bytes, 6 above 10 GHz).
pub fn to_bcd(hz: u64, bytes: usize) -> Vec<u8> {
    let mut n = hz;
    (0..bytes)
        .map(|_| {
            let lo = (n % 10) as u8;
            n /= 10;
            let hi = (n % 10) as u8;
            n /= 10;
            hi << 4 | lo
        })
        .collect()
}

pub fn from_bcd(bytes: &[u8]) -> Option<u64> {
    let mut hz = 0u64;
    for b in bytes.iter().rev() {
        let (hi, lo) = (b >> 4, b & 0x0F);
        if hi > 9 || lo > 9 {
            return None;
        }
        hz = hz * 100 + (hi * 10 + lo) as u64;
    }
    Some(hz)
}

fn icom_mode(code: u8) -> Option<RigMode> {
    Some(match code {
        0x00 => RigMode::Lsb,
        0x01 => RigMode::Usb,
        0x02 => RigMode::Am,
        0x03 => RigMode::Cw,
        0x04 => RigMode::Rtty,
        0x05 | 0x06 => RigMode::Fm,
        0x07 => RigMode::CwR,
        0x08 => RigMode::RttyR,
        _ => return None,
    })
}

fn icom_code(mode: RigMode) -> u8 {
    match mode {
        RigMode::Lsb => 0x00,
        RigMode::Usb | RigMode::Data => 0x01,
        RigMode::Am => 0x02,
        RigMode::Cw => 0x03,
        RigMode::Rtty => 0x04,
        RigMode::Fm => 0x05,
        RigMode::CwR => 0x07,
        RigMode::RttyR => 0x08,
    }
}

impl Protocol for Icom {
    fn answers_sets(&self) -> bool {
        true
    }

    fn poll_request(&mut self) -> Vec<u8> {
        self.poll_mode = !self.poll_mode;
        self.frame(&[if self.poll_mode { 0x03 } else { 0x04 }])
    }

    fn set_freq(&self, _vfo: Vfo, hz: u64) -> Vec<u8> {
        let mut body = vec![0x05];
        body.extend(to_bcd(hz, if hz >= 10_000_000_000 { 6 } else { 5 }));
        self.frame(&body)
    }

    fn set_mode(&self, mode: RigMode) -> Vec<u8> {
        self.frame(&[0x06, icom_code(mode)])
    }

    fn feed(&mut self, bytes: &[u8]) -> Vec<Reply> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            // Resynchronise on the preamble; drop anything before it.
            let Some(start) = self.buf.windows(2).position(|w| w == [0xFE, 0xFE]) else {
                self.buf.clear();
                break;
            };
            let Some(len) = self.buf[start..].iter().position(|b| *b == 0xFD) else {
                self.buf.drain(..start);
                break;
            };
            let frame: Vec<u8> = self.buf[start..start + len].to_vec();
            self.buf.drain(..start + len + 1);
            // frame = FE FE to from cmd data...
            if frame.len() < 5 {
                continue;
            }
            let (to, from, cmd, data) = (frame[2], frame[3], frame[4], &frame[5..]);
            // Our own frames echoed on the bus, and frames for other devices, are skipped.
            if from != self.address || (to != ICOM_CONTROLLER && to != 0x00) {
                continue;
            }
            match cmd {
                0x00 | 0x03 => {
                    if let Some(hz) =
                        from_bcd(data).filter(|hz| (1_800_000..=10_245_000_000).contains(hz))
                    {
                        out.push(Reply::Freq(Vfo::A, hz));
                    }
                }
                0x01 | 0x04 => {
                    if let Some(m) = data.first().and_then(|c| icom_mode(*c)) {
                        out.push(Reply::Mode(m));
                    }
                }
                0xFB => out.push(Reply::Ok),
                0xFA => out.push(Reply::Rejected),
                _ => {}
            }
        }
        out
    }
}

/// Which text dialect: digits of `FA`/`FB` and the mode table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Kenwood TS-590/890/990/2000 and Elecraft K3/K4: 11 digits, `MD3;`.
    Kenwood,
    /// Yaesu FT-991A, FTDX10/101, FT-710: 9 digits, `MD03;`.
    Yaesu9,
    /// Yaesu FT-2000, FTDX3000/5000/9000: 8 digits, `MD03;`.
    Yaesu8,
}

/// Kenwood, Elecraft and Yaesu text CAT.
pub struct TextCat {
    pub dialect: Dialect,
    buf: String,
    poll_mode: bool,
}

impl TextCat {
    pub fn new(dialect: Dialect) -> Self {
        TextCat {
            dialect,
            buf: String::new(),
            poll_mode: false,
        }
    }

    fn digits(&self) -> usize {
        match self.dialect {
            Dialect::Kenwood => 11,
            Dialect::Yaesu9 => 9,
            Dialect::Yaesu8 => 8,
        }
    }

    fn mode_of(&self, code: char) -> Option<RigMode> {
        let kenwood = self.dialect == Dialect::Kenwood;
        Some(match code {
            '1' => RigMode::Lsb,
            '2' => RigMode::Usb,
            '3' => RigMode::Cw,
            '4' => RigMode::Fm,
            '5' => RigMode::Am,
            '6' => RigMode::Rtty,
            '7' => RigMode::CwR,
            '9' => RigMode::RttyR,
            '8' | 'C' if !kenwood => RigMode::Data,
            'A' | 'B' if !kenwood => RigMode::Fm,
            'D' if !kenwood => RigMode::Am,
            _ => return None,
        })
    }

    fn code_of(&self, mode: RigMode) -> char {
        match mode {
            RigMode::Lsb => '1',
            RigMode::Usb => '2',
            RigMode::Cw => '3',
            RigMode::Fm => '4',
            RigMode::Am => '5',
            RigMode::Rtty => '6',
            RigMode::CwR => '7',
            RigMode::RttyR => '9',
            RigMode::Data if self.dialect == Dialect::Kenwood => '2',
            RigMode::Data => 'C',
        }
    }

    fn mode_command(&self) -> &'static str {
        if self.dialect == Dialect::Kenwood {
            "MD"
        } else {
            "MD0"
        }
    }
}

impl Protocol for TextCat {
    fn answers_sets(&self) -> bool {
        false
    }

    fn poll_request(&mut self) -> Vec<u8> {
        self.poll_mode = !self.poll_mode;
        if self.poll_mode {
            b"FA;".to_vec()
        } else {
            format!("{};", self.mode_command()).into_bytes()
        }
    }

    fn set_freq(&self, vfo: Vfo, hz: u64) -> Vec<u8> {
        let cmd = if vfo == Vfo::A { "FA" } else { "FB" };
        format!("{cmd}{hz:0width$};", width = self.digits()).into_bytes()
    }

    fn set_mode(&self, mode: RigMode) -> Vec<u8> {
        format!("{}{};", self.mode_command(), self.code_of(mode)).into_bytes()
    }

    fn feed(&mut self, bytes: &[u8]) -> Vec<Reply> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        while let Some(end) = self.buf.find(';') {
            let reply: String = self.buf.drain(..=end).collect();
            let reply = reply.trim_end_matches(';').trim();
            if reply == "?" {
                out.push(Reply::Rejected);
                continue;
            }
            let (cmd, rest) = reply.split_at(reply.len().min(2));
            match cmd {
                "FA" | "FB" => {
                    if let Ok(hz) = rest.parse::<u64>() {
                        out.push(Reply::Freq(if cmd == "FA" { Vfo::A } else { Vfo::B }, hz));
                    }
                }
                "MD" => {
                    // Kenwood `MD3`, Yaesu `MD03` (the 0 is the receiver).
                    if let Some(m) = rest.chars().last().and_then(|c| self.mode_of(c)) {
                        out.push(Reply::Mode(m));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// A rig model: the name shown in settings and how to talk to it.
#[derive(Debug, Clone, Copy)]
pub struct Model {
    pub name: &'static str,
    pub family: Family,
    pub baud: u32,
    /// Two stop bits for the models zLog opens that way (TS-690, FT-2000 class).
    pub two_stop_bits: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Icom(u8),
    Text(Dialect),
}

impl Model {
    pub fn protocol(&self) -> Box<dyn Protocol> {
        match self.family {
            Family::Icom(a) => Box::new(Icom::new(a)),
            Family::Text(d) => Box::new(TextCat::new(d)),
        }
    }
}

/// Models with default settings; addresses and digit counts from zLog's tables.
pub const MODELS: &[Model] = &[
    Model {
        name: "IC-705",
        family: Family::Icom(0xA4),
        baud: 19200,
        two_stop_bits: false,
    },
    Model {
        name: "IC-7300",
        family: Family::Icom(0x94),
        baud: 19200,
        two_stop_bits: false,
    },
    Model {
        name: "IC-7610",
        family: Family::Icom(0x98),
        baud: 19200,
        two_stop_bits: false,
    },
    Model {
        name: "IC-7851",
        family: Family::Icom(0x8E),
        baud: 19200,
        two_stop_bits: false,
    },
    Model {
        name: "IC-9700",
        family: Family::Icom(0xA2),
        baud: 19200,
        two_stop_bits: false,
    },
    Model {
        name: "TS-590",
        family: Family::Text(Dialect::Kenwood),
        baud: 9600,
        two_stop_bits: false,
    },
    Model {
        name: "TS-890",
        family: Family::Text(Dialect::Kenwood),
        baud: 9600,
        two_stop_bits: false,
    },
    Model {
        name: "TS-990",
        family: Family::Text(Dialect::Kenwood),
        baud: 9600,
        two_stop_bits: false,
    },
    Model {
        name: "K3/K3S/KX3",
        family: Family::Text(Dialect::Kenwood),
        baud: 38400,
        two_stop_bits: false,
    },
    Model {
        name: "FT-991A",
        family: Family::Text(Dialect::Yaesu9),
        baud: 38400,
        two_stop_bits: true,
    },
    Model {
        name: "FTDX10",
        family: Family::Text(Dialect::Yaesu9),
        baud: 38400,
        two_stop_bits: true,
    },
    Model {
        name: "FTDX101",
        family: Family::Text(Dialect::Yaesu9),
        baud: 38400,
        two_stop_bits: true,
    },
    Model {
        name: "FT-710",
        family: Family::Text(Dialect::Yaesu9),
        baud: 38400,
        two_stop_bits: true,
    },
    Model {
        name: "FTDX5000",
        family: Family::Text(Dialect::Yaesu8),
        baud: 38400,
        two_stop_bits: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcd_round_trip() {
        assert_eq!(to_bcd(14_025_000, 5), vec![0x00, 0x50, 0x02, 0x14, 0x00]);
        assert_eq!(from_bcd(&[0x00, 0x50, 0x02, 0x14, 0x00]), Some(14_025_000));
        assert_eq!(from_bcd(&[0xAA]), None);
    }

    #[test]
    fn icom_frames_echo_and_garbage() {
        let mut p = Icom::new(0x94);
        assert_eq!(
            p.set_freq(Vfo::A, 14_025_000),
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x05, 0x00, 0x50, 0x02, 0x14, 0x00, 0xFD]
        );
        // Garbage, our own echo, then the reply split across two reads.
        let echo = p.frame(&[0x03]);
        let mut bytes = vec![0x12, 0x34];
        bytes.extend(&echo);
        bytes.extend([0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x50]);
        assert!(p.feed(&bytes).is_empty());
        assert_eq!(
            p.feed(&[0x02, 0x14, 0x00, 0xFD]),
            vec![Reply::Freq(Vfo::A, 14_025_000)]
        );
        assert_eq!(
            p.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x03, 0x01, 0xFD]),
            vec![Reply::Mode(RigMode::Cw)]
        );
    }

    #[test]
    fn text_dialects() {
        let k = TextCat::new(Dialect::Kenwood);
        assert_eq!(k.set_freq(Vfo::A, 14_025_000), b"FA00014025000;");
        assert_eq!(k.set_mode(RigMode::Cw), b"MD3;");
        let mut y = TextCat::new(Dialect::Yaesu9);
        assert_eq!(y.set_freq(Vfo::B, 7_012_000), b"FB007012000;");
        assert_eq!(y.set_mode(RigMode::Usb), b"MD02;");
        assert_eq!(y.feed(b"FA0140"), vec![]);
        assert_eq!(
            y.feed(b"25000;MD03;?;"),
            vec![
                Reply::Freq(Vfo::A, 14_025_000),
                Reply::Mode(RigMode::Cw),
                Reply::Rejected
            ]
        );
    }
}
