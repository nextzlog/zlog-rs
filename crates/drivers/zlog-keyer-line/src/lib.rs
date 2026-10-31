use std::time::Duration;

use zlog_keyer::driver::{get, port_field, text};
use zlog_keyer::lines::{Line, LineKeyer, LineSetup, SimLines};
use zlog_keyer::{ptt_delay, timing_of, Keyer, KeyerDrv};
use zlog_ui::device::{DeviceDriver, PortHub, SettingField, SettingKind};
use zlog_ui::Section;

zlog_core::texts!();

pub fn setup(settings: &Section) -> LineSetup {
    let ms = |key, default| Duration::from_millis(ptt_delay(get(settings, key, default)) as u64);
    let key = if settings.get("key_line").is_some_and(|v| v == "rts") {
        Line::Rts
    } else {
        Line::Dtr
    };
    LineSetup {
        key,
        phone: key,
        ptt: get(settings, "ptt", false),
        ptt_before: ms("ptt_before", 25),
        ptt_after: ms("ptt_after", 0),
    }
}

#[derive(Default)]
pub struct LineDrv {
    sim: std::sync::Arc<std::sync::Mutex<Option<SimLines>>>,
}

impl LineDrv {
    pub fn simulator(&self) -> Option<SimLines> {
        self.sim.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl DeviceDriver for LineDrv {
    fn id(&self) -> &str {
        "serial"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        let lines = vec![("dtr".into(), "DTR".into()), ("rts".into(), "RTS".into())];
        vec![
            port_field(language, zlog_serial::port_names_with_simulator()),
            SettingField::new(
                "key_line",
                texts().get(language, "key_line"),
                SettingKind::Choice(lines),
            )
            .default_value("dtr"),
            SettingField::new("ptt", text(language, "ptt_other_line"), SettingKind::Bool)
                .default_value("false"),
        ]
    }

    fn status(&self, _language: &str) -> Option<String> {
        self.simulator().map(|s| s.status())
    }
}

impl KeyerDrv for LineDrv {
    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
        let (setup, timing) = (setup(settings), timing_of(settings));
        let path = settings.get("port").map_or("", |s| s.as_str());
        if path == zlog_serial::SIMULATOR {
            let sim = SimLines::new(setup.key, timing);
            *self.sim.lock().unwrap_or_else(|e| e.into_inner()) = Some(sim.clone());
            return Ok(Box::new(LineKeyer::new(sim, setup, timing)));
        }
        let port = zlog_serial::open_shared(path, None)?;
        Ok(Box::new(LineKeyer::new(port, setup, timing)))
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.driver::<dyn zlog_keyer::KeyerDrv>(20, || Box::<LineDrv>::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_choose_the_lines() {
        let mut s = Section::new();
        for (k, v) in [("key_line", "rts"), ("ptt", "true"), ("ptt_after", "300")] {
            s.insert(k.into(), v.into());
        }
        assert_eq!(
            setup(&s),
            LineSetup {
                key: Line::Rts,
                phone: Line::Rts,
                ptt: true,
                ptt_before: Duration::from_millis(25),
                ptt_after: Duration::from_millis(255),
            }
        );
        let d = setup(&Section::new());
        assert_eq!(
            (d.key, d.ptt, d.ptt_after),
            (Line::Dtr, false, Duration::from_millis(1))
        );
        let keys: Vec<_> = LineDrv::default()
            .settings("ja")
            .iter()
            .map(|f| f.key)
            .collect();
        assert_eq!(keys, ["port", "key_line", "ptt"]);
        assert!(
            LineDrv::default().open(&s, &PortHub::new()).is_err(),
            "no port named"
        );
    }

    fn keyed(drv: &LineDrv, s: &Section, text: &str) -> (Box<dyn Keyer>, SimLines) {
        let mut k = drv.open(s, &PortHub::new()).unwrap();
        k.send(text);
        let start = std::time::Instant::now();
        while k.busy() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(2));
        }
        (k, drv.simulator().unwrap())
    }

    fn simulator(pairs: &[(&str, &str)]) -> Section {
        let mut s = Section::new();
        s.insert("port".into(), zlog_serial::SIMULATOR.into());
        for (k, v) in pairs {
            s.insert(k.to_string(), v.to_string());
        }
        zlog_keyer::with_timing(
            &s,
            zlog_keyer::Timing {
                wpm: 30,
                ..Default::default()
            },
        )
    }

    #[test]
    fn the_simulator_keys_the_real_line_keyer_and_reads_the_text_back() {
        let drv = LineDrv::default();
        assert_eq!(
            drv.settings("en")[0].kind,
            SettingKind::Port(zlog_serial::port_names_with_simulator())
        );
        assert!(drv.status("en").is_none());
        let (_k, sim) = keyed(&drv, &simulator(&[]), "TEST");
        std::thread::sleep(Duration::from_millis(300));
        let st = sim.state();
        assert_eq!(st.keyed.trim(), "TEST");
        assert!(!st.key && !st.ptt);
        assert!(st.edges.iter().all(|(l, _, _)| *l == Line::Dtr));
        assert_eq!(drv.status("en").unwrap(), "KEY OFF, PTT OFF, keyed: TEST");
    }

    #[test]
    fn the_simulator_sees_ptt_before_the_key_on_the_other_line() {
        let drv = LineDrv::default();
        let s = simulator(&[("key_line", "rts"), ("ptt", "true"), ("ptt_before", "50")]);
        let (_k, sim) = keyed(&drv, &s, "E");
        std::thread::sleep(Duration::from_millis(100));
        let edges = sim.state().edges;
        let ptt_on = edges
            .iter()
            .find(|(l, on, _)| *l == Line::Dtr && *on)
            .unwrap()
            .2;
        let key_on = edges
            .iter()
            .find(|(l, on, _)| *l == Line::Rts && *on)
            .unwrap()
            .2;
        assert!(key_on - ptt_on >= Duration::from_millis(50), "{edges:?}");
        assert!(!sim.state().ptt);
    }

    #[test]
    fn abort_releases_the_simulated_lines() {
        let drv = LineDrv::default();
        let s = simulator(&[("ptt", "true")]);
        let mut k = drv.open(&s, &PortHub::new()).unwrap();
        k.send("TEST TEST TEST");
        std::thread::sleep(Duration::from_millis(150));
        k.abort();
        let start = std::time::Instant::now();
        while k.busy() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(50));
        let st = drv.simulator().unwrap().state();
        assert!(!st.key && !st.ptt, "{st:?}");
    }

    #[test]
    fn phone_ptt_keys_the_key_line_as_the_delphi_zlog_does() {
        let drv = LineDrv::default();
        let mut k = drv
            .open(&simulator(&[("ptt", "true")]), &PortHub::new())
            .unwrap();
        let sim = drv.simulator().unwrap();
        let wait = |want: bool| {
            let start = std::time::Instant::now();
            while sim.state().key != want {
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        k.set_phone_ptt(true);
        wait(true);
        assert!(!sim.state().ptt, "the CW PTT line stays off");
        k.set_phone_ptt(false);
        wait(false);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
