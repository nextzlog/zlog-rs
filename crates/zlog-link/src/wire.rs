//! Text encoding of a QSO and of lines on the wire, as zLog writes them.
//!
//! A QSO travels as `TQSO.QSOinText` (UzLogQSO.pas): 31 fields joined by `~` with
//! Delphi's `TStringList.DelimitedText` (strict delimiter, `"` as quote character),
//! the first being the tag `ZLOGQSODATA:`. Lines are Shift_JIS (the ANSI code page
//! of Japanese Windows, which ICS's `SendStr` and the receive buffer use).

use encoding_rs::SHIFT_JIS;
use zlog_core::qso::QslState;
use zlog_core::{Band, DelphiTime, Mode, Power, Qso};

/// First field of every QSO text.
pub const TAG: &str = "ZLOGQSODATA:";

/// Field separator (`_sep` in UzlogConst.pas).
pub const SEP: char = '~';

const QUOTE: char = '"';

/// `TStringList.DelimitedText` with `StrictDelimiter`: a field holding the delimiter,
/// the quote character or NUL is quoted (`AnsiQuotedStr`), the others are written as is.
pub fn join_delimited(fields: &[String]) -> String {
    if fields.len() == 1 && fields[0].is_empty() {
        return "\"\"".into();
    }
    let quoted: Vec<String> = fields
        .iter()
        .map(|f| {
            if f.contains([SEP, QUOTE, '\0']) {
                format!("\"{}\"", f.replace('"', "\"\""))
            } else {
                f.clone()
            }
        })
        .collect();
    quoted.join(&SEP.to_string())
}

