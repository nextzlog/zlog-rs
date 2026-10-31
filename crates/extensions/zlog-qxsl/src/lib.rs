// Ported from qxsl (https://github.com/autodyne/qxsl, LGPL-3.0) by permission of its
// author, who allows this port to be distributed under the MIT License.

use rxsl::{Band as QBand, Item, QName, Time};
use zlog_core::{Band, Contest, DelphiTime, Extension, Log, LogFormat, Mode, Qso};

pub mod cabrillo;
pub mod delphi;
pub mod text;

pub const IMPORT_OFFSET: i32 = 540;

pub struct Qxsl;

impl Extension for Qxsl {
    fn name(&self) -> &str {
        "qxsl formats"
    }

    fn formats(&self) -> Vec<Box<dyn LogFormat>> {
        rxsl::formats()
            .into_iter()
            .map(|f| Box::new(QxslFormat::new(f)) as Box<dyn LogFormat>)
            .collect()
    }
}

pub struct QxslFormat {
    format: Box<dyn rxsl::Format>,
    name: String,
    mycall: Option<String>,
}

impl QxslFormat {
    pub fn new(format: Box<dyn rxsl::Format>) -> Self {
        let name = format.name().to_string();
        QxslFormat {
            format,
            name,
            mycall: None,
        }
    }

    pub fn with_mycall(mut self, call: &str) -> Self {
        self.mycall = (!call.trim().is_empty()).then(|| call.trim().to_ascii_uppercase());
        self
    }

    pub fn import_counted(&self, bytes: &[u8]) -> Result<(Vec<Qso>, usize), String> {
        let kind = self.format.kind();
        let raw = self.format.decode(bytes).map_err(|e| e.to_string())?;
        let mut dropped = 0;
        let qsos = raw
            .iter()
            .map(|r| {
                let (mut q, d) = to_qso(&rxsl::conv::normalize(r, kind), IMPORT_OFFSET);
                let freq = r
                    .attrs
                    .adif("FREQ")
                    .and_then(|m| QBand::parse(&format!("{}MHz", m.trim())).ok())
                    .map(|b| b.khz());
                if let Some(b) = r
                    .attrs
                    .qxsl("band")
                    .or(freq.as_deref())
                    .and_then(exact_band)
                {
                    q.band = b;
                }
                if let Some(k) = r.attrs.adif("FREQ").and_then(mhz_to_khz) {
                    q.freq = k;
                }
                if matches!(kind, "zall" | "zdos") {
                    zlog_fields(&mut q, r);
                }
                dropped += d;
                q
            })
            .collect();
        Ok((qsos, dropped))
    }
}

fn zlog_fields(q: &mut Qso, r: &Item) {
    zlog_core::zlo::parse_memo_str(q);
    let new = r.attrs.qxsl("newmult") == Some("true");
    q.new_multi1 = new && !q.multi1.is_empty();
    q.new_multi2 = new && !q.multi2.is_empty();
    if let Some(b) = r.attrs.qxsl("band").and_then(|b| QBand::parse(b).ok()) {
        if exact_band(&b.khz()).is_none() {
            q.freq = format!("{}.{}", b.0 / 1000, b.0 % 1000 / 100);
        }
    }
}

pub fn add_adif_fields(item: &mut Item, fields: &[(String, String)]) {
    for (name, value) in fields {
        if !name.is_empty() && !value.is_empty() {
            item.attrs.set(
                QName::new(rxsl::ADIF, name.to_ascii_uppercase()),
                value.clone(),
            );
        }
    }
}

impl LogFormat for QxslFormat {
    fn name(&self) -> &str {
        &self.name
    }

    fn extensions(&self) -> &[&str] {
        self.format.extensions()
    }

    fn can_import(&self) -> bool {
        true
    }

    fn can_export(&self) -> bool {
        true
    }

