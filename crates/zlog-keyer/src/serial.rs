//! Keying a serial port's DTR or RTS line (zLog's COM keying, `CW_ON`, `CW_OFF`,
//! `ControlPTT`), with the timing on a thread of its own.
//!
//! zLog steps a 1 ms multimedia timer through a code table; this sleeps to just
//! before each edge and spins the rest, which keeps edges within tens of microseconds.

use std::io;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::keyer::{Keyer, KeyerEvent};
use crate::morse::{self, Timing, MAX_WPM, MIN_WPM};

/// A modem control line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Dtr,
    Rts,
}

impl Line {
    pub fn other(self) -> Line {
        match self {
            Line::Dtr => Line::Rts,
            Line::Rts => Line::Dtr,
        }
    }
}

/// The control lines of a port; a serial port, or a fake in tests.
pub trait Lines: Send + 'static {
    fn set(&mut self, line: Line, on: bool) -> io::Result<()>;
}

impl Lines for Box<dyn serialport::SerialPort> {
    fn set(&mut self, line: Line, on: bool) -> io::Result<()> {
        match line {
            Line::Dtr => self.write_data_terminal_ready(on),
            Line::Rts => self.write_request_to_send(on),
        }
        .map_err(io::Error::other)
    }
}

/// Open a port for line keying; both lines off.
pub fn open_port(path: &str) -> Result<Box<dyn serialport::SerialPort>, String> {
    let mut port = serialport::new(path, 9600)
        .timeout(Duration::from_millis(1))
        .open()
        .map_err(|e| e.to_string())?;
    let _ = port.write_data_terminal_ready(false);
    let _ = port.write_request_to_send(false);
    Ok(port)
}

/// How the lines are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSetup {
    /// The keying line; PTT, when used, is the other one.
    pub key: Line,
    pub ptt: bool,
    /// Time from PTT on to the first mark and from the last mark to PTT off
    /// (`SetPTTDelay`, zLog default 25 and 0 ms, used clamped to 1..=255 ms).
    pub ptt_before: Duration,
    pub ptt_after: Duration,
}

enum Cmd {
    /// Text and the abort count when it was given.
    Send(String, u32),
    Ptt(bool),
    /// PTT for phone, on the PTT line whatever the CW PTT setting.
    PhonePtt(bool),
}

/// A keyer on DTR or RTS.
pub struct LineKeyer {
    commands: Sender<Cmd>,
    events: Receiver<KeyerEvent>,
    /// Texts given and not yet sent (or dropped by an abort); busy while not zero.
    /// A counter rather than a flag, so that text given during the PTT tail of the
    /// previous message keeps the keyer busy and gets a Finished of its own.
    pending: Arc<AtomicU32>,
    /// Aborts so far; text given before the latest abort is dropped.
    aborts: Arc<AtomicU32>,
    wpm: Arc<AtomicU32>,
    /// Weight and space factors, read at the start of each text.
    timing: Arc<Mutex<Timing>>,
}

impl LineKeyer {
    pub fn new(lines: impl Lines, setup: LineSetup, timing: Timing) -> Self {
        let (commands, rx) = mpsc::channel();
        let (tx, events) = mpsc::channel();
        let pending = Arc::new(AtomicU32::new(0));
        let aborts = Arc::new(AtomicU32::new(0));
        let wpm = Arc::new(AtomicU32::new(timing.wpm));
        let shared_timing = Arc::new(Mutex::new(timing));
        let worker = Worker {
            lines,
            setup,
            timing,
            shared_timing: shared_timing.clone(),
            commands: rx,
            events: tx,
            pending: pending.clone(),
            done: 0,
            aborts: aborts.clone(),
            wpm: wpm.clone(),
            ptt: false,
            manual_ptt: false,
        };
        thread::Builder::new()
            .name("zlog-keyer".into())
            .spawn(move || worker.run())
            .expect("keyer thread");
        LineKeyer {
            commands,
            events,
            pending,
            aborts,
            wpm,
            timing: shared_timing,
        }
    }
}

