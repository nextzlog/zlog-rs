use std::io;
use std::sync::{Arc, Mutex};

use haml::hid::{Backend, Hid, HidHub};
use haml::keyer::if4cw::report::{MIC, PID, RIG, VID};
use haml::keyer::if4cw::{If4cwKeyer, RigSelect, Setup, Usb};
use haml::ports::Scanner;
use haml::sim::if4cw::PRODUCT;
use haml::sim::If4cwSim;
use zlog_keyer::driver::{get, text};
use zlog_keyer::{ptt_delay, timing_of, Keyer, KeyerCaps, KeyerDrv, KeyerEvent, Timing};
use zlog_serial::{Shared, Transport, SIMULATOR};
use zlog_so2r::{So2r, So2rDrv};
use zlog_ui::device::{DeviceDriver, PortHub, SettingField, SettingKind};
use zlog_ui::Section;

zlog_core::texts!();

pub const PORT: &str = "usbif4cw";

struct BitsPort(Usb);

impl Transport for BitsPort {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        for [mask, active] in bytes.as_chunks::<2>().0 {
            self.0.set(*mask, *active != 0);
        }
        Ok(())
    }

    fn read(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Ok(0)
    }
}

pub fn setup(s: &Section) -> Setup {
    Setup {
        ptt: get(s, "ptt", false),
        ptt_before_ms: ptt_delay(get(s, "ptt_before", 25)),
        ptt_after_ms: ptt_delay(get(s, "ptt_after", 0)),
        paddle_reverse: get(s, "paddle_reverse", false),
        sync_wpm: get(s, "sync_wpm", false),
        gen3_mic: get(s, "gen3_mic", false),
    }
}

struct Published {
    keyer: If4cwKeyer,
    ports: PortHub,
}

impl Drop for Published {
    fn drop(&mut self) {
        self.ports.withdraw(PORT);
    }
}

impl Keyer for Published {
    fn caps(&self) -> KeyerCaps {
        self.keyer.caps()
    }
    fn send(&mut self, text: &str) {
        self.keyer.send(text);
    }
    fn abort(&mut self) {
        self.keyer.abort();
    }
    fn set_wpm(&mut self, wpm: u32) {
        self.keyer.set_wpm(wpm);
    }
    fn wpm(&self) -> u32 {
        self.keyer.wpm()
    }
    fn set_timing(&mut self, timing: Timing) {
        self.keyer.set_timing(timing);
    }
    fn set_ptt(&mut self, on: bool) {
        self.keyer.set_ptt(on);
    }
    fn set_phone_ptt(&mut self, on: bool) {
        self.keyer.set_phone_ptt(on);
    }
    fn busy(&self) -> bool {
        self.keyer.busy()
    }
    fn poll(&mut self) -> Vec<KeyerEvent> {
        self.keyer.poll()
    }
}

pub fn open_keyer(usb: Usb, setup: Setup, timing: Timing, ports: &PortHub) -> Box<dyn Keyer> {
    let keyer = If4cwKeyer::open(usb.clone(), setup, timing);
    ports.publish(PORT, Box::new(BitsPort(usb)));
    Box::new(Published {
        keyer,
        ports: ports.clone(),
    })
}

#[derive(Clone)]
pub struct Hids {
    hub: HidHub,
    found: Arc<Mutex<Vec<String>>>,
}

impl Default for Hids {
    fn default() -> Self {
        static REAL: std::sync::OnceLock<Hids> = std::sync::OnceLock::new();
        REAL.get_or_init(|| Hids::new(HidHub::native(VID, PID)))
            .clone()
    }
}

impl Hids {
    pub fn new(hub: HidHub) -> Self {
        Hids {
            hub,
            found: Arc::default(),
        }
    }

    pub fn spawn(backend: impl Backend) -> Self {
        Hids::new(HidHub::spawn(backend))
    }

