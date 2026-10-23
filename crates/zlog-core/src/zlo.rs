//! ZLO (256-byte records) and ZLOX (384-byte records) files.
//!
//! Both are arrays of Delphi packed records (`TQSOData`, `TQSODataEx`), little endian.
//! Strings are Delphi `ShortString`s (length byte, then Shift_JIS text).
//! Record 0 is the log header, not a contact; see [`crate::log::Log::header`].
//!
//! ZLO has no fields for the frequency, the QSL state and the forced, dupe and QSY
//! violation marks; zLog writes them into the memo ([`memo_str`]) and reads them back
//! out of it ([`parse_memo_str`]).

use encoding_rs::SHIFT_JIS;

use crate::qso::{DelphiTime, QslState, Qso};
use crate::types::{Band, Mode, Power};

pub const ZLO_RECORD: usize = 256;
pub const ZLOX_RECORD: usize = 384;
const ZLOX_MAGIC: &[u8; 4] = b"ZLOX";

/// Memo marks of ZLO files (`UzlogConst.pas`).
pub const MEMO_DUPE: &str = "-DUPE-";
pub const MEMO_PSE_QSL: &str = "PSE QSL";
pub const MEMO_NO_QSL: &str = "NO QSL";
pub const MEMO_QSY_VIOLATION: &str = "*QSY Violation*";

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
}

/// Records decoded from a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Records {
    pub format: Format,
    /// The header (record 0) first, then the contacts.
    pub records: Vec<Qso>,
    /// How many mode, band or power bytes were out of range. zLog reads such bytes as
    /// they are; here they are read as mode Other, band Unknown and power 10W.
    pub bad_bytes: usize,
}

/// Detect the format: ZLOX starts with the magic `ZLOX`, everything else is ZLO.
///
/// Departure: zLog also reads a file named `*.ZLOX` as ZLOX whatever its content
/// (`TLog.LoadFromFile`). Every ZLOX file zLog writes carries the magic, so the
/// content alone decides here.
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
/// ignored; [`trailing_bytes`] tells how many bytes that was. A mode, band or power
/// byte out of range does not refuse the file; see [`Records::bad_bytes`].
pub fn read_records(bytes: &[u8]) -> Result<Records, ZloError> {
    let format = detect(bytes);
    let size = format.record_size();
    if bytes.len() < size {
        return Err(ZloError::Empty);
    }
    let mut bad_bytes = 0;
    let records = bytes[..bytes.len() / size * size]
        .chunks_exact(size)
        .enumerate()
        .map(|(index, rec)| decode(rec, format, index, &mut bad_bytes))
        .collect();
    Ok(Records {
        format,
        records,
        bad_bytes,
    })
}

/// Bytes after the last whole record, which [`read_records`] ignores.
pub fn trailing_bytes(bytes: &[u8]) -> usize {
    bytes.len() % detect(bytes).record_size()
}

/// Encode all records, header included. The ZLOX magic is written into record 0.
/// ZLO memos are written by [`memo_str`] with zLog's default QSL state
/// (`Settings._qsl_default`), which they do not mark.
pub fn write_records_with(records: &[Qso], format: Format, qsl_default: QslState) -> Vec<u8> {
    let mut out = Vec::with_capacity(records.len() * format.record_size());
    for (i, q) in records.iter().enumerate() {
        let mut rec = encode(q, format, qsl_default);
        if i == 0 && format == Format::Zlox {
            rec[0..4].copy_from_slice(ZLOX_MAGIC);
            let n = (records.len() - 1) as i32;
            rec[4..8].copy_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&rec);
    }
    out
}

/// The memo of a ZLO record (`TQSO.GetMemoStr`): the QSL mark unless the state is
/// `qsl_default`, `*` when forced, [`MEMO_DUPE`], the frequency in parentheses, the
/// memo, and [`MEMO_QSY_VIOLATION`], each after a space unless it comes first
/// (`AddStr`).
pub fn memo_str(q: &Qso, qsl_default: QslState) -> String {
    let mut s = String::new();
    let mut add = |part: &str| {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(part);
    };
    if q.qsl_state != qsl_default {
        match q.qsl_state {
            QslState::PseQsl => add(MEMO_PSE_QSL),
            QslState::NoQsl => add(MEMO_NO_QSL),
            // `qsNone` adds an empty string; zLog has no other states.
            QslState::None | QslState::Other(_) => add(""),
        }
    }
    if q.forced {
        add("*");
    }
    if q.dupe {
        add(MEMO_DUPE);
    }
    if !q.freq.is_empty() {
        add(&format!("({})", q.freq));
    }
    add(&q.memo);
    if q.qsy_violation {
        add(MEMO_QSY_VIOLATION);
    }
    s
}

