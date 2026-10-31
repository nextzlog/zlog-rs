use zlog_ui::device::{AudioDrv, DeviceDriver, DriverChoice};
use zlog_ui::{Section, SettingsPage};

use crate::heard::Filters;
use crate::texts;

pub const SECTION: &str = "panel.rtty";

pub const SOUND: &str = "sound";

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

pub const DEFAULT_SP_MESSAGES: [&str; 12] = [
    "$M $M",
    "$C $R $X $X",
    "TU",
    "",
    "NR? NR?",
    "$C? $C?",
    "$M",
    "AGN AGN",
    "",
    "",
    "",
    "",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub leading_zeros: bool,
    pub messages: [String; 12],
    pub sp_messages: [String; 12],
    pub scope: bool,
    pub filters: Filters,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            leading_zeros: true,
            messages: DEFAULT_MESSAGES.map(String::from),
            sp_messages: DEFAULT_SP_MESSAGES.map(String::from),
            scope: false,
            filters: Filters::default(),
        }
    }
}

impl Settings {
    pub fn load(s: &Section) -> Self {
        let d = Settings::default();
        let text = |key: &str, default: &str| s.get(key).cloned().unwrap_or(default.into());
        let flag = |key: &str, d: bool| s.get(key).and_then(|v| v.parse().ok()).unwrap_or(d);
        Settings {
            leading_zeros: flag("leading_zeros", d.leading_zeros),
            messages: std::array::from_fn(|n| text(&format!("F{}", n + 1), DEFAULT_MESSAGES[n])),
            sp_messages: std::array::from_fn(|n| {
                text(&format!("sp.F{}", n + 1), DEFAULT_SP_MESSAGES[n])
            }),
            scope: flag("scope", d.scope),
            filters: Filters {
                callsign: text("scope.callsign", &d.filters.callsign),
                number: text("scope.number", &d.filters.number),
            },
        }
    }

    pub fn save(&self, s: &mut Section) {
        let mut put = |k: &str, v: String| {
            s.insert(k.into(), v);
        };
        put("leading_zeros", self.leading_zeros.to_string());
        for n in 0..12 {
            put(&format!("F{}", n + 1), self.messages[n].clone());
            put(&format!("sp.F{}", n + 1), self.sp_messages[n].clone());
        }
        put("scope", self.scope.to_string());
        put("scope.callsign", self.filters.callsign.trim().into());
        put("scope.number", self.filters.number.trim().into());
    }

    pub fn bank(&self, bank: rtty_ui::Bank) -> &[String; 12] {
        match bank {
            rtty_ui::Bank::Cq => &self.messages,
            rtty_ui::Bank::Sp => &self.sp_messages,
        }
    }

    pub fn buttons(&self, bank: rtty_ui::Bank) -> Vec<rtty_ui::Message> {
        let label = |n: usize, m: &str| match m.trim().is_empty() {
            true => String::new(),
            false => format!("F{}", n + 1),
        };
        let pairs = self.bank(bank).iter().enumerate();
        pairs
            .map(|(n, m)| rtty_ui::Message::new(&label(n, m), m))
            .collect()
    }

    pub fn apply(&self, p: &mut rtty_ui::RttyPanel) {
        p.messages = self.buttons(rtty_ui::Bank::Cq);
        p.sp_messages = self.buttons(rtty_ui::Bank::Sp);
    }
}

pub fn load_panel(p: &mut rtty_ui::RttyPanel, s: &Section) {
    for (k, v) in rtty_ui::RttyPanel::new("en").settings() {
        if k != "my_call" {
            p.load_setting(k, &v);
        }
    }
    p.host_ptt = true;
    for (k, _) in p.settings() {
        if let Some(v) = s.get(k).filter(|_| k != "my_call") {
            p.load_setting(k, v);
        }
    }
}

pub fn save_panel(p: &rtty_ui::RttyPanel, s: &mut Section) {
    for (k, v) in p.settings() {
        if k != "my_call" {
            s.insert(k.into(), v);
        }
    }
    for old in ["theme", "scheme", "ptt_before", "ptt_after"] {
        s.remove(old);
    }
}

pub fn options_of(s: &Section) -> rtty::Options {
    let mut p = rtty_ui::RttyPanel::new("en");
    load_panel(&mut p, s);
    rtty::Options {
        rtty: p.options,
        ..rtty::Options::default()
    }
}

pub struct RttyOptions {
    drivers: Vec<Box<dyn AudioDrv>>,
    device: Option<DriverChoice>,
    panel: rtty_ui::RttyPanel,
}

impl RttyOptions {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>) -> Self {
        let mut panel = rtty_ui::RttyPanel::new("en");
        panel.host_call = true;
        RttyOptions {
            drivers,
            device: None,
            panel,
        }
    }
}

pub fn device_ui(
    ui: &mut egui::Ui,
    device: &mut Option<DriverChoice>,
    drivers: &[Box<dyn AudioDrv>],
    section: &mut Section,
    lang: &str,
) {
    let t = |k| texts().get(lang, k);
    let list: Vec<&dyn DeviceDriver> = drivers.iter().map(|d| &**d as _).collect();
    let saved = |d: &DriverChoice| {
        let mut s = Section::new();
        d.save(&mut s);
        s
    };
    let stale = device
        .as_ref()
        .is_none_or(|d| saved(d).iter().any(|(k, v)| section.get(k) != Some(v)));
    if stale {
        *device = Some(DriverChoice::load(section, SOUND, &list));
    }
    zlog_ui::device::own_sound_ui(ui, section, lang, |ui, section| {
        let d = device.as_mut().expect("loaded");
        d.ui(ui, lang, t("device"), t("none"), &list);
        d.save(section);
    });
}

