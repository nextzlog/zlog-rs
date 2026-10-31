use std::path::Path;
use std::time::{Duration, Instant};

pub trait Player {
    fn play(&mut self, path: &Path) -> Result<(), String>;
    fn stop(&mut self);
    fn playing(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub ptt: bool,
    pub before: Duration,
    pub after: Duration,
    pub interval: Duration,
    pub max: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Lead(usize, Instant),
    Playing(usize),
    Tail(Instant),
    Waiting(Instant),
}

pub struct Voice<P> {
    pub player: P,
    state: State,
    cq: Option<u32>,
    ptt_on: bool,
    requests: Vec<bool>,
}

impl<P: Player> Voice<P> {
    pub fn new(player: P) -> Self {
        Voice {
            player,
            state: State::Idle,
            cq: None,
            ptt_on: false,
            requests: Vec::new(),
        }
    }

    pub fn busy(&self) -> bool {
        self.state != State::Idle
    }

    pub fn cq_running(&self) -> bool {
        self.cq.is_some()
    }

    pub fn current(&self) -> Option<usize> {
        match self.state {
            State::Lead(n, _) | State::Playing(n) => Some(n),
            _ => None,
        }
    }

    pub fn play(&mut self, n: usize, path: &Path, t: Timing, now: Instant) -> Result<(), String> {
        self.abort();
        self.start(n, path, t, now)
    }

    pub fn start_cq(
        &mut self,
        n: usize,
        path: &Path,
        t: Timing,
        now: Instant,
    ) -> Result<(), String> {
        self.play(n, path, t, now)?;
        self.cq = Some(0);
        Ok(())
    }

    #[cfg(test)]
    fn stop_cq(&mut self) {
        self.cq = None;
        if matches!(self.state, State::Waiting(_)) {
            self.state = State::Idle;
        }
    }

    pub fn abort(&mut self) -> bool {
        let was = self.busy() || self.cq.is_some();
        self.player.stop();
        self.ptt(false);
        self.state = State::Idle;
        self.cq = None;
        was
    }

    fn start(&mut self, n: usize, path: &Path, t: Timing, now: Instant) -> Result<(), String> {
        if !path.is_file() {
            return Err(format!("{}: no such file", path.display()));
        }
        if t.ptt {
            self.ptt(true);
            self.state = State::Lead(n, now + t.before);
            Ok(())
        } else {
            self.begin(n, path)
        }
    }

    fn begin(&mut self, n: usize, path: &Path) -> Result<(), String> {
        match self.player.play(path) {
            Ok(()) => {
                self.state = State::Playing(n);
                Ok(())
            }
            Err(e) => {
                self.abort();
                Err(e)
            }
        }
    }

    fn ptt(&mut self, on: bool) {
        if self.ptt_on != on {
            self.ptt_on = on;
            self.requests.push(on);
        }
    }

    pub fn tick<'a>(
        &mut self,
        now: Instant,
        t: Timing,
        file: impl Fn(usize) -> Option<&'a Path>,
    ) -> Result<(), String> {
        match self.state {
            State::Lead(n, at) if now >= at => match file(n) {
                Some(path) => self.begin(n, path)?,
                None => {
                    self.abort();
                }
            },
            State::Playing(_) if !self.player.playing() => {
                if t.ptt && !t.after.is_zero() {
                    self.state = State::Tail(now + t.after);
                } else {
                    self.ended(now, t);
                }
            }
            State::Tail(at) if now >= at => self.ended(now, t),
            State::Waiting(at) if now >= at => match file(0) {
                Some(path) => {
                    if let Err(e) = self.start(0, path, t, now) {
                        self.abort();
                        return Err(e);
                    }
                }
                None => {
                    self.abort();
                }
            },
            _ => {}
        }
        Ok(())
    }

    fn ended(&mut self, now: Instant, t: Timing) {
        self.ptt(false);
        self.state = State::Idle;
        if let Some(sent) = self.cq {
            let sent = sent + 1;
            if sent >= t.max {
                self.cq = None;
            } else {
                self.cq = Some(sent);
                self.state = State::Waiting(now + t.interval);
            }
        }
    }

