use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

pub use haml::{Command as RigCommand, Rig, RigCaps, RigEvent};

use zlog_core::{Band, Mode};

use crate::Sections;

pub trait RigEntry {
    fn band(&self) -> Band;

    fn mode(&self) -> Mode;

    fn set_band(&mut self, band: Band);

    fn set_mode(&mut self, mode: Mode);
}

pub trait RigControl {
    fn number(&self) -> u8;

    fn hz(&self) -> Option<u64>;

    fn connected(&self) -> bool;

    fn select(&mut self, n: u8) -> bool;

    fn toggle(&mut self) -> bool;

    fn follow(&mut self, now: Instant, entry: &mut dyn RigEntry);

    fn qsy(&mut self, hz: u64, mode: Option<Mode>, entry: &mut dyn RigEntry);

    fn sections(&self) -> Vec<String>;

    fn load_sections(&mut self, sections: &Sections, changed: &[String]);

    fn use_port(&mut self, _n: u8, _port: &str, _section: &mut crate::Section) {}
}

pub fn qsy_entry(entry: &mut dyn RigEntry, band: Option<Band>, mode: Option<Mode>) {
    if let Some(band) = band {
        if entry.band() != band {
            entry.set_band(band);
        }
    }
    if let Some(m) = mode {
        entry.set_mode(m);
    }
}

pub fn select_number(current: &mut u8, n: u8) -> bool {
    if !(1..=2).contains(&n) || n == *current {
        return false;
    }
    *current = n;
    true
}

pub trait RigSource: Send + Sync {
    fn ports(&self) -> Vec<String>;

    fn open(&self, port: &str) -> Option<Result<Box<dyn Rig>, String>>;

    fn owns(&self, port: &str) -> bool;
}

#[derive(Default)]
struct Shown {
    rig: Option<(String, RigCaps)>,
    inbox: Vec<RigCommand>,
    subscribers: Vec<Sender<RigEvent>>,
}

#[derive(Clone, Default)]
pub struct Exposed(Arc<Mutex<Shown>>);

impl Exposed {
    fn lock(&self) -> MutexGuard<'_, Shown> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set(&self, rig: Option<(String, RigCaps)>) {
        let mut s = self.lock();
        s.rig = rig;
        s.inbox.clear();
    }

    pub fn rig(&self) -> Option<(String, RigCaps)> {
        self.lock().rig.clone()
    }

    pub fn push(&self, command: RigCommand) {
        let mut s = self.lock();
        if s.rig.is_some() {
            s.inbox.push(command);
        }
    }

    pub fn take(&self) -> Vec<RigCommand> {
        std::mem::take(&mut self.lock().inbox)
    }

    pub fn publish(&self, event: RigEvent) {
        self.lock().subscribers.retain(|s| s.send(event).is_ok());
    }

    pub fn subscribe(&self) -> Receiver<RigEvent> {
        let (tx, rx) = mpsc::channel();
        self.lock().subscribers.push(tx);
        rx
    }
}

pub fn exposed(n: u8) -> Exposed {
    static RIGS: OnceLock<[Exposed; 2]> = OnceLock::new();
    RIGS.get_or_init(Default::default)[usize::from(n == 2)].clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exposed_rig_takes_commands_only_while_connected() {
        let e = Exposed::default();
        let events = e.subscribe();
        e.push(RigCommand::ClearRit);
        assert!(e.take().is_empty(), "not connected");
        e.set(Some(("IC-7300".into(), RigCaps::ALL)));
        e.push(RigCommand::ClearRit);
        assert_eq!(e.take(), [RigCommand::ClearRit]);
        e.publish(RigEvent::Link(haml::Link::Connected));
        assert_eq!(
            events.try_recv().ok(),
            Some(RigEvent::Link(haml::Link::Connected))
        );
        assert_eq!(e.rig().unwrap().0, "IC-7300");
    }

    struct Row(Band, Mode, usize);

    impl RigEntry for Row {
        fn band(&self) -> Band {
            self.0
        }
        fn mode(&self) -> Mode {
            self.1
        }
        fn set_band(&mut self, band: Band) {
            self.0 = band;
            self.2 += 1;
        }
        fn set_mode(&mut self, mode: Mode) {
            self.1 = mode;
        }
    }

    #[test]
    fn a_qsy_moves_the_row_only_where_it_differs() {
        let mut r = Row(Band::B7, Mode::Cw, 0);
        qsy_entry(&mut r, Some(Band::B7), Some(Mode::Ssb));
        assert_eq!((r.0, r.1, r.2), (Band::B7, Mode::Ssb, 0));
        qsy_entry(&mut r, Some(Band::B14), None);
        assert_eq!((r.0, r.1, r.2), (Band::B14, Mode::Ssb, 1));
        qsy_entry(&mut r, None, None);
        assert_eq!(r.0, Band::B14);
    }

    #[test]
    fn two_rigs_without_rig_control() {
        let mut n = 1;
        assert!(!select_number(&mut n, 1) && !select_number(&mut n, 3));
        assert!(select_number(&mut n, 2));
        assert_eq!(n, 2);
    }
}
