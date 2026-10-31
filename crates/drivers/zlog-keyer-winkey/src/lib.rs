use std::io;
use std::sync::{Arc, Mutex};

pub use haml::keyer::winkeyer::{Setup, WinKeyer};
pub use haml::sim::WinKeyerSim;
use zlog_keyer::driver::{get, port_field};
use zlog_keyer::{ptt_delay, timing_of, Keyer, KeyerDrv};
use zlog_serial::{Line, Lines, SIMULATOR};
use zlog_ui::device::{Boxed, DeviceDriver, PortHub, SettingField, SettingKind, KEYER_PORT};
use zlog_ui::Section;

zlog_core::texts!();

pub const NEO: &str = "neo";

pub const NEO_DEVICES: [&str; 2] = [NEO, "neo_keyer"];

pub fn setup(settings: &Section) -> Setup {
    Setup {
        wpm: timing_of(settings).wpm,
        paddle_swap: get(settings, "paddle_reverse", false),
        sidetone: get(settings, "sidetone", true),
        ptt_before_ms: ptt_delay(get(settings, "ptt_before", 25)),
        ptt_after_ms: ptt_delay(get(settings, "ptt_after", 0)),
        speed_pot: !settings
            .get(zlog_keyer::SO2R_KEY)
            .is_some_and(|d| NEO_DEVICES.contains(&d.as_str())),
    }
}

pub fn open_on(port: Boxed, settings: &Section, ports: &PortHub) -> io::Result<Box<dyn Keyer>> {
    let slot = ports.slot(KEYER_PORT);
    Ok(Box::new(WinKeyer::publish(port, &setup(settings), &slot)?))
}

pub type Slot = Arc<Mutex<Option<WinKeyerSim>>>;

pub fn baud(settings: &Section) -> u32 {
    let neo = settings
        .get(zlog_keyer::SO2R_KEY)
        .is_some_and(|d| NEO_DEVICES.contains(&d.as_str()));
    if neo {
        9600
    } else {
        1200
    }
}

fn open_port(path: &str, baud: u32, slot: &Slot) -> Result<Boxed, String> {
    let sim = (path == SIMULATOR).then(WinKeyerSim::new);
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = sim.clone();
    if let Some(sim) = sim {
        return Ok(Box::new(sim.transport()));
    }
    let mut port = zlog_serial::open(path, baud, 1)?;
    let _ = port.set(Line::Dtr, true);
    let _ = port.set(Line::Rts, false);
    Ok(Box::new(port))
}

pub fn port_names() -> Vec<String> {
    zlog_serial::port_names_with_simulator()
}

#[derive(Default)]
pub struct WinKeyerDrv {
    pub sim: Slot,
}

impl DeviceDriver for WinKeyerDrv {
    fn id(&self) -> &str {
        "winkeyer"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        let keyer = |k| zlog_keyer::driver::text(language, k);
        vec![
            port_field(language, port_names()),
            SettingField::new(
                "sidetone",
                texts().get(language, "sidetone"),
                SettingKind::Bool,
            )
            .default_value("true"),
            SettingField::new("paddle_reverse", keyer("paddle_reverse"), SettingKind::Bool)
                .default_value("false"),
        ]
    }

    fn status(&self, language: &str) -> Option<String> {
        let sim = self.sim.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
        let t = |k| texts().get(language, k);
        let on_off = |on| t(if on { "sim.on" } else { "sim.off" });
        let s = sim.state();
        let tail: String = s
            .sent
            .chars()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        Some(format!(
            "{} {}, {} {}, {} {}, {} {}, {}: {tail}",
            t("sim.host"),
            on_off(s.open),
            t("sim.wpm"),
            s.wpm,
            t("sim.ptt"),
            on_off(s.ptt),
            t("sim.sidetone"),
            on_off(s.sidetone & 0x80 == 0),
            t("sim.sent"),
        ))
    }
}

