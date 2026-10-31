use zlog_core::{Band, DelphiTime, Qso};
use zlog_ui::Section;

use crate::driver::Proto;

pub const SCOPE: &str = "scope";

pub fn delphi(t: chrono::NaiveDateTime) -> DelphiTime {
    use chrono::{Datelike, Timelike};
    DelphiTime::from_ymd_hms(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
    )
}

pub fn qso(q: &ftxl::Qso, band: Band, time: DelphiTime, operator: &str) -> Qso {
    let dial = q.dial_hz.filter(|hz| *hz > 0);
    Qso {
        time,
        callsign: q.callsign.clone(),
        mode: Proto::of(q.mode).mode(),
        band: dial.and_then(zlog_core::band_of).unwrap_or(band),
        freq: dial.map_or_else(String::new, |hz| {
            format!("{}.{}", hz / 1000, hz % 1000 / 100)
        }),
        operator: operator.into(),
        memo: q.grid.clone(),
        nr_sent: exchange(&q.sent),
        nr_rcvd: exchange(&q.rcvd),
        ..Qso::default()
    }
}

fn exchange(text: &str) -> String {
    let text = text.trim();
    match text.strip_prefix('R') {
        Some(r) if r.starts_with(['+', '-']) => r.into(),
        _ => text.into(),
    }
}

fn migrated(s: &Section) -> Section {
    let mut s = s.clone();
    if let (None, Some(g)) = (s.get("my_grid"), s.get("grid").cloned()) {
        s.insert("my_grid".into(), g);
    }
    let old: Vec<(String, String)> = s
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix("modem.")?.to_string(), v.clone())))
        .collect();
    for (k, v) in old {
        s.entry(k).or_insert(v);
    }
    s
}

pub fn load_panel(p: &mut ftxl_ui::FtPanel, s: &Section) {
    let s = migrated(s);
    let defaults = ftxl_ui::FtPanel::new("en").settings();
    for (k, d) in defaults {
        if k != "my_call" {
            p.load_setting(k, s.get(k).unwrap_or(&d));
        }
    }
}

pub fn save_panel(p: &ftxl_ui::FtPanel, s: &mut Section) {
    s.remove("grid");
    s.retain(|k, _| !k.starts_with("modem."));
    for (k, v) in p.settings() {
        if k != "my_call" {
            s.insert(k.into(), v);
        }
    }
}

pub fn scope_of(s: &Section) -> bool {
    s.get(SCOPE).is_none_or(|v| v != "false")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(pairs: &[(&str, &str)]) -> Section {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn old_keys_migrate_to_the_ft_panel() {
        let old = section(&[
            ("mode", "ft4"),
            ("grid", "pm95"),
            ("rx_hz", "812"),
            ("parity", "odd"),
            ("auto", "false"),
            ("modem.passes", "2"),
            ("modem.fading", "false"),
            ("my_call", "JH1ABC"),
            ("scope", "false"),
        ]);
        let mut p = ftxl_ui::FtPanel::new("en");
        load_panel(&mut p, &old);
        assert_eq!(p.proto, ftxl::Proto::Ft4);
        assert_eq!((p.my_grid.as_str(), p.rx_hz), ("PM95", 812.0));
        assert_eq!(p.parity, ftxl::Parity::Odd);
        assert!(!p.seq.auto);
        assert_eq!((p.options.ft.passes, p.options.ft.fading), (2, false));
        assert!(p.my_call.is_empty() && !scope_of(&old));
        let mut saved = old.clone();
        save_panel(&p, &mut saved);
        assert!(!saved.contains_key("grid") && !saved.contains_key("modem.passes"));
        assert_eq!(
            (saved["my_grid"].as_str(), saved["passes"].as_str()),
            ("PM95", "2")
        );
        let mut q = ftxl_ui::FtPanel::new("en");
        load_panel(&mut q, &saved);
        assert_eq!(q.settings(), p.settings());
        load_panel(&mut q, &Section::new());
        assert_eq!(q.settings(), ftxl_ui::FtPanel::new("en").settings());
        assert!(scope_of(&Section::new()));
    }

    #[test]
    fn a_completed_contact_is_logged_with_the_grid_and_the_exchange() {
        let mut q = ftxl::Qso {
            mode: ftxl::Proto::Ft4,
            callsign: "JA1YWX".into(),
            grid: "PM95".into(),
            sent: "-09".into(),
            rcvd: "R-14".into(),
            hz: 1500.0,
            dial_hz: None,
            at: std::time::SystemTime::UNIX_EPOCH,
        };
        let time = DelphiTime::from_ymd_hms(2026, 10, 25, 0, 1, 0);
        let l = qso(&q, Band::B7, time, "JA1ZLO");
        assert_eq!(
            (l.callsign.as_str(), l.mode, l.band, l.memo.as_str()),
            ("JA1YWX", zlog_core::Mode::Ft4, Band::B7, "PM95")
        );
        assert_eq!((l.nr_sent.as_str(), l.nr_rcvd.as_str()), ("-09", "-14"));
        assert!(l.freq.is_empty());
        q.rcvd = "13M".into();
        assert_eq!(qso(&q, Band::B7, time, "JA1ZLO").nr_rcvd, "13M");
        q.dial_hz = Some(14_080_000);
        let l = qso(&q, Band::B7, time, "JA1ZLO");
        assert_eq!((l.band, l.freq.as_str()), (Band::B14, "14080.0"));
    }
}
