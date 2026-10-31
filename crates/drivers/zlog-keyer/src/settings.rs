use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zlog_ui::{Section, View};

use crate::macros::{self, Abbrev, Fields};
use crate::morse::{self, Timing};

pub const DEFAULT_MESSAGES: [&str; 12] = [
    "CQ TEST $M TEST",
    "$C $R$X",
    "TU $M TEST",
    "",
    "NR?",
    "$C?",
    "$M",
    "$R$X",
    "TU",
    "",
    "",
    "",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub timing: Timing,
    pub banks: [[String; 12]; 2],
    pub cq_repeat: f64,
    pub cq_max: u32,
    pub greeting_hours: [u32; 3],
    pub abbrev: Abbrev,
    pub not_send_leading_zeros: bool,
    pub pad: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            timing: Timing::default(),
            banks: [
                DEFAULT_MESSAGES.map(String::from),
                std::array::from_fn(|_| String::new()),
            ],
            cq_repeat: 2.0,
            cq_max: 15,
            greeting_hours: macros::GREETING_HOURS,
            abbrev: Abbrev::default(),
            not_send_leading_zeros: false,
            pad: String::new(),
        }
    }
}

pub(crate) fn get<T: std::str::FromStr>(s: &Section, key: &str, default: T) -> T {
    s.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

pub(crate) fn get_char(s: &Section, key: &str, default: char) -> char {
    s.get(key).and_then(|v| v.chars().next()).unwrap_or(default)
}

const GREETING_KEYS: [&str; 3] = ["greeting_morning", "greeting_afternoon", "greeting_evening"];

pub fn ptt_delay(ms: u32) -> u32 {
    ms.clamp(1, 255)
}

fn message_key(bank: usize, n: usize) -> String {
    format!("F{}{}", n + 1, if bank == 0 { "" } else { "B" })
}

impl Settings {
    pub fn load(s: &Section) -> Self {
        let d = Settings::default();
        let text = |key: &str, default: &str| s.get(key).cloned().unwrap_or(default.into());
        let banks = std::array::from_fn(|b| {
            std::array::from_fn(|n| text(&message_key(b, n), &d.banks[b][n]))
        });
        Settings {
            timing: Timing {
                wpm: get(s, "speed", d.timing.wpm).clamp(morse::MIN_WPM, morse::MAX_WPM),
                weight: get(s, "weight", d.timing.weight).min(100),
                space_factor: get(s, "space_factor", d.timing.space_factor).max(1),
                ei_space_factor: get(s, "ei_space_factor", d.timing.ei_space_factor).max(1),
            },
            banks,
            cq_repeat: get(s, "cq_repeat", d.cq_repeat).clamp(0.0, 60.0),
            cq_max: get(s, "cq_max", d.cq_max),
            greeting_hours: std::array::from_fn(|i| get(s, GREETING_KEYS[i], d.greeting_hours[i])),
            abbrev: Abbrev {
                zero: get_char(s, "zero", 'O'),
                one: get_char(s, "one", 'A'),
                nine: get_char(s, "nine", 'N'),
            },
            not_send_leading_zeros: get(s, "not_send_leading_zeros", false),
            pad: text("pad", ""),
        }
    }

    pub fn save(&self, s: &mut Section) {
        let mut put = |k: &str, v: String| {
            s.insert(k.into(), v);
        };
        put("speed", self.timing.wpm.to_string());
        put("weight", self.timing.weight.to_string());
        put("space_factor", self.timing.space_factor.to_string());
        put("ei_space_factor", self.timing.ei_space_factor.to_string());
        for (b, bank) in self.banks.iter().enumerate() {
            for (n, m) in bank.iter().enumerate() {
                put(&message_key(b, n), m.clone());
            }
        }
        put("cq_repeat", self.cq_repeat.to_string());
        put("cq_max", self.cq_max.to_string());
        for (k, h) in GREETING_KEYS.iter().zip(self.greeting_hours) {
            put(k, h.to_string());
        }
        put("zero", self.abbrev.zero.to_string());
        put("one", self.abbrev.one.to_string());
        put("nine", self.abbrev.nine.to_string());
        put(
            "not_send_leading_zeros",
            self.not_send_leading_zeros.to_string(),
        );
        put("pad", self.pad.clone());
    }

    pub fn fields(&self, view: &View<'_>, unix_secs: u64, local_hour: u32) -> Fields {
        let zeros = !self.not_send_leading_zeros;
        macros::fields(view, unix_secs, local_hour, zeros, &self.greeting_hours)
    }

    pub fn stamp(&self, f: &mut Fields, unix_secs: u64, local_hour: u32) {
        macros::stamp(f, unix_secs, local_hour, &self.greeting_hours)
    }

    pub fn cq_interval(&self) -> Duration {
        Duration::from_secs(self.cq_repeat.clamp(0.0, 60.0).trunc() as u64)
    }
}

pub(crate) fn now() -> (u64, u32) {
    use chrono::Timelike;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (secs, chrono::Local::now().hour())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand;
    use crate::tests::{with_contest, with_view, Serials};

    #[test]
    fn settings_round_trip() {
        let mut s = Settings::default();
        s.banks[1][3] = "QRZ?".into();
        s.abbrev.nine = '9';
        s.timing.wpm = 32;
        s.pad = "TU\nQRL?".into();
        let mut sec = Section::new();
        s.save(&mut sec);
        assert_eq!(sec["F1"], "CQ TEST $M TEST");
        assert_eq!(sec["F4B"], "QRZ?");
        assert_eq!(Settings::load(&sec), s);
        assert_eq!(Settings::load(&Section::new()), Settings::default());
    }

    #[test]
    fn fields_from_view() {
        let s = Settings::default();
        let f = with_view(|v| s.fields(v, 3 * 3600 + 4 * 60, 3));
        assert_eq!(f.sent_template, "$V$P");
        assert_eq!(f.serial, "002");
        assert_eq!(f.time_utc, "0304");
        assert_eq!(f.last_callsign, "JA1YAA");
        assert_eq!(f.last_number, "11H");
        assert_eq!(f.power_code, "H");
        assert_eq!(f.nr_rcvd, "25M");
        assert_eq!(f.rst_sent, "599");
        assert_eq!(f.greeting, "GM");
        assert_eq!(expand("$C $R$X $M", &f, &s.abbrev), "JA1ZLO 5NNAOH JR1ABC");
    }

    #[test]
    fn serial_is_the_contest_serial_on_the_band() {
        let mut s = Settings::default();
        let f = with_contest(&Serials, zlog_core::Mode::Cw, |v| s.fields(v, 0, 0));
        assert_eq!(f.serial, "041");
        s.not_send_leading_zeros = true;
        let f = with_contest(&Serials, zlog_core::Mode::Cw, |v| s.fields(v, 0, 0));
        assert_eq!(f.serial, "41");
        let f = with_view(|v| s.fields(v, 0, 0));
        assert_eq!(f.serial, "2");
    }

    #[test]
    fn ptt_delay_range() {
        assert_eq!(ptt_delay(0), 1);
        assert_eq!(ptt_delay(1), 1);
        assert_eq!(ptt_delay(25), 25);
        assert_eq!(ptt_delay(255), 255);
        assert_eq!(ptt_delay(256), 255);
        assert_eq!(ptt_delay(u32::MAX), 255);
    }

    #[test]
    fn cq_interval_in_whole_seconds() {
        let at = |secs: f64| {
            Settings {
                cq_repeat: secs,
                ..Settings::default()
            }
            .cq_interval()
        };
        assert_eq!(at(2.5), Duration::from_secs(2));
        assert_eq!(at(2.0), Duration::from_secs(2));
        assert_eq!(at(0.9), Duration::ZERO);
        assert_eq!(at(0.0), Duration::ZERO);
        assert_eq!(at(-3.0), Duration::ZERO);
        assert_eq!(at(59.99), Duration::from_secs(59));
        assert_eq!(at(1000.0), Duration::from_secs(60));
        assert_eq!(at(f64::NAN), Duration::ZERO);
    }

    #[test]
    fn greeting_by_pc_local_hour() {
        let s = Settings::default();
        let f = with_view(|v| s.fields(v, 3 * 3600 + 4 * 60, 20));
        assert_eq!(f.greeting, "GE");
        assert_eq!(f.time_utc, "0304");
        let s = Settings {
            greeting_hours: [5, 10, 15],
            ..Settings::default()
        };
        assert_eq!(with_view(|v| s.fields(v, 0, 4)).greeting, "GE");
        assert_eq!(with_view(|v| s.fields(v, 0, 9)).greeting, "GM");
        assert_eq!(with_view(|v| s.fields(v, 0, 10)).greeting, "GA");
        assert_eq!(with_view(|v| s.fields(v, 0, 15)).greeting, "GE");
    }

    #[test]
    fn greeting_hours_settings() {
        let s = Settings {
            greeting_hours: [6, 11, 17],
            ..Settings::default()
        };
        let mut sec = Section::new();
        s.save(&mut sec);
        assert_eq!(sec["greeting_afternoon"], "11");
        assert_eq!(Settings::load(&sec), s);
        let mut bad = Section::new();
        bad.insert("greeting_morning".into(), "abc".into());
        bad.insert("greeting_evening".into(), "".into());
        bad.insert("greeting_afternoon".into(), "13".into());
        assert_eq!(Settings::load(&bad).greeting_hours, [0, 13, 18]);
    }

    #[test]
    fn stamp_sets_time_and_greeting() {
        let s = Settings::default();
        let mut f = Fields::default();
        s.stamp(&mut f, 86_400 + 23 * 3600 + 59 * 60 + 59, 13);
        assert_eq!(f.time_utc, "2359");
        assert_eq!(f.greeting, "GA");
        s.stamp(&mut f, 0, 0);
        assert_eq!(f.time_utc, "0000");
        assert_eq!(f.greeting, "GM");
    }
}
