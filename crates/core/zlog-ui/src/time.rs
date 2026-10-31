use zlog_core::DelphiTime;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StartTimeDone {
    Set { start: DelphiTime, judge: bool },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StartTimeForm {
    pub date: String,
    pub time: String,
    pub judge: bool,
    pub new_log: bool,
}

impl StartTimeForm {
    pub fn new(start: DelphiTime, new_log: bool) -> Self {
        let (y, mo, d, h, mi, _) = start.to_ymd_hms();
        StartTimeForm {
            date: format!("{y:04}-{mo:02}-{d:02}"),
            time: format!("{h:02}:{mi:02}"),
            judge: false,
            new_log,
        }
    }

    pub fn parse(&self) -> Option<zlog_core::DelphiTime> {
        parse_time(&format!("{} {}", self.date.trim(), self.time.trim()))
    }
}

pub fn entry_time(date: &str, time: &str, now: DelphiTime) -> Option<DelphiTime> {
    use chrono::{NaiveDate, NaiveTime};
    let (y, mo, d, h, mi, s) = now.to_ymd_hms();
    let date = date.trim().replace('/', "-");
    let (y, mo, d) = match date.split('-').count() {
        1 if date.is_empty() => (y, mo, d),
        2 => {
            let (m, dd) = date.split_once('-')?;
            (y, m.parse().ok()?, dd.parse().ok()?)
        }
        3 => {
            let n = NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()?;
            use chrono::Datelike;
            (n.year(), n.month(), n.day())
        }
        _ => return None,
    };
    NaiveDate::from_ymd_opt(y, mo, d)?;
    let time = time.trim();
    let (h, mi, s) = if time.is_empty() {
        (h, mi, s)
    } else {
        let t = ["%H:%M:%S", "%H:%M", "%H%M%S", "%H%M"]
            .iter()
            .find_map(|f| NaiveTime::parse_from_str(time, f).ok())?;
        use chrono::Timelike;
        (t.hour(), t.minute(), t.second())
    };
    Some(DelphiTime::from_ymd_hms(y, mo, d, h, mi, s))
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

    #[test]
    fn entry_time_fills_what_is_empty_from_now() {
        let now = DelphiTime::from_ymd_hms(2026, 6, 6, 21, 5, 9);
        assert_eq!(entry_time("", "", now), Some(now));
        let t = DelphiTime::from_ymd_hms(2026, 6, 6, 20, 30, 0);
        assert_eq!(entry_time("", "20:30", now), Some(t));
        assert_eq!(entry_time("", "2030", now), Some(t));
        let d = DelphiTime::from_ymd_hms(2026, 6, 5, 20, 30, 0);
        assert_eq!(entry_time("06/05", "2030", now), Some(d));
        assert_eq!(entry_time("2026-06-05", "20:30:00", now), Some(d));
        let only_date = DelphiTime::from_ymd_hms(2025, 1, 2, 21, 5, 9);
        assert_eq!(entry_time("2025/01/02", "", now), Some(only_date));
        for (date, time) in [("", "25:00"), ("13/01", ""), ("x", ""), ("", "abc")] {
            assert_eq!(entry_time(date, time, now), None, "{date} {time}");
        }
    }
}
