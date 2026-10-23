//! CAT protocols of the common transceiver families, as [`zlog_rig::Protocol`]s.
//!
//! - [`Icom`]: CI-V frames `FE FE <to> <from> <cmd> [data] FD`, BCD frequencies.
//! - [`TextCat`]: the `;`-terminated text commands of Kenwood, Elecraft and Yaesu,
//!   which differ in the number of frequency digits and in mode codes.
//!
//! Commands follow zLog (`URigCtrlIcom.pas`, `URigCtrlKenwood.pas`,
//! `URigCtrlYaesu.pas`) and the manufacturers' CAT references.

use zlog_core::Band;
use zlog_rig::{Protocol, Reply, RigMode, Vfo};

/// Address zLog uses for itself on the CI-V bus.
pub const ICOM_CONTROLLER: u8 = 0xE0;

/// Icom CI-V.
pub struct Icom {
    pub address: u8,
    buf: Vec<u8>,
    /// Polls alternate between frequency and mode.
    poll_mode: bool,
    /// Filter byte last reported per mode (`ModeWidth`), sent back with a mode change
    /// so the rig keeps its filter.
    widths: [Option<u8>; 8],
}

/// Slot of [`Icom::widths`]: one per log mode, as zLog keys `ModeWidth` by `TMode`.
fn width_slot(mode: RigMode) -> usize {
    match mode {
        RigMode::Lsb | RigMode::Usb => 0,
        RigMode::Cw | RigMode::CwR => 1,
        RigMode::Am => 2,
        RigMode::Fm => 3,
        RigMode::Rtty | RigMode::RttyR => 4,
        RigMode::Data => 5,
        RigMode::Dv => 6,
        RigMode::Other => 7,
    }
}

