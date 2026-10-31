use std::time::{Duration, SystemTime};

use zlog_core::Mode;
use zlog_ui::device::{
    Burst, Decode, DeviceDriver, Modem, ModemDrv, ModemKind, Parity, PortHub, SettingField,
    SettingKind, Stamp,
};
use zlog_ui::Section;

use crate::texts;

pub const RATE: u32 = 48000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Proto {
    #[default]
    Ft8,
    Ft4,
}

impl Proto {
    pub const ALL: [Proto; 2] = [Proto::Ft8, Proto::Ft4];

    pub fn id(self) -> &'static str {
        match self {
            Proto::Ft8 => "ft8",
            Proto::Ft4 => "ft4",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Proto::Ft8 => "FT8",
            Proto::Ft4 => "FT4",
        }
    }

    pub fn mode(self) -> Mode {
        match self {
            Proto::Ft8 => Mode::Ft8,
            Proto::Ft4 => Mode::Ft4,
        }
    }

    pub fn period(self) -> Duration {
        match self {
            Proto::Ft8 => Duration::from_secs(15),
            Proto::Ft4 => Duration::from_millis(7500),
        }
    }

    pub fn kind(self) -> ModemKind {
        ModemKind::Slots {
            period: self.period(),
        }
    }

    pub fn ftxl(self) -> ftxl::Proto {
        match self {
            Proto::Ft8 => ftxl::Proto::Ft8,
            Proto::Ft4 => ftxl::Proto::Ft4,
        }
    }

    pub fn of(p: ftxl::Proto) -> Self {
        match p {
            ftxl::Proto::Ft8 => Proto::Ft8,
            ftxl::Proto::Ft4 => Proto::Ft4,
        }
    }
}

