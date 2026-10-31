use zlog_core::Mode;
use zlog_ui::device::{
    Decode, DeviceDriver, Modem, ModemDrv, ModemKind, PortHub, SettingField, SettingKind, Stamp,
};
use zlog_ui::Section;

use crate::morse::{keying, Timing, MAX_WPM, MIN_WPM};
use crate::texts;

pub const RATE: u32 = 8000;
pub const TONE: u32 = 700;

const AMPLITUDE: f32 = 0.5;

pub struct CwModem {
    section: Section,
    timing: Timing,
    tone: f32,
}

fn get<T: std::str::FromStr>(s: &Section, key: &str, default: T) -> T {
    s.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl CwModem {
    pub fn new(section: Section) -> Self {
        let timing = Timing {
            wpm: get(&section, "speed", Timing::default().wpm).clamp(MIN_WPM, MAX_WPM),
            ..Timing::default()
        };
        let tone = get(&section, "tone", TONE).clamp(300, 3000) as f32;
        CwModem {
            section,
            timing,
            tone,
        }
    }

    pub fn timing(&self) -> Timing {
        self.timing
    }
}

impl Modem for CwModem {
    fn kind(&self) -> ModemKind {
        ModemKind::Stream
    }

    fn sample_rate(&self) -> u32 {
        RATE
    }

    fn rx(&mut self, _samples: &[f32], _at: Stamp) -> Vec<Decode> {
        Vec::new()
    }

    fn tx(&mut self, text: &str, hz: f32) -> Result<Vec<f32>, String> {
        let hz = if hz > 0.0 { hz } else { self.tone };
        let step = std::f64::consts::TAU * hz as f64 / RATE as f64;
        let mut out = Vec::new();
        for s in keying(text, &self.timing) {
            let n = (s.len.as_secs_f64() * RATE as f64) as usize;
            out.extend((0..n).map(|i| match s.key {
                true => AMPLITUDE * (step * i as f64).sin() as f32,
                false => 0.0,
            }));
        }
        Ok(out)
    }

    fn options(&self, language: &str) -> Vec<SettingField> {
        options(language)
    }

    fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        if !options("en").iter().any(|f| f.key == key) {
            return Err(format!("unknown option: {key}"));
        }
        self.section.insert(key.into(), value.into());
        let fresh = CwModem::new(std::mem::take(&mut self.section));
        (self.section, self.timing, self.tone) = (fresh.section, fresh.timing, fresh.tone);
        Ok(())
    }
}

fn options(language: &str) -> Vec<SettingField> {
    let t = |k| texts().get(language, k).to_string();
    let (min, max) = (MIN_WPM as i64, MAX_WPM as i64);
    vec![
        SettingField::new("speed", t("speed"), SettingKind::Int { min, max })
            .default_value(Timing::default().wpm.to_string()),
        SettingField::new(
            "tone",
            t("modem.tone"),
            SettingKind::Int {
                min: 300,
                max: 3000,
            },
        )
        .default_value(TONE.to_string()),
    ]
}

pub struct CwModemDrv;

impl DeviceDriver for CwModemDrv {
    fn id(&self) -> &str {
        "cw"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "modem").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        options(language)
    }
}

impl ModemDrv for CwModemDrv {
    fn mode(&self) -> Mode {
        Mode::Cw
    }

    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn Modem>, String> {
        Ok(Box::new(CwModem::new(settings.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modem_keys_text_and_leaves_decoding_to_a_backend() {
        let drv = CwModemDrv;
        assert_eq!(drv.mode(), Mode::Cw);
        let mut m = drv.open(&Section::new(), &PortHub::new()).unwrap();
        let audio = m.tx("E", 0.0).unwrap();
        let dot = Timing::default().dot().as_secs_f64() * RATE as f64;
        assert!(audio.iter().filter(|x| **x != 0.0).count() as f64 <= dot);
        assert!(m.rx(&audio, Stamp::default()).is_empty());
        assert_eq!(m.kind(), ModemKind::Stream);
    }

    #[test]
    fn options_change_the_modem_at_run_time() {
        let mut m = CwModem::new(Section::new());
        let slow = m.tx("E", 0.0).unwrap().len();
        m.set_option("speed", "50").unwrap();
        assert_eq!(m.timing().wpm, 50);
        assert!(m.tx("E", 0.0).unwrap().len() < slow);
        m.set_option("tone", "600").unwrap();
        assert!(m.set_option("weight", "50").is_err());
        assert_eq!(m.options("ja").len(), 2);
    }
}