/// Read the marks of [`memo_str`] back out of `q.memo` (`TQSO.SetFileRecord`).
///
/// As zLog does: the text between the first `(` and the next `)` is the frequency,
/// and the memo becomes the text after that `)` (what came before is dropped). Then
/// [`MEMO_DUPE`], [`MEMO_QSY_VIOLATION`], `*`, [`MEMO_PSE_QSL`] and [`MEMO_NO_QSL`],
/// in this order, set their field when found anywhere and are removed, trimming the
/// memo each time.
///
/// Departure: zLog keeps the space after `)` when no mark follows, so every save
/// and load adds one; here the memo after the frequency is trimmed too.
pub fn parse_memo_str(q: &mut Qso) {
    // Delphi `Trim` removes spaces and control characters at both ends.
    let trim = |s: &str| s.trim_matches(|c: char| c <= ' ').to_string();
    q.freq.clear();
    if let Some(open) = q.memo.find('(') {
        let rest = &q.memo[open + 1..];
        if let Some(close) = rest.find(')') {
            q.freq = rest[..close].to_string();
            q.memo = trim(&rest[close + 1..]);
        }
    }
    let mut take = |mark: &str| {
        let found = q.memo.contains(mark);
        if found {
            q.memo = trim(&q.memo.replace(mark, ""));
        }
        found
    };
    let dupe = take(MEMO_DUPE);
    let qsy_violation = take(MEMO_QSY_VIOLATION);
    let forced = take("*");
    let pse_qsl = take(MEMO_PSE_QSL);
    let no_qsl = take(MEMO_NO_QSL);
    q.dupe |= dupe;
    q.qsy_violation |= qsy_violation;
    q.forced |= forced;
    if pse_qsl {
        q.qsl_state = QslState::PseQsl;
    }
    if no_qsl {
        q.qsl_state = QslState::NoQsl;
    }
}

fn decode(r: &[u8], format: Format, index: usize, bad_bytes: &mut usize) -> Qso {
    let mut bad = |ok: bool| *bad_bytes += usize::from(!ok);
    let mode = Mode::from_byte(r[92]);
    let band = Band::from_byte(r[93]);
    let power = Power::from_byte(r[94]);
    bad(mode.is_some());
    bad(band.is_some());
    bad(power.is_some());

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
        mode: mode.unwrap_or(Mode::Other),
        band: band.unwrap_or(Band::Unknown),
        power: power.unwrap_or(Power::P010),
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
    } else {
        parse_memo_str(&mut q);
    }
    q
}

