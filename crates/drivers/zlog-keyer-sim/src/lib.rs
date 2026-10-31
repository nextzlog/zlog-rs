use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use haml::sim::SimKeyer;
use zlog_keyer::{timing_of, Keyer, KeyerDrv};
use zlog_ui::device::{DeviceDriver, PortHub};
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
    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
        let mut k = SimKeyer::new(timing_of(settings));
        k.sent = self.sent.clone();
        k.ptt = self.ptt.clone();
        Ok(Box::new(k))
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.driver::<dyn zlog_keyer::KeyerDrv>(40, || Box::<SimKeyerDrv>::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use zlog_keyer::morse::{self, Timing};
    use zlog_keyer::KeyerEvent;

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
