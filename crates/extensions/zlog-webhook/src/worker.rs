use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::outbox::Outcome;
use crate::site::Site;
use crate::webhook::Sender;
use crate::Hub;

const IDLE: Duration = Duration::from_secs(1);

pub fn start(hub: Arc<Hub>, site: Site, mut sender: Box<dyn Sender>) {
    std::thread::spawn(move || loop {
        let wait = step(&hub, site, sender.as_mut(), Instant::now())
            .unwrap_or(IDLE)
            .min(IDLE);
        let state = hub.state.lock().unwrap();
        let _ = hub.wake.wait_timeout(state, wait).unwrap();
    });
}

pub fn step(hub: &Hub, site: Site, sender: &mut dyn Sender, now: Instant) -> Option<Duration> {
    let (batch, url) = {
        let mut s = hub.state.lock().unwrap();
        if !s.config.ready(site) {
            return None;
        }
        match s.outbox_mut(site).take(now) {
            Some(batch) => (batch, s.config.target(site).url.clone()),
            None => return s.outbox(site).wait(now),
        }
    };
    let outcome = sender.send(site, &url, &batch.join("\n"));
    let answered = now.max(Instant::now());
    let mut s = hub.state.lock().unwrap();
    let moved = s.config.target(site).url != url;
    let outcome = match outcome {
        Outcome::Failed(_) | Outcome::Refused(_) if moved => Outcome::Wait(Duration::ZERO),
        o => o,
    };
    let o = s.outbox_mut(site);
    o.done(batch, outcome, answered);
    let wait = o.wait(answered);
    drop(s);
    zlog_core::wake::wake();
    wait
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outbox::PACE;

    struct Fake {
        answers: Vec<Outcome>,
        sent: Vec<String>,
    }

    impl Fake {
        fn new(answers: Vec<Outcome>) -> Self {
            Fake {
                answers,
                sent: Vec::new(),
            }
        }
    }

    impl Sender for Fake {
        fn send(&mut self, _site: Site, _url: &str, content: &str) -> Outcome {
            self.sent.push(content.into());
            self.answers.remove(0)
        }
    }

    fn hub(lines: usize) -> Hub {
        let hub = Hub::default();
        {
            let mut s = hub.state.lock().unwrap();
            for site in Site::ALL {
                let t = s.config.target_mut(site);
                t.enabled = true;
                t.url = "https://example.invalid/hook".into();
                for i in 0..lines {
                    s.outbox_mut(site).push(format!("QSO {i}"));
                }
            }
        }
        hub
    }

    const D: Site = Site::Discord;

    #[test]
    fn a_finished_post_wakes_the_window() {
        if !zlog_core::wake::alone(concat!(
            module_path!(),
            "::a_finished_post_wakes_the_window"
        )) {
            return;
        }
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        zlog_core::wake::set(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let hub = hub(1);
        let mut fake = Fake::new(vec![Outcome::Failed("offline".into())]);
        step(&hub, D, &mut fake, Instant::now());
        assert!(woken.load(std::sync::atomic::Ordering::SeqCst) > 0);
        assert!(hub.state.lock().unwrap().outbox(D).error.is_some());
    }

    #[test]
    fn batches_go_out_paced_and_retried_after_a_rate_limit() {
        let hub = hub(12);
        let mut fake = Fake::new(vec![
            Outcome::Wait(Duration::from_secs(2)),
            Outcome::Sent,
            Outcome::Sent,
        ]);
        let t = Instant::now() + Duration::from_secs(3600);
        assert_eq!(step(&hub, D, &mut fake, t), Some(Duration::from_secs(2)));
        assert_eq!(
            step(&hub, D, &mut fake, t + Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(fake.sent.len(), 1);
        assert_eq!(
            step(&hub, D, &mut fake, t + Duration::from_secs(2)),
            Some(PACE)
        );
        assert_eq!(
            step(&hub, D, &mut fake, t + Duration::from_secs(3)),
            Some(PACE - Duration::from_secs(1))
        );
        assert_eq!(step(&hub, D, &mut fake, t + Duration::from_secs(4)), None);
        assert_eq!(fake.sent[0], fake.sent[1]);
        assert_eq!(fake.sent[1].lines().count(), 10);
        assert_eq!(fake.sent[2], "QSO 10\nQSO 11");
        assert_eq!(hub.state.lock().unwrap().outbox(D).sent, 12);
    }

    #[test]
    fn the_wait_counts_from_the_answer() {
        struct Slow;
        impl Sender for Slow {
            fn send(&mut self, _site: Site, _url: &str, _content: &str) -> Outcome {
                std::thread::sleep(Duration::from_millis(300));
                Outcome::Wait(Duration::from_secs(5))
            }
        }
        let hub = hub(1);
        let before = Instant::now();
        let wait = step(&hub, D, &mut Slow, before).unwrap();
        assert!(wait <= Duration::from_secs(5));
        assert!(
            hub.state.lock().unwrap().outbox(D).wait(before).unwrap()
                >= Duration::from_millis(5300)
        );
    }

    #[test]
    fn nothing_is_sent_while_off() {
        let hub = hub(1);
        hub.state.lock().unwrap().config.target_mut(D).enabled = false;
        let mut fake = Fake::new(Vec::new());
        assert_eq!(step(&hub, D, &mut fake, Instant::now()), None);
        assert_eq!(hub.state.lock().unwrap().outbox(D).len(), 1);
    }

    #[test]
    fn a_late_refusal_of_an_old_url_is_ignored() {
        struct Moving<'a>(&'a Hub);
        impl Sender for Moving<'_> {
            fn send(&mut self, site: Site, _url: &str, _content: &str) -> Outcome {
                let mut s = self.0.state.lock().unwrap();
                s.config.target_mut(site).url = "https://example.invalid/new".into();
                s.outbox_mut(site).resume();
                Outcome::Refused("HTTP 404".into())
            }
        }
        let hub = hub(1);
        let t = Instant::now() + Duration::from_secs(3600);
        assert!(step(&hub, D, &mut Moving(&hub), t).is_some());
        let s = hub.state.lock().unwrap();
        let o = s.outbox(D);
        assert!(!o.refused && o.error.is_none() && o.len() == 1);
    }

    #[test]
    fn the_destinations_are_independent() {
        let both = hub(2);
        let t = Instant::now() + Duration::from_secs(3600);
        let mut slack = Fake::new(vec![Outcome::Refused("HTTP 404 no_service".into())]);
        assert_eq!(step(&both, Site::Slack, &mut slack, t), None);
        let mut discord = Fake::new(vec![Outcome::Sent]);
        assert_eq!(step(&both, D, &mut discord, t), None);
        assert_eq!(step(&both, Site::Slack, &mut slack, t + PACE * 10), None);
        assert_eq!(slack.sent.len(), 1);
        let s = both.state.lock().unwrap();
        assert_eq!((s.outbox(D).sent, s.outbox(D).error.as_deref()), (2, None));
        let o = s.outbox(Site::Slack);
        assert!(o.refused && o.len() == 2 && o.sent == 0);
        drop(s);
        let eleven = hub(11);
        let pace = |site| step(&eleven, site, &mut Fake::new(vec![Outcome::Sent]), t);
        assert_eq!(pace(Site::Slack), Some(Duration::from_secs(1)));
        assert_eq!(pace(D), Some(Duration::from_secs(2)));
    }
}
