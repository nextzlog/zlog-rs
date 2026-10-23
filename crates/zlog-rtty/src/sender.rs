//! Sending: PTT on, the lead, the AFSK audio, the tail and PTT off
//! (`TTTYConsole.PlayMessageRTTY`, `ToggleTXRX`, the RTTY PTT delays).

use std::time::{Duration, Instant};

use crate::afsk::Afsk;
use crate::modem::{Modulator, Params};

/// Something that plays samples without blocking.
pub trait Sink {
    /// The sample rate the samples are to be made at.
    fn rate(&mut self) -> Result<u32, String>;
    /// Start playing `samples`, stopping what was playing.
    fn play(&mut self, samples: Vec<f32>) -> Result<(), String>;
    fn stop(&mut self);
    fn playing(&self) -> bool;
}

/// PTT and its delays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub ptt: bool,
    pub before: Duration,
    pub after: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    /// PTT is on; the text starts at the instant.
    Lead(Instant),
    Playing,
    /// The text has ended; PTT goes off at the instant.
    Tail(Instant),
}

/// Sends text as AFSK, one transmission at a time; text sent meanwhile follows in
/// the same transmission. Driven by [`Sender::tick`].
pub struct Sender<S> {
    pub sink: S,
    state: State,
    /// Text waiting to be sent.
    queue: String,
    ptt_on: bool,
    /// PTT changes for the application to carry out.
    requests: Vec<bool>,
}

impl<S: Sink> Sender<S> {
    pub fn new(sink: S) -> Self {
        Sender {
            sink,
            state: State::Idle,
            queue: String::new(),
            ptt_on: false,
            requests: Vec::new(),
        }
    }

    pub fn busy(&self) -> bool {
        self.state != State::Idle
    }

    /// Send `text`, after what is being sent.
    pub fn send(&mut self, text: &str, t: Timing, now: Instant) {
        self.queue.push_str(text);
        if self.state == State::Idle {
            if t.ptt {
                self.ptt(true);
            }
            self.state = State::Lead(now + if t.ptt { t.before } else { Duration::ZERO });
        }
    }

    /// Stop at once, PTT off (Esc, `CQAbort`); true when something was sent.
    pub fn abort(&mut self) -> bool {
        let was = self.busy();
        self.sink.stop();
        self.queue.clear();
        self.ptt(false);
        self.state = State::Idle;
        was
    }

    fn ptt(&mut self, on: bool) {
        if self.ptt_on != on {
            self.ptt_on = on;
            self.requests.push(on);
        }
    }

    /// Play the text waiting; errors stop everything.
    fn begin(&mut self, p: Params) -> Result<(), String> {
        let text = std::mem::take(&mut self.queue);
        let r = self
            .sink
            .rate()
            .and_then(|rate| self.sink.play(Afsk::new(p, rate).modulate(&text)));
        match r {
            Ok(()) => {
                self.state = State::Playing;
                Ok(())
            }
            Err(e) => {
                self.abort();
                Err(e)
            }
        }
    }

    /// Move on at `now`.
    pub fn tick(&mut self, now: Instant, t: Timing, p: Params) -> Result<(), String> {
        match self.state {
            State::Lead(at) if now >= at => self.begin(p)?,
            State::Playing if !self.sink.playing() => {
                if !self.queue.is_empty() {
                    self.begin(p)?;
                } else if t.ptt && !t.after.is_zero() {
                    self.state = State::Tail(now + t.after);
                } else {
                    self.ptt(false);
                    self.state = State::Idle;
                }
            }
            State::Tail(_) if !self.queue.is_empty() => self.begin(p)?,
            State::Tail(at) if now >= at => {
                self.ptt(false);
                self.state = State::Idle;
            }
            _ => {}
        }
        Ok(())
    }

