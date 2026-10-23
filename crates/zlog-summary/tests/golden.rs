//! Byte-exact summary blocks for a small log, derived by hand from zLog's writers:
//! `WriteSummarySheetR1`, `WriteSummarySheetR2` and `CalcAll` (`UELogJarlEx.pas`)
//! and `buttonCreateLogClick` (`UELogCabrillo.pas`). The QSOs of each document must
//! be exactly what zlog-qxsl writes for the same QSOs (`jarl`, `cqww`).
//! `TStringList.SaveToFile` and `WriteLn` end every line with CRLF; the files are
//! Shift_JIS. Japanese fixed texts are written as escapes to keep the source ASCII.

use std::collections::BTreeMap;

use zlog_core::{
    Band, BandScore, Contest, ContestMode, DelphiTime, Log, Mode, Power, Qso, ScoreSheet, Scoring,
    Station,
};
use zlog_summary::cabrillo::{self, Header};
use zlog_summary::elog::{self, Form, Order, Score, Version, NEWCOMER, SENIOR_JUNIOR};

/// A contest without contest fields.
struct Plain;

impl Contest for Plain {
    fn name(&self) -> &str {
        "Plain"
    }
    fn band_range(&self) -> (Band, Band) {
        RANGE
    }
    fn contest_mode(&self) -> ContestMode {
        ContestMode::Mix
    }
    fn sent_number(&self, _: Band, _: Power, _: &Station) -> String {
        String::new()
    }
    fn valid_number(&self, _: &Qso) -> bool {
        true
    }
    fn score(&mut self, _: &Log) -> Scoring {
        Scoring::default()
    }
}

/// The log with only the QSOs whose callsign and time are listed, in that order.
fn only(picks: &[(&str, u32, u32)]) -> Log {
    let all = log();
    let mut out = all.clone();
    out.qsos = picks
        .iter()
        .map(|(c, d, h)| {
            all.qsos
                .iter()
                .find(|q| {
                    let (_, _, qd, qh, _, _) = q.time.to_ymd_hms();
                    q.callsign == *c && (qd, qh) == (*d, *h)
                })
                .unwrap()
                .clone()
        })
        .collect();
    out
}

/// What zlog-qxsl writes for `log` in `kind`.
fn qxsl_body(kind: &str, log: &Log) -> Vec<u8> {
    zlog_qxsl::encode(kind, log, &Plain, "").unwrap()
}

/// Contest period of the log, from 2026-04-25 21:00 JST.
const PERIOD: u32 = 24;

/// The contest's bands (`band_range`): 1.9 MHz to 430 MHz.
const RANGE: (Band, Band) = (Band::B1_9, Band::B430);

#[allow(clippy::too_many_arguments)]
fn qso(
    (d, h, m): (u32, u32, u32),
    call: &str,
    band: Band,
    mode: Mode,
    rst: u16,
    rcvd: &str,
    multi: Option<&str>,
    points: u8,
) -> Qso {
    Qso {
        time: DelphiTime::from_ymd_hms(2026, 4, d, h, m, 0),
        callsign: call.into(),
        band,
        mode,
        rst_sent: rst,
        rst_rcvd: rst,
        nr_sent: "10H".into(),
        nr_rcvd: rcvd.into(),
        multi1: multi.unwrap_or("").into(),
        new_multi1: multi.is_some(),
        points,
        ..Qso::default()
    }
}

/// Seven QSOs in JST: two new multipliers, a WARC QSO, a dupe, an invalid QSO,
/// one after the contest period and one on a band outside the contest.
fn log() -> Log {
    let mut log = Log::default();
    log.header.rst_sent = (-540i16) as u16;
    log.set_start_time(DelphiTime::from_ymd_hms(2026, 4, 25, 21, 0, 0));
    let mut first = qso(
        (25, 21, 3),
        "JA1ZLO",
        Band::B7,
        Mode::Cw,
        599,
        "13M",
        Some("13"),
        1,
    );
    first.freq = "7003.5".into();
    let mut invalid = qso(
        (25, 22, 5),
        "JH1CCC",
        Band::B430,
        Mode::Fm,
        59,
        "10M",
        None,
        1,
    );
    invalid.invalid = true;
    let mut dupe = qso(
        (25, 22, 0),
        "JA1ZLO",
        Band::B7,
        Mode::Cw,
        599,
        "13M",
        None,
        0,
    );
    dupe.dupe = true;
    log.qsos = vec![
        first,
        qso(
            (25, 21, 10),
            "JA2AAA",
            Band::B14,
            Mode::Ssb,
            59,
            "20M",
            Some("20"),
            1,
        ),
        qso(
            (25, 21, 30),
            "JA3BBB",
            Band::B10,
            Mode::Cw,
            599,
            "25M",
            None,
            1,
        ),
        dupe,
        invalid,
        qso(
            (26, 22, 0),
            "JA4DDD",
            Band::B7,
            Mode::Cw,
            599,
            "31M",
            None,
            1,
        ),
        qso(
            (25, 21, 50),
            "JA5EEE",
            Band::B1200,
            Mode::Cw,
            599,
            "12M",
            None,
            1,
        ),
    ];
    log
}

