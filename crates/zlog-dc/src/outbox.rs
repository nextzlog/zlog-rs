//! The lines waiting to be posted: a bounded queue taken in batches, at most one
//! post per pace of its destination, held back as long as the destination asks or
//! the network fails. Each destination has a queue of its own.
//!
//! Time is passed in, so the rules are tested without waiting or a network.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Most lines kept; beyond it the oldest is dropped and counted.
pub const CAPACITY: usize = 500;
/// Least time between two posts of a queue made with [`Default`], measured from
/// the answer: Discord's ([`crate::site::Site::pace`]).
pub const PACE: Duration = Duration::from_secs(2);
/// Wait after a failed post before trying again.
pub const RETRY: Duration = Duration::from_secs(30);
/// Most lines in one post, and most characters (Discord takes 2000).
pub const BATCH_LINES: usize = 10;
pub const BATCH_CHARS: usize = 1900;

/// How a post went.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Sent,
    /// Rate limited: try again after this long.
    Wait(Duration),
    /// Not delivered (offline, refused); kept and tried again after [`RETRY`].
    Failed(String),
    /// Refused for good (the webhook is gone or the URL is wrong: 400, 401, 403,
    /// 404): kept, and not tried again until [`Outbox::resume`].
    Refused(String),
}

#[derive(Debug)]
pub struct Outbox {
    lines: VecDeque<String>,
    /// Least time between two posts.
    pace: Duration,
    not_before: Option<Instant>,
    /// Lines posted since start-up.
    pub sent: usize,
    /// Lines dropped because the queue was full.
    pub dropped: usize,
    /// Why the last post failed, until one goes through.
    pub error: Option<String>,
    /// Posting stopped after an [`Outcome::Refused`], until [`Outbox::resume`].
    pub refused: bool,
}

impl Default for Outbox {
    fn default() -> Self {
        Outbox::new(PACE)
    }
}

impl Outbox {
    /// An empty queue posting at most once per `pace`.
    pub fn new(pace: Duration) -> Self {
        Outbox {
            lines: VecDeque::new(),
            pace,
            not_before: None,
            sent: 0,
            dropped: 0,
            error: None,
            refused: false,
        }
    }

    pub fn push(&mut self, line: String) {
        self.lines.push_back(line);
        self.trim();
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The next batch, oldest first, when a post may go out at `now`.
    pub fn take(&mut self, now: Instant) -> Option<Vec<String>> {
        if self.refused || self.lines.is_empty() || self.not_before.is_some_and(|t| now < t) {
            return None;
        }
        let mut batch = Vec::new();
        let mut chars = 0;
        while let Some(next) = self.lines.front() {
            let size = next.chars().count() + 1;
            if !batch.is_empty() && (batch.len() == BATCH_LINES || chars + size > BATCH_CHARS) {
                break;
            }
            chars += size;
            batch.extend(self.lines.pop_front());
        }
        Some(batch)
    }

    /// Record how the post of `batch` went; undelivered lines go back to the front.
    pub fn done(&mut self, batch: Vec<String>, outcome: Outcome, now: Instant) {
        let wait = match outcome {
            Outcome::Sent => {
                self.sent += batch.len();
                self.error = None;
                self.pace
            }
            Outcome::Wait(d) => {
                self.requeue(batch);
                d.max(self.pace)
            }
            Outcome::Failed(e) => {
                self.requeue(batch);
                self.error = Some(e);
                RETRY
            }
            Outcome::Refused(e) => {
                self.requeue(batch);
                self.error = Some(e);
                self.refused = true;
                RETRY
            }
        };
        self.not_before = Some(now + wait);
    }

    /// Try again at once after the settings changed or posting was switched on
    /// again, the errors and the count of dropped lines seen.
    pub fn resume(&mut self) {
        self.refused = false;
        self.error = None;
        self.dropped = 0;
        self.not_before = None;
    }

    /// How long until the next post may go out; `None` when nothing waits (or
    /// posting stopped after a refusal).
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        if self.refused || self.lines.is_empty() {
            return None;
        }
        Some(self.not_before.map_or(Duration::ZERO, |t| t - now.min(t)))
    }