fn encode(q: &Qso, format: Format, qsl_default: QslState) -> Vec<u8> {
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
    match format {
        Format::Zlo => put_short_string(&mut r, 175, 64, &memo_str(q, qsl_default)),
        Format::Zlox => put_short_string(&mut r, 175, 64, &q.memo),
    }
    r[240] = q.cq as u8;
    r[241] = q.dupe as u8;
    // zLog always writes 0 here (`GetFileRecord`, `GetFileRecordEx`: `Reserve := 0`).
    r[242] = 0;
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
/// A character Shift_JIS lacks becomes `?`, as zLog writes it (encoding_rs alone
/// would write an HTML entity, `&#NNNN;`).
fn put_short_string(r: &mut [u8], at: usize, max: usize, s: &str) {
    let (bytes, _, errors) = SHIFT_JIS.encode(s);
    let bytes: Vec<u8> = if errors {
        let mut buf = [0u8; 4];
        s.chars()
            .flat_map(|c| {
                let (b, _, bad) = SHIFT_JIS.encode(c.encode_utf8(&mut buf));
                if bad {
                    vec![b'?']
                } else {
                    b.into_owned()
                }
            })
            .collect()
    } else {
        bytes.into_owned()
    };
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

    /// Z7: a character Shift_JIS lacks is written as `?`.
    #[test]
    fn unmappable_characters_are_written_as_question_marks() {
        let mut r = [0u8; 16];
        put_short_string(&mut r, 0, 10, "A\u{1F600}B");
        assert_eq!(&r[..4], &[3, b'A', b'?', b'B']);
    }

    /// Encode with no QSL state as the default.
    fn write_records(records: &[Qso], format: Format) -> Vec<u8> {
        write_records_with(records, format, QslState::None)
    }

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

    fn read(bytes: &[u8]) -> (Format, Vec<Qso>) {
        let r = read_records(bytes).unwrap();
        (r.format, r.records)
    }

    /// The memo ShortString of record 1 of a ZLO file.
    fn memo_bytes(bytes: &[u8]) -> &[u8] {
        let at = ZLO_RECORD + 175;
        &bytes[at + 1..at + 1 + bytes[at] as usize]
    }

    fn marked() -> Qso {
        Qso {
            callsign: "JA1ZLO".into(),
            memo: "abc".into(),
            qsl_state: QslState::PseQsl,
            forced: true,
            dupe: true,
            qsy_violation: true,
            ..Qso::default()
        }
    }

    #[test]
    fn zlox_round_trip() {
        let bytes = write_records(&sample(), Format::Zlox);
        assert_eq!(bytes.len(), 2 * ZLOX_RECORD);
        assert_eq!(&bytes[0..4], b"ZLOX");
        let (format, back) = read(&bytes);
        assert_eq!(format, Format::Zlox);
        assert_eq!(back, sample());
        assert_eq!(write_records(&back, Format::Zlox), bytes);
    }

    #[test]
    fn zlo_round_trip_keeps_freq_in_memo() {
        let bytes = write_records(&sample(), Format::Zlo);
        assert_eq!(
            memo_bytes(&bytes),
            &*SHIFT_JIS.encode("(7012.0) 東京大学").0
        );
        let (format, back) = read(&bytes);
        assert_eq!(format, Format::Zlo);
        assert_eq!(back[1].memo, "東京大学");
        assert_eq!(back[1].freq, "7012.0");
        assert_eq!(write_records(&back, Format::Zlo), bytes);
    }

    #[test]
    fn zlo_memo_is_written_as_get_memo_str() {
        let header = Qso::default();
        let bytes = write_records(&[header.clone(), marked()], Format::Zlo);
        assert_eq!(memo_bytes(&bytes), b"PSE QSL * -DUPE- abc *QSY Violation*");
        let no_qsl = Qso {
            qsl_state: QslState::NoQsl,
            ..marked()
        };
        let bytes = write_records(&[header.clone(), no_qsl], Format::Zlo);
        assert_eq!(memo_bytes(&bytes), b"NO QSL * -DUPE- abc *QSY Violation*");
        let with_freq = Qso {
            freq: "7012.0".into(),
            qsl_state: QslState::None,
            ..marked()
        };
        let bytes = write_records(&[header, with_freq], Format::Zlo);
        assert_eq!(memo_bytes(&bytes), b"* -DUPE- (7012.0) abc *QSY Violation*");
    }

    #[test]
    fn memo_str_edges() {
        // Nothing to mark: the memo alone, no spaces.
        let plain = Qso {
            memo: "abc".into(),
            ..Qso::default()
        };
        assert_eq!(memo_str(&plain, QslState::None), "abc");
        assert_eq!(memo_str(&Qso::default(), QslState::None), "");
        // An empty memo still adds its space (`AddStr(strMemo, FMemo)`).
        let dupe = Qso {
            dupe: true,
            ..Qso::default()
        };
        assert_eq!(memo_str(&dupe, QslState::None), "-DUPE- ");
        // The default QSL state is not marked; none as a non-default adds nothing.
        assert_eq!(
            memo_str(&marked(), QslState::PseQsl),
            "* -DUPE- abc *QSY Violation*"
        );
        assert_eq!(memo_str(&plain, QslState::PseQsl), "abc");
        let bytes = write_records_with(&[Qso::default(), marked()], Format::Zlo, QslState::PseQsl);
        assert_eq!(memo_bytes(&bytes), b"* -DUPE- abc *QSY Violation*");
        // ZLOX keeps the plain memo.
        let bytes = write_records(&[Qso::default(), marked()], Format::Zlox);
        let at = ZLOX_RECORD + 175;
        assert_eq!(&bytes[at..at + 4], b"\x03abc");
    }

    #[test]
    fn zlo_memo_is_parsed_as_set_file_record() {
        let bytes = write_records(&[Qso::default(), marked()], Format::Zlo);
        let (_, back) = read(&bytes);
        assert_eq!(back[1], marked());
        let no_qsl = Qso {
            qsl_state: QslState::NoQsl,
            ..marked()
        };
        let (_, back) = read(&write_records(
            &[Qso::default(), no_qsl.clone()],
            Format::Zlo,
        ));
        assert_eq!(back[1], no_qsl);
    }

    #[test]
    fn parse_memo_str_follows_zlog() {
        let parse = |memo: &str| {
            let mut q = Qso {
                memo: memo.into(),
                ..Qso::default()
            };
            parse_memo_str(&mut q);
            q
        };
        // As zLog: marks before the frequency are lost with the text before `(`.
        let q = parse("PSE QSL * -DUPE- (7012.0) abc");
        assert_eq!((q.freq.as_str(), q.memo.as_str()), ("7012.0", "abc"));
        assert_eq!(
            (q.qsl_state, q.forced, q.dupe),
            (QslState::None, false, false)
        );
        // A `(` without `)` is memo text.
        let q = parse("(7012 abc");
        assert_eq!((q.freq.as_str(), q.memo.as_str()), ("", "(7012 abc"));
        // Marks are found anywhere, and every `*` means forced.
        let q = parse("a*b -DUPE- c");
        assert_eq!(q.memo, "ab  c");
        assert!(q.forced && q.dupe);
        // The QSY mark is taken before `*`, so it does not mark forced.
        let q = parse("x *QSY Violation*");
        assert!(q.qsy_violation && !q.forced);
        assert_eq!(q.memo, "x");
        // NO QSL wins over PSE QSL, as it is taken last.
        assert_eq!(parse("PSE QSL NO QSL").qsl_state, QslState::NoQsl);
        assert_eq!(parse("").memo, "");
        assert_eq!(parse("()").freq, "");
    }

    #[test]
    fn reserve_byte_is_written_as_zero() {
        let q = Qso {
            reserve: 7,
            ..Qso::default()
        };
        for format in [Format::Zlo, Format::Zlox] {
            let bytes = write_records(&[Qso::default(), q.clone()], format);
            assert_eq!(bytes[format.record_size() + 242], 0);
        }
    }

    #[test]
    fn bad_enum_bytes_do_not_refuse_the_file() {
        let mut bytes = write_records(&sample(), Format::Zlox);
        bytes[ZLOX_RECORD + 92] = 200;
        bytes[ZLOX_RECORD + 93] = 24;
        bytes[ZLOX_RECORD + 94] = 255;
        let r = read_records(&bytes).unwrap();
        assert_eq!(r.bad_bytes, 3);
        let q = &r.records[1];
        assert_eq!(
            (q.mode, q.band, q.power),
            (Mode::Other, Band::Unknown, Power::P010)
        );
        assert_eq!(q.callsign, "JA1ZLO");
        // Only one byte bad.
        let mut bytes = write_records(&sample(), Format::Zlo);
        bytes[ZLO_RECORD + 93] = 99;
        assert_eq!(read_records(&bytes).unwrap().bad_bytes, 1);
        assert_eq!(
            read_records(&write_records(&sample(), Format::Zlo))
                .unwrap()
                .bad_bytes,
            0
        );
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
        assert!(matches!(read_records(&[]), Err(ZloError::Empty)));
        let mut bytes = write_records(&sample(), Format::Zlo);
        bytes.extend([1, 2, 3]);
        let (_, back) = read(&bytes);
        assert_eq!(back.len(), 2);
        assert_eq!(trailing_bytes(&bytes), 3);
    }
}