/// What the contest's score window showed.
fn sheet() -> ScoreSheet {
    let s = |qsos, points, multi| BandScore {
        qsos,
        cw_qsos: 0,
        points,
        multi,
    };
    let mut bands = BTreeMap::new();
    bands.insert(Band::B7, s(2, 2, 1));
    bands.insert(Band::B10, s(1, 1, 0));
    bands.insert(Band::B14, s(1, 1, 1));
    bands.insert(Band::B430, s(0, 0, 0));
    bands.insert(Band::B1200, s(1, 1, 0));
    ScoreSheet {
        bands,
        total: s(5, 5, 2),
        score: 10,
        with_coeff: None,
    }
}

fn form(version: Version) -> Form {
    Form {
        version,
        contest_name: "ALL JA1".into(),
        category_code: "PN".into(),
        category_name: "Newcomer".into(),
        callsign: "JA1ZLO".into(),
        op_callsign: "JA1ZLO".into(),
        address: "Tokyo\nBunkyo\n".into(),
        tel: "03-0000-0000".into(),
        name: "Nextzlog".into(),
        email: "zlog@example.org".into(),
        license: "1".into(),
        power: "10".into(),
        qth: "Bunkyo".into(),
        power_supply: "AC".into(),
        equipment: "IC-7300".into(),
        license_date: Some((2025, 3, 4)),
        comments: "Thanks.".into(),
        multi_op_list: String::new(),
        club_id: "10001".into(),
        club_name: "ZLOG".into(),
        oath: "I swear.".into(),
        date: "2026-04-26".into(),
        signature: "Nextzlog".into(),
        newcomer_categories: NEWCOMER.into(),
        senior_junior_categories: SENIOR_JUNIOR.into(),
        ..Form::default()
    }
}

fn text(lines: &[&str]) -> String {
    lines.iter().map(|l| format!("{l}\r\n")).collect()
}

/// `CalcAll`: 7 MHz counts the dupe and the QSO after the period (zLog counts
/// every QSO of its work copy), not the invalid one; 10 MHz (WARC) and 1200 MHz
/// (outside the contest) have no row, 430 MHz no points.
#[test]
fn score_recounted_as_calc_all() {
    let score = Score::new(&sheet(), &log(), RANGE);
    let row = |qsos, cw_qsos, points, multi| BandScore {
        qsos,
        cw_qsos,
        points,
        multi,
    };
    assert_eq!(
        score.bands,
        vec![(Band::B7, row(3, 3, 2, 1)), (Band::B14, row(1, 0, 1, 1))]
    );
    assert_eq!(score.total, row(4, 3, 3, 2));
    assert_eq!(score.coeff, None);
    assert_eq!(score.total_score, 6.0);
}

const POWER_RATED: &str = "\u{5b9a}\u{683c}\u{51fa}\u{529b}";

