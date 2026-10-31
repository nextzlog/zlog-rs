use std::time::SystemTime;

use rtty::Modem as _;
use zlog_core::Mode;
use zlog_ui::device::{
    Decode, DeviceDriver, Modem, ModemDrv, ModemKind, PortHub, SettingField, SettingKind, Stamp,
};
use zlog_ui::Section;

use rtty_ui::console::{level, BAUDS};

use crate::settings::{options_of, Settings};
use crate::texts;

pub const RATE: u32 = 48000;

pub struct RttyModem {
    section: Section,
    settings: Settings,
    modem: rtty::Rtty,
}

impl RttyModem {
    pub fn new(section: Section) -> Self {
        let settings = Settings::load(&section);
        let modem = rtty::Rtty::new(RATE, options_of(&section)).expect("valid rate");
        RttyModem {
            section,
            settings,
            modem,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}

pub fn decode(d: rtty::Decode) -> Decode {
    Decode {
        text: d.text,
        hz: d.hz,
        snr: Some(d.snr),
        at: d.at.into(),
        ..Decode::default()
    }
}

impl Modem for RttyModem {
    fn kind(&self) -> ModemKind {
        ModemKind::Stream
    }

    fn sample_rate(&self) -> u32 {
        RATE
    }

    fn rx(&mut self, samples: &[f32], at: Stamp) -> Vec<Decode> {
        self.modem
            .push(samples, at.into())
            .into_iter()
            .map(|d| {
                let level = format!("{:.2}", level(d.snr));
                let mut d = decode(d);
                d.fields.insert("level".into(), level);
                d
            })
            .collect()
    }

    fn tx(&mut self, text: &str, hz: f32) -> Result<Vec<f32>, String> {
        let hz = if hz > 0.0 {
            hz
        } else {
            self.modem.options().rtty.mark
        };
        Ok(self.modem.transmit(text, hz, SystemTime::now())?.audio)
    }

    fn options(&self, language: &str) -> Vec<SettingField> {
        options(language)
    }

    fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        if !options("en").iter().any(|f| f.key == key) {
            return Err(format!("unknown option: {key}"));
        }
        self.section.insert(key.into(), value.into());
        self.settings = Settings::load(&self.section);
        self.modem.set_options(options_of(&self.section));
        Ok(())
    }
}

fn options(language: &str) -> Vec<SettingField> {
    let t = |k| rtty_ui::text::texts().get(language, k).to_string();
    let d = rtty::RttyOptions::default();
    let bool = |key, v: bool| {
        SettingField::new(key, t(key), SettingKind::Bool).default_value(v.to_string())
    };
    let bauds = BAUDS.map(|b| (b.to_string(), b.to_string())).to_vec();
    vec![
        SettingField::new(
            "mark",
            t("mark"),
            SettingKind::Int {
                min: 300,
                max: 3000,
            },
        )
        .default_value(d.mark.to_string()),
        SettingField::new("shift", t("shift"), SettingKind::Int { min: 50, max: 1000 })
            .default_value(d.shift.to_string()),
        SettingField::new("baud", t("baud"), SettingKind::Choice(bauds))
            .default_value(d.baud.to_string()),
        bool("reverse", d.reverse),
        bool("uos", d.uos),
        SettingField::new("squelch", t("squelch"), SettingKind::Text)
            .default_value(d.squelch.to_string()),
        bool("atc", d.atc),
        bool("blanker", d.blanker),
        SettingField::new(
            "leading_zeros",
            texts().get(language, "leading_zeros"),
            SettingKind::Bool,
        )
        .default_value(Settings::default().leading_zeros.to_string()),
    ]
}

pub struct RttyModemDrv;

impl DeviceDriver for RttyModemDrv {
    fn id(&self) -> &str {
        "rtty"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "modem").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        options(language)
    }
}

