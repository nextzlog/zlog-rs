//! Morse code and its timing (`TdmZLogKeyer.InitializeBGK`, `SetWPM`, `TimerProcess`).
//!
//! zLog has no Farnsworth spacing; instead it stretches the gap after each character
//! by a space factor (`SpaceFactor`, percent) and the gap after E and I further by an
//! E/I space factor (`EISpaceFactor`), and splits each pair of dot periods between
//! mark and space by a weight (`Weight`, percent; 50 is standard).

use std::time::Duration;

/// Elements of a character in zLog's keyer alphabet: `.` dot, `-` dash.
///
/// Lower-case letters are zLog's prosigns: `a` AR, `b` and `~` BK, `s` SK (also VA),
/// `k` KN, `t` BT.
pub fn pattern(c: char) -> Option<&'static str> {
    Some(match c {
        'A' => ".-",
        'B' => "-...",
        'C' => "-.-.",
        'D' => "-..",
        'E' => ".",
        'F' => "..-.",
        'G' => "--.",
        'H' => "....",
        'I' => "..",
        'J' => ".---",
        'K' => "-.-",
        'L' => ".-..",
        'M' => "--",
        'N' => "-.",
        'O' => "---",
        'P' => ".--.",
        'Q' => "--.-",
        'R' => ".-.",
        'S' => "...",
        'T' => "-",
        'U' => "..-",
        'V' => "...-",
        'W' => ".--",
        'X' => "-..-",
        'Y' => "-.--",
        'Z' => "--..",
        '0' => "-----",
        '1' => ".----",
        '2' => "..---",
        '3' => "...--",
        '4' => "....-",
        '5' => ".....",
        '6' => "-....",
        '7' => "--...",
        '8' => "---..",
        '9' => "----.",
        '-' => "-....-",
        '=' => "-...-",
        '/' => "-..-.",
        '?' => "..--..",
        'a' => ".-.-.",
        'b' | '~' => "-...-.-",
        's' => "...-.-",
        'k' => "-.--.",
        't' => "-...-",
        _ => return None,
    })
}

pub const MIN_WPM: u32 = 5;
pub const MAX_WPM: u32 = 60;

/// Keying speed and spacing (`CW` section of zLog.ini).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// Words per minute (PARIS), 5 to 60 in zLog (`MINWPM`, `MAXWPM`).
    pub wpm: u32,
    /// Share of mark in a dot and the gap after it, percent (`Weight`, default 50).
    pub weight: u32,
    /// Gap after a character, percent of three dots (`SpaceFactor`, default 100).
    pub space_factor: u32,
    /// Extra factor for the gap after E and I, percent (`EISpaceFactor`, default 100).
    pub ei_space_factor: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            wpm: 25,
            weight: 50,
            space_factor: 100,
            ei_space_factor: 100,
        }
    }
}

impl Timing {
    /// One dot period is 1.2 s / WPM.
    fn period_us(&self) -> f64 {
        1_200_000.0 / self.wpm.clamp(1, 99) as f64
    }

    fn weight(&self) -> f64 {
        self.weight.min(100) as f64 / 100.0
    }

    /// Mark of a dot (`FDotCount`).
    pub fn dot(&self) -> Duration {
        us(2.0 * self.weight() * self.period_us())
    }

    /// Mark of a dash, three dots (`FDashCount`).
    pub fn dash(&self) -> Duration {
        self.dot() * 3
    }

    /// Space between the elements of a character (`FBlank1Count`).
    pub fn element_gap(&self) -> Duration {
        us(2.0 * (1.0 - self.weight()) * self.period_us())
    }

    /// Space after a character (`FBlank3Count` times the space factor).
    pub fn char_gap(&self) -> Duration {
        us(self.blank3() * self.space_factor as f64 / 100.0)
    }

    /// Space after E and I (code `$E` of the code table).
    pub fn ei_gap(&self) -> Duration {
        us(self.blank3()
            * (self.space_factor as f64 / 100.0)
            * (self.ei_space_factor as f64 / 100.0))
    }

    fn blank3(&self) -> f64 {
        6.0 * (1.0 - self.weight()) * self.period_us()
    }
}

fn us(v: f64) -> Duration {
    Duration::from_micros(v.max(0.0).round() as u64)
}

/// One stretch of keying: key down (`true`) or up for a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub key: bool,
    pub len: Duration,
}