    fn import(&self, bytes: &[u8]) -> Result<Vec<Qso>, String> {
        self.import_counted(bytes).map(|(q, _)| q)
    }

    fn export(&self, log: &Log, contest: &dyn Contest) -> Result<Vec<u8>, String> {
        match self.format.kind() {
            "zall" => return Ok(text::zlog_all(log)),
            "zdos" => return Ok(text::zlog_dos(log)),
            "adis" => return Ok(text::adif(log, contest)),
            "cqww" => {
                return text::cabrillo(log, contest, self.mycall.as_deref().unwrap_or(""));
            }
            _ => {}
        }
        let offset = log.utc_offset_minutes().unwrap_or(0);
        let adif = matches!(self.format.kind(), "adis" | "adxs");
        let items: Vec<Item> = log
            .qsos
            .iter()
            .map(|q| {
                let mut it = to_item(q, offset);
                if adif {
                    add_adif_fields(&mut it, &contest.adif_fields(q));
                    if let Some(m) = khz_to_mhz(&q.freq) {
                        it.attrs.set(QName::adif("FREQ"), m);
                    }
                }
                if let Some(call) = &self.mycall {
                    it.sent.set(QName::qxsl("call"), call.as_str());
                }
                it
            })
            .collect();
        let items: Vec<Item> = items
            .iter()
            .map(|it| {
                let mut t = rxsl::conv::transform(it, self.format.kind());
                let own = it.attrs.qxsl("band").filter(|k| exact_band(k).is_some());
                let own = own.filter(|_| self.format.kind() != "cqww");
                let folded = own.and_then(rxsl::conv::nominal_band);
                if let Some(k) = own.filter(|_| t.attrs.qxsl("band") == folded.as_deref()) {
                    t.attrs.set(q("band"), k);
                }
                t
            })
            .collect();
        self.format.encode(&items).map_err(|e| e.to_string())
    }
}

pub fn encode(
    kind: &str,
    log: &Log,
    contest: &dyn Contest,
    mycall: &str,
) -> Result<Vec<u8>, String> {
    let format = rxsl::formats()
        .into_iter()
        .find(|f| f.kind() == kind)
        .ok_or_else(|| format!("no qxsl format {kind}"))?;
    QxslFormat::new(format)
        .with_mycall(mycall)
        .export(log, contest)
}

fn khz_to_mhz(khz: &str) -> Option<String> {
    let k: f64 = khz.trim().parse().ok()?;
    (k > 0.0).then(|| format!("{:.4}", k / 1000.0))
}

fn mhz_to_khz(mhz: &str) -> Option<String> {
    let m: f64 = mhz.trim().parse().ok()?;
    (m > 0.0).then(|| format!("{:.1}", m * 1000.0))
}

const BANDS: &[(Band, u64)] = &[
    (Band::B1_9, 1900),
    (Band::B3_5, 3500),
    (Band::B7, 7000),
    (Band::B10, 10000),
    (Band::B14, 14000),
    (Band::B18, 18000),
    (Band::B21, 21000),
    (Band::B24, 24000),
    (Band::B28, 28000),
    (Band::B50, 50000),
    (Band::B144, 144000),
    (Band::B430, 430000),
    (Band::B1200, 1200000),
    (Band::B2400, 2400000),
    (Band::B5600, 5600000),
    (Band::B10G, 10000000),
    (Band::B10_4G, 10400000),
    (Band::B24G, 24000000),
    (Band::B47G, 47000000),
    (Band::B77G, 77000000),
    (Band::B135G, 135000000),
    (Band::B248G, 248000000),
];

pub(crate) fn exact_band(khz: &str) -> Option<Band> {
    let hz = QBand::parse(khz).ok()?.0;
    BANDS.iter().find(|(_, k)| k * 1000 == hz).map(|b| b.0)
}