/// Setting `TStringList.DelimitedText` with `StrictDelimiter` (`SetDelimitedText`).
///
/// A field starting with `"` is read as a quoted string (`AnsiExtractQuotedStr`);
/// whatever follows its closing quote up to the delimiter becomes a field of its own,
/// as in Delphi. A trailing delimiter adds one empty field.
pub fn split_delimited(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == QUOTE {
            let mut s = String::new();
            i += 1;
            while i < chars.len() {
                if chars[i] == QUOTE {
                    if chars.get(i + 1) == Some(&QUOTE) {
                        s.push(QUOTE);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                s.push(chars[i]);
                i += 1;
            }
            out.push(s);
        } else {
            let start = i;
            while i < chars.len() && chars[i] != SEP {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
        }
        if chars.get(i) == Some(&SEP) {
            if i + 1 == chars.len() {
                out.push(String::new());
            }
            i += 1;
        }
    }
    out
}

/// Delphi `FloatToStr`: general format with 15 significant digits, `.` as the decimal
/// separator (the Japanese locale), trailing zeros removed.
pub fn float_to_str(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let sci = format!("{:.14e}", v.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if v < 0.0 { "-" } else { "" };
    if !(-5..15).contains(&exp) {
        let d = digits.trim_end_matches('0');
        let m = if d.len() > 1 {
            format!("{}.{}", &d[..1], &d[1..])
        } else {
            d.to_string()
        };
        return format!("{sign}{m}E{exp}");
    }
    let s = if exp >= 0 {
        let (int, frac) = digits.split_at(exp as usize + 1);
        format!("{int}.{frac}")
    } else {
        format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
    };
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{sign}{s}")
}

/// `ZBoolToStr`.
fn bool_str(b: bool) -> String {
    if b { "1" } else { "0" }.into()
}

/// `ZStrToBool`: anything but `0` and the empty string is true.
fn str_bool(s: &str) -> bool {
    !(s == "0" || s.is_empty())
}

/// `TQSO.QSOinText`: the QSO as one line of text, fields in zLog's order.
pub fn qso_to_text(q: &Qso) -> String {
    let fields = vec![
        TAG.to_string(),
        float_to_str(q.time.0),
        q.callsign.clone(),
        q.nr_sent.clone(),
        q.nr_rcvd.clone(),
        q.rst_sent.to_string(),
        q.rst_rcvd.to_string(),
        q.serial.to_string(),
        (q.mode as u8).to_string(),
        (q.band as u8).to_string(),
        (q.power as u8).to_string(),
        q.multi1.clone(),
        q.multi2.clone(),
        bool_str(q.new_multi1),
        bool_str(q.new_multi2),
        q.points.to_string(),
        q.operator.clone(),
        q.memo.clone(),
        bool_str(q.cq),
        bool_str(q.dupe),
        q.reserve.to_string(),
        q.tx.to_string(),
        q.power2.to_string(),
        q.reserve2.to_string(),
        q.reserve3.to_string(),
        q.freq.clone(),
        bool_str(q.qsy_violation),
        q.pc_name.clone(),
        bool_str(q.forced),
        q.qsl_state.to_byte().to_string(),
        bool_str(q.invalid),
    ];
    join_delimited(&fields)
}

/// Delphi `StrToInt`, which skips leading blanks; `None` where it raises `EConvertError`.
fn int<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.trim_start().parse().ok()
}

/// [`int`] for a field zLog keeps as `Integer` but the record holds in fewer bits
/// (`u8`, `u16`): any 32-bit value is accepted and clamped into the field's range.
fn clamped<T: TryFrom<i32> + Into<i32> + Copy>(s: &str, min: T, max: T) -> Option<T> {
    let n: i32 = int(s)?;
    let n = n.clamp(min.into(), max.into());
    T::try_from(n).ok()
}

fn byte(s: &str) -> Option<u8> {
    clamped(s, u8::MIN, u8::MAX)
}

fn word(s: &str) -> Option<u16> {
    clamped(s, u16::MIN, u16::MAX)
}

/// `TQSO.TextToQSO`: `None` when the text does not start with the tag.
///
/// Missing trailing fields are empty, as zLog pads the text with 30 separators.
/// A number that does not parse stops the conversion where it is, leaving the
/// fields read so far, and sets the memo to `Convert Error!`, as zLog does.
/// Mode, band and power values out of range become `Other`, `Unknown` and the
/// default power, since Rust enums cannot hold them (zLog keeps the raw ordinal).
/// Numbers zLog keeps as `Integer` in fields stored narrower here (RST, points, TX,
/// mode, band, power) are clamped into range rather than refused; departure: zLog
/// would keep the out-of-range value until the record is saved.
pub fn text_to_qso(text: &str) -> Option<Qso> {
    let text = text.replace('\0', "");
    let mut f = split_delimited(&format!("{text}{}", SEP.to_string().repeat(30)));
    if f.first().map(String::as_str) != Some(TAG) {
        return None;
    }
    f.resize(31, String::new());
    let mut q = Qso::default();
    if fill(&mut q, &f).is_none() {
        q.memo = "Convert Error!".into();
    }
    Some(q)
}

fn fill(q: &mut Qso, f: &[String]) -> Option<()> {
    let time: f64 = f[1].trim_start().replace(',', ".").parse().ok()?;
    q.time = DelphiTime(time);
    q.callsign = f[2].clone();
    q.nr_sent = f[3].clone();
    q.nr_rcvd = f[4].clone();
    q.rst_sent = word(&f[5])?;
    q.rst_rcvd = word(&f[6])?;
    q.serial = int(&f[7])?;
    q.mode = Mode::from_byte(byte(&f[8])?).unwrap_or(Mode::Other);
    q.band = Band::from_byte(byte(&f[9])?).unwrap_or(Band::Unknown);
    q.power = Power::from_byte(byte(&f[10])?).unwrap_or(Qso::default().power);
    q.multi1 = f[11].clone();
    q.multi2 = f[12].clone();
    q.new_multi1 = str_bool(&f[13]);
    q.new_multi2 = str_bool(&f[14]);
    q.points = byte(&f[15])?;
    q.operator = f[16].clone();
    q.memo = f[17].clone();
    q.cq = str_bool(&f[18]);
    q.dupe = str_bool(&f[19]);
    q.reserve = byte(&f[20])?;
    q.tx = byte(&f[21])?;
    q.power2 = int(&f[22])?;
    q.reserve2 = int(&f[23])?;
    q.reserve3 = int(&f[24])?;
    q.freq = f[25].clone();
    q.qsy_violation = str_bool(&f[26]);
    q.pc_name = f[27].clone();
    q.forced = str_bool(&f[28]);
    // `StrToIntDef(..., 0)`.
    q.qsl_state = QslState::from_byte(byte(&f[29]).unwrap_or(0));
    q.invalid = str_bool(&f[30]);
    Some(())
}

/// Text to Shift_JIS; a character Shift_JIS lacks becomes `?`, as Windows'
/// `WideCharToMultiByte` does (encoding_rs alone would write an HTML entity).
pub fn encode(text: &str) -> Vec<u8> {
    let (bytes, _, errors) = SHIFT_JIS.encode(text);
    if !errors {
        return bytes.into_owned();
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4];
    for c in text.chars() {
        let (b, _, bad) = SHIFT_JIS.encode(c.encode_utf8(&mut buf));
        if bad {
            out.push(b'?');
        } else {
            out.extend_from_slice(&b);
        }
    }
    out
}

/// Shift_JIS to text.
pub fn decode(bytes: &[u8]) -> String {
    SHIFT_JIS.decode_without_bom_handling(bytes).0.into_owned()
}

/// `FillLeft`: right-aligned in `len` Shift_JIS bytes, cut from the left if longer.
pub fn fill_left(s: &str, len: usize) -> String {
    let mut b = vec![b' '; len];
    b.extend(encode(s));
    decode(&b[b.len() - len..])
}

/// `FillRight`: left-aligned in `len` Shift_JIS bytes, cut on the right if longer.
pub fn fill_right(s: &str, len: usize) -> String {
    let mut b = encode(s);
    b.resize(b.len().max(len), b' ');
    b.truncate(len);
    decode(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A QSO as zLog would log it in the ALL JA contest.
    fn sample() -> Qso {
        Qso {
            time: DelphiTime::from_ymd_hms(2026, 4, 25, 21, 0, 0),
            callsign: "JA1ZLO".into(),
            nr_sent: "100M".into(),
            nr_rcvd: "13M".into(),
            rst_sent: 599,
            rst_rcvd: 599,
            serial: 12,
            mode: Mode::Cw,
            band: Band::B14,
            power: Power::P010,
            multi1: "13".into(),
            new_multi1: true,
            points: 1,
            operator: "JR8PPG".into(),
            memo: "東京".into(),
            tx: 1,
            reserve3: 1_000_123_400,
            freq: "14050.0".into(),
            pc_name: "PC1".into(),
            ..Qso::default()
        }
    }

    #[test]
    fn qso_in_text_has_zlog_field_order() {
        assert_eq!(
            qso_to_text(&sample()),
            "ZLOGQSODATA:~46137.875~JA1ZLO~100M~13M~599~599~12~0~4~3~13~~1~0~1~JR8PPG~東京~0~0~0~1~0~0~1000123400~14050.0~0~PC1~0~0~0"
        );
    }

    #[test]
    fn text_round_trip() {
        let q = sample();
        assert_eq!(text_to_qso(&qso_to_text(&q)).unwrap(), q);
        let mut q = sample();
        q.memo = "a~b \"c\"".into();
        q.time = DelphiTime(46137.8751157407);
        let t = qso_to_text(&q);
        assert!(t.contains("~\"a~b \"\"c\"\"\"~"));
        assert_eq!(text_to_qso(&t).unwrap(), q);
    }

    #[test]
    fn short_and_bad_texts() {
        // An old zLog sends fewer fields: the rest are empty.
        let q =
            text_to_qso("ZLOGQSODATA:~46137.5~JA1AAA~1~2~59~59~1~1~2~3~~~0~0~1~~~0~0~0~0~0~0~1200")
                .unwrap();
        assert_eq!(q.callsign, "JA1AAA");
        assert_eq!(q.reserve3, 1200);
        assert_eq!(q.pc_name, "");
        // A bad number stops the conversion.
        let q = text_to_qso("ZLOGQSODATA:~46137.5~JA1AAA~1~2~x").unwrap();
        assert_eq!(q.callsign, "JA1AAA");
        assert_eq!(q.memo, "Convert Error!");
        assert!(text_to_qso("HELLO~1").is_none());
    }

    #[test]
    fn wide_integers_are_clamped() {
        // zLog's fields are `Integer`; values beyond this record's field are clamped.
        let q = text_to_qso(
            "ZLOGQSODATA:~46137.5~JA1AAA~1~2~70000~-5~1~1~2~3~~~0~0~300~~~0~0~0~99999~0~0~1200",
        )
        .unwrap();
        assert_eq!((q.rst_sent, q.rst_rcvd), (u16::MAX, 0));
        assert_eq!((q.points, q.tx, q.reserve3), (255, 255, 1200));
        assert_ne!(q.memo, "Convert Error!");
    }

    #[test]
    fn float_to_str_like_delphi() {
        assert_eq!(float_to_str(46137.875), "46137.875");
        assert_eq!(float_to_str(0.0), "0");
        assert_eq!(float_to_str(46137.0), "46137");
        assert_eq!(float_to_str(46137.87511574074), "46137.8751157407");
        assert_eq!(float_to_str(0.00125), "0.00125");
        assert_eq!(float_to_str(-1.5), "-1.5");
    }

    #[test]
    fn delimited_text_like_tstringlist() {
        assert_eq!(split_delimited("a~~b~"), vec!["a", "", "b", ""]);
        assert_eq!(split_delimited("\"x~y\"~z"), vec!["x~y", "z"]);
        assert_eq!(join_delimited(&["a b".into(), "".into()]), "a b~");
    }

    #[test]
    fn shift_jis_on_the_wire() {
        assert_eq!(encode("東京"), vec![0x93, 0x8C, 0x8B, 0x9E]);
        assert_eq!(decode(&[0x93, 0x8C, 0x8B, 0x9E]), "東京");
        assert_eq!(encode("a😀"), b"a?".to_vec());
        assert_eq!(fill_left("7> ", 10), "       7> ");
        assert_eq!(fill_left("東京> ", 6), "東京> ");
        assert_eq!(fill_right("14", 5), "14   ");
    }
}
