//! A single contact (`TQSO`).

use crate::types::{Band, Mode, Power};

/// Delphi `TDateTime`: days since 1899-12-30, fraction is the time of day.
///
/// zLog stores local time (JST by default); the log header tells the offset.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DelphiTime(pub f64);

impl DelphiTime {
    /// Build from a civil date and time.
    pub fn from_ymd_hms(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> Self {
        let days = days_from_civil(y, m, d) - days_from_civil(1899, 12, 30);
        let secs = (hh * 3600 + mm * 60 + ss) as f64;
        DelphiTime(days as f64 + secs / 86400.0)
    }

    /// Split into (year, month, day, hour, minute, second), rounded to the second.
    pub fn to_ymd_hms(self) -> (i32, u32, u32, u32, u32, u32) {
        let total = (self.0 * 86400.0).round() as i64;
        let days = total.div_euclid(86400);
        let secs = total.rem_euclid(86400) as u32;
        let (y, m, d) = civil_from_days(days + days_from_civil(1899, 12, 30));
        (y, m, d, secs / 3600, secs / 60 % 60, secs % 60)
    }
}

impl std::fmt::Display for DelphiTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (y, mo, d, h, mi, s) = self.to_ymd_hms();
        write!(f, "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
    }
}

// Howard Hinnant's algorithms for the proleptic Gregorian calendar.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y as i32, m, d)
}

/// QSL request state (`TQslState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QslState {
    #[default]
    None,
    PseQsl,
    NoQsl,
    /// A value this version does not know; kept as is.
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

/// One contact, with every field of the ZLOX record.
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
    /// QSO ID: `TTSSSSRRCC` (TX, serial counter, random, edit counter).
    pub reserve3: i32,
    // Fields below exist only in ZLOX.
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
    /// Callsign without a portable suffix (`CoreCall`): `JA1ZLO/1` becomes `JA1ZLO`.
    ///
    /// As in zLog, a slash in the first four characters is kept (`7K1/...`).
    pub fn core_call(&self) -> &str {
        match self.callsign.find('/') {
            Some(p) if p > 3 => &self.callsign[..p],
            _ => &self.callsign,
        }
    }

    /// Mode group used for dupe checking with "all phone" on (`Mode2Str`).
    ///
    /// As zLog's `ModeString2`: SSB, FM and AM are phone, FT4 and FT8 are one digital
    /// group, DV is a group of its own.
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
