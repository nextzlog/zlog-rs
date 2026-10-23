//! The keyer interface (`TdmZLogKeyer`) and a simulated keyer.

use std::time::{Duration, Instant};

use crate::morse::{self, Timing};

/// Something a keyer reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyerEvent {
    /// Everything given has been sent (`OnSendFinishProc`).
    Finished,
    /// The speed knob of the keyer was turned (WinKeyer speed pot).
    Speed(u32),
    /// The paddle was used (WinKeyer break-in), true while active.
    Paddle(bool),
    /// The device failed; the keyer is unusable until reopened.
    Error(String),
}

/// A CW keyer. Every method returns at once; sending goes on in the device or on a
/// thread of the keyer.
pub trait Keyer: Send {
    /// Queue text in zLog's keyer alphabet (an expanded message, see
    /// [`crate::macros::expand`]) after what is being sent (`SendStrFIFO`).
    fn send(&mut self, text: &str);

    /// Stop sending and drop the queue (`ClrBuffer`, `WinKeyerClear`).
    fn abort(&mut self);

    fn set_wpm(&mut self, wpm: u32);

    fn wpm(&self) -> u32;

    /// Change the speed, weight and space factors of a connected keyer (zLog applies
    /// the settings to the running keyer). Keyers that time the Morse themselves
    /// (WinKeyer) take only the speed; this default does just that.
    fn set_timing(&mut self, timing: Timing) {
        self.set_wpm(timing.wpm);
    }

    /// Key the transmitter by hand (`ControlPTT`).
    fn set_ptt(&mut self, on: bool);

    /// Key the transmitter for phone (voice memories, zLog `PTTEnabledPH`): on
    /// the PTT line even when PTT is not used for CW. By default as
    /// [`Keyer::set_ptt`].
    fn set_phone_ptt(&mut self, on: bool) {
        self.set_ptt(on);
    }

    /// Write `bytes` for a device sharing the keyer's port (SO2R Neo on the
    /// WinKeyer port, zLog `So2rNeo*`); false when the keyer has no such port or
    /// the write failed. By default there is none.
    fn write_port(&mut self, bytes: &[u8]) -> bool {
        let _ = bytes;
        false
    }

    /// Whether something is being sent (`IsPlaying`).
    fn busy(&self) -> bool;

    /// Events since the last call; call it often (it also reads the device).
    fn poll(&mut self) -> Vec<KeyerEvent>;
}

/// A keyer without hardware: it takes as long as the Morse would and records what it
/// was given.
pub struct SimKeyer {
    timing: Timing,
    until: Option<Instant>,
    ptt: bool,
    /// Every text given to [`Keyer::send`], in order.
    pub sent: Vec<String>,
}

impl SimKeyer {
    pub fn new(timing: Timing) -> Self {
        SimKeyer {
            timing,
            until: None,
            ptt: false,
            sent: Vec::new(),
        }
    }

    pub fn ptt(&self) -> bool {
        self.ptt
    }
}

impl Keyer for SimKeyer {
    fn send(&mut self, text: &str) {
        let len: Duration = morse::total(&morse::keying(text, &self.timing));
        let from = self.until.filter(|t| *t > Instant::now());
        self.until = Some(from.unwrap_or_else(Instant::now) + len);
        self.sent.push(text.to_string());
    }

    fn abort(&mut self) {
        self.until = Some(Instant::now());
    }

    fn set_wpm(&mut self, wpm: u32) {
        self.timing.wpm = wpm.clamp(morse::MIN_WPM, morse::MAX_WPM);
    }

    fn wpm(&self) -> u32 {
        self.timing.wpm
    }

    fn set_timing(&mut self, timing: Timing) {
        self.timing = timing;
        self.set_wpm(timing.wpm);
    }

    fn set_ptt(&mut self, on: bool) {
        self.ptt = on;
    }

    fn busy(&self) -> bool {
        self.until.is_some_and(|t| t > Instant::now())
    }

    fn poll(&mut self) -> Vec<KeyerEvent> {
        match self.until {
            Some(t) if t <= Instant::now() => {
                self.until = None;
                vec![KeyerEvent::Finished]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(k.sent, vec!["CQ TEST"]);
        k.set_wpm(200);
        assert_eq!(k.wpm(), morse::MAX_WPM);
    }
}
