//! Spot lines of a packet cluster or of the Reverse Beacon Network (`TSpot.Analyze`).

use zlog_core::Mode;
use zlog_ui::Spot;

/// Parse one line received from the cluster into a spot, or `None` if it is not one.
///
/// Two forms are understood, as in `TSpot.Analyze` of `USpotClass.pas`:
///
/// - the announcement `DX de JA1ZLO:  7012.5  JA1YAA  CQ test  1208Z`, also as sent by
///   RBN skimmers (`DX de W3LPL-#:  14010.6  SM5DYC  CW 14 dB 22 WPM CQ  1208Z`);
/// - the reply to `SH/DX`: `7012.5  JA1YAA  20-Oct-2026 1208Z  CQ test  <JA1ZLO>`.
///
/// The callsign is kept as received, as zLog does.
///
/// `now` is the time of reception in seconds since the Unix epoch.
///
/// zLog estimates the mode from its band plan (`BandPlan.GetEstimatedMode`); here the
/// mode is taken from the comment when it names one (RBN always does) and is left
/// `None` otherwise, for the receiver to decide.
pub fn parse_line(line: &str, now: i64) -> Option<Spot> {
    if line.len() < 5 {
        return None;
    }
    let text = line.trim();
    // Only a line that starts with "DX de" is an announcement. zLog's `Delete(temp,
    // 1, i)` also removes the 'D' when text (a prompt, a beep) comes before it, so
    // such a line is never a spot.
    if text.starts_with("DX de") {
        return parse_announcement(text, now);
    }
    if text.contains("DX de") {
        return None;
    }
    if text.starts_with("To ALL") {
        return None;
    }
    parse_show_dx(text, now)
}

fn parse_announcement(text: &str, now: i64) -> Option<Spot> {
    // zLog requires 75 characters (the fixed layout of the line); we only need the fields.
    let rest = text.strip_prefix("DX de")?;
    let (spotter, rest) = rest.split_once(':')?;
    let spotter = spotter.trim();
    let mut words = rest.split_whitespace();
    let hz = khz_to_hz(words.next()?)?;
    let callsign = words.next()?.to_string();
    let rest: Vec<&str> = words.collect();
    // The time (`1208Z`) ends the line; everything between it and the call is comment.
    let (comment, _time) = match rest.split_last() {
        Some((last, init)) if is_time(last) => (init.join(" "), Some(*last)),
        _ => (rest.join(" "), None),
    };
    Some(Spot {
        mode: mode_in(&comment),
        callsign,
        hz,
        spotter: spotter.into(),
        comment,
        time: now,
    })
}

fn parse_show_dx(text: &str, now: i64) -> Option<Spot> {
    // The spotter closes the line in angle brackets.
    let text = text.strip_suffix('>')?;
    let p = text.rfind(" <")?;
    let spotter = &text[p + 2..];
    let mut words = text[..p].split_whitespace();
    let hz = khz_to_hz(words.next()?)?;
    let callsign = words.next()?.to_string();
    let _date = words.next()?;
    if !is_time(words.next()?) {
        return None;
    }
    let comment = words.collect::<Vec<_>>().join(" ");
    Some(Spot {
        mode: mode_in(&comment),
        callsign,
        hz,
        spotter: spotter.into(),
        comment,
        time: now,
    })
}

/// `1208Z`: four digits and a Z.
fn is_time(s: &str) -> bool {
    s.len() == 5 && s.ends_with('Z') && s[..4].bytes().all(|b| b.is_ascii_digit())
}

fn khz_to_hz(s: &str) -> Option<u64> {
    let khz: f64 = s.parse().ok()?;
    (khz > 0.0).then(|| (khz * 1000.0).round() as u64)
}

/// The first word of the comment that names a mode (`CW`, `FT8`, `USB`).
pub fn mode_in(comment: &str) -> Option<Mode> {
    comment.split_whitespace().find_map(|w| match w {
        "CW" => Some(Mode::Cw),
        "RTTY" => Some(Mode::Rtty),
        "FT8" => Some(Mode::Ft8),
        "FT4" => Some(Mode::Ft4),
        "SSB" | "USB" | "LSB" => Some(Mode::Ssb),
        "FM" => Some(Mode::Fm),
        "AM" => Some(Mode::Am),
        _ => None,
    })
}

/// The command that announces a spot (`TSpotForm.OKBtnClick`): `DX 7012.5 JA1ZLO comment`.
pub fn spot_command(hz: u64, callsign: &str, comment: &str) -> String {
    let khz = format!("{:.1}", hz as f64 / 1000.0);
    format!("DX {khz} {callsign} {}", comment.trim())
        .trim_end()
        .to_string()
}