impl SettingsPage for RttyOptions {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, lang: &str) {
        let t = |k| texts().get(lang, k);
        device_ui(ui, &mut self.device, &self.drivers, section, lang);
        ui.separator();
        self.panel.language = lang.into();
        load_panel(&mut self.panel, section);
        self.panel.settings_ui(ui);
        save_panel(&self.panel, section);
        let mut s = Settings::load(section);
        ui.checkbox(&mut s.leading_zeros, t("leading_zeros"));
        ui.separator();
        ui.checkbox(&mut s.scope, t("scope.enabled"))
            .on_hover_text(t("scope.tip"));
        let labels = [t("scope.callsign"), t("scope.number")];
        zlog_ui::form(ui, "rtty-scope-grid", &labels, |ui, form| {
            ui.label(t("scope.callsign"));
            let edit = egui::TextEdit::singleline(&mut s.filters.callsign)
                .hint_text(crate::heard::DEFAULT_CALLSIGN)
                .font(egui::TextStyle::Monospace);
            form.text(ui, edit).on_hover_text(t("scope.pattern.tip"));
            ui.end_row();
            ui.label(t("scope.number"));
            let edit = egui::TextEdit::singleline(&mut s.filters.number)
                .hint_text(t("scope.number.hint"))
                .font(egui::TextStyle::Monospace);
            form.text(ui, edit).on_hover_text(t("scope.pattern.tip"));
            ui.end_row();
        });
        let error = zlog_ui::palette::error(ui);
        if let Some(e) = s.filters.callsign_error() {
            ui.colored_label(error, format!("{}: {e}", t("scope.invalid.callsign")));
        }
        if let Some(e) = s.filters.number_error() {
            ui.colored_label(error, format!("{}: {e}", t("scope.invalid.number")));
        }
        for (key, tip, id, bank) in [
            ("messages", "messages.tip", "rtty-cq-grid", &mut s.messages),
            (
                "sp_messages",
                "sp_messages.tip",
                "rtty-sp-grid",
                &mut s.sp_messages,
            ),
        ] {
            ui.separator();
            ui.label(t(key)).on_hover_text(t(tip));
            zlog_ui::form(ui, id, &["F12"], |ui, form| {
                for (n, m) in bank.iter_mut().enumerate() {
                    ui.label(format!("F{}", n + 1));
                    form.text(ui, egui::TextEdit::singleline(m));
                    ui.end_row();
                }
            });
        }
        s.save(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let mut s = Settings {
            leading_zeros: false,
            scope: true,
            filters: Filters {
                callsign: "JA[0-9][A-Z]{3}".into(),
                number: "[0-9]{3}".into(),
            },
            ..Settings::default()
        };
        s.messages[3] = "QRZ?".into();
        s.sp_messages[2] = "TU 73".into();
        let mut sec = Section::new();
        s.save(&mut sec);
        assert_eq!(
            (sec["F4"].as_str(), sec["sp.F3"].as_str()),
            ("QRZ?", "TU 73")
        );
        assert_eq!(Settings::load(&sec), s);
        assert_eq!(Settings::load(&Section::new()), Settings::default());
        let b = s.buttons(rtty_ui::Bank::Cq);
        assert_eq!((b[0].label.as_str(), b[3].label.as_str()), ("F1", "F4"));
        assert!(b[8].label.is_empty());
        let mut p = rtty_ui::RttyPanel::new("en");
        s.apply(&mut p);
        p.bank = rtty_ui::Bank::Sp;
        assert_eq!(p.bank_messages()[2].text, "TU 73");
        assert_eq!(p.messages[3].text, "QRZ?");
    }

    fn section(pairs: &[(&str, &str)]) -> Section {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn old_keys_reach_the_rtty_panel() {
        let old = section(&[
            ("mark", "1275"),
            ("baud", "50"),
            ("reverse", "true"),
            ("squelch", "0.4"),
            ("ptt_before", "100"),
            ("highlight", "JA1ZLO 599"),
            ("theme", "1"),
            ("my_call", "JH1ABC"),
        ]);
        let mut p = rtty_ui::RttyPanel::new("en");
        load_panel(&mut p, &old);
        let o = p.options;
        assert_eq!((o.mark, o.baud, o.squelch), (1275.0, 50.0, 0.4));
        assert!(o.reverse);
        assert_eq!(p.highlight, "JA1ZLO 599");
        assert!(p.my_call.is_empty());
        let mut saved = old.clone();
        save_panel(&p, &mut saved);
        for old in ["theme", "scheme", "ptt_before"] {
            assert!(!saved.contains_key(old), "{old}");
        }
        assert_eq!(saved["baud"], "50");
        let mut q = rtty_ui::RttyPanel::new("en");
        load_panel(&mut q, &saved);
        assert_eq!(q.settings(), p.settings());
        assert_eq!(options_of(&Section::new()), rtty::Options::default());
    }
}
