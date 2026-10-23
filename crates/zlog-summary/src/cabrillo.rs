//! Cabrillo 3.0 (`TformELogCabrillo`, `TLog.SaveToFileAsCabrillo`): the header
//! entered in the form, then the QSO lines and `END-OF-LOG:`.
//!
//! The header is written as zLog writes it. The QSO lines are the QxSL `cqww`
//! format as zlog-qxsl writes it ([`zlog_qxsl::encode`]): the station's callsign
//! as the own call, UTC times, and `X-QSO:` for invalid QSOs, as zLog writes them.
//! Departures from zLog's own lines (`SaveToFileAsCabrillo`): the band designator
//! instead of the actual frequency on HF (24 MHz as `24000`, zLog `24500`), the
//! modes as `CW`, `PH`, `DG` (zLog `RY` for RTTY, `FM`, `DV`), no JST times on
//! request, no TX number, no CQ WW RTTY exchange and no QTC lines.

use zlog_core::{Contest, Log};

use crate::delphi::CRLF;
use crate::without_out_of_period;

/// The header tags in the order zLog writes them, with their keys in
/// `zlog_cbr.ini` (the same text). `CREATED-BY` is fixed.
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

/// The choices of the form's drop-down lists (`UELogCabrillo.dfm`); tags not
/// listed are free text.
pub fn choices(tag: &str) -> &'static [&'static str] {
    match tag {
        "CONTEST" => &[
            "ARRL-10",
            "ARRL-10-GHZ",
            "ARRL-160",
            "ARRL-DIGI",
            "ARRL-DX-CW",
            "ARRL-DX-SSB",
            "ARRL-EME",
            "ARRL-SS-CW",
            "ARRL-SS-SSB",
            "BARTG-RTTY",
            "CQ-160-CW",
            "CQ-160-SSB",
            "CQ-WPX-CW",
            "CQ-WPX-RTTY",
            "CQ-WPX-SSB",
            "CQ-VHF",
            "CQ-WW-CW",
            "CQ-WW-RTTY",
            "CQ-WW-SSB",
            "IARU-HF",
            "NAQP-CW",
            "NAQP-SSB",
            "NAQP-RTTY",
            "RDXC",
            "RSGB-IOTA",
            "SPDXC",
            "SPDXC-RTTY",
            "TARA-RTTY",
            "WAG",
            "WW-DIGI",
        ],
        "CATEGORY-ASSISTED" => &["ASSISTED", "NON-ASSISTED"],
        "CATEGORY-BAND" => &[
            "ALL",
            "160M",
            "80M",
            "40M",
            "20M",
            "15M",
            "10M",
            "6M",
            "4M",
            "2M",
            "222",
            "432",
            "902",
            "1.2G",
            "2.3G",
            "3.4G",
            "5.7G",
            "10G",
            "24G",
            "47G",
            "75G",
            "122G",
            "134G",
            "241G",
            "Light",
            "VHF-3-BAND and VHF-FM-ONLY",
        ],
        "CATEGORY-MODE" => &["CW", "DIGI", "FM", "RTTY", "SSB", "MIXED"],
        "CATEGORY-OPERATOR" => &["SINGLE-OP", "MULTI-OP", "CHECKLOG"],
        "CATEGORY-POWER" => &["HIGH", "LOW", "QRP"],
        "CATEGORY-STATION" => &[
            "DISTRIBUTED",
            "FIXED",
            "MOBILE",
            "PORTABLE",
            "ROVER",
            "ROVER-LIMITED",
            "ROVER-UNLIMITED",
            "EXPEDITION",
            "HQ",
            "SCHOOL",
            "EXPLORER",
        ],
        "CATEGORY-TIME" => &["6-HOURS", "8-HOURS", "12-HOURS", "24-HOURS"],
        "CATEGORY-TRANSMITTER" => &["ONE", "TWO", "LIMITED", "UNLIMITED", "SWL"],
        "CATEGORY-OVERLAY" => &[
            "CLASSIC",
            "ROOKIE",
            "TB-WIRES",
            "YOUTH",
            "NOVICE-TECH",
            "YL",
        ],
        "CERTIFICATE" => &["YES", "NO"],
        _ => &[],
    }
}

/// The header as entered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    /// Values by tag; missing tags are written empty.
    pub values: std::collections::BTreeMap<String, String>,
}

impl Header {
    pub fn get(&self, tag: &str) -> &str {
        self.values.get(tag).map_or("", String::as_str)
    }
}

/// The header block, from `START-OF-LOG: 3.0` to the last tag.
pub fn header_block(header: &Header) -> String {
    let mut out = format!("START-OF-LOG: 3.0{CRLF}");
    for tag in TAGS {
        out.push_str(&format!("{tag}: {}{CRLF}", header.get(tag)));
        if tag == "CLUB" {
            out.push_str(&format!("CREATED-BY: ZLOG{CRLF}"));
        }
    }
    out
}