    /// PTT changes since the last call, true for on.
    pub fn take_requests(&mut self) -> Vec<bool> {
        std::mem::take(&mut self.requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use crate::fsk::Fsk;
    use crate::modem::Demodulator;

    /// A sink that keeps what it played and plays until the test says it ended.
    #[derive(Default, Clone)]
    struct Fake {
        playing: Rc<Cell<bool>>,
        played: Rc<RefCell<Vec<f32>>>,
        fail: bool,
    }

    impl Sink for Fake {
        fn rate(&mut self) -> Result<u32, String> {
            if self.fail {
                return Err("no sound device".into());
            }
            Ok(8000)
        }
        fn play(&mut self, samples: Vec<f32>) -> Result<(), String> {
            self.played.borrow_mut().extend(samples);
            self.playing.set(true);
            Ok(())
        }
        fn stop(&mut self) {
            self.playing.set(false);
        }
        fn playing(&self) -> bool {
            self.playing.get()
        }
    }

    fn timing(ptt: bool) -> Timing {
        Timing {
            ptt,
            before: Duration::from_millis(100),
            after: Duration::from_millis(50),
        }
    }

    #[test]
    fn ptt_goes_on_before_and_off_after_the_text() {
        let fake = Fake::default();
        let mut s = Sender::new(fake.clone());
        let (t, p) = (timing(true), Params::default());
        let t0 = Instant::now();
        s.send("CQ TEST JA1ZLO", t, t0);
        assert_eq!(s.take_requests(), [true]);
        s.tick(t0 + Duration::from_millis(50), t, p).unwrap();
        assert!(fake.played.borrow().is_empty());
        s.tick(t0 + Duration::from_millis(100), t, p).unwrap();
        let text = Fsk::new(p, 8000).demodulate(&fake.played.borrow());
        assert_eq!(text, "CQ TEST JA1ZLO");
        fake.playing.set(false);
        let end = t0 + Duration::from_secs(3);
        s.tick(end, t, p).unwrap();
        assert!(s.take_requests().is_empty() && s.busy());
        s.tick(end + Duration::from_millis(50), t, p).unwrap();
        assert_eq!(s.take_requests(), [false]);
        assert!(!s.busy());
    }

    #[test]
    fn text_sent_meanwhile_follows_in_the_same_transmission() {
        let fake = Fake::default();
        let mut s = Sender::new(fake.clone());
        let (t, p) = (timing(true), Params::default());
        let t0 = Instant::now();
        s.send("TU ", t, t0);
        s.tick(t0 + Duration::from_secs(1), t, p).unwrap();
        s.send("QRZ", t, t0);
        fake.playing.set(false);
        s.tick(t0 + Duration::from_secs(2), t, p).unwrap();
        assert!(fake.playing.get());
        let text = Fsk::new(p, 8000).demodulate(&fake.played.borrow());
        assert_eq!(text, "TU QRZ");
        assert_eq!(s.take_requests(), [true]);
    }

    #[test]
    fn without_ptt_the_text_starts_at_once() {
        let fake = Fake::default();
        let mut s = Sender::new(fake.clone());
        let t0 = Instant::now();
        s.send("RYRY", timing(false), t0);
        s.tick(t0, timing(false), Params::default()).unwrap();
        assert!(fake.playing.get());
        fake.playing.set(false);
        s.tick(t0, timing(false), Params::default()).unwrap();
        assert!(!s.busy() && s.take_requests().is_empty());
    }

    #[test]
    fn escape_and_errors_turn_ptt_off() {
        let fake = Fake::default();
        let mut s = Sender::new(fake.clone());
        let (t, p) = (timing(true), Params::default());
        let t0 = Instant::now();
        s.send("CQ", t, t0);
        s.tick(t0 + Duration::from_secs(1), t, p).unwrap();
        assert!(s.abort());
        assert!(!fake.playing.get() && !s.busy());
        assert_eq!(s.take_requests(), [true, false]);
        assert!(!s.abort());
        let mut s = Sender::new(Fake {
            fail: true,
            ..Fake::default()
        });
        s.send("CQ", t, t0);
        assert!(s.tick(t0 + Duration::from_secs(1), t, p).is_err());
        assert_eq!(s.take_requests(), [true, false]);
        assert!(!s.busy());
    }
}
