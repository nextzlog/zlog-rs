use zlog_ui::device::{DeviceDriver, PortHub, SettingField, SettingKind};
use zlog_ui::Section;

use crate::morse::{self, Timing};
use crate::Keyer;

pub trait KeyerDrv: DeviceDriver + Send + Sync {
    fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn Keyer>, String>;
}

const TIMING_KEYS: [&str; 4] = ["speed", "weight", "space_factor", "ei_space_factor"];

pub fn with_timing(settings: &Section, timing: Timing) -> Section {
    let mut s = settings.clone();
    let values = [
        timing.wpm,
        timing.weight,
        timing.space_factor,
        timing.ei_space_factor,
    ];
    for (k, v) in TIMING_KEYS.iter().zip(values) {
        s.insert(k.to_string(), v.to_string());
    }
    s
}

pub fn timing_of(settings: &Section) -> Timing {
    let d = Timing::default();
    let get = |i: usize, default: u32| {
        settings
            .get(TIMING_KEYS[i])
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    Timing {
        wpm: get(0, d.wpm).clamp(morse::MIN_WPM, morse::MAX_WPM),
        weight: get(1, d.weight).min(100),
        space_factor: get(2, d.space_factor).max(1),
        ei_space_factor: get(3, d.ei_space_factor).max(1),
    }
}

pub fn get<T: std::str::FromStr>(settings: &Section, key: &str, default: T) -> T {
    settings
        .get(key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub fn ptt_delay_fields(language: &str) -> Vec<SettingField> {
    let t = |k| crate::texts().get(language, k).to_string();
    vec![
        SettingField::new(
            "ptt_before",
            t("ptt_before"),
            SettingKind::Int { min: 1, max: 255 },
        )
        .default_value("25"),
        SettingField::new(
            "ptt_after",
            t("ptt_after"),
            SettingKind::Int { min: 0, max: 255 },
        )
        .default_value("0"),
    ]
}

pub const PTT_KEYS: [&str; 2] = ["ptt_before", "ptt_after"];

pub fn ptt_keys(section: &Section, device: &Section) -> Section {
    PTT_KEYS
        .iter()
        .filter_map(|k| {
            let v = section.get(*k).or_else(|| device.get(*k))?;
            Some((k.to_string(), v.clone()))
        })
        .collect()
}

pub fn ptt_delays(settings: &Section) -> zlog_ui::tx::Delays {
    let ms = |key, default| {
        let v: u64 = get(settings, key, default);
        std::time::Duration::from_millis(v.min(255))
    };
    zlog_ui::tx::Delays {
        before: ms("ptt_before", 25),
        after: ms("ptt_after", 0),
    }
}

pub fn text(language: &str, key: &str) -> String {
    crate::texts().get(language, key).into()
}

pub fn port_field(language: &str, found: Vec<String>) -> SettingField {
    SettingField::new(
        "port",
        crate::texts().get(language, "port"),
        SettingKind::Port(found),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timing_reaches_the_driver_and_back() {
        let t = Timing {
            wpm: 32,
            weight: 60,
            space_factor: 110,
            ei_space_factor: 120,
        };
        let s = with_timing(&Section::new(), t);
        assert_eq!(s["speed"], "32");
        assert_eq!(timing_of(&s), t);
        assert_eq!(timing_of(&Section::new()), Timing::default());
        let mut bad = Section::new();
        bad.insert("speed".into(), "500".into());
        assert_eq!(timing_of(&bad).wpm, morse::MAX_WPM);
    }

    #[test]
    fn ptt_fields_keep_the_old_keys_and_defaults() {
        let f = ptt_delay_fields("en");
        assert_eq!((f[0].key, f[0].default.as_str()), ("ptt_before", "25"));
        assert_eq!((f[1].key, f[1].default.as_str()), ("ptt_after", "0"));
        assert_eq!(get(&Section::new(), "ptt_before", 25u32), 25);
    }

    #[test]
    fn the_device_ptt_delays_feed_the_transmitter() {
        let d = ptt_delays(&Section::new());
        assert_eq!((d.before.as_millis(), d.after.as_millis()), (25, 0));
        let mut s = Section::new();
        s.insert("ptt_before".into(), "300".into());
        s.insert("ptt_after".into(), "120".into());
        let d = ptt_delays(&s);
        assert_eq!((d.before.as_millis(), d.after.as_millis()), (255, 120));
    }
}
