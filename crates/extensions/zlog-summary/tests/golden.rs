use std::collections::BTreeMap;

use rxsl::sheets::summary::Version;
use zlog_core::{
    Band, BandScore, Contest, ContestMode, DelphiTime, Log, Mode, Power, Qso, ScoreSheet, Scoring,
    Station,
};
use zlog_summary::cabrillo::{self, Header};
use zlog_summary::form::{self, Form, Order, Score};

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

const ELOG_HEAD: &str =
    "DATE (JST) TIME   BAND MODE  CALLSIGN      SENTNo      RCVDNo      Mlt    Pts";

fn elog_line(call: &str, h: u32) -> &'static str {
    match (call, h) {
        ("JA1ZLO", 21) => {
            "2026-04-25 21:03     7 CW    JA1ZLO        599 10H     599 13M     13       1"
        }
        ("JA2AAA", _) => {
            "2026-04-25 21:10    14 SSB   JA2AAA        59  10H     59  20M     20       1"
        }
        ("JA3BBB", _) => {
            "2026-04-25 21:30    10 CW    JA3BBB        599 10H     599 25M     -        1"
        }
        ("JA1ZLO", _) => {
            "2026-04-25 22:00     7 CW    JA1ZLO        599 10H     599 13M     -        0"
        }
        ("JH1CCC", _) => {
            "X 2026-04-25 22:05   430 FM    JH1CCC        59  10H     59  10M     -        1"
        }
        _ => "2026-04-25 21:50  1200 CW    JA5EEE        599 10H     599 12M     -        1",
    }
}

fn elog_body(log: &Log) -> Vec<u8> {
    let mut lines = vec![ELOG_HEAD];
    for q in &log.qsos {
        let (_, _, _, h, _, _) = q.time.to_ymd_hms();
        lines.push(elog_line(&q.callsign, h));
    }
    text(&lines).into_bytes()
}

const PERIOD: u32 = 24;

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
    let mut f = Form::default();
    let s = &mut f.summary;
    s.version = version;
    s.contest = "ALL JA1".into();
    s.category_code = "PN".into();
    s.category_name = "Newcomer".into();
    s.callsign = "JA1ZLO".into();
    s.op_callsign = "JA1ZLO".into();
    s.address = "Tokyo\nBunkyo\n".into();
    s.tel = "03-0000-0000".into();
    s.name = "Nextzlog".into();
    s.email = "zlog@example.org".into();
    s.license_class = "1".into();
    s.power = "10".into();
    s.op_place = "Bunkyo".into();
    s.power_supply = "AC".into();
    s.equipment = "IC-7300".into();
    s.comments = "Thanks.".into();
    s.club_number = "10001".into();
    s.club_name = "ZLOG".into();
    s.oath = "I swear.".into();
    s.date = "2026-04-26".into();
    s.signature = "Nextzlog".into();
    f.license_date = Some((2025, 3, 4));
    f
}

fn summary_of(file: &[u8]) -> String {
    let end = file.windows(10).position(|w| w == b"<LOGSHEET ").unwrap();
    rxsl::text::decode_sjis(&file[..end])
}

fn text(lines: &[&str]) -> String {
    lines.iter().map(|l| format!("{l}\r\n")).collect()
}

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
    let file = form::elog(&f, &score, &log(), PERIOD).unwrap();
    let doc = summary_of(&file);
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
    let body = only(&[
        ("JA1ZLO", 25, 21),
        ("JA2AAA", 25, 21),
        ("JA3BBB", 25, 21),
        ("JA5EEE", 25, 21),
        ("JA1ZLO", 25, 22),
        ("JH1CCC", 25, 22),
    ]);
    let mut want = zlog_qxsl::delphi::ansi(&doc);
    want.extend(b"<LOGSHEET TYPE=\"ZLOG_REIWA\">\r\n");
    want.extend(elog_body(&body));
    want.extend(b"</LOGSHEET>\r\n");
    assert_eq!(file, want);
    let bytes = zlog_qxsl::delphi::ansi(&doc);
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
    let file = form::elog(&f, &score, &log(), PERIOD).unwrap();
    let doc = summary_of(&file);
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
    let body = only(&[
        ("JA1ZLO", 25, 21),
        ("JA2AAA", 25, 21),
        ("JA3BBB", 25, 21),
        ("JA1ZLO", 25, 22),
        ("JH1CCC", 25, 22),
        ("JA5EEE", 25, 21),
    ]);
    let mut want = zlog_qxsl::delphi::ansi(&doc);
    want.extend(b"<LOGSHEET TYPE=\"ZLOG_REIWA\">\r\n");
    want.extend(elog_body(&body));
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
    let file = cabrillo::document(&h, &log(), &Plain, "JA1ZLO/1", PERIOD).unwrap();
    let end = file.windows(4).position(|w| w == b"QSO:").unwrap();
    let doc = String::from_utf8(file[..end].to_vec()).unwrap();
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
    let mut want = doc.into_bytes();
    want.extend(
        text(&[
            "QSO:  7003 CW 2026-04-25 1203 JA1ZLO/1      599 10H    JA1ZLO        599 13M    0",
            "QSO: 14000 PH 2026-04-25 1210 JA1ZLO/1       59 10H    JA2AAA         59 20M    0",
            "QSO: 10000 CW 2026-04-25 1230 JA1ZLO/1      599 10H    JA3BBB        599 25M    0",
            "QSO:  1.2G CW 2026-04-25 1250 JA1ZLO/1      599 10H    JA5EEE        599 12M    0",
            "QSO:  7000 CW 2026-04-25 1300 JA1ZLO/1      599 10H    JA1ZLO        599 13M    0",
            "X-QSO:   432 FM 2026-04-25 1305 JA1ZLO/1       59 10H    JH1CCC         59 10M    0",
        ])
        .bytes(),
    );
    want.extend(b"END-OF-LOG:\r\n");
    assert_eq!(file, want);
}

#[test]
fn no_period_writes_every_qso() {
    let score = Score::new(&sheet(), &log(), RANGE);
    let doc = form::elog(&form(Version::R2), &score, &log(), 0).unwrap();
    assert!(String::from_utf8_lossy(&doc).contains("JA4DDD"));
    let doc = cabrillo::document(&Header::default(), &log(), &Plain, "", 0).unwrap();
    assert!(String::from_utf8_lossy(&doc).contains("JA4DDD"));
    let doc = cabrillo::document(&Header::default(), &log(), &Plain, "", PERIOD).unwrap();
    assert!(!String::from_utf8_lossy(&doc).contains("JA4DDD"));
}
