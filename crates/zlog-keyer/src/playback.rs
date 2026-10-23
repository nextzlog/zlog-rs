//! The keyer as the panel and the CQ repeat share it: opening the configured keyer,
//! reading its events and the CQ repeat thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::keyer::{Keyer, KeyerEvent, SimKeyer};
use crate::macros::Fields;
use crate::settings::{ptt_delay, Device, Settings};
use crate::{serial, winkeyer};

/// The keyer and what it last said, shared with the CQ repeat thread.
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) keyer: Option<Box<dyn Keyer>>,
    pub(crate) status: String,
    /// WPM from the keyer's speed knob, for the panel to take over.
    pub(crate) pot_wpm: Option<u32>,
    /// The macro values of the entry row as of the last frame, for the CQ repeat to
    /// expand its message again before each CQ.
    pub(crate) fields: Fields,
}

impl Shared {
    /// Read the keyer's events. The paddle stops the message being sent
    /// (`TMainForm.OnPaddle` calls `CQAbort`, main.pas); true when it was used, for
    /// the caller to stop the CQ repeat as well.
    pub(crate) fn pump(&mut self) -> bool {
        let Some(k) = self.keyer.as_mut() else {
            return false;
        };
        let mut paddle = false;
        for e in k.poll() {
            match e {
                KeyerEvent::Finished => {}
                KeyerEvent::Speed(w) => self.pot_wpm = Some(w),
                KeyerEvent::Paddle(on) => paddle |= on,
                KeyerEvent::Error(e) => self.status = e,
            }
        }
        if paddle {
            k.abort();
        }
        paddle
    }
}

pub(crate) type Opened = Result<Box<dyn Keyer>, String>;

/// Open the configured keyer (WinKeyer takes about a second).
pub(crate) fn open(s: &Settings) -> Opened {
    match s.device {
        Device::None => Err("no keyer".into()),
        Device::Simulated => Ok(Box::new(SimKeyer::new(s.timing))),
        Device::WinKeyer => {
            let port = winkeyer::open_port(&s.port)?;
            let setup = winkeyer::Setup {
                wpm: s.timing.wpm,
                paddle_swap: s.paddle_swap,
                sidetone: s.sidetone,
                ptt_before_ms: ptt_delay(s.ptt_before_ms),
                ptt_after_ms: ptt_delay(s.ptt_after_ms),
            };
            winkeyer::WinKeyer::open(port, &setup)
                .map(|k| Box::new(k) as Box<dyn Keyer>)
                .map_err(|e| e.to_string())
        }
        Device::Serial => {
            let port = serial::open_port(&s.port)?;
            let setup = serial::LineSetup {
                key: s.key_line,
                ptt: s.ptt,
                ptt_before: Duration::from_millis(ptt_delay(s.ptt_before_ms) as u64),
                ptt_after: Duration::from_millis(ptt_delay(s.ptt_after_ms) as u64),
            };
            Ok(Box::new(serial::LineKeyer::new(port, setup, s.timing)))
        }
    }
}

/// Send the message `text` makes up to `max` times, `interval` after each end, until
/// `stop` (`CQRepeatProc`, `StartCqRepeatTimer`). The message is made again before
/// each CQ, as zLog expands it on every play, so `$D`, `$G` and the entry row are
/// current.
pub(crate) fn cq_loop(
    shared: Arc<Mutex<Shared>>,
    text: impl Fn(&Shared) -> String,
    interval: Duration,
    max: u32,
    stop: Arc<AtomicBool>,
) {
    let pause = |len: Duration| {
        let mut left = len;
        while !left.is_zero() && !stop.load(Ordering::SeqCst) {
            let d = left.min(Duration::from_millis(20));
            thread::sleep(d);
            left -= d;
        }
    };
    for _ in 0..max {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let mut s = shared.lock().unwrap();
        // Esc may have stopped the loop while this thread waited for the lock.
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let text = text(&s);
        match s.keyer.as_mut() {
            Some(k) => k.send(&text),
            None => break,
        }
        drop(s);
        loop {
            pause(Duration::from_millis(20));
            let mut s = shared.lock().unwrap();
            if s.pump() {
                stop.store(true, Ordering::SeqCst);
            }
            let busy = s.keyer.as_ref().is_some_and(|k| k.busy());
            if stop.load(Ordering::SeqCst) || !busy {
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
    use crate::morse::Timing;
    use crate::tests::{calls, Recorder};

    #[test]
    fn cq_repeat_sends_up_to_max() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let fast = Timing {
            wpm: 60,
            ..Timing::default()
        };
        shared.lock().unwrap().keyer = Some(Box::new(SimKeyer::new(fast)));
        let stop = Arc::new(AtomicBool::new(false));
        cq_loop(
            shared.clone(),
            |_| "E".into(),
            Duration::ZERO,
            3,
            stop.clone(),
        );
        assert!(stop.load(Ordering::SeqCst));
        assert!(!shared.lock().unwrap().keyer.as_ref().unwrap().busy());
    }

    #[test]
    fn cq_repeat_stopped_while_waiting_for_the_lock_sends_nothing() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let r = Recorder::default();
        shared.lock().unwrap().keyer = Some(Box::new(r.clone()));
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
    fn paddle_without_a_keyer() {
        let mut s = Shared::default();
        assert!(!s.pump());
    }

    #[test]
    fn cq_repeat_makes_the_message_again_each_time() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let r = Recorder::default();
        shared.lock().unwrap().keyer = Some(Box::new(r.clone()));
        shared.lock().unwrap().fields.callsign = "JA1ZLO".into();
        let n = std::sync::atomic::AtomicU32::new(0);
        let make = |s: &Shared| {
            let i = n.fetch_add(1, Ordering::SeqCst);
            format!("{} {i}", s.fields.callsign)
        };
        let stop = Arc::new(AtomicBool::new(false));
        cq_loop(shared, make, Duration::ZERO, 3, stop);
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
        shared.lock().unwrap().keyer = Some(Box::new(r.clone()));
        cq_loop(shared, |_| "E".into(), Duration::ZERO, 3, stop);
        assert!(calls(&r).is_empty());
    }
}