    pub fn take_requests(&mut self) -> Vec<bool> {
        std::mem::take(&mut self.requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[derive(Default, Clone)]
    struct Fake {
        playing: Rc<Cell<bool>>,
        plays: Rc<Cell<u32>>,
    }

    impl Player for Fake {
        fn play(&mut self, _path: &Path) -> Result<(), String> {
            self.playing.set(true);
            self.plays.set(self.plays.get() + 1);
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
            before: Duration::from_millis(25),
            after: Duration::from_millis(10),
            interval: Duration::from_secs(2),
            max: 3,
        }
    }

    fn wav() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
    }

    #[test]
    fn ptt_goes_on_before_and_off_after_a_memory() {
        let fake = Fake::default();
        let mut v = Voice::new(fake.clone());
        let t = timing(true);
        let path = wav();
        let file = |_| Some(path.as_path());
        let t0 = Instant::now();
        v.play(4, &path, t, t0).unwrap();
        assert_eq!(v.take_requests(), vec![true]);
        assert_eq!(fake.plays.get(), 0);
        v.tick(t0 + Duration::from_millis(10), t, file).unwrap();
        assert_eq!(fake.plays.get(), 0);
        v.tick(t0 + Duration::from_millis(25), t, file).unwrap();
        assert_eq!((fake.plays.get(), v.current()), (1, Some(4)));
        fake.playing.set(false);
        let end = t0 + Duration::from_secs(1);
        v.tick(end, t, file).unwrap();
        assert!(v.take_requests().is_empty());
        v.tick(end + Duration::from_millis(10), t, file).unwrap();
        assert_eq!(v.take_requests(), vec![false]);
        assert!(!v.busy());
    }

    #[test]
    fn missing_files_are_not_played() {
        let mut v = Voice::new(Fake::default());
        let r = v.play(0, Path::new("/no/such.wav"), timing(true), Instant::now());
        assert!(r.is_err());
        assert!(v.take_requests().is_empty() && !v.busy());
    }

    #[test]
    fn cq_repeats_after_the_interval_up_to_the_most() {
        let fake = Fake::default();
        let mut v = Voice::new(fake.clone());
        let t = timing(false);
        let path = wav();
        let file = |_| Some(path.as_path());
        let mut now = Instant::now();
        v.start_cq(0, &path, t, now).unwrap();
        for sent in 1..=3 {
            assert_eq!(fake.plays.get(), sent);
            fake.playing.set(false);
            now += Duration::from_secs(3);
            v.tick(now, t, file).unwrap();
            if sent < 3 {
                assert!(v.cq_running());
                v.tick(now + Duration::from_millis(1999), t, file).unwrap();
                assert_eq!(fake.plays.get(), sent);
                now += Duration::from_secs(2);
                v.tick(now, t, file).unwrap();
            }
        }
        assert!(!v.cq_running() && !v.busy());
        assert_eq!(fake.plays.get(), 3);
    }

    #[test]
    fn escape_stops_playback_ptt_and_cq() {
        let fake = Fake::default();
        let mut v = Voice::new(fake.clone());
        let path = wav();
        let t0 = Instant::now();
        v.start_cq(0, &path, timing(true), t0).unwrap();
        v.tick(t0 + Duration::from_millis(30), timing(true), |_| {
            Some(path.as_path())
        })
        .unwrap();
        assert!(fake.playing.get());
        assert!(v.abort());
        assert!(!fake.playing.get() && !v.cq_running() && !v.busy());
        assert_eq!(v.take_requests(), vec![true, false]);
        assert!(!v.abort());
    }

    #[test]
    fn typing_stops_the_repeat_but_not_the_memory() {
        let fake = Fake::default();
        let mut v = Voice::new(fake.clone());
        let path = wav();
        let t = timing(false);
        let now = Instant::now();
        v.start_cq(0, &path, t, now).unwrap();
        v.stop_cq();
        assert!(fake.playing.get() && !v.cq_running());
        fake.playing.set(false);
        v.tick(now, t, |_| Some(path.as_path())).unwrap();
        assert!(!v.busy());
    }
}
