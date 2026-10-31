use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use std::sync::mpsc::Receiver;

use zlog_ui::tx::{ticket, Signal, TxCommand, TxEvent, TxHub};

use crate::macros::Fields;
use crate::{Keyer, KeyerEvent, Timing};

pub(crate) struct Shared {
    pub(crate) tx: TxHub,
    events: Receiver<TxEvent>,
    pub(crate) radio: u8,
    phone: Option<u64>,
    pub(crate) status: String,
    pub(crate) pot_wpm: Option<u32>,
    pub(crate) fields: Fields,
}

impl Default for Shared {
    fn default() -> Self {
        Shared::on(TxHub::default())
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        if self.configured() {
            self.tx.set_keyer(None);
        }
    }
}

impl Shared {
    pub(crate) fn on(tx: TxHub) -> Self {
        Shared {
            events: tx.subscribe(),
            tx,
            radio: 1,
            phone: None,
            status: String::new(),
            pot_wpm: None,
            fields: Fields::default(),
        }
    }

    pub(crate) fn install(&mut self, keyer: Option<Box<dyn Keyer>>) {
        self.tx.set_keyer(keyer);
        self.events.try_iter().count();
    }

    pub(crate) fn configured(&self) -> bool {
        self.tx.has_keyer()
    }

    pub(crate) fn send(&mut self, text: &str) -> bool {
        if !self.configured() {
            return false;
        }
        self.tx.send(TxCommand::Send {
            id: ticket(),
            radio: self.radio,
            signal: Signal::Cw(text.into()),
        });
        true
    }

    pub(crate) fn abort(&mut self) {
        if self.configured() {
            self.tx.send(TxCommand::Stop);
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.configured() && self.tx.busy()
    }

    pub(crate) fn set_wpm(&mut self, wpm: u32) {
        self.tx.send(TxCommand::SetWpm(wpm));
    }

    pub(crate) fn set_timing(&mut self, timing: Timing) {
        self.tx.set_timing(timing);
    }

    pub(crate) fn phone(&mut self, on: bool) -> bool {
        if !self.configured() {
            return false;
        }
        match (on, self.phone) {
            (true, None) => {
                let id = ticket();
                self.phone = Some(id);
                self.tx.send(TxCommand::Send {
                    id,
                    radio: self.radio,
                    signal: Signal::Voice,
                });
            }
            (false, Some(id)) => {
                self.phone = None;
                self.tx.send(TxCommand::Release(id));
            }
            _ => {}
        }
        true
    }

    pub(crate) fn pump(&mut self) -> bool {
        if !self.configured() {
            return false;
        }
        self.tx.tick();
        let mut paddle = false;
        for e in self.events.try_iter() {
            match e {
                TxEvent::Keyer(KeyerEvent::Speed(w)) => self.pot_wpm = Some(w),
                TxEvent::Keyer(KeyerEvent::Paddle(on)) => paddle |= on,
                TxEvent::Keyer(KeyerEvent::Error(e)) | TxEvent::Error(e) => self.status = e,
                TxEvent::Stopped => self.phone = None,
                _ => {}
            }
        }
        if paddle {
            self.tx.send(TxCommand::Stop);
        }
        paddle
    }
}

pub(crate) type Opened = Result<Box<dyn Keyer>, String>;

pub(crate) fn cq_loop(
    shared: Arc<Mutex<Shared>>,
    text: impl Fn(&Shared) -> String,
    interval: Duration,
    max: u32,
    stop: Arc<AtomicBool>,
) {
    let halted = || stop.load(Ordering::SeqCst) || Arc::strong_count(&stop) == 1;
    let pause = |len: Duration| {
        let mut left = len;
        while !left.is_zero() && !halted() {
            let d = left.min(Duration::from_millis(20));
            thread::sleep(d);
            left -= d;
        }
    };
    for _ in 0..max {
        if halted() {
            break;
        }
        let mut s = shared.lock().unwrap();
        if halted() {
            break;
        }
        let text = text(&s);
        if !s.send(&text) {
            break;
        }
        drop(s);
        loop {
            pause(Duration::from_millis(20));
            let mut s = shared.lock().unwrap();
            if s.pump() {
                stop.store(true, Ordering::SeqCst);
            }
            let busy = s.busy();
            if halted() || !busy {
                break;
            }
        }
        pause(interval);
    }
    stop.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{calls, Recorder};

    #[test]
    fn cq_repeat_sends_up_to_max() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let r = Recorder::default();
        shared.lock().unwrap().install(Some(Box::new(r.clone())));
        let stop = Arc::new(AtomicBool::new(false));
        cq_loop(
            shared.clone(),
            |_| "E".into(),
            Duration::ZERO,
            3,
            stop.clone(),
        );
        assert!(stop.load(Ordering::SeqCst));
        assert_eq!(calls(&r), ["send E", "send E", "send E"]);
    }

    #[test]
    fn cq_repeat_stopped_while_waiting_for_the_lock_sends_nothing() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let r = Recorder::default();
        shared.lock().unwrap().install(Some(Box::new(r.clone())));
        let stop = Arc::new(AtomicBool::new(false));
        let guard = shared.lock().unwrap();
        let t = {
            let (shared, stop) = (shared.clone(), stop.clone());
            thread::spawn(move || cq_loop(shared, |_| "E".into(), Duration::ZERO, 3, stop))
        };
        thread::sleep(Duration::from_millis(100));
        stop.store(true, Ordering::SeqCst);
        drop(guard);
        t.join().unwrap();
        assert!(calls(&r).is_empty());
    }

    #[test]
    fn cq_repeat_ends_when_nobody_can_stop_it() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let r = Recorder::default();
        shared.lock().unwrap().install(Some(Box::new(r.clone())));
        cq_loop(
            shared,
            |_| "E".into(),
            Duration::ZERO,
            3,
            Arc::new(AtomicBool::new(false)),
        );
        assert!(calls(&r).is_empty());
    }

    #[test]
    fn paddle_without_a_keyer() {
        let mut s = Shared::default();
        assert!(!s.pump());
    }

    #[test]
    fn cq_repeat_makes_the_message_again_each_time() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let r = Recorder::default();
        shared.lock().unwrap().install(Some(Box::new(r.clone())));
        shared.lock().unwrap().fields.callsign = "JA1ZLO".into();
        let n = std::sync::atomic::AtomicU32::new(0);
        let make = |s: &Shared| {
            let i = n.fetch_add(1, Ordering::SeqCst);
            format!("{} {i}", s.fields.callsign)
        };
        let stop = Arc::new(AtomicBool::new(false));
        cq_loop(shared, make, Duration::ZERO, 3, stop.clone());
        assert_eq!(
            calls(&r),
            ["send JA1ZLO 0", "send JA1ZLO 1", "send JA1ZLO 2"]
        );
    }

    #[test]
    fn cq_repeat_without_a_keyer_or_stopped() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        cq_loop(
            shared.clone(),
            |_| "E".into(),
            Duration::ZERO,
            3,
            stop.clone(),
        );
        assert!(stop.load(Ordering::SeqCst));
        let r = Recorder::default();
        shared.lock().unwrap().install(Some(Box::new(r.clone())));
        cq_loop(shared, |_| "E".into(), Duration::ZERO, 3, stop);
        assert!(calls(&r).is_empty());
    }
}
