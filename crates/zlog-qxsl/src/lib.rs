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
use zlog_core::{Band, DelphiTime, Extension, Log, LogFormat, Mode, Qso};

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
}

impl QxslFormat {
    pub fn new(format: Box<dyn qxsl::Format>) -> Self {
        let name = format.name().to_string();
        QxslFormat { format, name }
    }

    /// The contacts of a file and the number of attributes that had no place in them.
    pub fn import_counted(&self, bytes: &[u8]) -> Result<(Vec<Qso>, usize), String> {
        let items = self.format.read(bytes).map_err(|e| e.to_string())?;
        let mut dropped = 0;
        let qsos = items
            .iter()
            .map(|i| {
                let (q, d) = to_qso(i, IMPORT_OFFSET);
                dropped += d;
                q
            })
            .collect();
        Ok((qsos, dropped))
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

    fn export(&self, log: &Log) -> Result<Vec<u8>, String> {
        let offset = log.utc_offset_minutes().unwrap_or(0);
        let items: Vec<Item> = log.qsos.iter().map(|q| to_item(q, offset)).collect();
        self.format.write(&items).map_err(|e| e.to_string())
    }
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

/// The zLog band of a frequency in kHz.
pub fn band_of(khz: &str) -> Band {
    let Some(nominal) = qxsl::conv::nominal_band(khz) else {
        return Band::Unknown;
    };
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
        "RTTY" | "DG" => Mode::Rtty,
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
    it.attrs.put(q("call"), nonempty(&qso.callsign));
    let khz = BANDS.iter().find(|(b, _)| *b == qso.band).map(|b| b.1);
    it.attrs.put(q("band"), khz.map(|k| k.to_string()));
    it.attrs.set(q("mode"), qso.mode.label());
    it.attrs.put(q("name"), nonempty(&qso.operator));
    it.attrs.put(q("note"), nonempty(&qso.memo));
    it.attrs.put(q("mul1"), nonempty(&qso.multi1));
    it.attrs.put(q("mul2"), nonempty(&qso.multi2));
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
                "time" | "call" | "band" | "mode" | "name" | "note" | "mul1" | "mul2"
            );
        if !known && !v.is_empty() {
            dropped += 1;
        }
    }
    for n in [&item.rcvd, &item.sent] {
        dropped += n
            .iter()
            .filter(|(k, v)| {
                !(k.space == qxsl::QXSL && matches!(k.local.as_str(), "rstq" | "code"))
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
    qso.callsign = get("call");
    qso.band = item.attrs.qxsl("band").map_or(Band::Unknown, band_of);
    let mode = get("mode");
    qso.mode = mode_of(&mode);
    qso.operator = get("name");
    qso.memo = get("note");
    qso.multi1 = get("mul1");
    qso.multi2 = get("mul2");
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
            let bytes = f.export(&log).unwrap();
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
        assert_eq!(dropped, 2);
        assert_eq!(qsos[0].band, Band::B14);
        assert_eq!(qsos[0].rst_rcvd, 599);
        assert_eq!(qsos[0].time.to_ymd_hms(), (2017, 6, 4, 9, 0, 0));
    }
}
