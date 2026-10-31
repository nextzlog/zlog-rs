use std::ops::{Deref, DerefMut};
use std::time::Instant;

use zlog_ui::rig::RigEntry;

use crate::control::{Control, Drivers};

pub const SECTIONS: [&str; 2] = ["rig", "rig2"];

#[derive(Default)]
pub struct Rigs {
    slots: [Control; 2],
    current: usize,
    switched: bool,
    reopened: u64,
}

impl Deref for Rigs {
    type Target = Control;

    fn deref(&self) -> &Control {
        &self.slots[self.current]
    }
}

impl DerefMut for Rigs {
    fn deref_mut(&mut self) -> &mut Control {
        &mut self.slots[self.current]
    }
}

impl Rigs {
    pub fn new(drivers: Drivers) -> Self {
        let mut slots = [Control::new(drivers.clone()), Control::new(drivers)];
        for (n, slot) in (1..).zip(slots.iter_mut()) {
            slot.exposed = Some(zlog_ui::rig::exposed(n));
        }
        Rigs {
            slots,
            ..Default::default()
        }
    }

    pub fn number(&self) -> u8 {
        self.current as u8 + 1
    }

    pub fn section(&self) -> &'static str {
        SECTIONS[self.current]
    }

    pub fn slot(&self, n: u8) -> &Control {
        &self.slots[usize::from(n == 2)]
    }

    pub fn slot_mut(&mut self, n: u8) -> &mut Control {
        &mut self.slots[usize::from(n == 2)]
    }

    pub fn select(&mut self, n: u8) -> bool {
        if !(1..=2).contains(&n) || n == self.number() {
            return false;
        }
        self.current = usize::from(n) - 1;
        self.slots[self.current].follow();
        self.switched = true;
        true
    }

    pub fn toggle(&mut self) -> bool {
        let other = 1 - self.current;
        self.slots[other].connected() && self.select(other as u8 + 1)
    }

    pub fn tick(&mut self, now: Instant, entry: &mut dyn RigEntry) -> bool {
        let epoch = zlog_ui::device::reopened();
        if epoch != self.reopened {
            self.reopened = epoch;
            for slot in &mut self.slots {
                slot.reopen();
            }
        }
        self.slots[1 - self.current].poll(now);
        if std::mem::take(&mut self.switched) {
            self.slots[self.current].follow_from((entry.band(), entry.mode()));
        }
        self.slots[self.current].tick(now, entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::tests::{drivers, sim, Entry};
    use crate::control::{RigConfig, SIMULATOR};
    use std::time::Duration;
    use zlog_core::{Band, Mode, Power};

    fn config() -> RigConfig {
        RigConfig {
            model: 1,
            port: SIMULATOR.into(),
            baud: 19200,
            transverter_khz: 0,
            init_freq_on_mode: false,
            ignore_rig_mode: false,
            rtty_keeps_rig_mode: false,
        }
    }

    fn run(rigs: &mut Rigs, t: &mut Instant, entry: &mut Entry, ms: u64) {
        for _ in 0..ms / 10 {
            *t += Duration::from_millis(10);
            rigs.tick(*t, entry);
        }
    }

    #[test]
    fn the_entry_row_follows_the_selected_rig() {
        let mut rigs = Rigs::new(drivers());
        rigs.slot_mut(1).connect(config(), "en");
        rigs.slot_mut(2).connect(config(), "en");
        sim(rigs.slot_mut(2)).turn_dial(21_030_000);
        sim(rigs.slot_mut(2)).press_mode(haml::Mode::Usb);
        let mut entry = Entry::new(Band::B14, Mode::Ssb, Power::P010);
        let mut t = Instant::now();
        run(&mut rigs, &mut t, &mut entry, 600);
        assert_eq!(
            (rigs.number(), entry.band, entry.mode),
            (1, Band::B7, Mode::Cw)
        );

        assert!(rigs.toggle());
        run(&mut rigs, &mut t, &mut entry, 600);
        assert_eq!(
            (rigs.number(), entry.band, entry.mode),
            (2, Band::B21, Mode::Ssb)
        );

        entry.band = Band::B28;
        run(&mut rigs, &mut t, &mut entry, 600);
        let freq = |rigs: &mut Rigs, n| sim(rigs.slot_mut(n)).with(|r| r.freq_a);
        assert_eq!(freq(&mut rigs, 2), 28_600_000);
        assert_eq!(freq(&mut rigs, 1), 7_012_000);

        sim(rigs.slot_mut(1)).turn_dial(3_510_000);
        run(&mut rigs, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B28);
        assert!(rigs.toggle());
        run(&mut rigs, &mut t, &mut entry, 600);
        assert_eq!(
            (rigs.number(), entry.band, entry.mode),
            (1, Band::B3_5, Mode::Cw)
        );
    }

    #[test]
    fn a_band_typed_before_the_new_rig_reports_is_sent_to_it() {
        let mut rigs = Rigs::new(drivers());
        rigs.slot_mut(1).connect(config(), "en");
        let mut entry = Entry::new(Band::B14, Mode::Ssb, Power::P010);
        let mut t = Instant::now();
        run(&mut rigs, &mut t, &mut entry, 600);
        rigs.slot_mut(2).connect(config(), "en");
        assert!(rigs.toggle());
        rigs.tick(t, &mut entry);
        assert!(rigs.state().is_none_or(|s| s.freq().is_none()));
        entry.band = Band::B28;
        run(&mut rigs, &mut t, &mut entry, 600);
        let freq = sim(rigs.slot_mut(2)).with(|r| r.freq_a);
        assert_eq!((entry.band, freq / 1_000_000), (Band::B28, 28));
    }

    #[test]
    fn shift_x_keeps_the_only_rig() {
        let mut rigs = Rigs::new(drivers());
        rigs.slot_mut(1).connect(config(), "en");
        assert!(!rigs.toggle());
        assert_eq!(rigs.number(), 1);
        assert!(rigs.select(2));
        assert!(!rigs.select(2) && !rigs.select(3));
        assert_eq!(
            (rigs.number(), rigs.section(), rigs.connected()),
            (2, "rig2", false)
        );
    }
}
