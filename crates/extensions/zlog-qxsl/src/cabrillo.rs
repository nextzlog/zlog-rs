use zlog_core::{Band, Contest, ContestMode, DelphiTime, Log, Mode};

use crate::delphi::{ansi, date_time, CRLF};

pub const TAGS: [&str; 26] = [
    "CALLSIGN",
    "CONTEST",
    "CATEGORY-ASSISTED",
    "CATEGORY-BAND",
    "CATEGORY-MODE",
    "CATEGORY-OPERATOR",
    "CATEGORY-POWER",
    "CATEGORY-STATION",
    "CATEGORY-TIME",
    "CATEGORY-TRANSMITTER",
    "CATEGORY-OVERLAY",
    "CERTIFICATE",
    "CLAIMED-SCORE",
    "CLUB",
    "EMAIL",
    "GRID-LOCATOR",
    "LOCATION",
    "NAME",
    "ADDRESS",
    "ADDRESS-CITY",
    "ADDRESS-STATE-PROVINCE",
    "ADDRESS-POSTALCODE",
    "ADDRESS-COUNTRY",
    "OPERATORS",
    "OFFTIME",
    "SOAPBOX",
];

const BANDS: [&str; 22] = [
    " 1800", " 3500", " 7000", "10000", "14000", "18000", "21000", "24500", "28000", "   50",
    "  144", "  432", " 1.2G", " 2.3G", " 5.7G", "  10G", "  10G", "  24G", "  47G", "  75G",
    " 134G", " 241G",
];

pub const ADIF_BANDS: [&str; 22] = [
    "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m", "2m", "70cm", "23cm",
    "13cm", "6cm", "3cm", "3cm", "1.25cm", "6mm", "4mm", "2mm", "1mm",
];

const MODES: [&str; 9] = ["CW", "PH", "FM", "PH", "RY", "DG", "DG", "  ", "DV"];

const USE_UTC: i16 = 32767;

fn band_text(band: Band) -> &'static str {
    BANDS.get(band as usize).copied().unwrap_or("")
}

fn mode_text(mode: Mode) -> &'static str {
    MODES.get(mode as usize).copied().unwrap_or("")
}

pub fn actual_freq(band: Band, freq: &str) -> String {
    let f = freq.trim().parse::<f64>().unwrap_or(0.0).trunc();
    let khz = (f > 0.0 && f.is_finite()).then_some(f as u64);
    let found = khz.and_then(|k| zlog_core::band_of(k.saturating_mul(1000)));
    if khz.is_none() || found != Some(band) || band > Band::B28 {
        return band_text(band).into();
    }
    let int = freq.split('.').next().unwrap_or("");
    let s = format!("     {int}");
    s[s.len() - 5..].into()
}

pub fn split_iota(nr: &str) -> String {
    for p in ["AS", "NA", "OC", "EU", "AF", "SA", "AN"] {
        if let Some(i) = nr.find(p) {
            return format!("{} {}-{}", &nr[..i], p, &nr[i + 2..]);
        }
    }
    nr.into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    Plain,
    CqwwRtty,
    Iota,
}

impl Exchange {
    pub fn of(contest_tag: &str, contest: &dyn Contest) -> Self {
        let cqww = contest.name() == "CQWW DX Contest";
        if contest_tag.eq_ignore_ascii_case("CQ-WW-RTTY")
            || (cqww && contest.contest_mode() == ContestMode::Rtty)
        {
            Exchange::CqwwRtty
        } else if contest.name() == "IOTA Contest" {
            Exchange::Iota
        } else {
            Exchange::Plain
        }
    }
}

fn pad_right(s: &str, len: usize) -> String {
    let fill = len.saturating_sub(ansi(s).len());
    format!("{s}{}", " ".repeat(fill))
}

fn pad_left(s: &str, len: usize) -> String {
    let fill = len.saturating_sub(ansi(s).len());
    format!("{}{s}", " ".repeat(fill))
}

fn hhmm(t: f64) -> String {
    let (_, _, _, h, m) = date_time(t);
    format!("{h:02}{m:02}")
}

fn delphi_copy(s: &[char], index: usize, count: i64) -> String {
    if count <= 0 || index == 0 || index > s.len() {
        return String::new();
    }
    s[index - 1..(index - 1 + count as usize).min(s.len())]
        .iter()
        .collect()
}