/// Whether received text asks for the login callsign (`TCommForm.CommProcess`).
pub fn is_login_prompt(text: &str) -> bool {
    [
        "login:",
        "Please enter your call:",
        "Please enter your callsign:",
    ]
    .iter()
    .any(|p| text.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rbn_cw_line() {
        let s = parse_line(
            "DX de W3LPL-#:   14010.6  SM5DYC       CW 14 dB 22 WPM CQ           Q 1208Z",
            100,
        )
        .unwrap();
        assert_eq!(s.spotter, "W3LPL-#");
        assert_eq!(s.hz, 14_010_600);
        assert_eq!(s.callsign, "SM5DYC");
        assert_eq!(s.mode, Some(Mode::Cw));
        assert_eq!(s.comment, "CW 14 dB 22 WPM CQ Q");
        assert_eq!(s.time, 100);
    }

    #[test]
    fn rbn_ft8_line() {
        let s = parse_line(
            "DX de JH1XYZ-#:   7074.0  JA1ZLO       FT8 -12 dB  CQ      0102Z",
            0,
        )
        .unwrap();
        assert_eq!(s.mode, Some(Mode::Ft8));
    }

    /// N4: text before "DX de" makes the line no spot (`TSpot.Analyze`).
    #[test]
    fn cluster_line_with_prompt_before_is_not_a_spot() {
        let line = "\x07JA1ZLO de JA1RL >DX de JA1RL:     7012.5  ja1yaa       cq test                        1208Z";
        assert!(parse_line(line, 0).is_none());
        assert!(parse_line("xDX de JA1RL: 7012.5 JA1YAA 1208Z", 0).is_none());
    }

    /// N4: the callsign keeps its case as received.
    #[test]
    fn callsign_case_is_kept() {
        let s = parse_line("DX de JA1RL:     7012.5  ja1yaa       cq test   1208Z", 0).unwrap();
        assert_eq!(s.spotter, "JA1RL");
        assert_eq!(s.callsign, "ja1yaa");
        assert_eq!(s.hz, 7_012_500);
        assert_eq!(s.comment, "cq test");
        assert_eq!(s.mode, None);
        let s = parse_line("  7012.5  ja1yaa  20-Oct-2026 1208Z  CQ  <JA1ZLO>", 0).unwrap();
        assert_eq!(s.callsign, "ja1yaa");
    }

    /// Leading blanks before "DX de" are not text.
    #[test]
    fn leading_blanks_are_trimmed() {
        let s = parse_line("   DX de JA1RL: 7012.5 JA1YAA 1208Z", 0).unwrap();
        assert_eq!(s.callsign, "JA1YAA");
        assert_eq!(s.comment, "");
    }

    #[test]
    fn malformed_announcements() {
        assert!(parse_line("", 0).is_none());
        assert!(parse_line("DX de", 0).is_none());
        assert!(parse_line("DX de JA1RL 7012.5 JA1YAA 1208Z", 0).is_none());
        assert!(parse_line("DX de JA1RL: 7012.5", 0).is_none());
        assert!(parse_line("DX de JA1RL: -7012.5 JA1YAA", 0).is_none());
    }

    #[test]
    fn show_dx_reply() {
        let s = parse_line(
            "  7012.5  JA1YAA      20-Oct-2026 1208Z  CQ test               <JA1ZLO>",
            0,
        )
        .unwrap();
        assert_eq!(s.spotter, "JA1ZLO");
        assert_eq!(s.callsign, "JA1YAA");
        assert_eq!(s.comment, "CQ test");
    }

    #[test]
    fn not_spots() {
        assert!(parse_line("To ALL de JA1ZLO: hello", 0).is_none());
        assert!(parse_line("Welcome to the cluster", 0).is_none());
        assert!(parse_line("DX de JA1ZLO: abc JA1YAA 1208Z", 0).is_none());
        assert!(parse_line("JA1ZLO de JA1RL 20-Oct-2026 1208Z dxspider >", 0).is_none());
    }

    #[test]
    fn spot_and_login() {
        assert_eq!(
            spot_command(7_012_500, "JA1YAA", " up 1 "),
            "DX 7012.5 JA1YAA up 1"
        );
        assert_eq!(spot_command(14_000_000, "JA1YAA", ""), "DX 14000.0 JA1YAA");
        assert!(is_login_prompt("Please enter your call: "));
        assert!(is_login_prompt("login: "));
        assert!(!is_login_prompt("Hello JA1ZLO"));
    }
}
