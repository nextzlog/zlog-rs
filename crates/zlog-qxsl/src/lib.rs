//! The log file formats of the `qxsl` crate as a zLog extension.
//!
//! Ported from qxsl (https://github.com/autodyne/qxsl, LGPL-3.0) by permission of its
//! author, who allows this port to be distributed under the MIT License.
//!
//! Each format reads items, brings them into qxsl's common form
//! ([`qxsl::conv::normalize`]) and maps them to [`Qso`]s; exporting goes the other
//! way. Attributes a [`Qso`] has no field for are dropped and counted
//! ([`QxslFormat::import_counted`]).
//!
//! Times: a file's times are converted to the zone of the log. On import the log's
//! zone is not known, so contacts are given in [`IMPORT_OFFSET`] (JST, zLog's
//! default); on export the log header's UTC offset is used.

use qxsl::{Band as QBand, Item, QName, Time};
use zlog_core::{Band, Contest, DelphiTime, Extension, Log, LogFormat, Mode, Qso};

/// UTC offset in minutes of the times of imported contacts.
pub const IMPORT_OFFSET: i32 = 540;

/// The extension: every format of the `qxsl` crate.
pub struct Qxsl;

impl Extension for Qxsl {
    fn name(&self) -> &str {
        "qxsl formats"
    }

    fn formats(&self) -> Vec<Box<dyn LogFormat>> {
        qxsl::formats()
            .into_iter()
            .map(|f| Box::new(QxslFormat::new(f)) as Box<dyn LogFormat>)
            .collect()
    }
}

/// One `qxsl` format as a [`LogFormat`].
pub struct QxslFormat {
    format: Box<dyn qxsl::Format>,
    name: String,
    /// Own callsign written as the sent `call` of every QSO, for formats that have a
    /// column for it (Cabrillo); without it QxSL writes asterisks.
    mycall: Option<String>,
}

impl QxslFormat {
    pub fn new(format: Box<dyn qxsl::Format>) -> Self {
        let name = format.name().to_string();
        QxslFormat {
            format,
            name,
            mycall: None,
        }
    }

    /// The same format, writing `call` as the own callsign of every QSO.
    pub fn with_mycall(mut self, call: &str) -> Self {
        self.mycall = (!call.trim().is_empty()).then(|| call.trim().to_uppercase());
        self
    }

    /// The contacts of a file and the number of attributes that had no place in them.
    pub fn import_counted(&self, bytes: &[u8]) -> Result<(Vec<Qso>, usize), String> {
        let kind = self.format.kind();
        let raw = self.format.decode(bytes).map_err(|e| e.to_string())?;
        let mut dropped = 0;
        let qsos = raw
            .iter()
            .map(|r| {
                let (mut q, d) = to_qso(&qxsl::conv::normalize(r, kind), IMPORT_OFFSET);
                // qxsl's common form folds 10.4 GHz into 10 GHz; the file's own
                // frequency of a zLog band is kept (see [`exact_band`]), from the
                // band or from ADIF `FREQ` (MHz).
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
                // `LoadFromFileAsADIF` keeps `FREQ` as written (MHz); departure:
                // here it is stored in kHz with one decimal, as zLog's other
                // imports and the entry of a frequency store `Freq`.
                if let Some(k) = r.attrs.adif("FREQ").and_then(mhz_to_khz) {
                    q.freq = k;
                }
                dropped += d;
                q
            })
            .collect();
        Ok((qsos, dropped))
    }
}