pub fn decode(d: ftxl::Decode) -> Decode {
    let fields = d.fields.map_or_else(Default::default, |f| {
        [("to", f.to), ("de", f.de), ("extra", f.extra)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    });
    Decode {
        text: d.text,
        hz: d.hz,
        snr: Some(d.snr),
        dt: d.dt,
        at: d.at.into(),
        slot: d.slot.map(Stamp::from),
        fields,
    }
}

fn get<T: std::str::FromStr>(s: &Section, key: &str, default: T) -> T {
    s.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

pub fn options_of(s: &Section) -> ftxl::Options {
    let d = ftxl::Options::default();
    ftxl::Options {
        min_hz: get(s, "min_hz", d.min_hz),
        max_hz: get(s, "max_hz", d.max_hz),
        ft: ftxl::FtOptions {
            passes: get(s, "passes", d.ft.passes),
            candidates: get(s, "candidates", d.ft.candidates),
            rx_width: get(s, "rx_width", d.ft.rx_width),
            blanker: get(s, "blanker", d.ft.blanker),
            fading: get(s, "fading", d.ft.fading),
            ..d.ft
        },
        ..d
    }
    .sanitized()
}

pub struct FtModem {
    proto: Proto,
    rate: u32,
    section: Section,
    modem: Box<dyn ftxl::Modem + Send>,
}

impl FtModem {
    pub fn new(proto: Proto, rate: u32, section: Section) -> Result<Self, String> {
        let modem = proto.ftxl().open(rate, options_of(&section))?;
        Ok(FtModem {
            proto,
            rate,
            section,
            modem,
        })
    }

    pub fn proto(&self) -> Proto {
        self.proto
    }

    pub fn set_rx_hz(&mut self, hz: Option<f32>) {
        let mut o = *self.modem.options();
        if o.rx_hz != hz {
            o.rx_hz = hz;
            self.modem.set_options(o);
        }
    }

    pub fn ftxl_options(&self) -> &ftxl::Options {
        self.modem.options()
    }
}

impl Modem for FtModem {
    fn kind(&self) -> ModemKind {
        self.proto.kind()
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn rx(&mut self, samples: &[f32], at: Stamp) -> Vec<Decode> {
        self.modem
            .push(samples, at.into())
            .into_iter()
            .map(decode)
            .collect()
    }

    fn tx(&mut self, text: &str, hz: f32) -> Result<Vec<f32>, String> {
        Ok(self.modem.transmit(text, hz, SystemTime::now())?.audio)
    }

    fn schedule(
        &mut self,
        text: &str,
        hz: f32,
        now: Stamp,
        parity: Parity,
    ) -> Result<Burst, String> {
        let slot = self.kind().next_slot(now, parity);
        let tx = self.modem.transmit(text, hz, slot.into())?;
        Ok(Burst {
            at: tx.start.into(),
            samples: tx.audio,
        })
    }

    fn options(&self, language: &str) -> Vec<SettingField> {
        options(language)
    }

    fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        if !options("en").iter().any(|f| f.key == key) {
            return Err(format!("unknown option: {key}"));
        }
        self.section.insert(key.into(), value.into());
        let rx_hz = self.modem.options().rx_hz;
        self.modem.set_options(ftxl::Options {
            rx_hz,
            ..options_of(&self.section)
        });
        Ok(())
    }
}

pub fn options(language: &str) -> Vec<SettingField> {
    let t = |k| ftxl_ui::text::texts().get(language, k).to_string();
    let d = ftxl::Options::default();
    let int = |key, min, max, v: f32| {
        SettingField::new(key, t(key), SettingKind::Int { min, max }).default_value(v.to_string())
    };
    let bool = |key, v: bool| {
        SettingField::new(key, t(key), SettingKind::Bool).default_value(v.to_string())
    };
    vec![
        int("min_hz", 0, 5000, d.min_hz),
        int("max_hz", 100, 5500, d.max_hz),
        int("passes", 1, 10, d.ft.passes as f32),
        int("candidates", 1, 5000, d.ft.candidates as f32),
        int("rx_width", 0, 1000, d.ft.rx_width),
        bool("blanker", d.ft.blanker),
        bool("fading", d.ft.fading),
    ]
}

pub struct FtModemDrv(pub Proto);

impl DeviceDriver for FtModemDrv {
    fn id(&self) -> &str {
        self.0.id()
    }

    fn title(&self, language: &str) -> String {
        format!("{} {}", self.0.name(), texts().get(language, "modem"))
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        options(language)
    }
}

impl ModemDrv for FtModemDrv {
    fn mode(&self) -> Mode {
        self.0.mode()
    }

    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn Modem>, String> {
        Ok(Box::new(FtModem::new(self.0, RATE, settings.clone())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftxl::sim::{render, Band, Source, Station};

    fn at(text: &str) -> Stamp {
        text.parse().unwrap()
    }

    fn heard(m: &mut dyn Modem, audio: &[f32], slot: Stamp) -> Vec<Decode> {
        let rate = m.sample_rate() as usize;
        let mut got = Vec::new();
        for (n, block) in audio.chunks(rate / 10).enumerate() {
            let t = zlog_ui::device::stamp_after(slot, n * (rate / 10), rate as u32);
            got.extend(m.rx(block, t));
        }
        got
    }

    #[test]
    fn ft8_decodes_through_the_modem_driver() {
        let drv = FtModemDrv(Proto::Ft8);
        assert_eq!((drv.id(), drv.mode()), ("ft8", Mode::Ft8));
        let mut m = drv.open(&Section::new(), &PortHub::new()).unwrap();
        let mut band = Band::slot(Source::Ft8);
        band.rate = m.sample_rate();
        let stations = [
            Station::new("CQ JA1ZLO PM95", 1200.0, 0.1, -8.0),
            Station::new("JA1ZLO JA1YWX R-07", 1800.0, 0.0, -12.0),
        ];
        let mut audio = render(&band, Source::Ft8, &stations, 7).unwrap();
        audio.extend(vec![0.0; RATE as usize]);
        let slot = at("2026-10-25T00:00:15Z");
        let mut got = heard(m.as_mut(), &audio, slot);
        got.sort_by(|a, b| a.hz.total_cmp(&b.hz));
        let texts: Vec<&str> = got.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(texts, ["CQ JA1ZLO PM95", "JA1ZLO JA1YWX R-07"]);
        assert!(got.iter().all(|d| d.slot == Some(slot) && d.snr.is_some()));
        assert!((got[0].hz - 1200.0).abs() < 5.0);
        assert_eq!(got[1].fields["extra"], "R-07");
        assert_eq!(got[0].fields["to"], "CQ");
        assert_eq!(m.kind(), Proto::Ft8.kind());
    }

    #[test]
    fn bursts_start_half_a_second_into_the_chosen_slot() {
        for proto in Proto::ALL {
            let mut m = FtModem::new(proto, 12000, Section::new()).unwrap();
            let now = at("2026-10-25T00:00:01Z");
            let even = m
                .schedule("CQ JA1ZLO PM95", 1500.0, now, Parity::Even)
                .unwrap();
            let odd = m
                .schedule("CQ JA1ZLO PM95", 1500.0, now, Parity::Odd)
                .unwrap();
            let p = chrono::TimeDelta::from_std(proto.period()).unwrap();
            let half = chrono::TimeDelta::milliseconds(500);
            assert_eq!(
                even.at,
                at("2026-10-25T00:00:00Z") + p + p + half,
                "{proto:?}"
            );
            assert_eq!(odd.at, at("2026-10-25T00:00:00Z") + p + half, "{proto:?}");
            assert!(!even.samples.is_empty());
        }
    }

    #[test]
    fn options_reach_the_library() {
        let mut m = FtModem::new(Proto::Ft4, RATE, Section::new()).unwrap();
        assert_eq!(*m.ftxl_options(), ftxl::Options::default());
        m.set_option("passes", "2").unwrap();
        m.set_option("fading", "false").unwrap();
        m.set_rx_hz(Some(1000.0));
        m.set_option("max_hz", "2500").unwrap();
        let o = m.ftxl_options();
        assert_eq!((o.ft.passes, o.ft.fading, o.max_hz), (2, false, 2500.0));
        assert_eq!(o.rx_hz, Some(1000.0));
        assert!(m.set_option("baud", "50").is_err());
        assert_eq!(m.options("ja").len(), 7);
        assert_eq!(
            m.kind(),
            ModemKind::Slots {
                period: Duration::from_millis(7500)
            }
        );
    }
}
