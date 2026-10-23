//! `$`-macros of CW messages (`UzLogCW.SetStr`).
//!
//! An upper-case macro letter sends its value with the cut numbers of the settings
//! (`Abbreviate`: 0, 1, 9 sent as O, A, N by default) where zLog abbreviates it; the
//! lower-case letter sends the value as it is. The result is upper-cased and the
//! prosigns `[AR] [SK] [VA] [KN] [BK] [BT]` become the keyer's prosign characters.

/// What the macros stand for; empty fields expand to nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    /// `$C`, `$B`: callsign of the entry row.
    pub callsign: String,
    /// `$X`: the contest's sent exchange (`MyContest.SentStr`).
    pub sent: String,
    /// `$Y` (cut), `$H`: number sent in the QSO (`NrSentStr`, `NrSent`).
    pub nr_sent: String,
    /// `$R`: RST sent (`RSTSentStr`).
    pub rst_sent: String,
    /// `$F`: number received.
    pub nr_rcvd: String,
    /// `$Z`: own CQ zone.
    pub cq_zone: String,
    /// `$I`: own ITU (IARU) zone.
    pub iaru_zone: String,
    /// `$Q`: own QTH as the contest writes it (`QTHString`).
    pub qth: String,
    /// `$V`: own prefecture number (`Prov`).
    pub prov: String,
    /// `$O`: operator.
    pub operator: String,
    /// `$T`: own IOTA reference.
    pub iota: String,
    /// `$S`: serial number.
    pub serial: String,
    /// `$P`: power code H, M, L, P (`NewPowerStr`).
    pub power_code: String,
    /// `$N`: power (`PowerStr`).
    pub power: String,
    /// `$A`: operator's age.
    pub age: String,
    /// `$L`: callsign of the last QSO.
    pub last_callsign: String,
    /// `$U`: number received in the last QSO.
    pub last_number: String,
    /// `$G`: greeting GM, GA or GE (`GetGreetingsCode`).
    pub greeting: String,
    /// `$D`: time of the QSO, UTC, HHMM.
    pub time_utc: String,
    /// `$M`: own callsign.
    pub my_call: String,
}

/// Cut numbers (`CW` settings `Zero`, `One`, `Nine`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abbrev {
    pub zero: char,
    pub one: char,
    pub nine: char,
}

impl Default for Abbrev {
    fn default() -> Self {
        Abbrev {
            zero: 'O',
            one: 'A',
            nine: 'N',
        }
    }
}

impl Abbrev {
    /// `Abbreviate`.
    pub fn apply(&self, s: &str) -> String {
        s.chars()
            .map(|c| match c {
                '0' => self.zero,
                '1' => self.one,
                '9' => self.nine,
                c => c,
            })
            .collect()
    }
}

/// zLog's default hours at which the greeting changes to GM, GA and GE (section
/// `greetings`, keys `morning`, `afternoon`, `evening`).
pub const GREETING_HOURS: [u32; 3] = [0, 12, 18];

/// The greeting for the PC's local hour (`GetGreetingsCode`): GE before the first
/// change hour, then GM, GA and GE again.
pub fn greeting(local_hour: u32, hours: &[u32; 3]) -> &'static str {
    match local_hour {
        h if h < hours[0] => "GE",
        h if h < hours[1] => "GM",
        h if h < hours[2] => "GA",
        _ => "GE",
    }
}