impl ModemDrv for RttyModemDrv {
    fn mode(&self) -> Mode {
        Mode::Rtty
    }

    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn Modem>, String> {
        Ok(Box::new(RttyModem::new(settings.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtty::sim::{render, Band, Source, Station};

    fn at(text: &str) -> Stamp {
        text.parse().unwrap()
    }

    fn heard(m: &mut dyn Modem, audio: &[f32], start: Stamp) -> Vec<Decode> {
        let mut got = Vec::new();
        for (n, block) in audio.chunks(4800).enumerate() {
            let t = zlog_ui::device::stamp_after(start, n * 4800, RATE);
            got.extend(m.rx(block, t));
        }
        got
    }

    fn words(got: &[Decode]) -> String {
        got.iter()
            .map(|d| d.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_modem_hears_what_it_sends() {
        let drv = RttyModemDrv;
        assert_eq!(drv.mode(), Mode::Rtty);
        let mut m = drv.open(&Section::new(), &PortHub::new()).unwrap();
        let mut audio = m.tx("CQ TEST 599 ", 0.0).unwrap();
        audio.extend(vec![0.0; RATE as usize]);
        let t = at("2026-10-25T00:00:00Z");
        let got = heard(m.as_mut(), &audio, t);
        assert_eq!(words(&got), "CQ TEST 599");
        assert!(got.iter().all(|d| d.at > t && d.slot.is_none()));
        assert!(got.iter().all(|d| d.fields.contains_key("level")));
        assert_eq!(got[0].hz, 2125.0);
        assert_eq!(m.kind(), ModemKind::Stream);
    }

    #[test]
    fn an_exchange_on_the_simulated_band_goes_through_the_driver() {
        let band = Band::audio(RATE, 14.0);
        let source = Source::Rtty(rtty::RttyOptions::default());
        let mut ja1zlo = RttyModemDrv.open(&Section::new(), &PortHub::new()).unwrap();
        let mut ja1ywx = RttyModemDrv.open(&Section::new(), &PortHub::new()).unwrap();
        let t = at("2026-10-25T00:00:00Z");
        let cq = Station::new("CQ TEST JA1ZLO JA1ZLO ", 2125.0, 0.5, 10.0);
        let audio = render(&band, source, &[cq], 1).unwrap();
        assert!(words(&heard(ja1ywx.as_mut(), &audio, t)).contains("CQ TEST JA1ZLO JA1ZLO"));
        let reply = Station::new("JA1ZLO 599 100110M ", 2125.0, 0.5, 10.0);
        let audio = render(&band, source, &[reply], 2).unwrap();
        let t = at("2026-10-25T00:00:20Z");
        let got = heard(ja1zlo.as_mut(), &audio, t);
        assert!(words(&got).contains("JA1ZLO 599 100110M"), "{got:?}");
        assert!(got.iter().all(|d| d.snr.is_some_and(|s| s > 0.0)));
    }

    #[test]
    fn options_change_the_modem_at_run_time() {
        let mut m = RttyModem::new(Section::new());
        assert!(m.settings().leading_zeros);
        m.set_option("leading_zeros", "false").unwrap();
        m.set_option("reverse", "true").unwrap();
        m.set_option("atc", "false").unwrap();
        m.set_option("squelch", "0.3").unwrap();
        assert!(!m.settings().leading_zeros);
        let o = m.modem.options().rtty;
        assert!(o.reverse && !o.atc && o.squelch == 0.3);
        assert!(m.set_option("ptt", "false").is_err());
        let keys: Vec<_> = m.options("ja").iter().map(|f| f.key).collect();
        for k in ["leading_zeros", "mark", "squelch", "atc", "blanker"] {
            assert!(keys.contains(&k), "{k}");
        }
        let mut audio = m.tx("RYRY ", 0.0).unwrap();
        audio.extend(vec![0.0; RATE as usize]);
        assert_eq!(words(&heard(&mut m, &audio, Stamp::default())), "RYRY");
    }
}