/// The keying of `text` in zLog's keyer alphabet, starting from `timing`.
///
/// A space is an element gap plus a character gap (seven dots after a character, as
/// the code table entry `0, 2`); `_` is one element gap; `\+n` and `\-n` (n = 1..9)
/// change the speed by n WPM from there on (`DecodeCommands`); `u` raises it by one
/// WPM (code `$41`, `IncWPM`, kept within zLog's range). `^` pauses (code `$99`,
/// `TimerProcess` clears `FSendOK`): nothing after it is keyed. `]` keys nothing
/// (its codes 4 and 5 have no case in `TimerProcess`). Other characters without
/// Morse (the control characters `( ) : * @ " #` included) are skipped.
///
/// Departures: zLog's table puts an extra element gap before the gap after I; this
/// treats E and I alike. zLog resumes after `^` with its play button (`ResumeCW`);
/// there is no such button here, so the rest of the text is dropped.
pub fn keying(text: &str, timing: &Timing) -> Vec<Step> {
    let mut t = *timing;
    let mut steps = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\\' => {
                let sign = chars.get(i).copied();
                let n = chars.get(i + 1).and_then(|d| d.to_digit(10)).unwrap_or(0);
                if n > 0 && matches!(sign, Some('+') | Some('-')) {
                    t.wpm = if sign == Some('+') {
                        (t.wpm + n).min(MAX_WPM)
                    } else {
                        t.wpm.saturating_sub(n).max(MIN_WPM)
                    };
                    i += 2;
                }
            }
            ' ' => {
                space(&mut steps, t.element_gap());
                space(&mut steps, t.char_gap());
            }
            '_' => space(&mut steps, t.element_gap()),
            'u' => t.wpm = (t.wpm + 1).clamp(MIN_WPM, MAX_WPM),
            '^' => break,
            _ => {
                let Some(p) = pattern(c) else { continue };
                for (n, e) in p.chars().enumerate() {
                    if n > 0 {
                        space(&mut steps, t.element_gap());
                    }
                    let len = if e == '.' { t.dot() } else { t.dash() };
                    steps.push(Step { key: true, len });
                }
                let gap = if c == 'E' || c == 'I' {
                    t.ei_gap()
                } else {
                    t.char_gap()
                };
                space(&mut steps, gap);
            }
        }
    }
    steps
}

/// Append key-up time, merging it with a key-up step before it.
fn space(steps: &mut Vec<Step>, len: Duration) {
    match steps.last_mut() {
        Some(s) if !s.key => s.len += len,
        _ => steps.push(Step { key: false, len }),
    }
}

/// Total time of a keying.
pub fn total(steps: &[Step]) -> Duration {
    steps.iter().map(|s| s.len).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn at(wpm: u32) -> Timing {
        Timing {
            wpm,
            ..Timing::default()
        }
    }

    #[test]
    fn paris_is_fifty_dots() {
        let t = at(20);
        assert_eq!(t.dot(), ms(60));
        assert_eq!(t.element_gap(), ms(60));
        assert_eq!(t.dash(), ms(180));
        assert_eq!(t.char_gap(), ms(180));
        assert_eq!(total(&keying("PARIS ", &t)), ms(3000));
    }

    #[test]
    fn weight_shifts_mark_and_space() {
        let t = Timing {
            weight: 60,
            ..at(20)
        };
        assert_eq!(t.dot(), ms(72));
        assert_eq!(t.element_gap(), ms(48));
    }

    #[test]
    fn space_factors() {
        let t = Timing {
            space_factor: 150,
            ei_space_factor: 50,
            ..at(20)
        };
        assert_eq!(t.char_gap(), ms(270));
        assert_eq!(t.ei_gap(), ms(135));
        let e = keying("E", &t);
        let want = vec![
            Step {
                key: true,
                len: ms(60),
            },
            Step {
                key: false,
                len: ms(135),
            },
        ];
        assert_eq!(e, want);
    }

    #[test]
    fn speed_change_and_skips() {
        let steps = keying("(\\+4E)", &at(20));
        // 24 WPM: 50 ms dot.
        assert_eq!(
            steps[0],
            Step {
                key: true,
                len: ms(50)
            }
        );
        assert_eq!(steps.len(), 2);
        assert_eq!(keying("@:*", &at(20)), vec![]);
        // Speed is kept within zLog's range.
        assert_eq!(keying("\\-9E", &at(10))[0].len, ms(240));
    }

    #[test]
    fn pause_stops_keying() {
        let t = at(20);
        assert_eq!(keying("E^E", &t), keying("E", &t));
        assert_eq!(keying("^PARIS", &t), vec![]);
        assert_eq!(keying("E^", &t), keying("E", &t));
        assert_eq!(keying("^", &t), vec![]);
    }

    #[test]
    fn u_raises_speed_by_one() {
        // 24 WPM after four `u`: 50 ms dot; `u` itself keys nothing.
        assert_eq!(keying("uuuuE", &at(20))[0].len, ms(50));
        assert_eq!(keying("u", &at(20)), vec![]);
        // Kept within zLog's range.
        assert_eq!(keying("uE", &at(MAX_WPM))[0].len, ms(20));
    }

    #[test]
    fn bracket_keys_nothing() {
        let t = at(20);
        assert_eq!(keying("]", &t), vec![]);
        assert_eq!(keying("E]E", &t), keying("EE", &t));
    }

    #[test]
    fn empty_text_keys_nothing() {
        assert_eq!(keying("", &at(20)), vec![]);
        // A lone backslash or an incomplete speed change is skipped.
        assert_eq!(keying("\\+", &at(20)), vec![]);
    }

    #[test]
    fn prosigns_and_spaces_merge() {
        assert_eq!(pattern('a'), Some(".-.-."));
        let steps = keying("T  T", &at(20));
        // T, then the char gap and two spaces merged into one key-up step.
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[1].len, ms(180 + 2 * 240));
    }
}
