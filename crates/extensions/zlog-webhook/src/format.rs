use zlog_core::{Band, Qso};

use crate::texts;

#[derive(Debug, Clone, PartialEq)]
pub enum Change<'a> {
    Logged(&'a Qso),
    Corrected { before: &'a Qso, after: &'a Qso },
    Deleted(&'a Qso),
}

pub fn qso_text(q: &Qso) -> String {
    let rst = (q.rst_rcvd != 0).then(|| q.rst_rcvd.to_string());
    [
        Some(q.callsign.trim().to_string()),
        Some(band_text(q.band)),
        Some(q.mode.to_string()),
        rst,
        Some(q.nr_rcvd.trim().to_string()),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

pub fn band_text(band: Band) -> String {
    let label = band.label();
    if label.ends_with('G') {
        format!("{label}Hz")
    } else if label.starts_with(|c: char| c.is_ascii_digit()) {
        format!("{label}MHz")
    } else {
        label.to_string()
    }
}

pub fn line(change: &Change<'_>, station: &str, contest: &str, language: &str) -> String {
    let t = texts();
    let text = match change {
        Change::Logged(q) => qso_text(q),
        Change::Corrected { before, after } => t.format(
            language,
            "post.corrected",
            &[&qso_text(before), &qso_text(after)],
        ),
        Change::Deleted(q) => t.format(language, "post.deleted", &[&qso_text(q)]),
    };
    let from: Vec<&str> = [station.trim(), contest.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    if from.is_empty() {
        text
    } else {
        format!("{text} ({})", from.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Mode;

    fn qso() -> Qso {
        Qso {
            callsign: "JA1ZLO".into(),
            band: Band::B7,
            mode: Mode::Cw,
            rst_rcvd: 599,
            nr_rcvd: "13M".into(),
            ..Qso::default()
        }
    }

    #[test]
    fn a_qso_is_one_short_line() {
        assert_eq!(qso_text(&qso()), "JA1ZLO 7MHz CW 599 13M");
        let q = Qso {
            band: Band::B10G,
            rst_rcvd: 0,
            nr_rcvd: String::new(),
            ..qso()
        };
        assert_eq!(qso_text(&q), "JA1ZLO 10.1GHz CW");
        assert_eq!(band_text(Band::B1_9), "1.9MHz");
        assert_eq!(band_text(Band::Unknown), "Unknown");
    }

    #[test]
    fn the_station_and_contest_follow_when_known() {
        let q = qso();
        let logged = Change::Logged(&q);
        assert_eq!(
            line(&logged, "JG1VPP", "ALLJA1", "en"),
            "JA1ZLO 7MHz CW 599 13M (JG1VPP, ALLJA1)"
        );
        assert_eq!(line(&logged, "", " ", "en"), "JA1ZLO 7MHz CW 599 13M");
        assert_eq!(
            line(&logged, "JG1VPP", "", "en"),
            "JA1ZLO 7MHz CW 599 13M (JG1VPP)"
        );
    }

    #[test]
    fn corrections_and_deletions_say_so() {
        let before = qso();
        let after = Qso {
            nr_rcvd: "14M".into(),
            ..qso()
        };
        let c = Change::Corrected {
            before: &before,
            after: &after,
        };
        assert_eq!(
            line(&c, "", "", "ja"),
            "訂正: JA1ZLO 7MHz CW 599 13M → JA1ZLO 7MHz CW 599 14M"
        );
        assert_eq!(
            line(&Change::Deleted(&before), "", "", "en"),
            "Deleted: JA1ZLO 7MHz CW 599 13M"
        );
    }
}
