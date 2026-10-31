use crate::types::{Band, Mode, Power};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DelphiTime(pub f64);

impl DelphiTime {
    const EPOCH: chrono::NaiveDate = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();

    pub fn from_ymd_hms(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> Self {
        let days = days_from_civil(y, m, d) - days_from_civil(1899, 12, 30);
        let secs = (hh * 3600 + mm * 60 + ss) as f64;
        DelphiTime(days as f64 + secs / 86400.0)
    }

    pub fn to_ymd_hms(self) -> (i32, u32, u32, u32, u32, u32) {
        use chrono::Datelike;
        let total = (self.0 * 86400.0).round() as i64;
        let days = total.div_euclid(86400);
        let secs = total.rem_euclid(86400) as u32;
        let date = chrono::TimeDelta::try_days(days)
            .and_then(|t| Self::EPOCH.checked_add_signed(t))
            .unwrap_or(if days < 0 {
                chrono::NaiveDate::MIN
            } else {
                chrono::NaiveDate::MAX
            });
        let (y, m, d) = (date.year(), date.month(), date.day());
        (y, m, d, secs / 3600, secs / 60 % 60, secs % 60)
    }
}

pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let months = i64::from(y) * 12 + i64::from(m) - 1;
    let first = i32::try_from(months.div_euclid(12))
        .ok()
        .and_then(|y| chrono::NaiveDate::from_ymd_opt(y, months.rem_euclid(12) as u32 + 1, 1))
        .unwrap_or(if months < 0 {
            chrono::NaiveDate::MIN
        } else {
            chrono::NaiveDate::MAX
        });
    i64::from(chrono::Datelike::num_days_from_ce(&first)) - 719163 + i64::from(d) - 1
}

impl std::fmt::Display for DelphiTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (y, mo, d, h, mi, s) = self.to_ymd_hms();
        write!(f, "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QslState {
    #[default]
    None,
    PseQsl,
    NoQsl,
    Other(u8),
}

impl QslState {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => QslState::None,
            1 => QslState::PseQsl,
            2 => QslState::NoQsl,
            n => QslState::Other(n),
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            QslState::None => 0,
            QslState::PseQsl => 1,
            QslState::NoQsl => 2,
            QslState::Other(n) => n,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Qso {
    pub time: DelphiTime,
    pub callsign: String,
    pub nr_sent: String,
    pub nr_rcvd: String,
    pub rst_sent: u16,
    pub rst_rcvd: u16,
    pub serial: i32,
    pub mode: Mode,
    pub band: Band,
    pub power: Power,
    pub multi1: String,
    pub multi2: String,
    pub new_multi1: bool,
    pub new_multi2: bool,
    pub points: u8,
    pub operator: String,
    pub memo: String,
    pub cq: bool,
    pub dupe: bool,
    pub reserve: u8,
    pub tx: u8,
    pub power2: i32,
    pub reserve2: i32,
    pub reserve3: i32,
    pub freq: String,
    pub qsy_violation: bool,
    pub pc_name: String,
    pub forced: bool,
    pub qsl_state: QslState,
    pub invalid: bool,
    pub rbn_verified: bool,
    pub continent: String,
    pub entity: String,
    pub multi3: String,
    pub new_multi3: bool,
}

impl Default for Qso {
    fn default() -> Self {
        Qso {
            time: DelphiTime::default(),
            callsign: String::new(),
            nr_sent: String::new(),
            nr_rcvd: String::new(),
            rst_sent: 59,
            rst_rcvd: 59,
            serial: 0,
            mode: Mode::Ssb,
            band: Band::B7,
            power: Power::P010,
            multi1: String::new(),
            multi2: String::new(),
            new_multi1: false,
            new_multi2: false,
            points: 0,
            operator: String::new(),
            memo: String::new(),
            cq: false,
            dupe: false,
            reserve: 0,
            tx: 0,
            power2: 0,
            reserve2: 0,
            reserve3: 0,
            freq: String::new(),
            qsy_violation: false,
            pc_name: String::new(),
            forced: false,
            qsl_state: QslState::None,
            invalid: false,
            rbn_verified: false,
            continent: String::new(),
            entity: String::new(),
            multi3: String::new(),
            new_multi3: false,
        }
    }
}

impl Qso {
    pub fn core_call(&self) -> &str {
        match self.callsign.find('/') {
            Some(p) if p > 3 => &self.callsign[..p],
            _ => &self.callsign,
        }
    }

    pub fn mode_group(&self) -> &'static str {
        match self.mode {
            Mode::Ssb | Mode::Fm | Mode::Am => "PH",
            Mode::Ft4 | Mode::Ft8 => "DG",
            m => m.label(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delphi_time_round_trip() {
        let t = DelphiTime::from_ymd_hms(2026, 4, 25, 21, 0, 0);
        assert_eq!(t.to_ymd_hms(), (2026, 4, 25, 21, 0, 0));
        assert_eq!(DelphiTime(0.0).to_string(), "1899-12-30 00:00:00");
        assert_eq!(DelphiTime(2.25).to_string(), "1900-01-01 06:00:00");
        assert_eq!(DelphiTime(-1.75).to_string(), "1899-12-28 06:00:00");
        assert_eq!(
            DelphiTime(-0.4 / 86400.0).to_string(),
            "1899-12-30 00:00:00"
        );
        assert_eq!(DelphiTime(43890.5).to_string(), "2020-02-29 12:00:00");
        assert_eq!(days_from_civil(2024, 2, 30), days_from_civil(2024, 3, 1));
        assert_eq!(days_from_civil(2025, 13, 1), days_from_civil(2026, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn core_call_strips_portable() {
        let mut q = Qso {
            callsign: "JA1ZLO/1".into(),
            ..Qso::default()
        };
        assert_eq!(q.core_call(), "JA1ZLO");
        q.callsign = "7K1/ABC".into();
        assert_eq!(q.core_call(), "7K1/ABC");
    }
}