fn qtc_time(date: &str, time: &str) -> Option<f64> {
    use chrono::{Datelike, Timelike};
    let s: Vec<char> = format!("{date} {time}").chars().collect();
    let text = format!(
        "{} {}:{}",
        delphi_copy(&s, 1, 10),
        delphi_copy(&s, 12, 2),
        delphi_copy(&s, 14, 2)
    );
    let t = chrono::NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M").ok()?;
    Some(DelphiTime::from_ymd_hms(t.year(), t.month(), t.day(), t.hour(), t.minute(), 0).0)
}

struct Record {
    seq: usize,
    kind: u8,
    time: f64,
    text: String,
}

fn join(parts: &[String]) -> String {
    parts.join(" ")
}

fn delimited(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while chars.peek().is_some() {
        let mut field = String::new();
        if chars.next_if_eq(&'"').is_some() {
            while let Some(c) = chars.next() {
                if c == '"' && chars.next_if_eq(&'"').is_none() {
                    break;
                }
                field.push(c);
            }
        } else {
            while let Some(c) = chars.next_if(|c| *c != ' ') {
                field.push(c);
            }
        }
        out.push(field);
        if chars.next_if_eq(&' ').is_some() && chars.peek().is_none() {
            out.push(String::new());
        }
    }
    out
}

pub fn qso_lines(
    log: &Log,
    exchange: Exchange,
    multi_tx: bool,
    mycall: &str,
    zone_hours: i32,
) -> Result<Vec<u8>, String> {
    qso_text(log, exchange, multi_tx, mycall, zone_hours).map(|t| ansi(&t))
}

