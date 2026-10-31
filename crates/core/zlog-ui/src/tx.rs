use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub use haml::tx::{ticket, Delays, Signal, TxCommand, TxEvent};
pub use haml::{Keyer, KeyerCaps, KeyerEvent, Rx, So2r, So2rCaps, Timing};

use haml::device::{Device, Driver};
use haml::Arbiter;

struct Inner {
    arbiter: Arbiter,
    delays: Delays,
    subscribers: Vec<Sender<TxEvent>>,
}

#[derive(Clone)]
pub struct TxHub(Arc<Mutex<Inner>>);

impl Default for TxHub {
    fn default() -> Self {
        let delays = Delays {
            before: Duration::ZERO,
            after: Duration::ZERO,
        };
        TxHub(Arc::new(Mutex::new(Inner {
            arbiter: Arbiter::new(2, delays),
            delays,
            subscribers: Vec::new(),
        })))
    }
}

pub fn hub() -> TxHub {
    static HUB: OnceLock<TxHub> = OnceLock::new();
    HUB.get_or_init(TxHub::default).clone()
}

impl TxHub {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn run<R>(&self, f: impl FnOnce(&mut Arbiter, &mut Vec<TxEvent>) -> R) -> R {
        let mut inner = self.lock();
        let mut out = Vec::new();
        let r = f(&mut inner.arbiter, &mut out);
        for e in out {
            inner.subscribers.retain(|s| s.send(e.clone()).is_ok());
        }
        r
    }

    pub fn send(&self, command: TxCommand) {
        self.run(|a, out| a.handle(command, Instant::now(), out));
    }

    pub fn tick(&self) {
        self.run(|a, out| a.tick(Instant::now(), out));
    }

    pub fn subscribe(&self) -> Receiver<TxEvent> {
        let (tx, rx) = mpsc::channel();
        self.lock().subscribers.push(tx);
        rx
    }

    pub fn set_keyer(&self, keyer: Option<Box<dyn Keyer>>) {
        self.run(|a, out| a.set_keyer(keyer, out));
    }

    pub fn set_so2r(&self, so2r: Option<Box<dyn So2r>>) {
        self.run(|a, _| a.set_so2r(so2r));
    }

    pub fn set_timing(&self, timing: Timing) {
        self.run(|a, _| a.set_timing(timing));
    }

    pub fn set_delays(&self, delays: Delays) {
        let mut inner = self.lock();
        inner.delays = delays;
        inner.arbiter.set_delays(delays);
    }

    pub fn delays(&self) -> Delays {
        self.lock().delays
    }

    pub fn transmit_on(&self, radio: u8) {
        self.run(|a, _| a.transmit_on(radio));
    }

    pub fn keyer<R>(&self, f: impl FnOnce(&dyn Keyer) -> R) -> Option<R> {
        self.lock().arbiter.keyer().map(f)
    }

    pub fn so2r<R>(&self, f: impl FnOnce(&dyn So2r) -> R) -> Option<R> {
        self.lock().arbiter.so2r().map(f)
    }

    pub fn has_so2r(&self) -> bool {
        self.so2r(|_| ()).is_some()
    }

    pub fn on_air(&self) -> Option<u8> {
        self.lock().arbiter.on_air()
    }

    pub fn has_keyer(&self) -> bool {
        self.keyer(|_| ()).is_some()
    }

    pub fn busy(&self) -> bool {
        self.lock().arbiter.busy()
    }

    pub fn transmitting(&self) -> Option<u8> {
        self.lock().arbiter.transmitting()
    }

