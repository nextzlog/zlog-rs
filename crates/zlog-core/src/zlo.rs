//! ZLO (256-byte records) and ZLOX (384-byte records) files.
//!
//! Both are arrays of Delphi packed records (`TQSOData`, `TQSODataEx`), little endian.
//! Strings are Delphi `ShortString`s (length byte, then Shift_JIS text).
//! Record 0 is the log header, not a contact; see [`crate::log::LogHeader`].

use encoding_rs::SHIFT_JIS;

use crate::qso::{DelphiTime, QslState, Qso};
use crate::types::{Band, Mode, Power};

pub const ZLO_RECORD: usize = 256;
pub const ZLOX_RECORD: usize = 384;
const ZLOX_MAGIC: &[u8; 4] = b"ZLOX";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zlo,
    Zlox,
}

impl Format {
    pub fn record_size(self) -> usize {
        match self {
            Format::Zlo => ZLO_RECORD,
            Format::Zlox => ZLOX_RECORD,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ZloError {
    #[error("file is empty")]
    Empty,
    #[error("record {index}: invalid {field} byte {value}")]
    BadEnum {
        index: usize,
        field: &'static str,
        value: u8,
    },
}

/// Detect the format: ZLOX starts with the magic `ZLOX`, everything else is ZLO.
pub fn detect(bytes: &[u8]) -> Format {
    if bytes.starts_with(ZLOX_MAGIC) {
        Format::Zlox
    } else {
        Format::Zlo
    }
}

/// Decode all records, header included.
///
/// As zLog does, a partial record at the end (left by an interrupted write) is
/// ignored; [`trailing_bytes`] tells how many bytes that was.
pub fn read_records(bytes: &[u8]) -> Result<(Format, Vec<Qso>), ZloError> {
    let format = detect(bytes);
    let size = format.record_size();
    if bytes.len() < size {
        return Err(ZloError::Empty);
    }
    let records = bytes[..bytes.len() / size * size]
        .chunks_exact(size)
        .enumerate()
        .map(|(index, rec)| decode(rec, format, index))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((format, records))
}

/// Bytes after the last whole record, which [`read_records`] ignores.
pub fn trailing_bytes(bytes: &[u8]) -> usize {
    bytes.len() % detect(bytes).record_size()
}

/// Encode all records, header included. The ZLOX magic is written into record 0.
pub fn write_records(records: &[Qso], format: Format) -> Vec<u8> {
    let mut out = Vec::with_capacity(records.len() * format.record_size());
    for (i, q) in records.iter().enumerate() {
        let mut rec = encode(q, format);
        if i == 0 && format == Format::Zlox {
            rec[0..4].copy_from_slice(ZLOX_MAGIC);
            let n = (records.len() - 1) as i32;
            rec[4..8].copy_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&rec);
    }
    out
}

fn decode(r: &[u8], format: Format, index: usize) -> Result<Qso, ZloError> {
    let byte_enum = |field: &'static str, value: u8, ok: bool| {
        if ok {
            Ok(())
        } else {
            Err(ZloError::BadEnum {
                index,
                field,
                value,
            })
        }
    };
    let mode = Mode::from_byte(r[92]);
    let band = Band::from_byte(r[93]);
    let power = Power::from_byte(r[94]);
    byte_enum("mode", r[92], mode.is_some())?;
    byte_enum("band", r[93], band.is_some())?;
    byte_enum("power", r[94], power.is_some())?;

    let time = if index == 0 && r.starts_with(ZLOX_MAGIC) {
        DelphiTime(0.0)
    } else {
        DelphiTime(f64_at(r, 0))
    };
    let mut q = Qso {
        time,
        callsign: short_string(r, 8, 12),
        nr_sent: short_string(r, 21, 30),
        nr_rcvd: short_string(r, 52, 30),
        rst_sent: u16_at(r, 84),
        rst_rcvd: u16_at(r, 86),
        serial: i32_at(r, 88),
        mode: mode.unwrap(),
        band: band.unwrap(),
        power: power.unwrap(),
        multi1: short_string(r, 95, 30),
        multi2: short_string(r, 126, 30),
        new_multi1: r[157] != 0,
        new_multi2: r[158] != 0,
        points: r[159],
        operator: short_string(r, 160, 14),
        memo: short_string(r, 175, 64),
        cq: r[240] != 0,
        dupe: r[241] != 0,
        reserve: r[242],
        tx: r[243],
        power2: i32_at(r, 244),
        reserve2: i32_at(r, 248),
        reserve3: i32_at(r, 252),
        ..Qso::default()
    };
    if format == Format::Zlox {
        q.freq = short_string(r, 256, 10);
        q.qsy_violation = r[267] != 0;
        q.pc_name = short_string(r, 268, 10);
        q.forced = r[279] != 0;
        q.qsl_state = QslState::from_byte(r[280]);
        q.invalid = r[281] != 0;
        q.rbn_verified = r[282] != 0;
        q.continent = short_string(r, 283, 2);
        q.entity = short_string(r, 286, 10);
        q.multi3 = short_string(r, 297, 30);
        q.new_multi3 = r[328] != 0;
    }
    Ok(q)
}

fn encode(q: &Qso, format: Format) -> Vec<u8> {
    let mut r = vec![0u8; format.record_size()];
    r[0..8].copy_from_slice(&q.time.0.to_le_bytes());
    put_short_string(&mut r, 8, 12, &q.callsign);
    put_short_string(&mut r, 21, 30, &q.nr_sent);
    put_short_string(&mut r, 52, 30, &q.nr_rcvd);
    r[84..86].copy_from_slice(&q.rst_sent.to_le_bytes());
    r[86..88].copy_from_slice(&q.rst_rcvd.to_le_bytes());
    r[88..92].copy_from_slice(&q.serial.to_le_bytes());
    r[92] = q.mode as u8;
    r[93] = q.band as u8;
    r[94] = q.power as u8;
    put_short_string(&mut r, 95, 30, &q.multi1);
    put_short_string(&mut r, 126, 30, &q.multi2);
    r[157] = q.new_multi1 as u8;
    r[158] = q.new_multi2 as u8;
    r[159] = q.points;
    put_short_string(&mut r, 160, 14, &q.operator);
    put_short_string(&mut r, 175, 64, &q.memo);
    r[240] = q.cq as u8;
    r[241] = q.dupe as u8;
    r[242] = q.reserve;
    r[243] = q.tx;
    r[244..248].copy_from_slice(&q.power2.to_le_bytes());
    r[248..252].copy_from_slice(&q.reserve2.to_le_bytes());
    r[252..256].copy_from_slice(&q.reserve3.to_le_bytes());
    if format == Format::Zlox {
        put_short_string(&mut r, 256, 10, &q.freq);
        r[267] = q.qsy_violation as u8;
        put_short_string(&mut r, 268, 10, &q.pc_name);
        r[279] = q.forced as u8;
        r[280] = q.qsl_state.to_byte();
        r[281] = q.invalid as u8;
        r[282] = q.rbn_verified as u8;
        put_short_string(&mut r, 283, 2, &q.continent);
        put_short_string(&mut r, 286, 10, &q.entity);
        put_short_string(&mut r, 297, 30, &q.multi3);
        r[328] = q.new_multi3 as u8;
    }
    r
}

fn u16_at(r: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(r[at..at + 2].try_into().unwrap())
}

fn i32_at(r: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}

fn f64_at(r: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(r[at..at + 8].try_into().unwrap())
}

fn short_string(r: &[u8], at: usize, max: usize) -> String {
    let len = (r[at] as usize).min(max);
    let (text, _, _) = SHIFT_JIS.decode(&r[at + 1..at + 1 + len]);
    text.into_owned()
}

/// Write a ShortString; text longer than `max` bytes is cut at a character boundary.
fn put_short_string(r: &mut [u8], at: usize, max: usize, s: &str) {
    let (bytes, _, _) = SHIFT_JIS.encode(s);
    let mut len = bytes.len().min(max);
    // Do not cut a two-byte Shift_JIS character in half.
    let mut i = 0;
    while i < len {
        let lead = bytes[i];
        let width = if (0x81..=0x9f).contains(&lead) || (0xe0..=0xfc).contains(&lead) {
            2
        } else {
            1
        };
        if i + width > len {
            len = i;
            break;
        }
        i += width;
    }
    r[at] = len as u8;
    r[at + 1..at + 1 + len].copy_from_slice(&bytes[..len]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Qso> {
        let header = Qso {
            rst_sent: 32767,
            rst_rcvd: 100,
            ..Qso::default()
        };
        let q = Qso {
            time: DelphiTime::from_ymd_hms(2026, 4, 25, 21, 3, 0),
            callsign: "JA1ZLO".into(),
            nr_sent: "10H".into(),
            nr_rcvd: "25M".into(),
            mode: Mode::Cw,
            band: Band::B7,
            rst_sent: 599,
            rst_rcvd: 599,
            memo: "東京大学".into(),
            freq: "7012.0".into(),
            ..Qso::default()
        };
        vec![header, q]
    }

    #[test]
    fn zlox_round_trip() {
        let bytes = write_records(&sample(), Format::Zlox);
        assert_eq!(bytes.len(), 2 * ZLOX_RECORD);
        assert_eq!(&bytes[0..4], b"ZLOX");
        let (format, back) = read_records(&bytes).unwrap();
        assert_eq!(format, Format::Zlox);
        assert_eq!(back, sample());
        assert_eq!(write_records(&back, Format::Zlox), bytes);
    }

    #[test]
    fn zlo_round_trip_drops_ex_fields() {
        let bytes = write_records(&sample(), Format::Zlo);
        let (format, back) = read_records(&bytes).unwrap();
        assert_eq!(format, Format::Zlo);
        assert_eq!(back[1].memo, "東京大学");
        assert_eq!(back[1].freq, "");
        assert_eq!(write_records(&back, Format::Zlo), bytes);
    }

    #[test]
    fn long_text_is_cut_on_character_boundary() {
        let mut r = vec![0u8; 16];
        put_short_string(&mut r, 0, 5, "あいう");
        assert_eq!(r[0], 4);
        assert_eq!(short_string(&r, 0, 5), "あい");
    }

    #[test]
    fn a_partial_last_record_is_ignored() {
        assert!(matches!(read_records(&[0u8; 100]), Err(ZloError::Empty)));
        let mut bytes = write_records(&sample(), Format::Zlo);
        bytes.extend([1, 2, 3]);
        let (_, back) = read_records(&bytes).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(trailing_bytes(&bytes), 3);
    }
}