pub fn qso_text(
    log: &Log,
    exchange: Exchange,
    multi_tx: bool,
    mycall: &str,
    zone_hours: i32,
) -> Result<String, String> {
    let offhour = match log.header.rst_sent as i16 {
        USE_UTC => 0.0,
        m => (m / 60) as f64,
    } + f64::from(zone_hours);
    let mut records = Vec::new();
    let mut seq = 1;
    for q in &log.qsos {
        let utc = q.time.0 + offhour / 24.0;
        let (y, mo, d, ..) = date_time(utc);
        let (sent, rcvd) = match exchange {
            Exchange::CqwwRtty => {
                let state = if q.multi3.is_empty() { "DX" } else { &q.multi3 };
                (
                    join(&[pad_right(&q.nr_sent, 2), pad_right("DX", 4)]),
                    join(&[pad_right(&q.multi1, 2), pad_right(state, 4)]),
                )
            }
            Exchange::Iota => (
                pad_right(&split_iota(&q.nr_sent), 6),
                pad_right(&split_iota(&q.nr_rcvd), 6),
            ),
            Exchange::Plain => (pad_right(&q.nr_sent, 6), pad_right(&q.nr_rcvd, 6)),
        };
        let tx = if multi_tx {
            q.tx.to_string()
        } else {
            "0".into()
        };
        let mut text = match q.invalid {
            true => "X-QSO: ".to_string(),
            false => "QSO: ".to_string(),
        };
        text.push_str(&join(&[
            actual_freq(q.band, &q.freq),
            mode_text(q.mode).into(),
            format!("{y:04}-{mo:02}-{d:02}"),
            hhmm(utc),
            pad_right(mycall, 13),
            pad_left(&q.rst_sent.to_string(), 3),
            sent,
            pad_right(&q.callsign, 13),
            pad_left(&q.rst_rcvd.to_string(), 3),
            rcvd,
            tx,
        ]));
        records.push(Record {
            seq: 0,
            kind: 0,
            time: utc,
            text,
        });
        let memo: Vec<char> = q.memo.chars().collect();
        let Some(index) = q.memo.find("[QTC").map(|b| q.memo[..b].chars().count() + 1) else {
            continue;
        };
        let body = delphi_copy(&memo, index + 4, memo.len() as i64 - 5);
        let f = delimited(&body);
        let bad = || format!("the QTC memo of {} is not valid: {}", q.callsign, q.memo);
        let [group, call, date, time, freq, ..] = &f[..] else {
            return Err(bad());
        };
        let (group, call, date, time, freq) = (
            group.as_str(),
            call.as_str(),
            date.as_str(),
            time.as_str(),
            freq.as_str(),
        );
        let at = qtc_time(date, time).ok_or_else(bad)?;
        let freq = if freq.contains('m') {
            let b = ADIF_BANDS.iter().position(|a| *a == freq);
            band_text(b.and_then(|i| Band::from_byte(i as u8)).unwrap_or(q.band)).to_string()
        } else {
            freq.to_string()
        };
        let mut text = "QTC: ".to_string();
        text.push_str(&join(&[
            pad_left(&freq, 5),
            mode_text(q.mode).into(),
            date.into(),
            time.into(),
            pad_right(call, 13),
            pad_right(group, 10),
            pad_right(mycall, 13),
            hhmm(utc),
            pad_right(&q.callsign, 13),
            pad_right(&q.nr_rcvd, 4),
        ]));
        records.push(Record {
            seq,
            kind: 1,
            time: at,
            text,
        });
        seq += 1;
    }
    records.sort_by(|a, b| {
        (a.seq, a.kind)
            .cmp(&(b.seq, b.kind))
            .then(a.time.total_cmp(&b.time))
    });
    Ok(records.into_iter().map(|r| r.text + CRLF).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Qso;

    fn one(f: impl FnOnce(&mut Qso)) -> Log {
        let mut l = Log::default();
        l.header.rst_sent = (-540i16) as u16;
        let mut q = Qso {
            time: DelphiTime::from_ymd_hms(2026, 11, 28, 10, 15, 0),
            callsign: "DL1XYZ".into(),
            band: Band::B14,
            mode: Mode::Ssb,
            rst_sent: 59,
            rst_rcvd: 59,
            nr_sent: "25".into(),
            nr_rcvd: "14".into(),
            freq: "14205.5".into(),
            tx: 1,
            ..Qso::default()
        };
        f(&mut q);
        l.qsos = vec![q];
        l
    }

    fn lines(l: &Log, x: Exchange, multi: bool) -> String {
        String::from_utf8(qso_lines(l, x, multi, "JA1ZLO", 0).unwrap()).unwrap()
    }

    #[test]
    fn qso_line_matches_zlog() {
        assert_eq!(
            lines(&one(|_| {}), Exchange::Plain, false),
            "QSO: 14205 PH 2026-11-28 0115 JA1ZLO         59 25     DL1XYZ         59 14     0\r\n"
        );
        assert!(lines(&one(|_| {}), Exchange::Plain, true).ends_with(" 1\r\n"));
        let fm = lines(&one(|q| q.mode = Mode::Fm), Exchange::Plain, false);
        assert!(fm.starts_with("QSO: 14205 FM "));
        let other = lines(&one(|q| q.mode = Mode::Other), Exchange::Plain, false);
        assert!(other.starts_with("QSO: 14205    2026"));
        let utc = one(|q| q.freq = "7003".into());
        let mut utc = utc;
        utc.header.rst_sent = 32767;
        assert!(lines(&utc, Exchange::Plain, false).starts_with("QSO: 14000 PH 2026-11-28 1015 "));
    }

    #[test]
    fn actual_frequency_only_on_hf_when_it_matches_the_band() {
        assert_eq!(actual_freq(Band::B14, "14205.5"), "14205");
        assert_eq!(actual_freq(Band::B7, "7003"), " 7003");
        assert_eq!(actual_freq(Band::B7, "14205"), " 7000");
        assert_eq!(actual_freq(Band::B7, ""), " 7000");
        assert_eq!(actual_freq(Band::B7, "abc"), " 7000");
        assert_eq!(actual_freq(Band::B50, "50100"), "   50");
        assert_eq!(actual_freq(Band::B1200, "1294000"), " 1.2G");
        assert_eq!(actual_freq(Band::Unknown, ""), "");
    }

    #[test]
    fn cqww_rtty_and_iota_exchanges() {
        let l = one(|q| {
            q.mode = Mode::Rtty;
            q.multi1 = "05".into();
            q.multi3 = "MA".into();
        });
        assert_eq!(
            lines(&l, Exchange::CqwwRtty, false),
            "QSO: 14205 RY 2026-11-28 0115 JA1ZLO         59 25 DX   DL1XYZ         59 05 MA   0\r\n"
        );
        let l = one(|q| q.mode = Mode::Rtty);
        assert!(lines(&l, Exchange::CqwwRtty, false)
            .contains(" 59 25 DX   DL1XYZ         59    DX   0"));
        let l = one(|q| {
            q.nr_sent = "001AS007".into();
            q.nr_rcvd = "002EU005".into();
        });
        assert!(lines(&l, Exchange::Iota, false)
            .contains(" 59 001 AS-007 DL1XYZ         59 002 EU-005 0"));
        assert_eq!(split_iota("599"), "599");
    }

    #[test]
    fn exchange_kind_from_the_contest_tag() {
        assert_eq!(
            Exchange::of("", &crate::tests::Rtty(false)),
            Exchange::Plain
        );
        assert_eq!(
            Exchange::of("cq-ww-rtty", &crate::tests::Rtty(false)),
            Exchange::CqwwRtty
        );
    }

    #[test]
    fn qtc_lines_follow_all_qsos_in_sequence() {
        let mut l = one(|q| q.memo = "[QTC1/10 DL8WPX 2026-11-28 0130 14205]".into());
        let mut second = l.qsos[0].clone();
        second.time = DelphiTime::from_ymd_hms(2026, 11, 28, 11, 0, 0);
        second.callsign = "DL2AAA".into();
        second.memo = "[QTC2/10 DL8WPX 2026-11-28 0100 20m] note".into();
        l.qsos.push(second);
        let mut plain = l.qsos[0].clone();
        plain.memo = "no qtc".into();
        l.qsos.push(plain.clone());
        l.qsos.push(plain);
        let text = lines(&l, Exchange::Plain, false);
        let rows: Vec<&str> = text.split(CRLF).collect();
        assert!(rows[0].contains("0115 JA1ZLO") && rows[0].contains("DL1XYZ"));
        assert!(rows[1].contains("0115 JA1ZLO"));
        assert!(rows[2].contains("0115 JA1ZLO"));
        assert!(rows[3].contains("0200 JA1ZLO") && rows[3].contains("DL2AAA"));
        assert_eq!(
            rows[4],
            "QTC: 14205 PH 2026-11-28 0130 DL8WPX        1/10       JA1ZLO        0115 DL1XYZ        14  "
        );
        assert_eq!(
            rows[5],
            "QTC: 14000 PH 2026-11-28 0100 DL8WPX        2/10       JA1ZLO        0200 DL2AAA        14  "
        );
        assert_eq!(rows.len(), 7);
    }

    #[test]
    fn a_bad_qtc_memo_stops_the_export_like_zlog() {
        for memo in ["[QTC3/10 DL8WPX]", "[QTC4/10 DL8WPX 2026-02-31 0130 14205]"] {
            let l = one(|q| q.memo = memo.into());
            let e = qso_lines(&l, Exchange::Plain, false, "JA1ZLO", 0).unwrap_err();
            assert!(e.contains("DL1XYZ"), "{e}");
        }
    }

    #[test]
    fn qtc_fields_may_be_quoted() {
        assert_eq!(delimited("a \"b c\" d"), ["a", "b c", "d"]);
        assert_eq!(delimited("\"x\"\"y\" z"), ["x\"y", "z"]);
        assert_eq!(delimited("a  b"), ["a", "", "b"]);
        assert_eq!(delimited("a "), ["a", ""]);
        assert_eq!(delimited("\"ab\"cd ef"), ["ab", "cd", "ef"]);
        assert_eq!(delimited("\"a b"), ["a b"]);
        assert_eq!(delimited(" a"), ["", "a"]);
        assert!(delimited("").is_empty());
        let l = one(|q| q.memo = "[QTC1/10 \"DL8WPX\" 2026-11-28 0130 14205]".into());
        assert!(lines(&l, Exchange::Plain, false).contains(" DL8WPX        1/10 "));
    }

    #[test]
    fn the_time_zone_offset_moves_the_qso_time() {
        let l = one(|_| {});
        let jst =
            String::from_utf8(qso_lines(&l, Exchange::Plain, false, "JA1ZLO", 9).unwrap()).unwrap();
        assert!(jst.starts_with("QSO: 14205 PH 2026-11-28 1015 "), "{jst}");
    }

    #[test]
    fn unmappable_text_becomes_question_marks() {
        let l = one(|q| q.callsign = "DL\u{1F600}".into());
        let b = qso_lines(&l, Exchange::Plain, false, "\u{1F600}", 0).unwrap();
        assert!(String::from_utf8(b).unwrap().contains(" ?             "));
    }
}