/// Put contest fields into an item as ADIF attributes (upper-case names). Fields
/// with an empty name or value are left out; departure: zLog writes an empty
/// exchange field (`<cqz:0>`) and leaves out only an empty contest id.
pub fn add_adif_fields(item: &mut Item, fields: &[(String, String)]) {
    for (name, value) in fields {
        if !name.is_empty() && !value.is_empty() {
            item.attrs.set(
                QName::new(qxsl::ADIF, name.to_ascii_uppercase()),
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

    /// ADIF files (ADI, ADX) also get the contest's fields of each QSO
    /// ([`Contest::adif_fields`]: `CONTEST_ID` and the exchange, as
    /// `TContest.ADIF_Export` writes them). Other formats ignore them.
    fn export(&self, log: &Log, contest: &dyn Contest) -> Result<Vec<u8>, String> {
        let offset = log.utc_offset_minutes().unwrap_or(0);
        let adif = matches!(self.format.kind(), "adis" | "adxs");
        let items: Vec<Item> = log
            .qsos
            .iter()
            .map(|q| {
                let mut it = to_item(q, offset);
                if adif {
                    add_adif_fields(&mut it, &contest.adif_fields(q));
                    // `GetFreqStr2`: the frequency (kHz) in MHz with four decimals.
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
        // As `Format::write`, but a zLog band's own frequency stays in the items
        // where the format writes frequencies, since qxsl's common form folds
        // 10.4 GHz into 10 GHz.
        let items: Vec<Item> = items
            .iter()
            .map(|it| {
                let mut t = qxsl::conv::transform(it, self.format.kind());
                // Only where the transformed item still holds the folded kHz (not
                // a format's own band name, as ADIF and Cabrillo write).
                // Cabrillo writes band designators (10 GHz and 10.4 GHz are both
                // `10G`), which a frequency must not replace.
                let own = it.attrs.qxsl("band").filter(|k| exact_band(k).is_some());
                let own = own.filter(|_| self.format.kind() != "cqww");
                let folded = own.and_then(qxsl::conv::nominal_band);
                if let Some(k) = own.filter(|_| t.attrs.qxsl("band") == folded.as_deref()) {
                    t.attrs.set(q("band"), k);
                }
                t
            })
            .collect();
        self.format.encode(&items).map_err(|e| e.to_string())
    }
}

/// The file contents of the log in the qxsl format of type id `kind` (`jarl`,
/// `cqww`, `adis`, ...), as [`LogFormat::export`] writes them. For writers that
/// wrap a format's body in their own document, such as the summary sheets. `mycall`
/// fills the own callsign (the sent `call`) where the format has a field for it
/// (Cabrillo, ADIF `STATION_CALLSIGN`, QXML).
pub fn encode(
    kind: &str,
    log: &Log,
    contest: &dyn Contest,
    mycall: &str,
) -> Result<Vec<u8>, String> {
    let format = qxsl::formats()
        .into_iter()
        .find(|f| f.kind() == kind)
        .ok_or_else(|| format!("no qxsl format {kind}"))?;
    QxslFormat::new(format)
        .with_mycall(mycall)
        .export(log, contest)
}

/// `TQSO.GetFreqStr2`: zLog's `Freq` (kHz) as ADIF `FREQ` (MHz, four decimals);
/// nothing for an empty or zero frequency.
fn khz_to_mhz(khz: &str) -> Option<String> {
    let k: f64 = khz.trim().parse().ok()?;
    (k > 0.0).then(|| format!("{:.4}", k / 1000.0))
}

/// ADIF `FREQ` (MHz) as zLog's `Freq` (kHz, `%.1f`).
fn mhz_to_khz(mhz: &str) -> Option<String> {
    let m: f64 = mhz.trim().parse().ok()?;
    (m > 0.0).then(|| format!("{:.1}", m * 1000.0))
}

/// Nominal frequency in kHz of each zLog band (as qxsl's band tables use).
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

/// The zLog band whose own nominal frequency `khz` is exactly, if any.
fn exact_band(khz: &str) -> Option<Band> {
    let hz = QBand::parse(khz).ok()?.0;
    BANDS.iter().find(|(_, k)| k * 1000 == hz).map(|b| b.0)
}

/// The zLog band of a frequency in kHz. A band's own nominal frequency maps to
/// that band first, so that 10.4 GHz (written as 10400000) comes back as 10.4G
/// rather than folded into the 10 GHz range by qxsl's band table.
pub fn band_of(khz: &str) -> Band {
    if let Some(band) = exact_band(khz) {
        return band;
    }
    let Some(nominal) = qxsl::conv::nominal_band(khz) else {
        return Band::Unknown;
    };
    // Cabrillo's `75G` (75.5 GHz to 81 GHz) is the band zLog calls 77G.
    if nominal == "75000000" {
        return Band::B77G;
    }
    let hz = QBand::parse(&nominal).map(|b| b.0).unwrap_or(0);
    BANDS
        .iter()
        .find(|(_, k)| k * 1000 == hz)
        .map_or(Band::Unknown, |(b, _)| *b)
}

/// The zLog mode of a mode name as the formats write it.
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

/// An item in qxsl's common form; `offset` is the UTC offset of the QSO's time.
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

/// A QSO from an item in qxsl's common form, with its time at `offset`, and the
/// number of attributes the QSO has no field for.
pub fn to_qso(item: &Item, offset: i32) -> (Qso, usize) {
    let mut qso = Qso::default();
    let mut dropped = 0;
    let get = |k: &str| item.attrs.qxsl(k).unwrap_or("").to_string();
    for (k, v) in item.attrs.iter() {
        let known = k.space == qxsl::QXSL
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
            || (k.space == qxsl::ADIF && k.local == "FREQ");
        if !known && !v.is_empty() {
            dropped += 1;
        }
    }
    // The sent `call` is the own station's, which a QSO does not keep; it is
    // ignored rather than counted as dropped.
    for n in [&item.rcvd, &item.sent] {
        dropped += n
            .iter()
            .filter(|(k, v)| {
                !(k.space == qxsl::QXSL && matches!(k.local.as_str(), "call" | "rstq" | "code"))
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
    let default_rst = if qxsl::conv::is_phone(&mode) { 59 } else { 599 };
    let rst = |n: &qxsl::Node| {
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
mod tests {
    use super::*;

    #[test]
    fn every_band_round_trips_through_its_frequency() {
        // Before the fix 10.4G came back as 10.1G.
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
        for f in Qxsl.formats() {
            let bytes = f.export(&log, &Rtty(false)).unwrap();
            let back = f.import(&bytes).unwrap();
            if back[0].band != Band::B10_4G {
                lost.push(f.name().to_string());
            }
        }
        // Before the fix every format lost it. ADIF carries it as FREQ, zLog ALL in
        // its five-character band column; what remains cannot hold it: Cabrillo has
        // one designator (10G) for 10 GHz and 10.4 GHz, and CTESTWIN's LG8 band index
        // has no 10.4 GHz.
        assert_eq!(lost, ["Cabrillo (CQWW)", "CTESTWIN (LG8)"]);
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
            let f = qxsl::formats().into_iter().find(|f| f.kind() == kind);
            let f = QxslFormat::new(f.unwrap());
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
        assert!(text.starts_with("X-QSO: 14000 CW"), "{text}");
        assert!(back[0].invalid);
        let (text, back) = carried("adis");
        assert!(text.contains("<COMMENT:4>memo"), "{text}");
        assert!(text.contains("<SRX_STRING:3>13M"), "{text}");
        assert_eq!(back[0].memo, "memo");
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
        for f in Qxsl.formats() {
            let bytes = f.export(&log, &Rtty(false)).unwrap();
            let back = f.import(&bytes).unwrap();
            assert_eq!(back.len(), log.qsos.len(), "{}", f.name());
            for (a, b) in log.qsos.iter().zip(&back) {
                let n = f.name();
                assert_eq!(a.callsign, b.callsign, "{n}");
                assert_eq!(a.band, b.band, "{n}");
                assert_eq!(a.nr_rcvd, b.nr_rcvd, "{n}");
                assert_eq!(a.rst_rcvd, b.rst_rcvd, "{n}");
                let (_, mo, d, h, mi, _) = a.time.to_ymd_hms();
                let (_, mo2, d2, h2, mi2, _) = b.time.to_ymd_hms();
                assert_eq!((mo, d, h, mi), (mo2, d2, h2, mi2), "{n}");
                if !n.contains("Cabrillo") {
                    assert_eq!(a.mode, b.mode, "{n}");
                } else {
                    assert_eq!(
                        b.mode,
                        if a.mode == Mode::Cw {
                            Mode::Cw
                        } else {
                            Mode::Ssb
                        }
                    );
                }
                if !n.contains("SWL") && !n.contains("HAMLOG") {
                    assert_eq!(a.nr_sent, b.nr_sent, "{n}");
                }
            }
        }
    }

    #[test]
    fn unknown_attributes_are_counted() {
        let f = QxslFormat::new(Box::new(qxsl::formats::adis::Adis));
        let src = b"<EOH><CALL:6>JA1ZLO<QSO_DATE:8>20170604<TIME_ON:4>0000<BAND:3>20m<MODE:2>CW\
<COMMENT:2>hi<MY_GRIDSQUARE:4>PM95<EOR>";
        let (qsos, dropped) = f.import_counted(src).unwrap();
        // MY_GRIDSQUARE; COMMENT is the memo.
        assert_eq!(dropped, 1);
        assert_eq!(qsos[0].memo, "hi");
        assert_eq!(qsos[0].band, Band::B14);
        assert_eq!(qsos[0].rst_rcvd, 599);
        assert_eq!(qsos[0].time.to_ymd_hms(), (2017, 6, 4, 9, 0, 0));
    }

    /// A contest with the ADIF fields of JARL WW RTTY (UzLogContest.pas:2661, 2688)
    /// when true, with none when false.
    struct Rtty(bool);

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

    fn adi() -> QxslFormat {
        QxslFormat::new(Box::new(qxsl::formats::adis::Adis))
    }

    #[test]
    fn adif_export_carries_the_contest_fields() {
        let bytes = adi().export(&log(), &Rtty(true)).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text.matches("<CONTEST_ID:12>JARL-WW-RTTY").count(), 3);
        assert!(text.contains("<AGE:3>10M"));
        // Empty values are left out.
        assert!(!text.contains("PFX"));
        // The plain export has none of them.
        let plain = String::from_utf8(adi().export(&log(), &Rtty(false)).unwrap()).unwrap();
        assert!(!plain.contains("CONTEST_ID"));
    }

    #[test]
    fn adif_freq_is_the_qso_frequency() {
        let mut log = log();
        log.qsos[0].freq = "14025.5".into();
        for f in [adi(), QxslFormat::new(Box::new(qxsl::formats::adxs::Adxs))] {
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
        let f = QxslFormat::new(Box::new(qxsl::formats::cqww::Cqww));
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
        // CONTEST_ID and AGE have no place in a QSO.
        assert_eq!(dropped, 6);
    }

    #[test]
    fn other_formats_ignore_the_contest_fields() {
        for f in qxsl::formats() {
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
            ("adis", "<STATION_CALLSIGN:6>JA1ZLO"),
            ("cqww", " JA1ZLO        599"),
            ("qxml", "qxsl:call=\"JA1ZLO\"/>"),
        ] {
            let f = qxsl::formats().into_iter().find(|f| f.kind() == kind);
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
        let jarl = QxslFormat::new(Box::new(qxsl::formats::jarl::Jarl::default()));
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
