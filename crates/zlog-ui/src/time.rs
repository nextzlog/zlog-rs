//! A QSO time typed by the operator, as the dialogs show it.

use zlog_core::DelphiTime;

/// The time typed in a dialog, as shown ("2026-06-06 21:05:00"); the seconds may
/// be left out (zLog's date and time pickers).
pub fn parse_time(text: &str) -> Option<DelphiTime> {
    let t = chrono::NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M"))
        .ok()?;
    use chrono::{Datelike, Timelike};
    Some(DelphiTime::from_ymd_hms(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_round_trips_through_its_text() {
        let t = DelphiTime::from_ymd_hms(2026, 6, 6, 21, 5, 9);
        assert_eq!(parse_time(&t.to_string()), Some(t));
        assert_eq!(
            parse_time(" 2026-06-06 21:05 "),
            Some(DelphiTime::from_ymd_hms(2026, 6, 6, 21, 5, 0))
        );
    }

    #[test]
    fn malformed_time_is_refused() {
        for text in [
            "",
            "21:05",
            "2026-02-30 10:00:00",
            "2026-06-06 24:00:00",
            "garbage",
        ] {
            assert_eq!(parse_time(text), None, "{text}");
        }
    }
}
