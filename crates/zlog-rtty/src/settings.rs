//! The settings of the RTTY console (section `RTTY` of zLog.ini, `PTTEnabledRTTY`,
//! `PTTBeforeRTTY`, `PTTAfterRTTY` of `Hardware`, and `URttyOptions`).

use std::time::Duration;

use zlog_ui::Section;

use crate::modem::Params;
use crate::sender::Timing;

/// The RTTY messages of F1 to F12 (`def_rtty_messages`).
pub const DEFAULT_MESSAGES: [&str; 12] = [
    "TEST $M $M CQ",
    "$C $R $X $X $C",
    "$C TU $M CQ",
    "",
    "NR? NR?",
    "$C? $C?",
    "$M $M",
    "$C $R $X $X $M TU",
    "",
    "",
    "",
    "",
];

/// The speeds offered; 45.45 baud is the one of contests.
pub const BAUDS: [f64; 4] = [45.45, 50.0, 75.0, 100.0];

/// The colours of the console: those of the theme, then zLog's (`DefaultColor`)
/// normal, dark, green and amber.
pub const SCHEMES: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Input and output devices by name, empty for the default ones.
    pub input: String,
    pub output: String,
    /// Mark tone and shift in hertz (`mark_freq`, `space_freq` less mark).
    pub mark: u32,
    pub shift: u32,
    pub baud: f64,
    /// Mark and space swapped (`fsk_reverse`).
    pub reverse: bool,
    /// Unshift on space (`use_txuos`).
    pub uos: bool,
    /// Characters less clear than this are taken for noise (0 to 1).
    pub squelch: f64,
    /// PTT through the keyer and its lead and tail in milliseconds.
    pub ptt: bool,
    pub ptt_before_ms: u32,
    pub ptt_after_ms: u32,
    /// F1 to F12 (`F1` to `F12` of `RTTY`).
    pub messages: [String; 12],
    /// Index of the colours, 0 for those of the theme, else `DefaultColor` + 1.
    pub scheme: usize,
    /// Words shown in bold colour, split by spaces (the keywords of `ColorCoding`).
    pub highlight: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            input: String::new(),
            output: String::new(),
            mark: 2125,
            shift: 170,
            baud: 45.45,
            reverse: false,
            uos: true,
            squelch: crate::fsk::SQUELCH,
            ptt: true,
            ptt_before_ms: 700,
            ptt_after_ms: 700,
            messages: DEFAULT_MESSAGES.map(String::from),
            scheme: 0,
            highlight: String::new(),
        }
    }
}

fn get<T: std::str::FromStr>(s: &Section, key: &str, default: T) -> T {
    s.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Settings {
    pub fn load(s: &Section) -> Self {
        let d = Settings::default();
        let text = |key: &str, default: &str| s.get(key).cloned().unwrap_or(default.into());
        Settings {
            input: text("input", ""),
            output: text("output", ""),
            mark: get(s, "mark", d.mark).clamp(300, 3000),
            shift: get(s, "shift", d.shift).clamp(50, 1000),
            baud: get(s, "baud", d.baud).clamp(10.0, 300.0),
            reverse: get(s, "reverse", d.reverse),
            uos: get(s, "uos", d.uos),
            squelch: get(s, "squelch", d.squelch).clamp(0.0, 1.0),
            ptt: get(s, "ptt", d.ptt),
            ptt_before_ms: get(s, "ptt_before", d.ptt_before_ms).min(2000),
            ptt_after_ms: get(s, "ptt_after", d.ptt_after_ms).min(2000),
            messages: std::array::from_fn(|n| text(&format!("F{}", n + 1), DEFAULT_MESSAGES[n])),
            scheme: match s.get("scheme") {
                Some(_) => get(s, "scheme", d.scheme),
                // Earlier versions kept zLog's index, 0 drawn in the theme's colours.
                None => match get(s, "theme", 0usize) {
                    0 => 0,
                    n => n + 1,
                },
            }
            .min(SCHEMES - 1),
            highlight: text("highlight", ""),
        }
    }

    pub fn save(&self, s: &mut Section) {
        let mut put = |k: &str, v: String| {
            s.insert(k.into(), v);
        };
        put("input", self.input.clone());
        put("output", self.output.clone());
        put("mark", self.mark.to_string());
        put("shift", self.shift.to_string());
        put("baud", self.baud.to_string());
        put("reverse", self.reverse.to_string());
        put("uos", self.uos.to_string());
        put("squelch", self.squelch.to_string());
        put("ptt", self.ptt.to_string());
        put("ptt_before", self.ptt_before_ms.to_string());
        put("ptt_after", self.ptt_after_ms.to_string());
        for n in 0..12 {
            put(&format!("F{}", n + 1), self.messages[n].clone());
        }
        put("scheme", self.scheme.to_string());
        put("highlight", self.highlight.clone());
    }

    /// The signal: space above mark by the shift, swapped when reversed.
    pub fn params(&self) -> Params {
        let (mark, space) = (self.mark as f64, (self.mark + self.shift) as f64);
        let (mark, space) = if self.reverse {
            (space, mark)
        } else {
            (mark, space)
        };
        Params {
            mark,
            space,
            baud: self.baud,
            uos: self.uos,
        }
    }

    pub fn timing(&self) -> Timing {
        Timing {
            ptt: self.ptt,
            before: Duration::from_millis(self.ptt_before_ms as u64),
            after: Duration::from_millis(self.ptt_after_ms as u64),
        }
    }

    /// Whether `word` is one of the words to highlight.
    pub fn highlighted(&self, word: &str) -> bool {
        self.highlight
            .split_whitespace()
            .any(|w| w.eq_ignore_ascii_case(word))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let mut s = Settings {
            mark: 1275,
            baud: 50.0,
            reverse: true,
            ptt_before_ms: 100,
            output: "USB Audio".into(),
            scheme: 2,
            highlight: "JA1ZLO 599".into(),
            ..Settings::default()
        };
        s.messages[3] = "QRZ?".into();
        let mut sec = Section::new();
        s.save(&mut sec);
        assert_eq!(Settings::load(&sec), s);
        assert_eq!(Settings::load(&Section::new()), Settings::default());
    }

    #[test]
    fn the_signal_of_the_settings() {
        let mut s = Settings::default();
        assert_eq!(s.params(), Params::default());
        s.reverse = true;
        let p = s.params();
        assert_eq!((p.mark, p.space), (2295.0, 2125.0));
        assert!(s.highlighted("ja1zlo") == s.highlight.contains("JA1ZLO"));
    }

    #[test]
    fn values_out_of_range_are_clamped() {
        let mut sec = Section::new();
        sec.insert("mark".into(), "10".into());
        sec.insert("scheme".into(), "9".into());
        sec.insert("squelch".into(), "-1".into());
        let s = Settings::load(&sec);
        assert_eq!((s.mark, s.scheme, s.squelch), (300, SCHEMES - 1, 0.0));
    }

    #[test]
    fn the_theme_is_followed_by_default_and_old_indices_kept() {
        assert_eq!(Settings::default().scheme, 0);
        let old = |n: &str| {
            let mut sec = Section::new();
            sec.insert("theme".into(), n.into());
            Settings::load(&sec).scheme
        };
        assert_eq!((old("0"), old("1"), old("3")), (0, 2, 4));
    }
}
