use zlog_core::Mode;
use zlog_ui::Spot;

pub const SOURCE: &str = "DXC";

pub fn parse_line(line: &str, now: i64) -> Option<Spot> {
    if line.len() < 5 {
        return None;
    }
    let text = line.trim();
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
    let rest = text.strip_prefix("DX de")?;
    let (spotter, rest) = rest.split_once(':')?;
    let spotter = spotter.trim();
    let mut words = rest.split_whitespace();
    let hz = khz_to_hz(words.next()?)?;
    let callsign = words.next()?.to_string();
    let rest: Vec<&str> = words.collect();
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
        source: SOURCE.into(),
        ..Spot::default()
    })
}

fn parse_show_dx(text: &str, now: i64) -> Option<Spot> {
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
        source: SOURCE.into(),
        ..Spot::default()
    })
}

fn is_time(s: &str) -> bool {
    s.len() == 5 && s.ends_with('Z') && s[..4].bytes().all(|b| b.is_ascii_digit())
}

const MAX_KHZ: f64 = 300_000_000.0;

fn khz_to_hz(s: &str) -> Option<u64> {
    let khz: f64 = s.parse().ok()?;
    (khz > 0.0 && khz <= MAX_KHZ).then(|| (khz * 1000.0).round() as u64)
}

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

pub fn spot_command(hz: u64, callsign: &str, comment: &str) -> String {
    let khz = format!("{:.1}", hz as f64 / 1000.0);
    format!("DX {khz} {callsign} {}", comment.trim())
        .trim_end()
        .to_string()
}

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
    fn rejects_non_finite_and_huge_frequencies() {
        for f in ["inf", "NaN", "1e9", "-7000", "0"] {
            assert_eq!(khz_to_hz(f), None, "{f}");
        }
        assert_eq!(khz_to_hz("7025.5"), Some(7_025_500));
        assert_eq!(khz_to_hz("10368100"), Some(10_368_100_000));
    }

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
        assert_eq!((s.source.as_str(), s.number.as_str()), ("DXC", ""));
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

    #[test]
    fn cluster_line_with_prompt_before_is_not_a_spot() {
        let line = "\x07JA1ZLO de JA1RL >DX de JA1RL:     7012.5  ja1yaa       cq test                        1208Z";
        assert!(parse_line(line, 0).is_none());
        assert!(parse_line("xDX de JA1RL: 7012.5 JA1YAA 1208Z", 0).is_none());
    }

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
