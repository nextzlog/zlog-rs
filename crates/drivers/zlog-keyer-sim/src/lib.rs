use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use haml::sim::SimKeyer;
use zlog_audio::SoundDrv;
use zlog_keyer::morse::{self, Timing};
use zlog_keyer::sidetone::{self, Sidetone};
use zlog_keyer::{timing_of, Keyer, KeyerCaps, KeyerDrv, KeyerEvent};
use zlog_ui::device::{AudioDrv, AudioIo, DeviceDriver, DriverChoice, PortHub};
use zlog_ui::Section;

zlog_core::texts!();

#[derive(Default)]
pub struct SimKeyerDrv {
    pub sent: Arc<Mutex<Vec<String>>>,
    ptt: Arc<AtomicBool>,
}

impl DeviceDriver for SimKeyerDrv {
    fn id(&self) -> &str {
        "simulated"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn simulated(&self) -> bool {
        true
    }

    fn status(&self, _language: &str) -> Option<String> {
        let sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        let ptt = if self.ptt.load(Ordering::SeqCst) {
            "ON"
        } else {
            "OFF"
        };
        Some(format!(
            "PTT {ptt}, sent: {}",
            sent.last().map_or("", |s| s)
        ))
    }
}

impl KeyerDrv for SimKeyerDrv {
    fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
        let timing = timing_of(settings);
        let mut k = SimKeyer::new(timing);
        k.sent = self.sent.clone();
        k.ptt = self.ptt.clone();
        let tone = Sidetone::load(settings);
        let (io, note) = match tone.on.then(|| output(settings, ports)) {
            None => (None, None),
            Some(Ok(io)) => (Some(io), None),
            Some(Err(e)) => (None, Some(e)),
        };
        Ok(Box::new(SidetoneKeyer {
            inner: k,
            timing,
            tone,
            io,
            note,
        }))
    }
}

fn output(settings: &Section, ports: &PortHub) -> Result<Box<dyn AudioIo>, String> {
    let sound: Section = settings
        .iter()
        .filter_map(|(k, v)| {
            let k = k.strip_prefix(zlog_keyer::SOUND_PREFIX)?;
            Some((k.to_string(), v.clone()))
        })
        .collect();
    let drv = SoundDrv;
    let choice = DriverChoice::load(&sound, drv.id(), &[&drv]);
    if choice.id != drv.id() {
        return Err(sidetone::NO_DEVICE.into());
    }
    drv.open(&choice.section(), ports)
        .map_err(|e| sidetone::failed(&e))
}

pub struct SidetoneKeyer {
    pub inner: SimKeyer,
    timing: Timing,
    tone: Sidetone,
    io: Option<Box<dyn AudioIo>>,
    note: Option<String>,
}

impl SidetoneKeyer {
    pub fn new(inner: SimKeyer, timing: Timing, tone: Sidetone, io: Box<dyn AudioIo>) -> Self {
        SidetoneKeyer {
            inner,
            timing,
            tone,
            io: Some(io),
            note: None,
        }
    }
}

impl SidetoneKeyer {
    fn fade(&mut self) {
        if let Some(io) = self.io.as_mut() {
            let n = io.sample_rate() as f64 * sidetone::FADE_MS / 1000.0;
            io.fade_out(n.round() as usize);
        }
    }
}

impl Keyer for SidetoneKeyer {
    fn caps(&self) -> KeyerCaps {
        self.inner.caps()
    }

    fn send(&mut self, text: &str) {
        if let Some(io) = self.io.as_mut().filter(|_| self.tone.on) {
            io.play(&self.tone.samples(text, &self.timing, io.sample_rate()));
        }
        self.inner.send(text);
    }

    fn abort(&mut self) {
        self.fade();
        self.inner.abort();
    }

    fn set_wpm(&mut self, wpm: u32) {
        self.timing.wpm = wpm.clamp(morse::MIN_WPM, morse::MAX_WPM);
        self.inner.set_wpm(wpm);
    }

    fn wpm(&self) -> u32 {
        self.inner.wpm()
    }

    fn set_timing(&mut self, timing: Timing) {
        self.timing = timing;
        self.timing.wpm = timing.wpm.clamp(morse::MIN_WPM, morse::MAX_WPM);
        self.inner.set_timing(timing);
    }

    fn set_ptt(&mut self, on: bool) {
        self.inner.set_ptt(on);
    }

    fn busy(&self) -> bool {
        self.inner.busy()
    }

    fn poll(&mut self) -> Vec<KeyerEvent> {
        let mut events = self.inner.poll();
        events.extend(self.note.take().map(KeyerEvent::Error));
        events
    }
}