impl Keyer for LineKeyer {
    fn send(&mut self, text: &str) {
        // Busy at once, so that a caller polling right after sees it.
        self.pending.fetch_add(1, Ordering::SeqCst);
        let epoch = self.aborts.load(Ordering::SeqCst);
        let _ = self.commands.send(Cmd::Send(text.to_string(), epoch));
    }

    fn abort(&mut self) {
        self.aborts.fetch_add(1, Ordering::SeqCst);
    }

    fn set_wpm(&mut self, wpm: u32) {
        self.wpm
            .store(wpm.clamp(MIN_WPM, MAX_WPM), Ordering::SeqCst);
    }

    fn wpm(&self) -> u32 {
        self.wpm.load(Ordering::SeqCst)
    }

    /// Takes effect from the next text; the speed as by [`Keyer::set_wpm`].
    fn set_timing(&mut self, timing: Timing) {
        self.set_wpm(timing.wpm);
        *self.timing.lock().unwrap() = timing;
    }

    fn set_ptt(&mut self, on: bool) {
        let _ = self.commands.send(Cmd::Ptt(on));
    }

    fn set_phone_ptt(&mut self, on: bool) {
        let _ = self.commands.send(Cmd::PhonePtt(on));
    }

    fn busy(&self) -> bool {
        self.pending.load(Ordering::SeqCst) > 0
    }

    fn poll(&mut self) -> Vec<KeyerEvent> {
        self.events.try_iter().collect()
    }
}

/// The keying thread; it ends when the [`LineKeyer`] is dropped.
struct Worker<L: Lines> {
    lines: L,
    setup: LineSetup,
    timing: Timing,
    shared_timing: Arc<Mutex<Timing>>,
    commands: Receiver<Cmd>,
    events: Sender<KeyerEvent>,
    pending: Arc<AtomicU32>,
    /// Texts handled since the last [`Worker::idle`], to take off `pending` there.
    done: u32,
    aborts: Arc<AtomicU32>,
    wpm: Arc<AtomicU32>,
    ptt: bool,
    manual_ptt: bool,
}