    pub fn refresh(&self) {
        let (mut hub, found) = (self.hub.clone(), self.found.clone());
        std::thread::spawn(move || {
            if let Ok(list) = hub.scan() {
                *found.lock().unwrap_or_else(|e| e.into_inner()) = haml::hid::listed(&list);
                zlog_core::wake::wake();
            }
        });
    }

    pub fn found(&self) -> Vec<String> {
        self.found.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn open(&self, path: &str) -> Result<(Box<dyn Hid>, String), String> {
        self.hub.open(path)
    }
}

pub type Slot = Arc<Mutex<Option<If4cwSim>>>;

pub fn open(path: &str, slot: &Slot, hids: &Hids) -> Result<Usb, String> {
    if path == SIMULATOR {
        let mut s = slot.lock().unwrap_or_else(|e| e.into_inner());
        let sim = s.get_or_insert_with(If4cwSim::new);
        return Ok(Usb::new(Box::new(sim.hid()), PRODUCT));
    }
    let (hid, product) = hids.open(path)?;
    Ok(Usb::new(hid, &product))
}

fn device_field(language: &str, hids: &Hids) -> SettingField {
    hids.refresh();
    let mut names = vec![SIMULATOR.to_string()];
    names.extend(hids.found());
    SettingField::new(
        "device",
        texts().get(language, "device"),
        SettingKind::Port(names),
    )
}

#[derive(Default)]
pub struct If4cwDrv {
    pub sim: Slot,
    pub hub: Hids,
}

impl DeviceDriver for If4cwDrv {
    fn id(&self) -> &str {
        "usbif4cw"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        let t = |k| texts().get(language, k).to_string();
        let bool = |key, label: String| {
            SettingField::new(key, label, SettingKind::Bool).default_value("false")
        };
        let mut f = vec![device_field(language, &self.hub), bool("ptt", t("ptt"))];
        f.push(bool("paddle_reverse", text(language, "paddle_reverse")));
        f.push(bool("sync_wpm", t("sync_wpm")));
        f.push(bool("gen3_mic", t("gen3_mic")));
        f
    }

    fn status(&self, language: &str) -> Option<String> {
        let sim = self.sim.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
        let t = |k| texts().get(language, k);
        let on_off = |on| t(if on { "sim.on" } else { "sim.off" });
        let s = sim.state();
        let wpm = s.paddle_wpm.map_or_else(|| "-".into(), |w| w.to_string());
        Some(format!(
            "{} {}, {} {}, {} {}, {} RIG{}, {} RIG{}, {} {wpm}, {}: {}",
            t("sim.host"),
            on_off(s.attached > 0),
            t("sim.key"),
            on_off(s.key()),
            t("sim.ptt"),
            on_off(s.ptt()),
            t("sim.rig"),
            s.rig(),
            t("sim.mic"),
            s.mic(),
            t("sim.wpm"),
            t("sim.keyed"),
            s.keyed.trim_end(),
        ))
    }
}

impl KeyerDrv for If4cwDrv {
    fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
        let usb = open(
            settings.get("device").map_or("", |s| s.as_str()),
            &self.sim,
            &self.hub,
        )?;
        Ok(open_keyer(usb, setup(settings), timing_of(settings), ports))
    }
}

struct OnKeyer {
    port: Shared,
    mic: bool,
}

impl So2r for OnKeyer {
    fn caps(&self) -> zlog_so2r::So2rCaps {
        zlog_so2r::So2rCaps::default()
    }

    fn tx(&mut self, rig: u8) {
        let active = u8::from(rig == 2);
        let mut bytes = vec![RIG, active];
        if self.mic {
            bytes.extend([MIC, active]);
        }
        let _ = self.port.write(&bytes);
    }

    fn rx(&mut self, _rx: zlog_so2r::Rx) {}
}

#[derive(Default)]
pub struct RigSelectDrv {
    pub sim: Slot,
    pub hub: Hids,
}