impl KeyerDrv for WinKeyerDrv {
    fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
        let path = settings.get("port").map_or("", |s| s.as_str());
        let port = open_port(path, baud(settings), &self.sim)?;
        open_on(port, settings, ports).map_err(|e| e.to_string())
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    let sim = Slot::default();
    b.driver::<dyn zlog_keyer::KeyerDrv>(10, move || Box::new(WinKeyerDrv { sim: sim.clone() }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use haml::keyer::winkeyer::{ADMIN, ADMIN_CLOSE, ADMIN_OPEN, PTT};
    use std::time::Duration;
    use zlog_keyer::{KeyerEvent, Timing};
    use zlog_serial::Transport;

    #[derive(Default)]
    struct Answering {
        written: Vec<u8>,
        replies: Vec<u8>,
    }

    impl zlog_serial::Emulator for Answering {
        fn receive(&mut self, bytes: &[u8]) {
            self.written.extend_from_slice(bytes);
            if bytes == [ADMIN, ADMIN_OPEN] {
                self.replies.push(23);
            }
        }
        fn send(&mut self, out: &mut Vec<u8>) {
            out.append(&mut self.replies);
        }
    }

    fn neo_skips_the_pot(s: &Section) {
        let mut s = s.clone();
        s.insert(zlog_keyer::SO2R_KEY.into(), NEO.into());
        assert!(
            !setup(&s).speed_pot,
            "zLog skips GET_SPEEDPOT with SO2R Neo"
        );
        s.insert(zlog_keyer::SO2R_KEY.into(), "neo_keyer".into());
        assert!(!setup(&s).speed_pot, "the Neo on the keyer port too");
        assert_eq!(baud(&s), 9600, "the Neo's WinKeyer runs at 9600 baud");
        s.insert(zlog_keyer::SO2R_KEY.into(), "otrsp".into());
        assert!(setup(&s).speed_pot);
        assert_eq!(baud(&s), 1200);
    }

    #[test]
    fn old_settings_set_up_the_keyer() {
        let mut s = Section::new();
        for (k, v) in [("sidetone", "false"), ("paddle_reverse", "true")] {
            s.insert(k.into(), v.into());
        }
        s.insert("ptt_before".into(), "0".into());
        let s = zlog_keyer::with_timing(&s, Timing::default());
        let got = setup(&s);
        assert_eq!(
            got,
            Setup {
                wpm: Timing::default().wpm,
                paddle_swap: true,
                sidetone: false,
                ptt_before_ms: 1,
                ptt_after_ms: 1,
                speed_pot: true,
            }
        );
        neo_skips_the_pot(&s);
        let keys: Vec<_> = WinKeyerDrv::default()
            .settings("en")
            .iter()
            .map(|f| f.key)
            .collect();
        assert_eq!(keys, ["port", "sidetone", "paddle_reverse"]);
    }

    #[test]
    fn the_port_is_shared_while_the_keyer_is_open() {
        let port = zlog_serial::Emulated::new(Answering::default());
        let hub = PortHub::new();
        let mut k = open_on(Box::new(port.clone()), &Section::new(), &hub).unwrap();
        port.with(|p| p.written.clear());
        let mut other = hub.open(KEYER_PORT).expect("published");
        other.write(&[0x94, 0x95]).unwrap();
        k.send("E");
        assert_eq!(port.with(|p| p.written.clone()), [0x94, 0x95, b'E']);
        drop(k);
        assert!(hub.open(KEYER_PORT).is_none());
        let w = port.with(|p| p.written.clone());
        assert!(w.ends_with(&[PTT, 0, ADMIN, ADMIN_CLOSE]));
    }

    #[test]
    fn a_keyer_that_does_not_answer_is_not_published() {
        let (mute, _peer) = zlog_serial::Loop::pair();
        let hub = PortHub::new();
        assert!(open_on(Box::new(mute), &Section::new(), &hub).is_err());
        assert!(hub.names().is_empty());
    }

    #[test]
    fn the_simulator_port_opens_the_emulator() {
        let drv = WinKeyerDrv::default();
        let SettingKind::Port(names) = &drv.settings("en")[0].kind else {
            panic!("port field");
        };
        assert_eq!(names[0], SIMULATOR);
        assert!(drv.status("en").is_none(), "no simulator yet");
        let mut s = Section::new();
        s.insert("port".into(), SIMULATOR.into());
        s.insert("sidetone".into(), "false".into());
        let hub = PortHub::new();
        let mut k = KeyerDrv::open(&drv, &s, &hub).unwrap();
        let sim = drv.sim.lock().unwrap().clone().expect("emulator kept");
        assert!(sim.state().open);
        assert_eq!(sim.state().sidetone, 0x86);
        assert!(hub.open(KEYER_PORT).is_some());
        k.send("E");
        assert_eq!(sim.state().buffer, "E");
        let start = std::time::Instant::now();
        while !k.poll().contains(&KeyerEvent::Finished) {
            assert!(start.elapsed() < Duration::from_secs(3), "never finished");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sim.state().sent, "E");
        let status = drv.status("en").unwrap();
        assert!(
            status.starts_with("Opened by zLog ON, Speed (WPM) "),
            "{status}"
        );
        assert!(status.ends_with(", Side tone OFF, Sent: E"), "{status}");
        drop(k);
        assert!(!sim.state().open);
        assert!(hub.open(KEYER_PORT).is_none());
        s.insert("port".into(), "/dev/zlog-no-such-port".into());
        assert!(KeyerDrv::open(&drv, &s, &hub).is_err());
        assert!(drv.status("en").is_none(), "a real port shows no simulator");
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