/// The whole file (`buttonCreateLogClick` with `SaveToFileAsCabrillo`): the
/// header in Shift_JIS, the QSOs sorted by time (as zLog sorts its records) in
/// QxSL's `cqww` format, and `END-OF-LOG:`. QSOs outside the contest period of
/// `period_hours` ([`Log::out_of_period`]) are left out, as zLog does with its
/// default `_output_outofperiod` off.
pub fn document(
    header: &Header,
    log: &Log,
    contest: &dyn Contest,
    mycall: &str,
    period_hours: u32,
) -> Result<Vec<u8>, String> {
    let mut body = without_out_of_period(log, period_hours);
    body.qsos.sort_by(|a, b| a.time.0.total_cmp(&b.time.0));
    let mut out = qxsl::text::encode_sjis(&header_block(header));
    out.extend(zlog_qxsl::encode("cqww", &body, contest, mycall)?);
    out.extend(format!("END-OF-LOG:{CRLF}").bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Plain;
    use zlog_core::{Band, DelphiTime, Mode, Qso};

    fn log() -> Log {
        let mut log = Log::default();
        log.header.rst_sent = (-540i16) as u16;
        let q = |h, call: &str, band| Qso {
            time: DelphiTime::from_ymd_hms(2026, 11, 28, h, 15, 0),
            callsign: call.into(),
            band,
            mode: Mode::Cw,
            rst_sent: 599,
            rst_rcvd: 599,
            nr_sent: "25".into(),
            nr_rcvd: "14".into(),
            ..Qso::default()
        };
        log.qsos = vec![
            q(10, "K1ABC", Band::B14),
            q(9, "DL1XYZ", Band::B50),
            q(11, "VK2AA", Band::B7),
        ];
        log
    }

    fn text(h: &Header, l: &Log, period: u32) -> String {
        String::from_utf8(document(h, l, &Plain, "", period).unwrap()).unwrap()
    }

    #[test]
    fn document_layout() {
        let mut h = Header::default();
        h.values.insert("CALLSIGN".into(), "JA1ZLO".into());
        h.values.insert("CONTEST".into(), "CQ-WW-CW".into());
        h.values.insert("CLAIMED-SCORE".into(), "12".into());
        let text = text(&h, &log(), 0);
        let lines: Vec<&str> = text.split(CRLF).collect();
        assert_eq!(lines[0], "START-OF-LOG: 3.0");
        assert_eq!(lines[1], "CALLSIGN: JA1ZLO");
        assert_eq!(lines[3], "CATEGORY-ASSISTED: ");
        assert_eq!(lines[13], "CLAIMED-SCORE: 12");
        assert_eq!(lines[15], "CREATED-BY: ZLOG");
        assert_eq!(lines[27], "SOAPBOX: ");
        // The QSO lines in time order, as zlog-qxsl writes them.
        let mut sorted = log();
        sorted.qsos.sort_by(|a, b| a.time.0.total_cmp(&b.time.0));
        let body =
            String::from_utf8(zlog_qxsl::encode("cqww", &sorted, &Plain, "").unwrap()).unwrap();
        assert_eq!(text, header_block(&h) + &body + "END-OF-LOG:\r\n");
        assert!(lines[28].contains("DL1XYZ") && lines[30].contains("VK2AA"));
        assert_eq!(lines[31], "END-OF-LOG:");
    }

    #[test]
    fn out_of_period_qsos_are_left_out() {
        // Before the fix every QSO was written whatever the period.
        let mut l = log();
        l.set_start_time(DelphiTime::from_ymd_hms(2026, 11, 28, 9, 15, 0));
        let h = Header::default();
        let calls = |text: String| {
            ["DL1XYZ", "K1ABC", "VK2AA"]
                .into_iter()
                .filter(|c| text.contains(c))
                .count()
        };
        // From 09:15 for 1 h: 09:15 and 10:15 are in (the end is inclusive).
        assert_eq!(calls(text(&h, &l, 1)), 2);
        assert_eq!(calls(text(&h, &l, 24)), 3);
        // No period: all.
        assert_eq!(calls(text(&h, &l, 0)), 3);
        // Start after every QSO: the header and the end only.
        l.set_start_time(DelphiTime::from_ymd_hms(2026, 11, 29, 0, 0, 0));
        let t = text(&h, &l, 24);
        assert_eq!(calls(t.clone()), 0);
        assert!(t.ends_with("SOAPBOX: \r\nEND-OF-LOG:\r\n"));
    }

    #[test]
    fn empty_log() {
        let t = text(&Header::default(), &Log::default(), 0);
        assert!(t.starts_with("START-OF-LOG: 3.0\r\nCALLSIGN: \r\n"));
        assert!(t.ends_with("SOAPBOX: \r\nEND-OF-LOG:\r\n"));
        assert_eq!(t.matches(CRLF).count(), 29);
    }
}