impl<L: Lines> Worker<L> {
    fn run(mut self) {
        'run: while let Ok(cmd) = self.commands.recv() {
            let mut next = Some(cmd);
            // Send everything queued before going idle (FIFO).
            while let Some(cmd) = next.take() {
                match cmd {
                    Cmd::Ptt(on) => {
                        self.manual_ptt = on;
                        self.ptt_line(on);
                    }
                    Cmd::PhonePtt(on) => {
                        // Held as by hand: CW sent meanwhile does not drop it.
                        self.manual_ptt = on;
                        self.ptt = on;
                        self.line(self.setup.key.other(), on);
                    }
                    Cmd::Send(text, epoch) => {
                        self.key(&text, epoch);
                        self.done += 1;
                    }
                }
                next = match self.commands.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        self.idle();
                        break 'run;
                    }
                };
            }
            self.idle();
        }
        // The keyer was dropped: leave both lines off, PTT too, even when it was
        // keyed by hand (`set_ptt(true)`), so the transmitter is never left on.
        let _ = self.lines.set(self.setup.key, false);
        if self.setup.ptt || self.ptt {
            let _ = self.lines.set(self.setup.key.other(), false);
        }
    }

    fn idle(&mut self) {
        if self.setup.ptt && self.ptt && !self.manual_ptt {
            self.wait(self.setup.ptt_after, None);
            // Text given during the tail: keep PTT on and key it next.
            if self.pending.load(Ordering::SeqCst) > self.done {
                return;
            }
            self.ptt_line(false);
        }
        if self.done == 0 {
            return;
        }
        let left = self.pending.fetch_sub(self.done, Ordering::SeqCst) - self.done;
        self.done = 0;
        // Finished only when nothing more was given meanwhile; that text gets its own.
        if left == 0 {
            let _ = self.events.send(KeyerEvent::Finished);
        }
    }

    fn line(&mut self, line: Line, on: bool) {
        if let Err(e) = self.lines.set(line, on) {
            let _ = self.events.send(KeyerEvent::Error(e.to_string()));
        }
    }

    fn ptt_line(&mut self, on: bool) {
        if self.setup.ptt {
            self.ptt = on;
            self.line(self.setup.key.other(), on);
        }
    }

    fn key(&mut self, text: &str, epoch: u32) {
        if self.aborted(epoch) {
            return;
        }
        if self.setup.ptt && !self.ptt {
            self.ptt_line(true);
            self.wait(self.setup.ptt_before, None);
        }
        self.timing = *self.shared_timing.lock().unwrap();
        self.timing.wpm = self.wpm.load(Ordering::SeqCst);
        for step in morse::keying(text, &self.timing) {
            if self.aborted(epoch) {
                break;
            }
            self.line(self.setup.key, step.key);
            self.wait(step.len, Some(epoch));
        }
        self.line(self.setup.key, false);
    }

    fn aborted(&self, epoch: u32) -> bool {
        self.aborts.load(Ordering::SeqCst) != epoch
    }

    /// Wait until `len` from now, or an abort after `epoch`: sleep, then spin the
    /// last two milliseconds.
    fn wait(&self, len: Duration, epoch: Option<u32>) {
        let until = Instant::now() + len;
        let spin = Duration::from_millis(2);
        loop {
            let now = Instant::now();
            if now >= until || epoch.is_some_and(|e| self.aborted(e)) {
                return;
            }
            let left = until - now;
            if left > spin {
                thread::sleep((left - spin).min(Duration::from_millis(20)));
            } else {
                std::hint::spin_loop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct FakeLines(Arc<Mutex<Vec<(Line, bool, Instant)>>>);

    impl Lines for FakeLines {
        fn set(&mut self, line: Line, on: bool) -> io::Result<()> {
            self.0.lock().unwrap().push((line, on, Instant::now()));
            Ok(())
        }
    }

    fn wait_idle(k: &mut LineKeyer) -> Vec<KeyerEvent> {
        let start = Instant::now();
        let mut events = Vec::new();
        while k.busy() && start.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(Duration::from_millis(5));
        events.extend(k.poll());
        events
    }

    #[test]
    fn keys_with_ptt_on_the_other_line() {
        let lines = FakeLines::default();
        let setup = LineSetup {
            key: Line::Dtr,
            ptt: true,
            ptt_before: Duration::from_millis(10),
            ptt_after: Duration::from_millis(10),
        };
        let timing = Timing {
            wpm: 60,
            ..Timing::default()
        };
        let mut k = LineKeyer::new(lines.clone(), setup, timing);
        k.send("EE");
        assert!(k.busy());
        assert_eq!(wait_idle(&mut k), vec![KeyerEvent::Finished]);
        let log = lines.0.lock().unwrap().clone();
        let seq: Vec<(Line, bool)> = log.iter().map(|(l, o, _)| (*l, *o)).collect();
        use Line::*;
        assert_eq!(
            seq,
            vec![
                (Rts, true),
                (Dtr, true),
                (Dtr, false),
                (Dtr, true),
                (Dtr, false),
                (Dtr, false),
                (Rts, false)
            ]
        );
        // A dot at 60 WPM is 20 ms; allow for a loaded test machine.
        let dot = log[2].2 - log[1].2;
        assert!(dot >= Duration::from_millis(20) && dot < Duration::from_millis(40));
        let lead = log[1].2 - log[0].2;
        assert!(lead >= Duration::from_millis(10));
    }

    #[test]
    fn send_during_the_ptt_tail_stays_busy() {
        let lines = FakeLines::default();
        let setup = LineSetup {
            key: Line::Dtr,
            ptt: true,
            ptt_before: Duration::from_millis(5),
            ptt_after: Duration::from_millis(150),
        };
        let timing = Timing {
            wpm: 60,
            ..Timing::default()
        };
        let mut k = LineKeyer::new(lines.clone(), setup, timing);
        k.send("E");
        // Wait for the end of the mark, then give more within the PTT tail.
        let start = Instant::now();
        while !lines
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.0 == Line::Dtr && !e.1)
            && start.elapsed() < Duration::from_secs(5)
        {
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(Duration::from_millis(60));
        k.send("E");
        assert_eq!(wait_idle(&mut k), vec![KeyerEvent::Finished]);
        let log = lines.0.lock().unwrap().clone();
        let marks = log.iter().filter(|e| e.0 == Line::Dtr && e.1).count();
        assert_eq!(marks, 2);
        // PTT stayed on between the two and is off at the end.
        let ptt: Vec<bool> = log
            .iter()
            .filter(|e| e.0 == Line::Rts)
            .map(|e| e.1)
            .collect();
        assert_eq!(ptt, vec![true, false]);
    }

    #[test]
    fn timing_changes_apply_to_a_connected_keyer() {
        let lines = FakeLines::default();
        let setup = LineSetup {
            key: Line::Dtr,
            ptt: false,
            ptt_before: Duration::ZERO,
            ptt_after: Duration::ZERO,
        };
        let timing = Timing {
            wpm: 60,
            ..Timing::default()
        };
        let mut k = LineKeyer::new(lines.clone(), setup, timing);
        k.set_timing(Timing {
            weight: 80,
            ..timing
        });
        k.send("E");
        wait_idle(&mut k);
        let log = lines.0.lock().unwrap().clone();
        let on = log.iter().position(|e| e.0 == Line::Dtr && e.1).unwrap();
        // A dot of 20 ms at weight 50; heavier at 80.
        let mark = log[on + 1].2 - log[on].2;
        assert!(mark >= Duration::from_millis(28), "{mark:?}");
    }

    #[test]
    fn dropping_turns_ptt_off() {
        let lines = FakeLines::default();
        let setup = LineSetup {
            key: Line::Dtr,
            ptt: true,
            ptt_before: Duration::ZERO,
            ptt_after: Duration::ZERO,
        };
        let mut k = LineKeyer::new(lines.clone(), setup, Timing::default());
        k.set_ptt(true);
        drop(k);
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let log = lines.0.lock().unwrap().clone();
            if log.last().map(|e| (e.0, e.1)) == Some((Line::Rts, false)) {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("PTT left on: {:?}", lines.0.lock().unwrap());
    }

    /// Phone PTT keys the PTT line even without PTT for CW, and dropping the
    /// keyer turns it off.
    #[test]
    fn phone_ptt_works_without_cw_ptt_and_is_left_off() {
        let lines = FakeLines::default();
        let setup = LineSetup {
            key: Line::Dtr,
            ptt: false,
            ptt_before: Duration::ZERO,
            ptt_after: Duration::ZERO,
        };
        let mut k = LineKeyer::new(lines.clone(), setup, Timing::default());
        k.set_phone_ptt(true);
        let start = Instant::now();
        while lines.0.lock().unwrap().is_empty() && start.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            lines.0.lock().unwrap().first().map(|e| (e.0, e.1)),
            Some((Line::Rts, true))
        );
        drop(k);
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let log = lines.0.lock().unwrap().clone();
            if log.last().map(|e| (e.0, e.1)) == Some((Line::Rts, false)) {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("PTT left on: {:?}", lines.0.lock().unwrap());
    }

    #[test]
    fn abort_stops_keying() {
        let lines = FakeLines::default();
        let setup = LineSetup {
            key: Line::Rts,
            ptt: false,
            ptt_before: Duration::ZERO,
            ptt_after: Duration::ZERO,
        };
        let mut k = LineKeyer::new(lines.clone(), setup, Timing::default());
        k.send("CQ CQ CQ TEST");
        thread::sleep(Duration::from_millis(30));
        k.abort();
        assert_eq!(wait_idle(&mut k), vec![KeyerEvent::Finished]);
        let log = lines.0.lock().unwrap().clone();
        assert!(log.len() < 10);
        assert_eq!(log.last().map(|e| (e.0, e.1)), Some((Line::Rts, false)));
        // Usable again after the abort.
        k.send("E");
        assert_eq!(wait_idle(&mut k), vec![KeyerEvent::Finished]);
    }
}