impl Icom {
    pub fn new(address: u8) -> Self {
        Icom {
            address,
            buf: Vec::new(),
            poll_mode: false,
            widths: [None; 8],
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
        // A long run of valid digits (line noise) must not overflow.
        hz = hz.checked_mul(100)?.checked_add((hi * 10 + lo) as u64)?;
    }
    Some(hz)
}

/// Mode code as `TICOM.ExecuteCommand` reads it: the BCD byte 17 is DV, and any code
/// it does not know is Other (`mOther`).
fn icom_mode(code: u8) -> RigMode {
    match code {
        0x00 => RigMode::Lsb,
        0x01 => RigMode::Usb,
        0x02 => RigMode::Am,
        0x03 => RigMode::Cw,
        0x04 => RigMode::Rtty,
        0x05 | 0x06 => RigMode::Fm,
        0x07 => RigMode::CwR,
        0x08 => RigMode::RttyR,
        0x17 => RigMode::Dv,
        _ => RigMode::Other,
    }
}

/// Mode code to send; none for data and other modes, for which zLog sends nothing
/// (`TICOM.SetMode` exits on modes other than SSB, CW, FM, AM, RTTY and DV; DV is 17).
fn icom_code(mode: RigMode) -> Option<u8> {
    Some(match mode {
        RigMode::Lsb => 0x00,
        RigMode::Usb => 0x01,
        RigMode::Am => 0x02,
        RigMode::Cw => 0x03,
        RigMode::Rtty => 0x04,
        RigMode::Fm => 0x05,
        RigMode::CwR => 0x07,
        RigMode::RttyR => 0x08,
        RigMode::Dv => 0x17,
        RigMode::Data | RigMode::Other => return None,
    })
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

    /// `06 <mode> [<filter>]`; the filter byte last reported for the mode (1 to 3) is
    /// sent along, as `TICOM.SetMode` does.
    fn set_mode(&self, _vfo: Vfo, mode: RigMode) -> Vec<u8> {
        let Some(code) = icom_code(mode) else {
            return Vec::new();
        };
        let mut body = vec![0x06, code];
        if let Some(w) = self.widths[width_slot(mode)].filter(|w| (1..=3).contains(w)) {
            body.push(w);
        }
        self.frame(&body)
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
                // No end in sight (line noise, a wrong baud rate): never grow without bound.
                if self.buf.len() > MAX_PENDING {
                    self.buf.clear();
                }
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
                    if let Some(m) = data.first().map(|c| icom_mode(*c)) {
                        if let Some(w) = data.get(1) {
                            self.widths[width_slot(m)] = Some(*w);
                        }
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

/// Bytes of an unfinished answer kept before it is dropped as noise; answers are far
/// shorter (the longest, a Kenwood `IF`, is under 40).
const MAX_PENDING: usize = 256;

/// Which text dialect: digits of `FA`/`FB`, the status commands and the mode table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Kenwood TS-590/890/990/2000 and Elecraft K3/K4: 11 digits, `MD3;`, status by `IF;`.
    Kenwood,
    /// Yaesu FT-991A, FTDX10, FT-710: 9 digits, `MD03;`, status by `IF;`/`OI;`,
    /// VFO by `FT;` (`TFT991`, `TFTDX10`).
    Yaesu9,
    /// Yaesu FTDX101: as [`Dialect::Yaesu9`], but the VFO by `FR;`, the receiver in
    /// use (`TFTDX101.PollingProcess`, `TFTDX101.FunctionRx`): in split the entry row
    /// follows the receiving VFO.
    Ftdx101,
    /// Yaesu FT-2000, FTDX3000/5000/9000: 8 digits, `MD03;`, status by `IF;`/`OI;`,
    /// VFO by `FR;` (`TFT2000`).
    Yaesu8,
}

/// Kenwood, Elecraft and Yaesu text CAT.
pub struct TextCat {
    pub dialect: Dialect,
    buf: String,
    /// Position in the cycle of status requests.
    step: usize,
    /// VFO in use as last reported; status of the other VFO carries no mode.
    vfo: Vfo,
    /// Kenwood: the rig was last seen in CW-R, so CW is set as CW-R (`_CWR`).
    cw_reverse: bool,
}

impl TextCat {
    pub fn new(dialect: Dialect) -> Self {
        TextCat {
            dialect,
            buf: String::new(),
            step: 0,
            vfo: Vfo::A,
            cw_reverse: false,
        }
    }

    fn digits(&self) -> usize {
        match self.dialect {
            Dialect::Kenwood => 11,
            Dialect::Yaesu9 | Dialect::Ftdx101 => 9,
            Dialect::Yaesu8 => 8,
        }
    }

    fn yaesu(&self) -> bool {
        self.dialect != Dialect::Kenwood
    }

    /// The status requests polled in turn. Kenwood: `IF;` gives the frequency, mode and
    /// VFO in use (`TTS690.PollingProcess`); `FA;`/`FB;` in between keep both VFOs shown.
    /// Yaesu: `IF;` (VFO A), `OI;` (VFO B) and the VFO in use (`TFT991.PollingProcess`,
    /// `TFT2000.PollingProcess`, `TFTDX101.PollingProcess`).
    fn cycle(&self) -> &'static [&'static str] {
        match self.dialect {
            Dialect::Kenwood => &["IF;", "FA;", "IF;", "FB;"],
            Dialect::Yaesu9 => &["IF;", "OI;", "FT;"],
            Dialect::Yaesu8 | Dialect::Ftdx101 => &["IF;", "OI;", "FR;"],
        }
    }

    /// Mode codes as zLog reads them; codes it does not know are Other (`mOther`).
    /// Kenwood: 6 and 8 are RTTY, 9 and anything else Other (`TTS690.ExecuteCommand`).
    /// Yaesu: 9 is RTTY; 0, 8 and A to E (data, narrow FM, AM-N, C4FM) are Other,
    /// since `StrToIntDef` reads no letter as a number (`TFT991.ExecuteCommand`,
    /// `TFT2000.ExecuteCommand`).
    fn mode_of(&self, code: char) -> RigMode {
        match code {
            '1' => RigMode::Lsb,
            '2' => RigMode::Usb,
            '3' => RigMode::Cw,
            '4' => RigMode::Fm,
            '5' => RigMode::Am,
            '6' => RigMode::Rtty,
            '7' => RigMode::CwR,
            '8' if !self.yaesu() => RigMode::Rtty,
            '9' if self.yaesu() => RigMode::RttyR,
            _ => RigMode::Other,
        }
    }

    /// A mode the rig reported; Kenwood remembers whether CW was reversed.
    fn reported(&mut self, mode: RigMode, out: &mut Vec<Reply>) {
        if !self.yaesu() {
            match mode {
                RigMode::Cw => self.cw_reverse = false,
                RigMode::CwR => self.cw_reverse = true,
                _ => {}
            }
        }
        out.push(Reply::Mode(mode));
    }

    fn code_of(&self, mode: RigMode) -> Option<char> {
        Some(match mode {
            RigMode::Lsb => '1',
            RigMode::Usb => '2',
            // `TTS690.SetMode`: CW stays reversed when the rig was in CW-R.
            RigMode::Cw if !self.yaesu() && self.cw_reverse => '7',
            RigMode::Cw => '3',
            RigMode::Fm => '4',
            RigMode::Am => '5',
            RigMode::Rtty => '6',
            RigMode::CwR => '7',
            RigMode::RttyR => '9',
            // zLog sends no mode for data modes to Yaesu (`TFT2000.SetMode` exits) and
            // CW to Kenwood (`TTS690.SetMode` defaults to 3); nothing is sent here.
            RigMode::Data | RigMode::Dv | RigMode::Other => return None,
        })
    }

    /// `IF` (Kenwood): `IF` + 11 digits of frequency + ... + mode at 30 + VFO at 31
    /// (1-based, `TTS690.ExecuteCommand`); VFO 2 (memory) leaves the VFO unchanged.
    fn kenwood_info(&mut self, reply: &str, out: &mut Vec<Reply>) {
        let b = reply.as_bytes();
        if b.len() < 37 {
            return;
        }
        match b[30] {
            b'0' => self.vfo = Vfo::A,
            b'1' => self.vfo = Vfo::B,
            _ => {}
        }
        out.push(Reply::Vfo(self.vfo));
        if let Ok(hz) = reply[2..13].parse() {
            out.push(Reply::Freq(self.vfo, hz));
        }
        let m = self.mode_of(b[29] as char);
        self.reported(m, out);
    }

    /// `IF` (VFO A) and `OI` (VFO B), Yaesu: `IF` + 3 digits of memory channel + the
    /// frequency + 5 of clarifier + 2 flags + mode + ... (27 characters with 8 digits,
    /// 28 with 9; `TFT2000.ExecuteCommand`, `TFT991.ExecuteCommand`).
    fn yaesu_info(&mut self, vfo: Vfo, reply: &str, out: &mut Vec<Reply>) {
        let d = self.digits();
        if reply.len() != 18 + d || !reply.is_ascii() {
            return;
        }
        if let Ok(hz) = reply[5..5 + d].parse() {
            out.push(Reply::Freq(vfo, hz));
        }
        // The mode of the VFO not in use is not the rig's mode.
        if vfo == self.vfo {
            if let Some(c) = reply[12 + d..].chars().next() {
                let m = self.mode_of(c);
                self.reported(m, out);
            }
        }
    }
}

impl Protocol for TextCat {
    fn answers_sets(&self) -> bool {
        false
    }

    fn poll_request(&mut self) -> Vec<u8> {
        let cycle = self.cycle();
        let request = cycle[self.step % cycle.len()];
        self.step = (self.step + 1) % cycle.len();
        request.as_bytes().to_vec()
    }

    fn set_freq(&self, vfo: Vfo, hz: u64) -> Vec<u8> {
        let cmd = if vfo == Vfo::A { "FA" } else { "FB" };
        format!("{cmd}{hz:0width$};", width = self.digits()).into_bytes()
    }

    /// Kenwood `MD<code>;`; Yaesu `MD<vfo><code>;` with 0 for VFO A and 1 for VFO B
    /// (`TFT2000.SetMode`).
    fn set_mode(&self, vfo: Vfo, mode: RigMode) -> Vec<u8> {
        let Some(code) = self.code_of(mode) else {
            return Vec::new();
        };
        if self.yaesu() {
            let p1 = if vfo == Vfo::A { '0' } else { '1' };
            format!("MD{p1}{code};").into_bytes()
        } else {
            format!("MD{code};").into_bytes()
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Vec<Reply> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        // Without a `;` for this long the stream is noise: drop it, never grow without bound.
        if !self.buf.contains(';') && self.buf.len() > MAX_PENDING {
            self.buf.clear();
        }
        while let Some(end) = self.buf.find(';') {
            let reply: String = self.buf.drain(..=end).collect();
            let reply = reply.trim_end_matches(';').trim();
            if reply == "?" {
                out.push(Reply::Rejected);
                continue;
            }
            // Every field is sliced by byte offset; a frame with a non-ASCII byte (line
            // noise, a wrong baud rate) is dropped whole, as the Yaesu path always did.
            if !reply.is_ascii() {
                continue;
            }
            let (cmd, rest) = reply.split_at(reply.len().min(2));
            match cmd {
                // Exactly the dialect's digits, as zLog's fixed-offset parsing assumes
                // (`TTS690.ExecuteCommand`, `TFT2000.ExecuteCommand`); a short or long
                // answer (noise, a wrong dialect) is dropped.
                "FA" | "FB"
                    if rest.len() == self.digits() && rest.bytes().all(|b| b.is_ascii_digit()) =>
                {
                    if let Ok(hz) = rest.parse::<u64>() {
                        out.push(Reply::Freq(if cmd == "FA" { Vfo::A } else { Vfo::B }, hz));
                    }
                }
                "IF" if !self.yaesu() => self.kenwood_info(reply, &mut out),
                "IF" => self.yaesu_info(Vfo::A, reply, &mut out),
                "OI" if self.yaesu() => self.yaesu_info(Vfo::B, reply, &mut out),
                // Yaesu VFO in use: `FT0`/`FT1` (`TFT991.FunctionTx`); `FR0`/`FR1` is VFO A,
                // other values VFO B (`TFT2000.FunctionRx`).
                "FT" if self.dialect == Dialect::Yaesu9 => {
                    let vfo = match rest {
                        "0" => Vfo::A,
                        "1" => Vfo::B,
                        _ => continue,
                    };
                    self.vfo = vfo;
                    out.push(Reply::Vfo(vfo));
                }
                // FTDX101 `FR<main mute><sub mute>`: 10 (main muted) is VFO B, 00, 01 and
                // 11 VFO A (`TFTDX101.FunctionRx`).
                "FR" if self.dialect == Dialect::Ftdx101 => {
                    let vfo = match rest {
                        "10" => Vfo::B,
                        "00" | "01" | "11" => Vfo::A,
                        _ => continue,
                    };
                    self.vfo = vfo;
                    out.push(Reply::Vfo(vfo));
                }
                "FR" if self.dialect == Dialect::Yaesu8 && !rest.is_empty() => {
                    self.vfo = if rest.starts_with(['0', '1']) {
                        Vfo::A
                    } else {
                        Vfo::B
                    };
                    out.push(Reply::Vfo(self.vfo));
                }
                "MD" => {
                    // Kenwood `MD3`, Yaesu `MD03` (the 0 is the receiver).
                    if let Some(c) = rest.chars().last() {
                        let m = self.mode_of(c);
                        self.reported(m, &mut out);
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
    /// Two stop bits for the models zLog opens that way (`TFT2000` and its kin).
    pub two_stop_bits: bool,
    /// Lowest and highest band the rig covers (`URigControl.pas`, `ICOMLIST` in
    /// `UzlogConst.pas`); band changes outside them are not sent (`TRig.SetBand`).
    pub min_band: Band,
    pub max_band: Band,
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

    /// Whether the rig covers a band.
    pub fn covers(&self, band: Band) -> bool {
        (self.min_band..=self.max_band).contains(&band)
    }
}

const fn model(
    name: &'static str,
    family: Family,
    baud: u32,
    two_stop_bits: bool,
    max_band: Band,
) -> Model {
    Model {
        name,
        family,
        baud,
        two_stop_bits,
        min_band: Band::B1_9,
        max_band,
    }
}

/// Models with default settings; addresses, stop bits and bands from zLog's tables.
pub const MODELS: &[Model] = &[
    model("IC-705", Family::Icom(0xA4), 19200, false, Band::B430),
    model("IC-7300", Family::Icom(0x94), 19200, false, Band::B50),
    model("IC-7610", Family::Icom(0x98), 19200, false, Band::B50),
    model("IC-7851", Family::Icom(0x8E), 19200, false, Band::B50),
    Model {
        min_band: Band::B144,
        ..model("IC-9700", Family::Icom(0xA2), 19200, false, Band::B1200)
    },
    model(
        "TS-590",
        Family::Text(Dialect::Kenwood),
        9600,
        false,
        Band::B50,
    ),
    model(
        "TS-890",
        Family::Text(Dialect::Kenwood),
        9600,
        false,
        Band::B50,
    ),
    model(
        "TS-990",
        Family::Text(Dialect::Kenwood),
        9600,
        false,
        Band::B50,
    ),
    model(
        "K3/K3S/KX3",
        Family::Text(Dialect::Kenwood),
        38400,
        false,
        Band::B50,
    ),
    model(
        "FT-991A",
        Family::Text(Dialect::Yaesu9),
        38400,
        true,
        Band::B430,
    ),
    model(
        "FTDX10",
        Family::Text(Dialect::Yaesu9),
        38400,
        true,
        Band::B50,
    ),
    model(
        "FTDX101",
        Family::Text(Dialect::Ftdx101),
        38400,
        true,
        Band::B50,
    ),
    // `TFT710.Create` sets one stop bit.
    model(
        "FT-710",
        Family::Text(Dialect::Yaesu9),
        38400,
        false,
        Band::B50,
    ),
    model(
        "FTDX5000",
        Family::Text(Dialect::Yaesu8),
        38400,
        true,
        Band::B50,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Random bytes, and frames that start like real replies but carry non-ASCII bytes
    /// at the slicing offsets, never panic any protocol (regression: a Kenwood `IF`
    /// reply with a multi-byte character panicked with "not a char boundary").
    #[test]
    fn feed_random_bytes_never_panics() {
        let mut k = TextCat::new(Dialect::Kenwood);
        let mut reply = b"IF0001234567".to_vec();
        reply.push(0xC3);
        reply.push(0xA9);
        reply.extend_from_slice(&[b'0'; 30]);
        reply.push(b';');
        assert!(k.feed(&reply).is_empty());
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let heads: [&[u8]; 9] = [
            b"IF",
            b"OI",
            b"FA",
            b"FB",
            b"FT",
            b"FR",
            b"MD",
            b"?",
            &[0xFE, 0xFE],
        ];
        for round in 0..20_000 {
            let mut protocols: Vec<Box<dyn Protocol>> = vec![
                Box::new(Icom::new(0x94)),
                Box::new(TextCat::new(Dialect::Kenwood)),
                Box::new(TextCat::new(Dialect::Yaesu9)),
                Box::new(TextCat::new(Dialect::Yaesu8)),
                Box::new(TextCat::new(Dialect::Ftdx101)),
            ];
            let mut bytes = heads[next() as usize % heads.len()].to_vec();
            let len = next() as usize % 48;
            for _ in 0..len {
                let r = next();
                bytes.push(match r % 4 {
                    0 => b'0' + (r >> 8) as u8 % 10,
                    1 => 0x80 | (r >> 8) as u8,
                    _ => (r >> 8) as u8,
                });
            }
            bytes.push(if round % 2 == 0 { b';' } else { 0xFD });
            for p in &mut protocols {
                p.feed(&bytes);
                p.feed(&bytes);
            }
        }
    }

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
    fn unfinished_answers_do_not_grow_without_bound() {
        let mut i = Icom::new(0x94);
        let mut noise = vec![0xFE, 0xFE];
        noise.extend([0x11; 400]);
        assert_eq!(i.feed(&noise), vec![]);
        assert!(i.buf.len() <= MAX_PENDING);
        let mut k = TextCat::new(Dialect::Kenwood);
        assert_eq!(k.feed(&[b'F'; 400]), vec![]);
        assert!(k.buf.len() <= MAX_PENDING);
        // Still works afterwards.
        assert_eq!(
            k.feed(b"FA00014025000;"),
            vec![Reply::Freq(Vfo::A, 14_025_000)]
        );
    }

    #[test]
    fn frequency_needs_the_dialects_digits() {
        let mut k = TextCat::new(Dialect::Kenwood);
        assert_eq!(k.feed(b"FA14025000;FB0001402500000;FA0001402500x;"), vec![]);
        let mut y = TextCat::new(Dialect::Yaesu8);
        assert_eq!(
            y.feed(b"FA014025000;FA14025000;"),
            vec![Reply::Freq(Vfo::A, 14_025_000)]
        );
    }

    #[test]
    fn text_dialects() {
        let k = TextCat::new(Dialect::Kenwood);
        assert_eq!(k.set_freq(Vfo::A, 14_025_000), b"FA00014025000;");
        assert_eq!(k.set_mode(Vfo::A, RigMode::Cw), b"MD3;");
        let mut y = TextCat::new(Dialect::Yaesu9);
        assert_eq!(y.set_freq(Vfo::B, 7_012_000), b"FB007012000;");
        assert_eq!(y.set_mode(Vfo::A, RigMode::Usb), b"MD02;");
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

    /// A Kenwood `IF` answer: frequency, mode code and VFO (0 A, 1 B, 2 memory).
    fn kenwood_if(hz: u64, mode: char, vfo: char) -> String {
        format!("IF{hz:011}     +0000000000{mode}{vfo}000000;")
    }

    /// A Yaesu `IF`/`OI` answer with `d` digits of frequency.
    fn yaesu_if(cmd: &str, d: usize, hz: u64, mode: char) -> String {
        format!("{cmd}001{hz:0d$}+000000{mode}00000;")
    }

    #[test]
    fn kenwood_polls_if_and_reads_the_vfo() {
        let mut k = TextCat::new(Dialect::Kenwood);
        let polls: Vec<Vec<u8>> = (0..5).map(|_| k.poll_request()).collect();
        assert_eq!(polls[0], b"IF;");
        assert_eq!(polls[2], b"IF;");
        assert_eq!(polls[4], b"IF;");
        assert_eq!(kenwood_if(14_025_000, '3', '1').len(), 38);
        assert_eq!(
            k.feed(kenwood_if(14_025_000, '3', '1').as_bytes()),
            vec![
                Reply::Vfo(Vfo::B),
                Reply::Freq(Vfo::B, 14_025_000),
                Reply::Mode(RigMode::Cw)
            ]
        );
        // Memory mode keeps the VFO last seen.
        assert_eq!(
            k.feed(kenwood_if(7_012_000, '2', '2').as_bytes())[..2],
            [Reply::Vfo(Vfo::B), Reply::Freq(Vfo::B, 7_012_000)]
        );
        // Short or malformed answers are ignored.
        assert!(k.feed(b"IF00014025000;").is_empty());
        assert!(k.feed(b"IF;").is_empty());
        assert!(k.feed(b";;").is_empty());
    }

    #[test]
    fn kenwood_mode_codes() {
        let mut k = TextCat::new(Dialect::Kenwood);
        assert_eq!(k.feed(b"MD8;"), vec![Reply::Mode(RigMode::Rtty)]);
        assert_eq!(k.feed(b"MD6;"), vec![Reply::Mode(RigMode::Rtty)]);
        assert_eq!(k.feed(b"MD7;"), vec![Reply::Mode(RigMode::CwR)]);
        // Codes zLog does not know switch the entry row to Other; 9 (FSK-R) included.
        for code in ["0", "9", "C"] {
            assert_eq!(
                k.feed(format!("MD{code};").as_bytes()),
                vec![Reply::Mode(RigMode::Other)],
                "{code}"
            );
        }
        assert!(k.feed(b"MD;").is_empty());
        assert_eq!(
            k.feed(kenwood_if(3_530_000, '9', '0').as_bytes())[2],
            Reply::Mode(RigMode::Other)
        );
        assert_eq!(
            k.feed(kenwood_if(3_530_000, '8', '0').as_bytes())[2],
            Reply::Mode(RigMode::Rtty)
        );
    }

    #[test]
    fn yaesu_polls_if_oi_and_the_vfo() {
        let mut y = TextCat::new(Dialect::Yaesu9);
        let polls: Vec<Vec<u8>> = (0..4).map(|_| y.poll_request()).collect();
        assert_eq!(polls, [&b"IF;"[..], b"OI;", b"FT;", b"IF;"]);
        let mut y8 = TextCat::new(Dialect::Yaesu8);
        let polls: Vec<Vec<u8>> = (0..3).map(|_| y8.poll_request()).collect();
        assert_eq!(polls, [&b"IF;"[..], b"OI;", b"FR;"]);

        assert_eq!(yaesu_if("IF", 9, 1, '3').len(), 28);
        assert_eq!(yaesu_if("IF", 8, 1, '3').len(), 27);
        // VFO A in use: IF gives frequency and mode, OI only VFO B's frequency.
        assert_eq!(
            y.feed(yaesu_if("IF", 9, 7_012_000, '3').as_bytes()),
            vec![Reply::Freq(Vfo::A, 7_012_000), Reply::Mode(RigMode::Cw)]
        );
        assert_eq!(
            y.feed(yaesu_if("OI", 9, 14_250_000, '2').as_bytes()),
            vec![Reply::Freq(Vfo::B, 14_250_000)]
        );
        // The operator switches to VFO B.
        assert_eq!(y.feed(b"FT1;"), vec![Reply::Vfo(Vfo::B)]);
        assert_eq!(
            y.feed(yaesu_if("OI", 9, 14_250_000, '2').as_bytes()),
            vec![Reply::Freq(Vfo::B, 14_250_000), Reply::Mode(RigMode::Usb)]
        );
        assert_eq!(y.feed(b"FT0;"), vec![Reply::Vfo(Vfo::A)]);
        assert!(y.feed(b"FT2;FT;").is_empty());
        // Wrong lengths are ignored.
        assert!(y.feed(b"IF0010070120;").is_empty());
        assert!(y
            .feed(yaesu_if("IF", 8, 7_012_000, '3').as_bytes())
            .is_empty());

        assert_eq!(
            y8.feed(yaesu_if("IF", 8, 7_012_000, '3').as_bytes()),
            vec![Reply::Freq(Vfo::A, 7_012_000), Reply::Mode(RigMode::Cw)]
        );
        assert_eq!(y8.feed(b"FR4;"), vec![Reply::Vfo(Vfo::B)]);
        assert_eq!(y8.feed(b"FR0;"), vec![Reply::Vfo(Vfo::A)]);
        assert!(y8.feed(b"FR;").is_empty());
    }

    /// Regression: Yaesu 8, A to E and 0 left the mode as it was; zLog reads them as
    /// `mOther` and the entry row goes to Other.
    #[test]
    fn yaesu_other_modes_are_other() {
        for (dialect, d) in [
            (Dialect::Yaesu9, 9),
            (Dialect::Yaesu8, 8),
            (Dialect::Ftdx101, 9),
        ] {
            let mut y = TextCat::new(dialect);
            for code in ['8', 'A', 'B', 'C', 'D', 'E', 'F', '0'] {
                assert_eq!(
                    y.feed(format!("MD0{code};").as_bytes()),
                    vec![Reply::Mode(RigMode::Other)],
                    "{dialect:?} {code}"
                );
                let r = y.feed(yaesu_if("IF", d, 7_012_000, code).as_bytes());
                assert_eq!(
                    r,
                    vec![Reply::Freq(Vfo::A, 7_012_000), Reply::Mode(RigMode::Other)],
                    "{dialect:?} {code}"
                );
            }
            assert_eq!(y.feed(b"MD09;"), vec![Reply::Mode(RigMode::RttyR)]);
            assert!(y.feed(b"MD;").is_empty());
        }
    }

    /// Regression: the FTDX101 was polled with `FT;` (the transmitting VFO); zLog
    /// polls `FR;` and reads the receiving VFO, 10 being VFO B (`TFTDX101`).
    #[test]
    fn ftdx101_follows_the_receiving_vfo() {
        let mut y = TextCat::new(Dialect::Ftdx101);
        let polls: Vec<Vec<u8>> = (0..4).map(|_| y.poll_request()).collect();
        assert_eq!(polls, [&b"IF;"[..], b"OI;", b"FR;", b"IF;"]);
        assert_eq!(y.feed(b"FR10;"), vec![Reply::Vfo(Vfo::B)]);
        // VFO B in use: its OI carries the mode.
        assert_eq!(
            y.feed(yaesu_if("OI", 9, 14_250_000, '2').as_bytes()),
            vec![Reply::Freq(Vfo::B, 14_250_000), Reply::Mode(RigMode::Usb)]
        );
        for p in ["00", "01", "11"] {
            assert_eq!(
                y.feed(format!("FR{p};").as_bytes()),
                vec![Reply::Vfo(Vfo::A)],
                "{p}"
            );
        }
        // Malformed answers leave the VFO alone; FT is not read.
        for bad in ["FR;", "FR1;", "FR0;", "FR2;", "FR100;", "FRx0;", "FT1;"] {
            assert!(y.feed(bad.as_bytes()).is_empty(), "{bad}");
        }
        assert_eq!(y.feed(b"FR10;FR00;").len(), 2);
        // The other 9-digit Yaesus still read FT, not FR.
        let mut y9 = TextCat::new(Dialect::Yaesu9);
        assert!(y9.feed(b"FR10;").is_empty());
        assert_eq!(y9.feed(b"FT1;"), vec![Reply::Vfo(Vfo::B)]);
        // Set commands are those of the 9-digit Yaesus.
        assert_eq!(y.set_freq(Vfo::B, 7_012_000), b"FB007012000;");
        assert_eq!(y.set_mode(Vfo::B, RigMode::Cw), b"MD13;");
    }

    /// Regression: a Kenwood seen in CW-R was switched to CW-U by a CW mode change;
    /// `TTS690.SetMode` sends 7 while `_CWR` is set.
    #[test]
    fn kenwood_cw_stays_reversed() {
        let mut k = TextCat::new(Dialect::Kenwood);
        assert_eq!(k.set_mode(Vfo::A, RigMode::Cw), b"MD3;");
        k.feed(b"MD7;");
        assert_eq!(k.set_mode(Vfo::A, RigMode::Cw), b"MD7;");
        // Other modes do not touch the flag.
        k.feed(b"MD2;");
        assert_eq!(k.set_mode(Vfo::A, RigMode::Cw), b"MD7;");
        assert_eq!(k.set_mode(Vfo::A, RigMode::Usb), b"MD2;");
        // CW-U reported (here in an IF answer) clears it.
        k.feed(kenwood_if(7_012_000, '3', '0').as_bytes());
        assert_eq!(k.set_mode(Vfo::A, RigMode::Cw), b"MD3;");
        k.feed(kenwood_if(7_012_000, '7', '0').as_bytes());
        assert_eq!(k.set_mode(Vfo::A, RigMode::Cw), b"MD7;");
        // Yaesu has no such flag.
        let mut y = TextCat::new(Dialect::Yaesu9);
        y.feed(b"MD07;");
        assert_eq!(y.set_mode(Vfo::A, RigMode::Cw), b"MD03;");
    }

    /// `?;` is a refused command, in every text dialect, alone or among replies.
    #[test]
    fn question_mark_is_rejected() {
        for d in [
            Dialect::Kenwood,
            Dialect::Yaesu9,
            Dialect::Yaesu8,
            Dialect::Ftdx101,
        ] {
            let mut p = TextCat::new(d);
            assert_eq!(p.feed(b"?;"), vec![Reply::Rejected], "{d:?}");
            assert_eq!(p.feed(b" ? ;"), vec![Reply::Rejected], "{d:?}");
            assert_eq!(p.feed(b"?"), vec![], "{d:?}");
            assert_eq!(p.feed(b";?;"), vec![Reply::Rejected, Reply::Rejected]);
            // `??` is not a refusal.
            assert!(p.feed(b"??;").is_empty(), "{d:?}");
        }
        // Icom refuses with FA.
        let mut i = Icom::new(0x94);
        assert_eq!(
            i.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD]),
            vec![Reply::Rejected]
        );
    }

    /// Regression: Icom mode 17 (DV) was dropped and DV could not be set; codes zLog
    /// does not know were dropped instead of going to Other (`TICOM.ExecuteCommand`,
    /// `TICOM.SetMode`).
    #[test]
    fn icom_dv_and_other_modes() {
        let mut p = Icom::new(0x94);
        assert_eq!(
            p.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x17, 0x01, 0xFD]),
            vec![Reply::Mode(RigMode::Dv)]
        );
        assert_eq!(
            p.feed(&[0xFE, 0xFE, 0x00, 0x94, 0x01, 0x17, 0xFD]),
            vec![Reply::Mode(RigMode::Dv)]
        );
        for code in [0x09, 0x10, 0x12, 0x22, 0x99, 0xFF] {
            assert_eq!(
                p.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x04, code, 0xFD]),
                vec![Reply::Mode(RigMode::Other)],
                "{code:#x}"
            );
        }
        assert_eq!(
            p.set_mode(Vfo::A, RigMode::Dv),
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x06, 0x17, 0x01, 0xFD]
        );
        assert!(p.set_mode(Vfo::A, RigMode::Other).is_empty());
        // DV keeps its own filter slot.
        assert_eq!(
            p.set_mode(Vfo::A, RigMode::Usb),
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x06, 0x01, 0xFD]
        );
        let k = TextCat::new(Dialect::Kenwood);
        assert!(k.set_mode(Vfo::A, RigMode::Dv).is_empty());
        assert!(k.set_mode(Vfo::A, RigMode::Other).is_empty());
    }

    #[test]
    fn mode_commands_name_the_vfo_and_skip_data() {
        let y = TextCat::new(Dialect::Yaesu9);
        assert_eq!(y.set_mode(Vfo::A, RigMode::Cw), b"MD03;");
        assert_eq!(y.set_mode(Vfo::B, RigMode::Lsb), b"MD11;");
        assert!(y.set_mode(Vfo::A, RigMode::Data).is_empty());
        let y8 = TextCat::new(Dialect::Yaesu8);
        assert_eq!(y8.set_mode(Vfo::B, RigMode::Rtty), b"MD16;");
        let k = TextCat::new(Dialect::Kenwood);
        assert_eq!(k.set_mode(Vfo::B, RigMode::CwR), b"MD7;");
        assert!(k.set_mode(Vfo::A, RigMode::Data).is_empty());
        let i = Icom::new(0x94);
        assert!(i.set_mode(Vfo::A, RigMode::Data).is_empty());
        assert_eq!(k.set_freq(Vfo::B, 0), b"FB00000000000;");
    }

    #[test]
    fn icom_keeps_the_filter() {
        let mut p = Icom::new(0x94);
        // No filter reported yet: none sent.
        assert_eq!(
            p.set_mode(Vfo::A, RigMode::Usb),
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x06, 0x01, 0xFD]
        );
        // The rig reports LSB with filter 2; USB (same log mode) keeps it.
        p.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x00, 0x02, 0xFD]);
        assert_eq!(
            p.set_mode(Vfo::A, RigMode::Usb),
            vec![0xFE, 0xFE, 0x94, 0xE0, 0x06, 0x01, 0x02, 0xFD]
        );
        // CW has its own filter, not yet known.
        assert_eq!(p.set_mode(Vfo::A, RigMode::Cw).len(), 7);
        p.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x03, 0x03, 0xFD]);
        assert_eq!(p.set_mode(Vfo::A, RigMode::CwR)[6], 0x03);
        // Out-of-range filter bytes are not sent back.
        p.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x05, 0x07, 0xFD]);
        assert_eq!(p.set_mode(Vfo::A, RigMode::Fm).len(), 7);
        // A mode reply without a filter byte keeps the one known.
        p.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x01, 0x00, 0xFD]);
        assert_eq!(p.set_mode(Vfo::A, RigMode::Lsb)[6], 0x02);
    }

    #[test]
    fn icom_malformed_frames() {
        let mut p = Icom::new(0x94);
        assert!(p.feed(&[]).is_empty());
        assert!(p.feed(&[0xFE, 0xFE, 0xFD]).is_empty());
        assert!(p.feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x04, 0xFD]).is_empty());
        // Below 1.8 MHz is ignored, as in `TICOM.ExecuteCommand`.
        assert!(p
            .feed(&[0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0xFD])
            .is_empty());
    }

    #[test]
    fn model_table() {
        let get = |n: &str| *MODELS.iter().find(|m| m.name == n).unwrap();
        let icom = [
            ("IC-705", 0xA4, Band::B1_9, Band::B430),
            ("IC-7300", 0x94, Band::B1_9, Band::B50),
            ("IC-7610", 0x98, Band::B1_9, Band::B50),
            ("IC-7851", 0x8E, Band::B1_9, Band::B50),
            ("IC-9700", 0xA2, Band::B144, Band::B1200),
        ];
        for (name, addr, lo, hi) in icom {
            let m = get(name);
            assert_eq!(m.family, Family::Icom(addr), "{name}");
            assert_eq!((m.min_band, m.max_band), (lo, hi), "{name}");
            assert_eq!((m.baud, m.two_stop_bits), (19200, false), "{name}");
        }
        let text = [
            ("TS-590", Dialect::Kenwood, 9600, false, Band::B50),
            ("TS-890", Dialect::Kenwood, 9600, false, Band::B50),
            ("TS-990", Dialect::Kenwood, 9600, false, Band::B50),
            ("K3/K3S/KX3", Dialect::Kenwood, 38400, false, Band::B50),
            ("FT-991A", Dialect::Yaesu9, 38400, true, Band::B430),
            ("FTDX10", Dialect::Yaesu9, 38400, true, Band::B50),
            ("FTDX101", Dialect::Ftdx101, 38400, true, Band::B50),
            ("FT-710", Dialect::Yaesu9, 38400, false, Band::B50),
            ("FTDX5000", Dialect::Yaesu8, 38400, true, Band::B50),
        ];
        for (name, dialect, baud, two, hi) in text {
            let m = get(name);
            assert_eq!(m.family, Family::Text(dialect), "{name}");
            assert_eq!((m.baud, m.two_stop_bits), (baud, two), "{name}");
            assert_eq!((m.min_band, m.max_band), (Band::B1_9, hi), "{name}");
        }
        assert_eq!(MODELS.len(), icom.len() + text.len());
        let names: std::collections::HashSet<_> = MODELS.iter().map(|m| m.name).collect();
        assert_eq!(names.len(), MODELS.len());
        assert!(get("IC-7300").covers(Band::B50));
        assert!(!get("IC-7300").covers(Band::B144));
        assert!(!get("IC-9700").covers(Band::B50));
        assert!(get("IC-9700").covers(Band::B144));
    }
}