impl Drop for SidetoneKeyer {
    fn drop(&mut self) {
        self.fade();
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.driver::<dyn zlog_keyer::KeyerDrv>(40, || Box::<SimKeyerDrv>::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<f32>>>, Arc<Mutex<Vec<f32>>>);

    impl AudioIo for Buffer {
        fn sample_rate(&self) -> u32 {
            8000
        }
        fn play(&mut self, samples: &[f32]) {
            self.0.lock().unwrap().extend(samples);
        }
        fn stop(&mut self) {
            self.0.lock().unwrap().clear();
        }
        fn fade_out(&mut self, samples: usize) {
            let mut q: std::collections::VecDeque<f32> = self.0.lock().unwrap().drain(..).collect();
            zlog_audio::fade(&mut q, samples);
            self.1.lock().unwrap().extend(q);
        }
        fn playing(&self) -> bool {
            !self.0.lock().unwrap().is_empty()
        }
        fn record(&mut self, _out: &mut Vec<f32>) {}
    }

    #[test]
    fn the_sidetone_follows_the_keyer_and_stops_on_abort() {
        let b = Buffer::default();
        let t = Timing {
            wpm: 30,
            ..Timing::default()
        };
        let tone = Sidetone::default();
        let mut k = SidetoneKeyer::new(SimKeyer::new(t), t, tone, Box::new(b.clone()));
        k.send("CQ");
        assert_eq!(*b.0.lock().unwrap(), tone.samples("CQ", &t, 8000));
        k.set_wpm(40);
        k.send("E");
        let fast = Timing { wpm: 40, ..t };
        let n = tone.samples("CQ", &t, 8000).len() + tone.samples("E", &fast, 8000).len();
        assert_eq!(b.0.lock().unwrap().len(), n);
        assert!(k.busy() && b.playing());
        k.abort();
        assert!(!k.busy() && !b.playing());
        let tail = b.1.lock().unwrap().clone();
        assert_eq!(tail.len(), 40);
        assert!(tail.last().unwrap().abs() < 1e-6);
        b.1.lock().unwrap().clear();
        assert_eq!(k.poll(), vec![KeyerEvent::Finished]);
        let mut off = SidetoneKeyer::new(
            SimKeyer::new(t),
            t,
            Sidetone { on: false, ..tone },
            Box::new(b.clone()),
        );
        off.send("CQ");
        assert!(!b.playing() && off.busy());
    }

    #[test]
    fn a_stop_in_the_middle_of_a_tone_fades_without_a_step() {
        let tone = Sidetone::default();
        let t = Timing::default();
        let x = tone.samples("T", &t, 8000);
        let a = tone.amplitude();
        let sine = a * std::f32::consts::TAU * tone.pitch as f32 / 8000.0;
        for cut in [x.len() / 2, x.len() / 2 + 3, x.len() / 3 + 1] {
            let b = Buffer::default();
            let mut k = SidetoneKeyer::new(SimKeyer::new(t), t, tone, Box::new(b.clone()));
            k.send("T");
            b.0.lock().unwrap().drain(..cut);
            let before = x[cut - 1];
            k.abort();
            let tail = b.1.lock().unwrap().clone();
            let mut all = vec![before];
            all.extend(&tail);
            all.push(0.0);
            let step = all
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0, f32::max);
            assert!(step <= sine * 1.05, "cut {cut}: {step} > {sine}");
            assert!(!b.playing());
        }
    }

    #[test]
    fn without_a_sound_device_the_keyer_works_silently_and_says_so_once() {
        zlog_audio::use_no_devices(true);
        let drv = SimKeyerDrv::default();
        let mut s = Section::new();
        s.insert("speed".into(), "60".into());
        let mut k = drv.open(&s, &PortHub::new()).unwrap();
        k.send("E");
        assert_eq!(
            k.poll(),
            vec![KeyerEvent::Error(sidetone::failed("no sound device"))]
        );
        assert!(k.poll().is_empty());
        s.insert("sound.device".into(), "loopback".into());
        let mut k = drv.open(&s, &PortHub::new()).unwrap();
        assert_eq!(
            k.poll(),
            vec![KeyerEvent::Error(sidetone::NO_DEVICE.into())]
        );
        s.insert(sidetone::ON.into(), "false".into());
        let mut k = drv.open(&s, &PortHub::new()).unwrap();
        assert!(k.poll().is_empty());
    }

    #[test]
    fn sim_finishes_after_abort() {
        let mut k = SimKeyer::new(Timing::default());
        k.send("CQ TEST");
        assert!(k.busy());
        assert!(k.poll().is_empty());
        k.abort();
        assert!(!k.busy());
        assert_eq!(k.poll(), vec![KeyerEvent::Finished]);
        assert!(k.poll().is_empty());
        assert_eq!(*k.sent.lock().unwrap(), ["CQ TEST"]);
        k.set_wpm(200);
        assert_eq!(k.wpm(), morse::MAX_WPM);
        k.set_ptt(true);
        assert!(k.ptt());
    }

    #[test]
    fn the_driver_opens_at_the_speed_of_the_panel_and_keeps_what_was_sent() {
        let drv = SimKeyerDrv::default();
        assert!(drv.simulated() && drv.settings("en").is_empty());
        assert_eq!(drv.title("ja"), "シミュレーション");
        let mut s = Section::new();
        s.insert("speed".into(), "32".into());
        s.insert(sidetone::ON.into(), "false".into());
        let mut k = drv.open(&s, &PortHub::new()).unwrap();
        assert_eq!(k.wpm(), 32);
        k.send("E");
        assert!(k.busy());
        let start = std::time::Instant::now();
        while k.poll().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(2), "never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!k.busy());
        assert_eq!(*drv.sent.lock().unwrap(), ["E"]);
        k.set_ptt(true);
        assert_eq!(drv.status("en").unwrap(), "PTT ON, sent: E");
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