pub fn band_of(khz: &str) -> Band {
    if let Some(band) = exact_band(khz) {
        return band;
    }
    let Some(nominal) = rxsl::conv::nominal_band(khz) else {
        return Band::Unknown;
    };
    if nominal == "75000000" {
        return Band::B77G;
    }
    let hz = QBand::parse(&nominal).map(|b| b.0).unwrap_or(0);
    BANDS
        .iter()
        .find(|(_, k)| k * 1000 == hz)
        .map_or(Band::Unknown, |(b, _)| *b)
}

pub fn mode_of(text: &str) -> Mode {
    match text.to_ascii_uppercase().as_str() {
        "CW" => Mode::Cw,
        "SSB" | "LSB" | "USB" | "DSB" | "PH" => Mode::Ssb,
        "FM" => Mode::Fm,
        "AM" => Mode::Am,
        "RTTY" | "DG" | "RY" => Mode::Rtty,
        "FT4" => Mode::Ft4,
        "FT8" => Mode::Ft8,
        "DV" | "D-STAR" | "C4FM" => Mode::Dv,
        _ => Mode::Other,
    }
}

fn q(local: &str) -> QName {
    QName::qxsl(local)
}

fn nonempty(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}

pub fn to_item(qso: &Qso, offset: i32) -> Item {
    let mut it = Item::default();
    let (y, mo, d, h, mi, s) = qso.time.to_ymd_hms();
    let time = Time::new((y, mo, d), (h, mi, s), offset);
    it.attrs.set(q("time"), time.to_iso());
    it.rcvd.put(q("call"), nonempty(&qso.callsign));
    let khz = BANDS.iter().find(|(b, _)| *b == qso.band).map(|b| b.1);
    it.attrs.put(q("band"), khz.map(|k| k.to_string()));
    it.attrs.set(q("mode"), qso.mode.label());
    it.attrs.put(q("name"), nonempty(&qso.operator));
    it.attrs.put(q("note"), nonempty(&qso.memo));
    it.attrs.put(q("mul1"), nonempty(&qso.multi1));
    it.attrs.put(q("mul2"), nonempty(&qso.multi2));
    it.attrs.set(q("points"), qso.points.to_string());
    it.attrs.set(q("newmult"), qso.new_multi1.to_string());
    if qso.invalid {
        it.attrs.set(q("invalid"), "true");
    }
    it.rcvd.set(q("rstq"), qso.rst_rcvd.to_string());
    it.rcvd.put(q("code"), nonempty(&qso.nr_rcvd));
    it.sent.set(q("rstq"), qso.rst_sent.to_string());
    it.sent.put(q("code"), nonempty(&qso.nr_sent));
    it
}