#[test]
fn elog_r1_byte_exact() {
    let mut f = form(Version::R1);
    f.order = Order::Time;
    let score = Score::new(&sheet(), &log(), RANGE);
    let doc = elog::summary(&f, &score);
    let power = format!("<POWERTYPE>{POWER_RATED}</POWERTYPE>");
    let expected = text(&[
        "<SUMMARYSHEET VERSION=R1.0>",
        "<CONTESTNAME>ALL JA1</CONTESTNAME>",
        "<CATEGORYCODE>PN</CATEGORYCODE>",
        "<CATEGORYNAME>Newcomer</CATEGORYNAME>",
        "<CALLSIGN>JA1ZLO</CALLSIGN>",
        "<OPCALLSIGN>JA1ZLO</OPCALLSIGN>",
        "<SCORE BAND=7MHz>3,2,1</SCORE>",
        "<SCORE BAND=14MHz>1,1,1</SCORE>",
        "<SCORE BAND=TOTAL>4,3,2</SCORE>",
        "<TOTALSCORE>6</TOTALSCORE>",
        "<ADDRESS>Tokyo",
        "Bunkyo",
        "</ADDRESS>",
        "<TEL>03-0000-0000</TEL>",
        "<NAME>Nextzlog</NAME>",
        "<EMAIL>zlog@example.org</EMAIL>",
        "<LICENSECLASS>1</LICENSECLASS>",
        "<POWER>10</POWER>",
        &power,
        "<OPPLACE>Bunkyo</OPPLACE>",
        "<POWERSUPPLY>AC</POWERSUPPLY>",
        "<EQUIPMENT>IC-7300",
        "</EQUIPMENT>",
        "<COMMENTS>Thanks.",
        "</COMMENTS>",
        "<REGCLUBNUMBER>10001</REGCLUBNUMBER>",
        "<REGCLUBNAME>ZLOG</REGCLUBNAME>",
        "<OATH>I swear.",
        "</OATH>",
        "<DATE>2026-04-26</DATE>",
        "<SIGNATURE>Nextzlog</SIGNATURE>",
        "</SUMMARYSHEET>",
    ]);
    assert_eq!(doc, expected);
    // The whole file: the summary, then the in-period QSOs in time order as
    // zlog-qxsl writes them in `jarl`.
    let file = elog::elog(&f, &score, &log(), &Plain, PERIOD).unwrap();
    let body = only(&[
        ("JA1ZLO", 25, 21),
        ("JA2AAA", 25, 21),
        ("JA3BBB", 25, 21),
        ("JA5EEE", 25, 21),
        ("JA1ZLO", 25, 22),
        ("JH1CCC", 25, 22),
    ]);
    let mut want = qxsl::text::encode_sjis(&doc);
    want.extend(b"<LOGSHEET TYPE=\"ZLOG_REIWA\">\r\n");
    want.extend(qxsl_body("jarl", &body));
    want.extend(b"</LOGSHEET>\r\n");
    assert_eq!(file, want);
    // Shift_JIS: the power type is the only non-ASCII text, two bytes a letter.
    let bytes = qxsl::text::encode_sjis(&doc);
    assert_eq!(bytes.len(), doc.len() - POWER_RATED.len() + 8);
    let at = bytes.windows(11).position(|w| w == b"<POWERTYPE>").unwrap();
    assert_eq!(
        &bytes[at + 11..at + 19],
        &[0x92, 0xe8, 0x8a, 0x69, 0x8f, 0x6f, 0x97, 0xcd]
    );
}

#[test]
fn elog_r2_byte_exact() {
    let f = form(Version::R2);
    let score = Score::new(&sheet(), &log(), RANGE);
    let doc = elog::summary(&f, &score);
    // `FormatDateTime('yyyy"年"mm"月"dd"日"')`.
    let license = "<LICENSEDATE>2025\u{5e74}03\u{6708}04\u{65e5}</LICENSEDATE>";
    let expected = text(&[
        "<SUMMARYSHEET VERSION=R2.1>",
        "<CONTESTNAME>ALL JA1</CONTESTNAME>",
        "<CATEGORYCODE>PN</CATEGORYCODE>",
        "<CALLSIGN>JA1ZLO</CALLSIGN>",
        "<OPCALLSIGN>JA1ZLO</OPCALLSIGN>",
        "<TOTALSCORE>6</TOTALSCORE>",
        "<ADDRESS>Tokyo",
        "Bunkyo",
        "</ADDRESS>",
        "<NAME>Nextzlog</NAME>",
        "<TEL>03-0000-0000</TEL>",
        "<EMAIL>zlog@example.org</EMAIL>",
        "<POWER>10</POWER>",
        "<OPPLACE>Bunkyo</OPPLACE>",
        "<POWERSUPPLY>AC</POWERSUPPLY>",
        license,
        "<COMMENTS>Thanks.",
        "</COMMENTS>",
        "<REGCLUBNUMBER>10001</REGCLUBNUMBER>",
        "<OATH>I swear.",
        "</OATH>",
        "<DATE>2026-04-26</DATE>",
        "<SIGNATURE>Nextzlog</SIGNATURE>",
        "</SUMMARYSHEET>",
    ]);
    assert_eq!(doc, expected);
    // As entered, without the QSO after the period.
    let file = elog::elog(&f, &score, &log(), &Plain, PERIOD).unwrap();
    let body = only(&[
        ("JA1ZLO", 25, 21),
        ("JA2AAA", 25, 21),
        ("JA3BBB", 25, 21),
        ("JA1ZLO", 25, 22),
        ("JH1CCC", 25, 22),
        ("JA5EEE", 25, 21),
    ]);
    let mut want = qxsl::text::encode_sjis(&doc);
    want.extend(b"<LOGSHEET TYPE=\"ZLOG_REIWA\">\r\n");
    want.extend(qxsl_body("jarl", &body));
    want.extend(b"</LOGSHEET>\r\n");
    assert_eq!(file, want);
}