/// Expand the macros of a CW message (`SetStr`).
///
/// `$E` stands for the callsign when one is typed, else for the last callsign.
/// Departures from zLog: `$C` is the callsign itself, not the placeholder zLog
/// replaces while sending (so a callsign corrected during sending is not followed);
/// `$E` does not track whether the callsign was edited after TAB; values are not
/// expanded again, where zLog's chain of replacements would expand a `$` in a value.
pub fn expand(template: &str, f: &Fields, abbrev: &Abbrev) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let Some(&m) = chars.peek() else {
            out.push(c);
            break;
        };
        // (value, whether the upper-case letter cuts numbers)
        let (value, cut): (&str, bool) = match m.to_ascii_uppercase() {
            'X' => (&f.sent, false),
            'Y' => (&f.nr_sent, true),
            'B' | 'C' => (&f.callsign, false),
            'R' => (&f.rst_sent, true),
            'F' => (&f.nr_rcvd, true),
            'Z' => (&f.cq_zone, true),
            'I' => (&f.iaru_zone, true),
            'Q' => (&f.qth, true),
            'V' => (&f.prov, true),
            'O' => (&f.operator, false),
            'T' => (&f.iota, true),
            'H' => (&f.nr_sent, false),
            'S' => (&f.serial, true),
            'P' => (&f.power_code, false),
            'A' => (&f.age, true),
            'N' => (&f.power, true),
            'L' => (&f.last_callsign, false),
            'U' => (&f.last_number, true),
            'G' => (&f.greeting, false),
            'D' => (&f.time_utc, true),
            'M' => (&f.my_call, false),
            // zLog replaces only the upper-case `$E`.
            'E' if m == 'E' => {
                let call = if f.callsign.is_empty() {
                    &f.last_callsign
                } else {
                    &f.callsign
                };
                (call, false)
            }
            _ => {
                out.push(c);
                continue;
            }
        };
        chars.next();
        if cut && m.is_ascii_uppercase() {
            out.push_str(&abbrev.apply(value));
        } else {
            out.push_str(value);
        }
    }
    prosigns(&out.to_uppercase())
}

/// `[AR]` and the other bracketed prosigns as keyer characters.
fn prosigns(s: &str) -> String {
    [
        ("[AR]", "a"),
        ("[SK]", "s"),
        ("[VA]", "s"),
        ("[KN]", "k"),
        ("[BK]", "b"),
        ("[BT]", "t"),
    ]
    .iter()
    .fold(s.to_string(), |s, (from, to)| s.replace(from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Fields {
        Fields {
            callsign: "JA1ZLO".into(),
            sent: "10H".into(),
            nr_sent: "10H".into(),
            rst_sent: "599".into(),
            nr_rcvd: "100109M".into(),
            serial: "019".into(),
            my_call: "jr1abc".into(),
            last_callsign: "JA1YAA".into(),
            ..Fields::default()
        }
    }

    #[test]
    fn default_messages() {
        let f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("CQ TEST $M TEST", &f, &a), "CQ TEST JR1ABC TEST");
        assert_eq!(expand("$C $R$X", &f, &a), "JA1ZLO 5NN10H");
        assert_eq!(expand("$C?", &f, &a), "JA1ZLO?");
    }

    #[test]
    fn cut_numbers_by_case() {
        let f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("$S $s $Y $y $H", &f, &a), "OAN 019 AOH 10H 10H");
        assert_eq!(expand("$F", &f, &a), "AOOAONM");
        let t = Abbrev {
            zero: 'T',
            ..Abbrev::default()
        };
        assert_eq!(expand("$S", &f, &t), "TAN");
    }

    #[test]
    fn prosigns_and_unknown() {
        let f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("tu [ar] $W $", &f, &a), "TU a $W $");
        assert_eq!(expand("[BK] [KN][BT][SK][VA]", &f, &a), "b ktss");
    }

    #[test]
    fn e_falls_back_to_last_callsign() {
        let mut f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("$E TU", &f, &a), "JA1ZLO TU");
        f.callsign.clear();
        assert_eq!(expand("$E TU", &f, &a), "JA1YAA TU");
        assert_eq!(expand("$e", &f, &a), "$E");
    }

    #[test]
    fn greetings() {
        let h = &GREETING_HOURS;
        assert_eq!(greeting(0, h), "GM");
        assert_eq!(greeting(6, h), "GM");
        assert_eq!(greeting(11, h), "GM");
        assert_eq!(greeting(12, h), "GA");
        assert_eq!(greeting(17, h), "GA");
        assert_eq!(greeting(18, h), "GE");
        assert_eq!(greeting(23, h), "GE");
    }

    #[test]
    fn greetings_with_configured_hours() {
        let h = [4, 10, 16];
        assert_eq!(greeting(3, &h), "GE");
        assert_eq!(greeting(4, &h), "GM");
        assert_eq!(greeting(10, &h), "GA");
        assert_eq!(greeting(16, &h), "GE");
        // Hours out of order or out of range behave as zLog's comparisons do.
        assert_eq!(greeting(5, &[24, 24, 24]), "GE");
        assert_eq!(greeting(5, &[0, 0, 0]), "GE");
    }
}