    fn requeue(&mut self, batch: Vec<String>) {
        for line in batch.into_iter().rev() {
            self.lines.push_front(line);
        }
        self.trim();
    }

    fn trim(&mut self) {
        while self.lines.len() > CAPACITY {
            self.lines.pop_front();
            self.dropped += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(n: usize) -> Outbox {
        let mut o = Outbox::default();
        for i in 0..n {
            o.push(format!("QSO {i}"));
        }
        o
    }

    #[test]
    fn a_lone_qso_goes_alone_and_a_backlog_in_batches() {
        let now = Instant::now();
        let mut o = filled(1);
        assert_eq!(o.take(now).unwrap(), ["QSO 0"]);
        let mut o = filled(25);
        let batch = o.take(now).unwrap();
        assert_eq!(batch.len(), BATCH_LINES);
        assert_eq!(batch[0], "QSO 0");
        assert_eq!(o.len(), 15);
    }

    #[test]
    fn a_batch_stays_within_the_message_size() {
        let mut o = Outbox::default();
        for _ in 0..5 {
            o.push("x".repeat(700));
        }
        assert_eq!(o.take(Instant::now()).unwrap().len(), 2);
    }

    #[test]
    fn posts_are_paced() {
        let now = Instant::now();
        let mut o = filled(30);
        let b = o.take(now).unwrap();
        o.done(b, Outcome::Sent, now);
        assert_eq!(o.sent, BATCH_LINES);
        assert!(o.take(now + PACE / 2).is_none());
        assert_eq!(o.wait(now), Some(PACE));
        assert!(o.take(now + PACE).is_some());
    }

    #[test]
    fn rate_limits_are_obeyed_and_nothing_is_lost() {
        let now = Instant::now();
        let mut o = filled(3);
        let b = o.take(now).unwrap();
        o.done(b, Outcome::Wait(Duration::from_secs(7)), now);
        assert_eq!(o.len(), 3);
        assert!(o.take(now + Duration::from_secs(6)).is_none());
        assert_eq!(
            o.take(now + Duration::from_secs(7)).unwrap(),
            ["QSO 0", "QSO 1", "QSO 2"]
        );
    }

    #[test]
    fn failures_are_kept_reported_and_retried_later() {
        let now = Instant::now();
        let mut o = filled(2);
        let b = o.take(now).unwrap();
        o.done(b, Outcome::Failed("offline".into()), now);
        o.push("QSO 2".into());
        assert_eq!(o.error.as_deref(), Some("offline"));
        assert!(o.take(now + RETRY / 2).is_none());
        let b = o.take(now + RETRY).unwrap();
        assert_eq!(b, ["QSO 0", "QSO 1", "QSO 2"]);
        o.done(b, Outcome::Sent, now + RETRY);
        assert_eq!(o.error, None);
        assert_eq!(o.wait(now), None);
    }

    #[test]
    fn a_full_queue_drops_the_oldest_and_counts_them() {
        let mut o = filled(CAPACITY + 3);
        assert_eq!(o.len(), CAPACITY);
        assert_eq!(o.dropped, 3);
        assert_eq!(o.take(Instant::now()).unwrap()[0], "QSO 3");
    }

    #[test]
    fn each_queue_keeps_its_own_pace() {
        let now = Instant::now();
        let mut o = Outbox::new(Duration::from_secs(1));
        o.push("a".into());
        o.push("b".into());
        let b = o.take(now).unwrap();
        o.push("c".into());
        o.done(b, Outcome::Sent, now);
        assert_eq!(o.wait(now), Some(Duration::from_secs(1)));
    }

    #[test]
    fn a_refusal_stops_posting_without_losing_lines() {
        let now = Instant::now();
        let mut o = filled(2);
        let b = o.take(now).unwrap();
        o.done(b, Outcome::Refused("HTTP 404".into()), now);
        assert!(o.take(now + RETRY * 10).is_none());
        assert_eq!(o.wait(now), None);
        assert_eq!((o.len(), o.error.as_deref()), (2, Some("HTTP 404")));
        o.resume();
        assert_eq!(o.take(now).unwrap(), ["QSO 0", "QSO 1"]);
    }
}
