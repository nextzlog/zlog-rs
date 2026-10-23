//! The play queue of SO2R (`TformMessageManager`, UMessageManager.pas): the
//! messages of the keyer and the voice memories waiting to be sent, played one at
//! a time and dropped once played.
//!
//! zLog queues every message in SO2R; a function key clears the queue, stopping
//! what is being sent (`PlayMessage`: `ClearQue`), adds its message (`AddQue`)
//! and starts it (`ContinueQue`). A message for a transmitter that already has
//! one waiting is not added (`AddQue` compares the rig). The queue is used only
//! while an SO2R box is in use ([`PlayQueue::so2r`]); otherwise the keyer and the
//! voice memories play as they always do. zLog's commands between messages
//! (`SwitchRig`, `ResetTx`, `AfterDelay`: TX and RX on different rigs, 2BSIQ)
//! are not implemented, as the entry row has one rig for both.
//!
//! The panels that play share one queue ([`shared`]); each plays the messages of
//! its own kind when it is idle ([`PlayQueue::next`]).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use zlog_core::Mode;

/// What a message plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Play {
    /// Expanded text for the keyer (CW) or the RTTY console.
    Text(String),
    /// Voice memory `n` (1 to 12).
    Voice(usize),
}

/// One message of the queue (`TPlayMessage`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    /// The rig that transmits it, 1 or 2.
    pub tx: u8,
    pub mode: Mode,
    pub play: Play,
}

impl Queued {
    /// The line shown for it (`TPlayMessage.GetText`): "[TX#] [MODE] text", or
    /// "[TX#] [MODE] Play Voice (n)".
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

/// The messages waiting, the first one being played once started.
#[derive(Debug, Default)]
pub struct PlayQueue {
    /// Whether an SO2R box is in use, so that messages go through the queue.
    pub so2r: bool,
    items: VecDeque<Queued>,
    /// Whether the first message is being played.
    started: bool,
    /// Counts [`PlayQueue::clear`]s, so that a panel playing a message of the
    /// queue sees it dropped by another panel and stops it (`ClearQue` stops
    /// every player).
    generation: u64,
}

/// The queue shared by the panels that play and the window that shows it.
pub type Shared = Arc<Mutex<PlayQueue>>;

/// The process-wide queue.
pub fn shared() -> Shared {
    static QUEUE: OnceLock<Shared> = OnceLock::new();
    QUEUE.get_or_init(Shared::default).clone()
}

impl PlayQueue {
    /// Drop every message (`ClearQue`); the caller stops what it is sending, and
    /// the other panels stop the message they play ([`PlayQueue::generation`]).
    pub fn clear(&mut self) {
        self.items.clear();
        self.started = false;
        self.generation += 1;
    }

    /// Changes at every [`PlayQueue::clear`]: a panel keeps it when it starts a
    /// message of the queue, and stops that message when it has changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Add a message at the end (`AddQue`), unless one for the same rig waits;
    /// whether it was added.
    pub fn add(&mut self, message: Queued) -> bool {
        if self.items.iter().any(|m| m.tx == message.tx) {
            return false;
        }
        self.items.push_back(message);
        true
    }

    /// For a panel that plays voice (`voice`) or text, idle when `idle`: drop the
    /// message it finished, and take the next one when it is of its kind
    /// (`ContinueQue`, `WMPlayQueue`). Nothing without an SO2R box.
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
        Some(front)
    }

    /// The lines shown, the one being played first (`Memo1`).
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
        // Without an SO2R box nothing is played from the queue.
        assert_eq!(q.next(false, true), None);
        q.so2r = true;
        // One message per rig waits at a time.
        assert!(!q.add(cw(1, "TU")));
        assert!(q.add(cw(2, "JA1ZLO")));
        // The keyer is busy: nothing yet.
        assert_eq!(q.next(false, false), None);
        assert_eq!(q.next(false, true), Some(cw(1, "CQ")));
        // While it is sent, it stays first.
        assert_eq!(q.next(false, false), None);
        assert_eq!(q.lines(), ["[1] [CW] CQ", "[2] [CW] JA1ZLO"]);
        // A voice panel does not take a CW message.
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
}