#[test]
fn cabrillo_byte_exact() {
    let mut h = Header::default();
    for (tag, v) in [
        ("CALLSIGN", "JA1ZLO/1"),
        ("CONTEST", "JA-ALL-JA1"),
        ("CATEGORY-OPERATOR", "SINGLE-OP"),
        ("CLAIMED-SCORE", "6"),
    ] {
        h.values.insert(tag.into(), v.into());
    }
    let doc = cabrillo::header_block(&h);
    let expected = text(&[
        "START-OF-LOG: 3.0",
        "CALLSIGN: JA1ZLO/1",
        "CONTEST: JA-ALL-JA1",
        "CATEGORY-ASSISTED: ",
        "CATEGORY-BAND: ",
        "CATEGORY-MODE: ",
        "CATEGORY-OPERATOR: SINGLE-OP",
        "CATEGORY-POWER: ",
        "CATEGORY-STATION: ",
        "CATEGORY-TIME: ",
        "CATEGORY-TRANSMITTER: ",
        "CATEGORY-OVERLAY: ",
        "CERTIFICATE: ",
        "CLAIMED-SCORE: 6",
        "CLUB: ",
        "CREATED-BY: ZLOG",
        "EMAIL: ",
        "GRID-LOCATOR: ",
        "LOCATION: ",
        "NAME: ",
        "ADDRESS: ",
        "ADDRESS-CITY: ",
        "ADDRESS-STATE-PROVINCE: ",
        "ADDRESS-POSTALCODE: ",
        "ADDRESS-COUNTRY: ",
        "OPERATORS: ",
        "OFFTIME: ",
        "SOAPBOX: ",
    ]);
    assert_eq!(doc, expected);
    // All ASCII: Shift_JIS is the same bytes.
    assert_eq!(qxsl::text::encode_sjis(&doc), doc.as_bytes());
    // The whole file: the header, the in-period QSOs in time order as zlog-qxsl
    // writes them in `cqww`, and the end.
    let file = cabrillo::document(&h, &log(), &Plain, "", PERIOD).unwrap();
    let body = only(&[
        ("JA1ZLO", 25, 21),
        ("JA2AAA", 25, 21),
        ("JA3BBB", 25, 21),
        ("JA5EEE", 25, 21),
        ("JA1ZLO", 25, 22),
        ("JH1CCC", 25, 22),
    ]);
    let mut want = doc.into_bytes();
    want.extend(qxsl_body("cqww", &body));
    want.extend(b"END-OF-LOG:\r\n");
    assert_eq!(file, want);
}

/// Without a contest period every QSO is written (zLog `UseContestPeriod` off).
#[test]
fn no_period_writes_every_qso() {
    let score = Score::new(&sheet(), &log(), RANGE);
    let doc = elog::elog(&form(Version::R2), &score, &log(), &Plain, 0).unwrap();
    assert!(String::from_utf8_lossy(&doc).contains("JA4DDD"));
    let doc = cabrillo::document(&Header::default(), &log(), &Plain, "", 0).unwrap();
    assert!(String::from_utf8_lossy(&doc).contains("JA4DDD"));
    let doc = cabrillo::document(&Header::default(), &log(), &Plain, "", PERIOD).unwrap();
    assert!(!String::from_utf8_lossy(&doc).contains("JA4DDD"));
}
