use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use zlog_core::Mode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Play {
    Text(String),
    Voice(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub tx: u8,
    pub mode: Mode,
    pub play: Play,
}

impl Queued {
    pub fn line(&self) -> String {
        match &self.play {
            Play::Text(text) => format!("[{}] [{}] {}", self.tx, self.mode, text),
            Play::Voice(n) => format!("[{}] [{}] Play Voice ({})", self.tx, self.mode, n),
        }
    }

    fn voice(&self) -> bool {
        matches!(self.play, Play::Voice(_))
    }
}

pub type OnStart = Box<dyn FnMut(&Queued) + Send>;

#[derive(Default)]
pub struct PlayQueue {
    pub so2r: bool,
    pub so2r_device: String,
    items: VecDeque<Queued>,
    started: bool,
    generation: u64,
    on_start: Option<OnStart>,
}

impl std::fmt::Debug for PlayQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayQueue")
            .field("so2r", &self.so2r)
            .field("so2r_device", &self.so2r_device)
            .field("items", &self.items)
            .field("started", &self.started)
            .field("generation", &self.generation)
            .finish()
    }
}

pub type Shared = Arc<Mutex<PlayQueue>>;

pub fn shared() -> Shared {
    static QUEUE: OnceLock<Shared> = OnceLock::new();
    QUEUE.get_or_init(Shared::default).clone()
}

impl PlayQueue {
    pub fn clear(&mut self) {
        self.items.clear();
        self.started = false;
        self.generation += 1;
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn on_start(&mut self, f: Option<OnStart>) {
        self.on_start = f;
    }

    pub fn add(&mut self, message: Queued) -> bool {
        if self.items.iter().any(|m| m.tx == message.tx) {
            return false;
        }
        self.items.push_back(message);
        true
    }

    pub fn next(&mut self, voice: bool, idle: bool) -> Option<Queued> {
        if !self.so2r {
            return None;
        }
        let front = self.items.front()?;
        if front.voice() != voice || !idle {
            return None;
        }
        if self.started {
            self.items.pop_front();
            self.started = false;
        }
        let front = self.items.front().filter(|m| m.voice() == voice)?.clone();
        self.started = true;
        if let Some(f) = self.on_start.as_mut() {
            f(&front);
        }
        Some(front)
    }

    pub fn lines(&self) -> Vec<String> {
        self.items.iter().map(Queued::line).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cw(tx: u8, text: &str) -> Queued {
        Queued {
            tx,
            mode: Mode::Cw,
            play: Play::Text(text.into()),
        }
    }

    #[test]
    fn lines_as_zlog_shows_them() {
        assert_eq!(cw(1, "CQ TEST").line(), "[1] [CW] CQ TEST");
        let v = Queued {
            tx: 2,
            mode: Mode::Ssb,
            play: Play::Voice(3),
        };
        assert_eq!(v.line(), "[2] [SSB] Play Voice (3)");
    }

    #[test]
    fn messages_play_in_turn_and_are_dropped_once_played() {
        let mut q = PlayQueue::default();
        assert!(q.add(cw(1, "CQ")));
        assert_eq!(q.next(false, true), None);
        q.so2r = true;
        assert!(!q.add(cw(1, "TU")));
        assert!(q.add(cw(2, "JA1ZLO")));
        assert_eq!(q.next(false, false), None);
        assert_eq!(q.next(false, true), Some(cw(1, "CQ")));
        assert_eq!(q.next(false, false), None);
        assert_eq!(q.lines(), ["[1] [CW] CQ", "[2] [CW] JA1ZLO"]);
        assert_eq!(q.next(true, true), None);
        assert_eq!(q.next(false, true), Some(cw(2, "JA1ZLO")));
        assert_eq!(q.next(false, true), None);
        assert!(q.is_empty());
        q.add(cw(1, "CQ"));
        let g = q.generation();
        q.clear();
        assert!(q.lines().is_empty());
        assert_ne!(q.generation(), g);
    }

    #[test]
    fn the_start_hook_hears_each_message_before_it_plays() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let mut q = PlayQueue {
            so2r: true,
            ..Default::default()
        };
        let h = heard.clone();
        q.on_start(Some(Box::new(move |m| h.lock().unwrap().push(m.tx))));
        q.add(cw(2, "TU"));
        q.add(cw(1, "CQ"));
        q.next(false, true);
        q.next(false, true);
        q.next(false, true);
        assert_eq!(*heard.lock().unwrap(), [2, 1]);
    }
}
