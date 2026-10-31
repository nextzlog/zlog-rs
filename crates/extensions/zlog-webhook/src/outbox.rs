use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const CAPACITY: usize = 500;
pub const PACE: Duration = Duration::from_secs(2);
pub const RETRY: Duration = Duration::from_secs(30);
pub const BATCH_LINES: usize = 10;
pub const BATCH_CHARS: usize = 1900;

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Sent,
    Wait(Duration),
    Failed(String),
    Refused(String),
}

#[derive(Debug)]
pub struct Outbox {
    lines: VecDeque<String>,
    pace: Duration,
    not_before: Option<Instant>,
    sending: bool,
    pub sent: usize,
    pub dropped: usize,
    pub error: Option<String>,
    pub refused: bool,
}

impl Default for Outbox {
    fn default() -> Self {
        Outbox::new(PACE)
    }
}

impl Outbox {
    pub fn new(pace: Duration) -> Self {
        Outbox {
            lines: VecDeque::new(),
            pace,
            not_before: None,
            sending: false,
            sent: 0,
            dropped: 0,
            error: None,
            refused: false,
        }
    }

    pub fn push(&mut self, line: String) {
        let mut chunk = String::new();
        let mut size = 0;
        for c in line.chars() {
            let n = escaped_len(c);
            if size + n > BATCH_CHARS {
                self.lines.push_back(std::mem::take(&mut chunk));
                size = 0;
            }
            chunk.push(c);
            size += n;
        }
        self.lines.push_back(chunk);
        self.trim();
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn busy(&self) -> bool {
        self.sending || !(self.refused || self.lines.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn take(&mut self, now: Instant) -> Option<Vec<String>> {
        if self.refused || self.lines.is_empty() || self.not_before.is_some_and(|t| now < t) {
            return None;
        }
        let mut batch = Vec::new();
        let mut chars = 0;
        while let Some(next) = self.lines.front() {
            let size = next.chars().map(escaped_len).sum::<usize>() + 1;
            if !batch.is_empty() && (batch.len() == BATCH_LINES || chars + size > BATCH_CHARS) {
                break;
            }
            chars += size;
            batch.extend(self.lines.pop_front());
        }
        self.sending = true;
        Some(batch)
    }

    pub fn done(&mut self, batch: Vec<String>, outcome: Outcome, now: Instant) {
        self.sending = false;
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

    pub fn resume(&mut self) {
        self.refused = false;
        self.error = None;
        self.dropped = 0;
        self.not_before = None;
    }

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

fn escaped_len(c: char) -> usize {
    crate::webhook::markdown_escape(c.encode_utf8(&mut [0; 4]))
        .chars()
        .count()
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
    fn the_escaped_batch_fits_discord() {
        let mut o = Outbox::new(Duration::ZERO);
        for _ in 0..10 {
            o.push(format!("JA1ZLO 7MHz ({})", "-:-(".repeat(40)));
        }
        o.push("-".repeat(5000));
        while let Some(b) = o.take(Instant::now()) {
            let content = crate::webhook::markdown_escape(&b.join("\n"));
            assert!(content.chars().count() <= BATCH_CHARS, "{}", content.len());
        }
        assert!(o.is_empty());
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
