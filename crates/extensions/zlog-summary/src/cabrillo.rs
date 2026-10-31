use rxsl::sheets::cab3::{self, Cab3Sheet};
use rxsl::{Sheet, SheetFormat};
use zlog_core::{Contest, Log};
pub use zlog_qxsl::cabrillo::TAGS;
use zlog_qxsl::cabrillo::{qso_text, Exchange};

use crate::without_out_of_period;

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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub values: std::collections::BTreeMap<String, String>,
    pub zone_hours: i32,
    pub utf8: bool,
}

impl Header {
    pub fn get(&self, tag: &str) -> &str {
        self.values.get(tag).map_or("", String::as_str)
    }
}

fn multi_tx(header: &Header) -> bool {
    header
        .get("CATEGORY-OPERATOR")
        .eq_ignore_ascii_case("MULTI-OP")
        && ["ONE", "TWO"]
            .iter()
            .any(|t| header.get("CATEGORY-TRANSMITTER").eq_ignore_ascii_case(t))
}

pub fn header_fields(header: &Header) -> Vec<rxsl::Field> {
    let mut values: Vec<(&str, &str)> = TAGS.iter().map(|t| (*t, header.get(t))).collect();
    values.push(("CREATED-BY", "ZLOG"));
    cab3::header(&values)
}

pub fn document(
    header: &Header,
    log: &Log,
    contest: &dyn Contest,
    mycall: &str,
    period_hours: u32,
) -> Result<Vec<u8>, String> {
    let body = without_out_of_period(log, period_hours);
    let text = qso_text(
        &body,
        Exchange::of(header.get("CONTEST"), contest),
        multi_tx(header),
        mycall,
        header.zone_hours,
    )?;
    let sheet = Sheet {
        version: cab3::VERSION.into(),
        fields: header_fields(header),
        table: text.into_bytes(),
        ..Sheet::default()
    };
    let utf8 = header.utf8;
    Cab3Sheet { utf8 }.encode(&sheet).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Plain;
    use zlog_core::{Band, DelphiTime, Mode, Qso};
    use zlog_qxsl::cabrillo::qso_lines;
    use zlog_qxsl::delphi::CRLF;

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
        let mut sorted = log();
        sorted.qsos.sort_by(|a, b| a.time.0.total_cmp(&b.time.0));
        let body =
            String::from_utf8(qso_lines(&sorted, Exchange::Plain, false, "", 0).unwrap()).unwrap();
        assert!(text.ends_with(&(lines[27].to_string() + CRLF + &body + "END-OF-LOG:\r\n")));
        assert!(lines[28].contains("DL1XYZ") && lines[30].contains("VK2AA"));
        assert_eq!(lines[31], "END-OF-LOG:");
    }

    #[test]
    fn out_of_period_qsos_are_left_out() {
        let mut l = log();
        l.set_start_time(DelphiTime::from_ymd_hms(2026, 11, 28, 9, 15, 0));
        let h = Header::default();
        let calls = |text: String| {
            ["DL1XYZ", "K1ABC", "VK2AA"]
                .into_iter()
                .filter(|c| text.contains(c))
                .count()
        };
        assert_eq!(calls(text(&h, &l, 1)), 2);
        assert_eq!(calls(text(&h, &l, 24)), 3);
        assert_eq!(calls(text(&h, &l, 0)), 3);
        l.set_start_time(DelphiTime::from_ymd_hms(2026, 11, 29, 0, 0, 0));
        let t = text(&h, &l, 24);
        assert_eq!(calls(t.clone()), 0);
        assert!(t.ends_with("SOAPBOX: \r\nEND-OF-LOG:\r\n"));
    }

    #[test]
    fn a_bad_qtc_memo_stops_the_document() {
        let mut l = log();
        l.qsos[0].memo = "[QTC3/10 DL8WPX]".into();
        assert!(document(&Header::default(), &l, &Plain, "", 0).is_err());
    }

    #[test]
    fn multi_transmitter_from_the_header() {
        let mut h = Header::default();
        h.values
            .insert("CATEGORY-OPERATOR".into(), "MULTI-OP".into());
        assert!(!multi_tx(&h));
        h.values.insert("CATEGORY-TRANSMITTER".into(), "TWO".into());
        assert!(multi_tx(&h));
    }

    #[test]
    fn utf8_keeps_what_ascii_replaces() {
        let mut h = Header::default();
        h.values.insert("NAME".into(), "\u{6771}\u{4eac}".into());
        let ascii = text(&h, &Log::default(), 0);
        assert!(ascii.contains("NAME: ??\r\n"), "{ascii}");
        h.utf8 = true;
        assert!(text(&h, &Log::default(), 0).contains("NAME: \u{6771}\u{4eac}\r\n"));
    }

    #[test]
    fn empty_log() {
        let t = text(&Header::default(), &Log::default(), 0);
        assert!(t.starts_with("START-OF-LOG: 3.0\r\nCALLSIGN: \r\n"));
        assert!(t.ends_with("SOAPBOX: \r\nEND-OF-LOG:\r\n"));
        assert_eq!(t.matches(CRLF).count(), 29);
    }
}