impl DeviceDriver for RigSelectDrv {
    fn id(&self) -> &str {
        "usbif4cw"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "so2r").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        vec![
            device_field(language, &self.hub),
            SettingField::new(
                "mic_follows",
                texts().get(language, "mic_follows"),
                SettingKind::Bool,
            )
            .default_value("true"),
        ]
    }
}

impl So2rDrv for RigSelectDrv {
    fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn So2r>, String> {
        let mic = get(settings, "mic_follows", true);
        if let Some(port) = ports.open(PORT) {
            return Ok(Box::new(OnKeyer { port, mic }));
        }
        let usb = open(
            settings.get("device").map_or("", |s| s.as_str()),
            &self.sim,
            &self.hub,
        )?;
        Ok(Box::new(RigSelect::new(usb, mic)))
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    let sim = Slot::default();
    let hub = Hids::default();
    hub.refresh();
    let (keyer, box_) = (sim.clone(), sim.clone());
    let (keyer_hub, box_hub) = (hub.clone(), hub);
    b.driver::<dyn zlog_keyer::KeyerDrv>(30, move || {
        Box::new(If4cwDrv {
            sim: keyer.clone(),
            hub: keyer_hub.clone(),
        })
    });
    b.driver::<dyn zlog_so2r::So2rDrv>(30, move || {
        Box::new(RigSelectDrv {
            sim: box_.clone(),
            hub: box_hub.clone(),
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use haml::keyer::if4cw::report;
    use haml::ports::{PortInfo, Scan};
    use std::collections::VecDeque;
    use std::time::Duration;

    #[derive(Clone, Default)]
    struct FakeHid {
        written: Arc<Mutex<Vec<[u8; 9]>>>,
        input: Arc<Mutex<VecDeque<Vec<u8>>>>,
    }

    impl Hid for FakeHid {
        fn write(&mut self, report: &[u8]) -> io::Result<()> {
            self.written
                .lock()
                .unwrap()
                .push(report.try_into().unwrap());
            Ok(())
        }
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.input.lock().unwrap().pop_front() {
                Some(r) => {
                    buf[..r.len()].copy_from_slice(&r);
                    Ok(r.len())
                }
                None => Ok(0),
            }
        }
    }

    impl FakeHid {
        fn take(&self) -> Vec<[u8; 9]> {
            std::mem::take(&mut self.written.lock().unwrap())
        }
    }

    fn port(bits: u8) -> [u8; 9] {
        [0, 1, bits, 0, 0, 0, 0, 0, 0]
    }

    fn keyer(product: &str, setup: Setup) -> (FakeHid, Box<dyn Keyer>, PortHub) {
        let hid = FakeHid::default();
        let usb = Usb::new(Box::new(hid.clone()), product);
        let hub = PortHub::new();
        let timing = Timing {
            wpm: 30,
            ..Timing::default()
        };
        (hid.clone(), open_keyer(usb, setup, timing, &hub), hub)
    }

    fn setup() -> Setup {
        super::setup(&Section::new())
    }

    #[test]
    fn opening_a_version_2_device_sends_zlogs_bytes() {
        let s = Setup {
            ptt: true,
            ptt_before_ms: 30,
            ptt_after_ms: 10,
            paddle_reverse: true,
            ..setup()
        };
        let (hid, _k, _hub) = keyer("USBIF4CW Ver.2.3", s);
        assert_eq!(
            hid.take(),
            [
                port(0xFF),
                [0, 0x04, 0x0F, 0x04, 0, 0, 0, 0, 0],
                [0, 0x26, 0, 0, 1, 0, 0, 0, 0xFC],
                [0, 0x24, 20, 0, 0, 0, 0, 0, 0xFC],
                [0, 0x22, 30, 50, 0, 0, 0, 0, 0],
                [0, 0x25, 1, 0, 0, 0, 0, 0, 0],
            ]
        );
    }

    #[test]
    fn an_old_device_gets_only_the_port_bits() {
        let (hid, _k, _hub) = keyer("USBIF4CW Ver.1.5", setup());
        assert_eq!(hid.take(), [port(0xFF), report::QUERY]);
        let (hid, _k, _hub) = keyer("USBIF4CW", setup());
        assert_eq!(hid.take().len(), 2);
    }

    #[test]
    fn keying_clears_the_key_bit_and_sends_only_changes() {
        let (hid, mut k, _hub) = keyer("Ver.2.0", setup());
        hid.take();
        k.send("E");
        let start = std::time::Instant::now();
        while k.busy() {
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(hid.take(), [port(0xFE), port(0xFF)]);
        k.set_ptt(true);
        k.set_phone_ptt(true);
        k.set_phone_ptt(true);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(hid.take(), [port(0xFD)]);
    }

    #[test]
    fn the_paddle_speed_follows_the_keyer_when_asked() {
        let (hid, mut k, _hub) = keyer("Ver.2.0", setup());
        hid.take();
        k.set_wpm(50);
        assert!(hid.take().is_empty());
        let s = Setup {
            sync_wpm: true,
            ..setup()
        };
        let (hid, mut k, _hub) = keyer("Ver.2.0", s);
        hid.take();
        k.set_wpm(50);
        assert_eq!(hid.take(), [[0, 0x24, 12, 0, 0, 0, 0, 0, 0xFC]]);
    }

    #[test]
    fn gen3_mic_select_follows_phone_ptt() {
        let s = Setup {
            gen3_mic: true,
            ..setup()
        };
        let (hid, mut k, _hub) = keyer("Ver.2.0", s);
        hid.take();
        k.set_phone_ptt(true);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(hid.take(), [port(0xF7), port(0xF5)]);
    }

    #[test]
    fn the_paddle_stops_sending_from_version_2() {
        let (hid, mut k, _hub) = keyer("Ver.2.0", setup());
        hid.take();
        let answer = |b: u8| vec![0x04, b, 0x04, 0, 0, 0, 0, 0];
        hid.input.lock().unwrap().extend([
            answer(0x0F),
            answer(0x07),
            answer(0x06),
            vec![0x01, 0x02, 0x00, 0, 0, 0, 0, 0],
            answer(0x0F),
            answer(0x0D),
        ]);
        assert_eq!(
            k.poll(),
            [KeyerEvent::Paddle(true), KeyerEvent::Paddle(false)]
        );
        assert_eq!(hid.take(), [report::QUERY], "asked again for each poll");
        hid.input.lock().unwrap().push_back(answer(0x00));
        assert_eq!(k.poll(), [KeyerEvent::Paddle(true)]);
        assert_eq!(hid.take(), [report::QUERY]);
        let (hid, mut k, _hub) = keyer("Ver.1.0", setup());
        hid.take();
        hid.input.lock().unwrap().push_back(answer(0x06));
        assert!(k.poll().is_empty());
        assert!(hid.take().is_empty());
    }

    #[test]
    fn the_rig_bit_goes_through_the_keyers_device() {
        let (hid, k, hub) = keyer("Ver.2.0", setup());
        hid.take();
        let mut s = Section::new();
        let mut b = RigSelectDrv::default().open(&s, &hub).unwrap();
        b.select_rig(2);
        b.select_rig(1);
        assert_eq!(hid.take(), [port(0xFB), port(0xF3), port(0xF7), port(0xFF)]);
        s.insert("mic_follows".into(), "false".into());
        let mut b = RigSelectDrv::default().open(&s, &hub).unwrap();
        b.select_rig(2);
        assert_eq!(hid.take(), [port(0xFB)]);
        drop(k);
        assert!(hub.open(PORT).is_none(), "withdrawn with the keyer");
    }

    #[test]
    fn old_style_settings_and_fields() {
        let mut s = Section::new();
        for (k, v) in [("ptt", "true"), ("ptt_before", "300"), ("gen3_mic", "true")] {
            s.insert(k.into(), v.into());
        }
        let got = super::setup(&s);
        assert!(got.ptt && got.gen3_mic && !got.sync_wpm);
        assert_eq!((got.ptt_before_ms, got.ptt_after_ms), (255, 1));
        let keys: Vec<_> = If4cwDrv::default()
            .settings("en")
            .iter()
            .map(|f| f.key)
            .collect();
        assert_eq!(
            keys,
            ["device", "ptt", "paddle_reverse", "sync_wpm", "gen3_mic"]
        );
        assert_eq!(RigSelectDrv::default().id(), If4cwDrv::default().id());
    }

    fn wait_idle(k: &mut dyn Keyer) -> Vec<KeyerEvent> {
        let start = std::time::Instant::now();
        let mut events = Vec::new();
        while k.busy() {
            assert!(start.elapsed() < Duration::from_secs(5), "never finished");
            events.extend(k.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        events.extend(k.poll());
        events
    }

    #[test]
    fn the_simulator_device_reads_back_what_was_keyed() {
        let drv = If4cwDrv::default();
        let SettingKind::Port(names) = &drv.settings("en")[0].kind else {
            panic!("device field");
        };
        assert_eq!(names[0], SIMULATOR);
        assert!(drv.status("en").is_none(), "no simulator yet");
        let mut s = Section::new();
        for (k, v) in [("device", SIMULATOR), ("speed", "30"), ("ptt", "true")] {
            s.insert(k.into(), v.into());
        }
        let hub = PortHub::new();
        let mut k = KeyerDrv::open(&drv, &s, &hub).unwrap();
        let sim = drv.sim.lock().unwrap().clone().expect("emulator kept");
        let st = sim.state();
        assert_eq!((st.attached, st.paddle_wpm), (1, Some(30)));
        assert_eq!(st.ptt_delays, Some((25, 50)));
        k.send("TEST");
        assert!(wait_idle(k.as_mut()).contains(&KeyerEvent::Finished));
        std::thread::sleep(Duration::from_millis(300));
        let st = sim.state();
        assert_eq!(st.keyed, "TEST ");
        assert!(!st.key() && !st.ptt());
        assert_eq!(
            drv.status("en").unwrap(),
            "Opened by zLog ON, Key OFF, PTT OFF, RIG select RIG1, MIC select RIG1, Paddle speed (WPM) 30, Keyed text: TEST"
        );
        let polls = |k: &mut dyn Keyer| (0..3).flat_map(|_| k.poll()).collect::<Vec<_>>();
        sim.press(true, false);
        assert_eq!(polls(k.as_mut()), [KeyerEvent::Paddle(true)]);
        sim.press(false, false);
        assert_eq!(polls(k.as_mut()), [KeyerEvent::Paddle(false)]);
        let mut b = So2rDrv::open(
            &RigSelectDrv {
                sim: drv.sim.clone(),
                hub: drv.hub.clone(),
            },
            &s,
            &hub,
        )
        .unwrap();
        b.select_rig(2);
        assert_eq!((sim.state().rig(), sim.state().mic()), (2, 2));
        drop(b);
        drop(k);
        let start = std::time::Instant::now();
        while sim.state().attached > 0 {
            assert!(start.elapsed() < Duration::from_secs(2), "the keyer let go");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut b = So2rDrv::open(
            &RigSelectDrv {
                sim: drv.sim.clone(),
                hub: drv.hub.clone(),
            },
            &s,
            &hub,
        )
        .unwrap();
        assert_eq!(sim.state().attached, 1, "a box of its own");
        b.select_rig(1);
        assert_eq!(sim.state().rig(), 1);
    }

    type Calls = Arc<Mutex<Vec<std::thread::ThreadId>>>;

    struct FakeBackend {
        calls: Calls,
        names: Arc<Mutex<Vec<String>>>,
        hid: FakeHid,
    }

    impl FakeBackend {
        fn note(&self) {
            self.calls.lock().unwrap().push(std::thread::current().id());
        }
    }

    impl Backend for FakeBackend {
        fn enumerate(&mut self) -> Scan {
            self.note();
            Ok(self
                .names
                .lock()
                .unwrap()
                .iter()
                .map(|n| PortInfo::named(n))
                .collect())
        }

        fn open(&mut self, _path: &str) -> Result<(Box<dyn Hid>, String), String> {
            self.note();
            Ok((Box::new(self.hid.clone()), "USBIF4CW Ver.2.3".into()))
        }
    }

    struct Fake {
        calls: Calls,
        names: Arc<Mutex<Vec<String>>>,
        hid: FakeHid,
        hub: Hids,
    }

    fn fake() -> Fake {
        let calls = Calls::default();
        let names = Arc::new(Mutex::new(vec!["/dev/hidraw7".to_string()]));
        let hid = FakeHid::default();
        let hub = Hids::spawn(FakeBackend {
            calls: calls.clone(),
            names: names.clone(),
            hid: hid.clone(),
        });
        Fake {
            calls,
            names,
            hid,
            hub,
        }
    }

    fn device_names(d: &dyn DeviceDriver) -> Vec<String> {
        let SettingKind::Port(names) = &d.settings("en")[0].kind else {
            panic!("device field");
        };
        names.clone()
    }

    fn until(what: &str, mut ok: impl FnMut() -> bool) {
        let start = std::time::Instant::now();
        while !ok() {
            assert!(start.elapsed() < Duration::from_secs(5), "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn settings_and_load_never_touch_hid_on_the_calling_thread() {
        let f = fake();
        let keyer = If4cwDrv {
            sim: Slot::default(),
            hub: f.hub.clone(),
        };
        let so2r = RigSelectDrv {
            sim: Slot::default(),
            hub: f.hub.clone(),
        };
        assert_eq!(device_names(&keyer), [SIMULATOR], "the cache, at once");
        let mut s = Section::new();
        s.insert("device".into(), "usbif4cw".into());
        zlog_ui::device::DriverChoice::load(&s, "none", &[&keyer, &so2r]);
        until("the worker refreshed", || device_names(&so2r).len() == 2);
        assert_eq!(device_names(&keyer), [SIMULATOR, "/dev/hidraw7"]);
        let calls = f.calls.lock().unwrap().clone();
        assert!(!calls.is_empty());
        assert!(calls.iter().all(|t| *t != std::thread::current().id()));
    }

    #[test]
    fn the_cached_list_follows_the_worker() {
        let f = fake();
        let drv = If4cwDrv {
            sim: Slot::default(),
            hub: f.hub.clone(),
        };
        drv.settings("en");
        until("first list", || device_names(&drv).len() == 2);
        f.names.lock().unwrap().push("/dev/hidraw8".into());
        until("second list", || device_names(&drv).len() == 3);
        f.names.lock().unwrap().clear();
        until("unplugged", || device_names(&drv) == [SIMULATOR]);
    }

    #[test]
    fn a_real_device_is_driven_from_the_worker() {
        let f = fake();
        let drv = If4cwDrv {
            sim: Slot::default(),
            hub: f.hub.clone(),
        };
        let mut s = Section::new();
        s.insert("device".into(), "/dev/hidraw7".into());
        let hub = PortHub::new();
        let mut k = KeyerDrv::open(&drv, &s, &hub).unwrap();
        assert_eq!(f.hid.take()[0], port(0xFF));
        f.hid
            .input
            .lock()
            .unwrap()
            .push_back(vec![0x04, 0x00, 0x04]);
        assert_eq!(k.poll(), [KeyerEvent::Paddle(true)]);
        let calls = f.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_ne!(calls[0], std::thread::current().id());
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