    pub fn device(&self) -> Device<TxCommand, TxEvent> {
        let events = self.subscribe();
        Device::spawn(
            "zlog-tx",
            Bridge {
                hub: self.clone(),
                events,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keying {
    Keyed(bool),
    Failed(String),
    Halt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Idle,
    Asked(u64, u8),
    Keyed(u64, u8),
    Cancelled(u64),
    Released(u64, u8),
    Tail(u8),
}

pub struct PttLink {
    hub: TxHub,
    events: Receiver<TxEvent>,
    link: Link,
}

impl PttLink {
    pub fn new(hub: TxHub) -> Self {
        PttLink {
            events: hub.subscribe(),
            hub,
            link: Link::Idle,
        }
    }

    pub fn lead(&self) -> Duration {
        self.hub.delays().before
    }

    pub fn active(&self) -> bool {
        self.link != Link::Idle
    }

    pub fn ptt(&mut self, on: bool, radio: u8) {
        self.link = match (on, self.link) {
            (true, Link::Idle) => {
                let id = ticket();
                self.hub.send(TxCommand::Send {
                    id,
                    radio,
                    signal: Signal::Voice,
                });
                Link::Asked(id, radio)
            }
            (false, Link::Asked(id, _)) => {
                self.hub.send(TxCommand::Release(id));
                Link::Cancelled(id)
            }
            (false, Link::Keyed(id, r)) => {
                self.hub.send(TxCommand::Release(id));
                Link::Released(id, r)
            }
            (_, link) => link,
        };
    }

    pub fn poll(&mut self) -> Vec<Keying> {
        self.hub.tick();
        let mut out = Vec::new();
        for e in self.events.try_iter().collect::<Vec<_>>() {
            match (e, self.link) {
                (TxEvent::Started { id, .. }, Link::Asked(mine, r)) if id == mine => {
                    self.link = Link::Keyed(mine, r);
                    out.push(Keying::Keyed(true));
                }
                (TxEvent::Finished { id, .. }, Link::Released(mine, r)) if id == mine => {
                    self.link = Link::Tail(r);
                }
                (TxEvent::Finished { id, .. }, Link::Cancelled(mine)) if id == mine => {
                    self.link = Link::Idle;
                    out.push(Keying::Keyed(false));
                }
                (TxEvent::Started { .. }, Link::Tail(_)) => {
                    self.link = Link::Idle;
                    out.push(Keying::Keyed(false));
                }
                (TxEvent::Error(e), Link::Asked(mine, _)) => {
                    self.hub.send(TxCommand::Release(mine));
                    self.events.try_iter().count();
                    self.link = Link::Idle;
                    out.push(Keying::Failed(e));
                    break;
                }
                (TxEvent::Stopped, link) if link != Link::Idle => {
                    self.link = Link::Idle;
                    out.push(Keying::Halt);
                }
                _ => {}
            }
        }
        if let Link::Tail(r) = self.link {
            if self.hub.transmitting() != Some(r) {
                self.link = Link::Idle;
                out.push(Keying::Keyed(false));
            }
        }
        out
    }
}

struct Bridge {
    hub: TxHub,
    events: Receiver<TxEvent>,
}

impl Driver for Bridge {
    type Command = TxCommand;
    type Event = TxEvent;

    fn handle(&mut self, command: TxCommand, _now: Instant, out: &mut Vec<TxEvent>) {
        self.hub.send(command);
        out.extend(self.events.try_iter());
    }

    fn tick(&mut self, _now: Instant, out: &mut Vec<TxEvent>) {
        self.hub.tick();
        out.extend(self.events.try_iter());
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Log(Arc<Mutex<Vec<String>>>);

    impl Log {
        fn push(&self, s: String) {
            self.0.lock().unwrap().push(s);
        }
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut self.0.lock().unwrap())
        }
    }

    struct K(Log);

    impl Keyer for K {
        fn caps(&self) -> KeyerCaps {
            KeyerCaps::default()
        }
        fn send(&mut self, text: &str) {
            self.0.push(format!("send {text}"));
        }
        fn abort(&mut self) {
            self.0.push("abort".into());
        }
        fn set_wpm(&mut self, _wpm: u32) {}
        fn wpm(&self) -> u32 {
            25
        }
        fn set_ptt(&mut self, on: bool) {
            self.0.push(format!("ptt {on}"));
        }
        fn busy(&self) -> bool {
            false
        }
        fn poll(&mut self) -> Vec<KeyerEvent> {
            Vec::new()
        }
    }

    #[test]
    fn only_the_hub_keys_and_every_subscriber_hears_it() {
        let hub = TxHub::default();
        let log = Log::default();
        assert!(!hub.has_keyer());
        hub.set_keyer(Some(Box::new(K(log.clone()))));
        let events = hub.subscribe();
        hub.send(TxCommand::Send {
            id: 1,
            radio: 1,
            signal: Signal::Cw("CQ".into()),
        });
        assert_eq!(log.take(), ["send CQ"]);
        hub.tick();
        assert!(!hub.busy());
        let got: Vec<_> = events.try_iter().collect();
        assert!(
            got.contains(&TxEvent::Finished { id: 1, radio: 1 }),
            "{got:?}"
        );
        hub.send(TxCommand::Send {
            id: 2,
            radio: 2,
            signal: Signal::Voice,
        });
        assert_eq!(log.take(), ["ptt true"]);
        hub.send(TxCommand::Release(2));
        assert_eq!(log.take(), ["ptt false"]);
    }

    #[test]
    fn the_device_carries_commands_and_events_across_threads() {
        let hub = TxHub::default();
        let log = Log::default();
        hub.set_keyer(Some(Box::new(K(log.clone()))));
        let device = hub.device();
        let events = device.subscribe();
        device
            .send(TxCommand::Send {
                id: 3,
                radio: 1,
                signal: Signal::Cw("TU".into()),
            })
            .unwrap();
        let start = Instant::now();
        while !events
            .try_iter()
            .any(|e| e == TxEvent::Finished { id: 3, radio: 1 })
        {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(log.take(), ["send TU"]);
    }

    #[test]
    fn a_ptt_link_waits_for_the_arbiter_and_releases_after_the_tail() {
        let hub = TxHub::default();
        let log = Log::default();
        hub.set_keyer(Some(Box::new(K(log.clone()))));
        hub.set_delays(Delays {
            before: Duration::from_millis(30),
            after: Duration::from_millis(30),
        });
        let mut link = PttLink::new(hub.clone());
        assert_eq!(link.lead(), Duration::from_millis(30));
        link.ptt(true, 2);
        assert!(link.poll().is_empty() && link.active());
        let start = Instant::now();
        let mut got = Vec::new();
        while got.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
            got = link.poll();
        }
        assert_eq!(got, [Keying::Keyed(true)]);
        link.ptt(false, 2);
        assert_eq!(hub.transmitting(), Some(2), "tail");
        while got != [Keying::Keyed(false)] {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
            got = link.poll();
        }
        assert!(!link.active() && hub.transmitting().is_none());
    }

    #[test]
    fn a_ptt_link_reports_failures_stops_and_cancels() {
        let hub = TxHub::default();
        let mut link = PttLink::new(hub.clone());
        link.ptt(true, 3);
        assert_eq!(link.poll(), [Keying::Failed("no radio 3".into())]);
        link.ptt(true, 1);
        assert_eq!(link.poll(), [Keying::Keyed(true)]);
        hub.send(TxCommand::Stop);
        assert_eq!(link.poll(), [Keying::Halt]);
        hub.set_delays(Delays {
            before: Duration::from_secs(60),
            after: Duration::ZERO,
        });
        link.ptt(true, 1);
        assert_eq!(hub.transmitting(), Some(1), "in the lead");
        link.ptt(false, 1);
        assert_eq!(link.poll(), [Keying::Keyed(false)]);
        assert!(!link.active() && !hub.busy());
    }

    #[test]
    fn two_links_on_one_radio_follow_their_own_tickets() {
        let hub = TxHub::default();
        let (mut ft8, mut voice) = (PttLink::new(hub.clone()), PttLink::new(hub.clone()));
        ft8.ptt(true, 1);
        voice.ptt(true, 1);
        assert_eq!(ft8.poll(), [Keying::Keyed(true)]);
        assert!(voice.poll().is_empty(), "the voice waits for the arbiter");
        voice.ptt(false, 1);
        assert_eq!(voice.poll(), [Keying::Keyed(false)]);
        assert!(ft8.poll().is_empty() && ft8.active());
        assert_eq!(hub.transmitting(), Some(1), "FT8 still holds the radio");
        ft8.ptt(false, 1);
        assert_eq!(ft8.poll(), [Keying::Keyed(false)]);
        assert!(voice.poll().is_empty());
        assert!(hub.transmitting().is_none());
    }
}