pub fn to_qso(item: &Item, offset: i32) -> (Qso, usize) {
    let mut qso = Qso::default();
    let mut dropped = 0;
    let get = |k: &str| item.attrs.qxsl(k).unwrap_or("").to_string();
    for (k, v) in item.attrs.iter() {
        let known = k.space == rxsl::QXSL
            && matches!(
                k.local.as_str(),
                "time"
                    | "band"
                    | "mode"
                    | "name"
                    | "note"
                    | "mul1"
                    | "mul2"
                    | "points"
                    | "newmult"
                    | "invalid"
            )
            || (k.space == rxsl::ADIF && k.local == "FREQ");
        if !known && !v.is_empty() {
            dropped += 1;
        }
    }
    for n in [&item.rcvd, &item.sent] {
        dropped += n
            .iter()
            .filter(|(k, v)| {
                !(k.space == rxsl::QXSL && matches!(k.local.as_str(), "call" | "rstq" | "code"))
                    && !v.is_empty()
            })
            .count();
    }
    if let Some(t) = item
        .attrs
        .qxsl("time")
        .and_then(|t| Time::parse_iso(t).ok())
    {
        let t = t.at_offset(offset);
        qso.time = DelphiTime::from_ymd_hms(t.year, t.month, t.day, t.hour, t.minute, t.second);
    }
    qso.callsign = item.rcvd.qxsl("call").unwrap_or("").to_string();
    qso.band = item.attrs.qxsl("band").map_or(Band::Unknown, band_of);
    let mode = get("mode");
    qso.mode = mode_of(&mode);
    qso.operator = get("name");
    qso.memo = get("note");
    qso.multi1 = get("mul1");
    qso.multi2 = get("mul2");
    if let Some(p) = item.attrs.qxsl("points").and_then(|p| p.parse().ok()) {
        qso.points = p;
    }
    qso.new_multi1 = item.attrs.qxsl("newmult") == Some("true");
    qso.invalid = item.attrs.qxsl("invalid") == Some("true");
    let default_rst = if rxsl::conv::is_phone(&mode) { 59 } else { 599 };
    let rst = |n: &rxsl::Node| {
        n.qxsl("rstq")
            .and_then(|r| r.parse().ok())
            .unwrap_or(default_rst)
    };
    qso.rst_rcvd = rst(&item.rcvd);
    qso.rst_sent = rst(&item.sent);
    qso.nr_rcvd = item.rcvd.qxsl("code").unwrap_or("").to_string();
    qso.nr_sent = item.sent.qxsl("code").unwrap_or("").to_string();
    (qso, dropped)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn every_band_round_trips_through_its_frequency() {
        for (band, khz) in BANDS {
            assert_eq!(band_of(&khz.to_string()), *band, "{khz}");
        }
    }

    #[test]
    fn band_of_edge_cases() {
        assert_eq!(band_of("7012.5"), Band::B7);
        assert_eq!(band_of(""), Band::Unknown);
        assert_eq!(band_of("abc"), Band::Unknown);
        assert_eq!(band_of("123"), Band::Unknown);
    }

    #[test]
    fn ten_point_four_gigahertz_survives_every_format() {
        let mut log = Log::default();
        log.qsos.push(Qso {
            callsign: "JA1ZLO".into(),
            band: Band::B10_4G,
            mode: Mode::Ssb,
            time: DelphiTime::from_ymd_hms(2026, 6, 6, 21, 0, 0),
            rst_sent: 59,
            rst_rcvd: 59,
            nr_sent: "10H".into(),
            nr_rcvd: "13M".into(),
            ..Qso::default()
        });
        let mut lost = Vec::new();
        for f in formats() {
            let bytes = f.export(&log, &Rtty(false)).unwrap();
            let back = f.import(&bytes).unwrap();
            if back[0].band != Band::B10_4G {
                lost.push(f.name().to_string());
            }
        }
        let lost_by_zlog = ["ADIF (ADI)", "Cabrillo (CQWW)", "CTESTWIN (LG8)"];
        assert_eq!(lost, lost_by_zlog);
    }

    #[test]
    fn memo_points_new_multiplier_and_invalid_map_to_qxsl() {
        let mut qso = Qso {
            callsign: "JA1ZLO".into(),
            band: Band::B14,
            mode: Mode::Cw,
            time: DelphiTime::from_ymd_hms(2026, 6, 6, 21, 0, 0),
            rst_sent: 599,
            rst_rcvd: 599,
            nr_sent: "10H".into(),
            nr_rcvd: "13M".into(),
            multi1: "13".into(),
            new_multi1: true,
            points: 3,
            memo: "memo".into(),
            ..Qso::default()
        };
        let it = to_item(&qso, 540);
        assert_eq!(it.attrs.qxsl("note"), Some("memo"));
        assert_eq!(it.attrs.qxsl("points"), Some("3"));
        assert_eq!(it.attrs.qxsl("newmult"), Some("true"));
        assert_eq!(it.attrs.qxsl("invalid"), None);
        let (back, dropped) = to_qso(&it, 540);
        assert_eq!(dropped, 0);
        assert_eq!(
            (back.points, back.new_multi1, back.invalid),
            (3, true, false)
        );
        qso.invalid = true;
        qso.new_multi1 = false;
        let mut log = Log::default();
        log.qsos.push(qso.clone());
        let carried = |kind: &str| {
            let f = rxsl::formats().into_iter().find(|f| f.kind() == kind);
            let f = QxslFormat::new(f.unwrap()).with_mycall("JA1ZZZ");
            let bytes = f.export(&log, &Rtty(false)).unwrap();
            (
                String::from_utf8_lossy(&bytes).into_owned(),
                f.import(&bytes).unwrap(),
            )
        };
        let (text, back) = carried("jarl");
        assert!(
            text.lines().nth(1).unwrap().starts_with("X 2026-06-0"),
            "{text}"
        );
        assert!(text.trim_end().ends_with("-        3"), "{text}");
        assert_eq!(
            (back[0].points, back[0].new_multi1, back[0].invalid),
            (3, false, true)
        );
        let (text, back) = carried("cqww");
        assert!(text.contains("\r\nX-QSO: 14000 CW"), "{text}");
        assert!(back[0].invalid);
        let (text, back) = carried("adis");
        assert!(!text.contains("memo"), "{text}");
        assert!(text.contains("<srx_string:3>13M"), "{text}");
        assert_eq!(back[0].nr_rcvd, "13M");
        let (_, back) = carried("qxml");
        assert_eq!(
            (back[0].points, back[0].new_multi1, back[0].invalid),
            (3, false, true)
        );
    }

    fn log() -> Log {
        let mut log = Log::default();
        // zLog keeps the negated UTC offset in the header: JST is -540.
        log.header.rst_sent = (-540i16) as u16;
        for (i, (call, mode, band)) in [
            ("JA1ZLO", Mode::Cw, Band::B14),
            ("JA2XYZ", Mode::Ssb, Band::B430),
            ("JR3ABC", Mode::Fm, Band::B1200),
        ]
        .into_iter()
        .enumerate()
        {
            let phone = mode != Mode::Cw;
            log.qsos.push(Qso {
                time: DelphiTime::from_ymd_hms(2017, 6, 4, 9, i as u32, 0),
                callsign: call.into(),
                mode,
                band,
                rst_sent: if phone { 59 } else { 599 },
                rst_rcvd: if phone { 57 } else { 579 },
                nr_sent: "100110".into(),
                nr_rcvd: format!("1{i}M"),
                operator: format!("OP{i}"),
                ..Qso::default()
            });
        }
        log
    }

    #[test]
    fn every_format_round_trips_the_fields_it_carries() {
        let log = log();
        assert_eq!(log.utc_offset_minutes(), Some(540));
        for f in formats() {
            let bytes = f.export(&log, &Rtty(false)).unwrap();
            let back = f.import(&bytes).unwrap();
            assert_eq!(back.len(), log.qsos.len(), "{}", f.name());
            for (a, b) in log.qsos.iter().zip(&back) {
                let n = f.name();
                assert_eq!(a.callsign, b.callsign, "{n}");
                assert_eq!(a.band, b.band, "{n}");
                if n != "zLog (DOS)" {
                    assert_eq!(a.nr_rcvd, b.nr_rcvd, "{n}");
                    assert_eq!(a.rst_rcvd, b.rst_rcvd, "{n}");
                }
                let (_, mo, d, h, mi, _) = a.time.to_ymd_hms();
                let (_, mo2, d2, h2, mi2, _) = b.time.to_ymd_hms();
                assert_eq!((mo, d, h, mi), (mo2, d2, h2, mi2), "{n}");
                assert_eq!(a.mode, b.mode, "{n}");
                if !n.contains("SWL") && !n.contains("HAMLOG") && !n.contains("DOS") {
                    assert_eq!(a.nr_sent, b.nr_sent, "{n}");
                }
            }
        }
    }

    #[test]
    fn unknown_attributes_are_counted() {
        let f = QxslFormat::new(Box::new(rxsl::formats::adis::Adis));
        let src = b"<EOH><CALL:6>JA1ZLO<QSO_DATE:8>20170604<TIME_ON:4>0000<BAND:3>20m<MODE:2>CW\
<COMMENT:2>hi<MY_GRIDSQUARE:4>PM95<EOR>";
        let (qsos, dropped) = f.import_counted(src).unwrap();
        assert_eq!(dropped, 1);
        assert_eq!(qsos[0].memo, "hi");
        assert_eq!(qsos[0].band, Band::B14);
        assert_eq!(qsos[0].rst_rcvd, 599);
        assert_eq!(qsos[0].time.to_ymd_hms(), (2017, 6, 4, 9, 0, 0));
    }

    pub(crate) struct Rtty(pub(crate) bool);

    impl Contest for Rtty {
        fn name(&self) -> &str {
            "RTTY"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B3_5, Band::B28)
        }
        fn contest_mode(&self) -> zlog_core::ContestMode {
            zlog_core::ContestMode::Rtty
        }
        fn sent_number(&self, _: Band, _: zlog_core::Power, _: &zlog_core::Station) -> String {
            String::new()
        }
        fn valid_number(&self, _: &Qso) -> bool {
            true
        }
        fn score(&mut self, log: &Log) -> zlog_core::Scoring {
            zlog_core::Scoring {
                qsos: vec![Default::default(); log.qsos.len()],
                ..Default::default()
            }
        }
        fn adif_fields(&self, q: &Qso) -> Vec<(String, String)> {
            if !self.0 {
                return Vec::new();
            }
            vec![
                ("contest_id".into(), "JARL-WW-RTTY".into()),
                ("age".into(), q.nr_rcvd.clone()),
                ("pfx".into(), String::new()),
            ]
        }
    }

    fn formats() -> Vec<QxslFormat> {
        let new = |f| QxslFormat::new(f).with_mycall("JA1ZZZ");
        rxsl::formats().into_iter().map(new).collect()
    }

    fn adi() -> QxslFormat {
        QxslFormat::new(Box::new(rxsl::formats::adis::Adis))
    }

    #[test]
    fn adif_export_carries_the_contest_fields() {
        let bytes = adi().export(&log(), &Rtty(true)).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text.matches("<contest_id:12>JARL-WW-RTTY<eor>").count(), 3);
        assert!(text.contains("<srx_string:3>10M<age:3>10M<band:3>20m"));
        assert!(!text.contains("pfx"));
        let plain = String::from_utf8(adi().export(&log(), &Rtty(false)).unwrap()).unwrap();
        assert!(!plain.contains("contest_id"));
    }

    #[test]
    fn adif_freq_is_the_qso_frequency() {
        let mut log = log();
        log.qsos[0].freq = "14025.5".into();
        for f in [adi(), QxslFormat::new(Box::new(rxsl::formats::adxs::Adxs))] {
            let bytes = f.export(&log, &Rtty(false)).unwrap();
            let text = String::from_utf8(bytes.clone()).unwrap();
            assert!(text.contains("14.0255"), "{text}");
            let back = f.import(&bytes).unwrap();
            assert_eq!(back[0].freq, "14025.5");
            assert_eq!(back[0].band, Band::B14);
            assert_eq!(back[1].freq, "");
        }
    }

    #[test]
    fn cabrillo_modes_and_single_spaced_lines_are_imported() {
        let f = QxslFormat::new(Box::new(rxsl::formats::cqww::Cqww));
        let text = "QSO: 7000 RY 2017-06-04 0015 JA1ZLO 599 1 QV1DOK 599 2\r\n\
                    QSO: 144 FM 2017-06-04 0016 JA1ZLO 59 1 QV1DOK 59 3\r\n\
                    QSO: 144 DV 2017-06-04 0017 JA1ZLO 59 1 QV1DOK 59 4\r\n\
                    QSO: 21000 PH 2017-06-04 0018 JA1ZLO 59 1 QV1DOK 59 5\r\n\
                    QSO: 21000 DG 2017-06-04 0019 JA1ZLO 599 1 QV1DOK 599 6\r\n";
        let q = f.import(text.as_bytes()).unwrap();
        let modes: Vec<Mode> = q.iter().map(|q| q.mode).collect();
        assert_eq!(
            modes,
            [Mode::Rtty, Mode::Fm, Mode::Dv, Mode::Ssb, Mode::Rtty]
        );
        assert_eq!(q[0].callsign, "QV1DOK");
        assert_eq!(q[0].nr_rcvd, "2");
        assert_eq!(q[1].band, Band::B144);
    }

    #[test]
    fn contest_fields_survive_an_adif_round_trip_as_unknown_fields() {
        let bytes = adi().export(&log(), &Rtty(true)).unwrap();
        let (qsos, dropped) = adi().import_counted(&bytes).unwrap();
        assert_eq!(qsos.len(), 3);
        assert_eq!(dropped, 9);
    }

    #[test]
    fn other_formats_ignore_the_contest_fields() {
        for f in rxsl::formats() {
            let f = QxslFormat::new(f);
            if matches!(f.format.kind(), "adis" | "adxs") {
                continue;
            }
            assert_eq!(
                f.export(&log(), &Rtty(true)).unwrap(),
                f.export(&log(), &Rtty(false)).unwrap(),
                "{}",
                f.name()
            );
        }
    }

    #[test]
    fn mycall_is_the_sent_call_and_ignored_on_import() {
        for (kind, want) in [
            ("cqww", " JA1ZLO        599"),
            ("qxml", "qxsl:call=\"JA1ZLO\"/>"),
        ] {
            let f = rxsl::formats().into_iter().find(|f| f.kind() == kind);
            let f = QxslFormat::new(f.unwrap()).with_mycall("ja1zlo");
            let bytes = f.export(&log(), &Rtty(false)).unwrap();
            assert!(String::from_utf8_lossy(&bytes).contains(want), "{kind}");
            let (qsos, dropped) = f.import_counted(&bytes).unwrap();
            assert_eq!(dropped, 0, "{kind}");
            let calls: Vec<_> = qsos.iter().map(|q| q.callsign.as_str()).collect();
            let want: Vec<_> = log().qsos.iter().map(|q| q.callsign.clone()).collect();
            assert_eq!(calls, want, "{kind}");
        }
    }

    #[test]
    fn peer_call_is_the_rcvd_call() {
        let it = to_item(&log().qsos[0], 0);
        assert_eq!(it.rcvd.qxsl("call"), Some(log().qsos[0].callsign.as_str()));
        assert_eq!(it.attrs.qxsl("call"), None);
        assert_eq!(it.sent.qxsl("call"), None);
    }

    #[test]
    fn encode_by_kind_equals_the_format_export() {
        let jarl = QxslFormat::new(Box::new(rxsl::formats::jarl::Jarl::default()));
        assert_eq!(
            encode("jarl", &log(), &Rtty(true), "").unwrap(),
            jarl.export(&log(), &Rtty(true)).unwrap()
        );
        assert!(encode("nothing", &log(), &Rtty(true), "").is_err());
    }

    #[test]
    fn add_adif_fields_skips_empty_names_and_values() {
        let mut it = Item::default();
        add_adif_fields(
            &mut it,
            &[
                ("".into(), "x".into()),
                ("cqz".into(), "".into()),
                ("ituz".into(), "45".into()),
            ],
        );
        assert_eq!(it.attrs.adif("ITUZ"), Some("45"));
        assert_eq!(it.attrs.adif("CQZ"), None);
        assert_eq!(it.attrs.iter().count(), 1);
    }
}
