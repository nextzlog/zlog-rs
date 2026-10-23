//! Two rigs, RIG1 and RIG2, one of them selected (zLog `FCurrentRigSet`): the entry
//! row works with the selected rig, the other is only polled.
//!
//! - Shift+X (`actionToggleRig`, `TMainForm.actionToggleRigExecute`) selects the other
//!   rig, but only when it is connected: `GetNextRigID` skips rigs that are not set up,
//!   so with one rig the key keeps it.
//! - Selecting a rig, by Shift+X, in the rig window or from a panel (the SO2R
//!   control panel), lets the entry row follow that rig's band and mode
//!   (`TMainForm.SwitchRig` calls `UpdateBandAndMode`).
//! - Each rig has a settings section of its own: [`SECTIONS`].
//!
//! [`Rigs`] dereferences to the selected rig, so the window uses it as it would one.

use std::ops::{Deref, DerefMut};
use std::time::Instant;

use zlog_core::Station;

use crate::entry::Entry;
use crate::rig::RigControl;

/// The settings sections of rig 1 and rig 2.
pub const SECTIONS: [&str; 2] = ["rig", "rig2"];

#[derive(Default)]
pub struct Rigs {
    slots: [RigControl; 2],
    /// Index of the selected rig in `slots`.
    current: usize,
    /// The selected rig is to be followed from the next tick (see
    /// [`RigControl::follow_from`]).
    switched: bool,
}

impl Deref for Rigs {
    type Target = RigControl;

    fn deref(&self) -> &RigControl {
        &self.slots[self.current]
    }
}

impl DerefMut for Rigs {
    fn deref_mut(&mut self) -> &mut RigControl {
        &mut self.slots[self.current]
    }
}

impl Rigs {
    /// The selected rig, 1 or 2.
    pub fn number(&self) -> u8 {
        self.current as u8 + 1
    }

    /// The settings section of the selected rig.
    pub fn section(&self) -> &'static str {
        SECTIONS[self.current]
    }

    /// Rig 1 or 2, selected or not.
    pub fn slot_mut(&mut self, n: u8) -> &mut RigControl {
        &mut self.slots[usize::from(n == 2)]
    }

    /// Select rig `n` (1 or 2), connected or not (`SwitchRig`); true when it changed.
    pub fn select(&mut self, n: u8) -> bool {
        if !(1..=2).contains(&n) || n == self.number() {
            return false;
        }
        self.current = usize::from(n) - 1;
        self.slots[self.current].follow();
        self.switched = true;
        true
    }

    /// Shift+X: select the other rig when it is connected; true when it changed.
    pub fn toggle(&mut self) -> bool {
        let other = 1 - self.current;
        self.slots[other].connected() && self.select(other as u8 + 1)
    }

    /// Run both rigs; the selected one is kept in step with the entry row. True when
    /// the row changed.
    pub fn tick(&mut self, now: Instant, entry: &mut Entry, station: &Station) -> bool {
        self.slots[1 - self.current].poll(now);
        if std::mem::take(&mut self.switched) {
            self.slots[self.current].follow_from((entry.band, entry.mode));
        }
        self.slots[self.current].tick(now, entry, station)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::{RigConfig, SIMULATOR};
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
        let st = Station::default();
        for _ in 0..ms / 10 {
            *t += Duration::from_millis(10);
            rigs.tick(*t, entry, &st);
        }
    }

    #[test]
    fn the_entry_row_follows_the_selected_rig() {
        let mut rigs = Rigs::default();
        rigs.slot_mut(1).connect(config());
        rigs.slot_mut(2).connect(config());
        rigs.slot_mut(2).sim.as_ref().unwrap().tune(21_030_000);
        rigs.slot_mut(2)
            .sim
            .as_ref()
            .unwrap()
            .press_mode(zlog_rig::RigMode::Usb);
        let mut entry = Entry::new(Band::B14, Mode::Ssb, Power::P010);
        let mut t = Instant::now();
        run(&mut rigs, &mut t, &mut entry, 600);
        // Rig 1 (the simulator starts on 7.012 MHz CW) has the entry row.
        assert_eq!(
            (rigs.number(), entry.band, entry.mode),
            (1, Band::B7, Mode::Cw)
        );

        // Shift+X: rig 2 is connected, so it is selected and the row follows it.
        assert!(rigs.toggle());
        run(&mut rigs, &mut t, &mut entry, 600);
        assert_eq!(
            (rigs.number(), entry.band, entry.mode),
            (2, Band::B21, Mode::Ssb)
        );

        // A band typed in the entry row tunes rig 2 only.
        entry.band = Band::B28;
        run(&mut rigs, &mut t, &mut entry, 600);
        let freq = |rigs: &mut Rigs, n| rigs.slot_mut(n).sim.as_ref().unwrap().with(|r| r.freq_a);
        assert_eq!(freq(&mut rigs, 2), 28_600_000);
        assert_eq!(freq(&mut rigs, 1), 7_012_000);

        // Turning rig 1's dial while rig 2 is selected leaves the row alone ...
        rigs.slot_mut(1).sim.as_ref().unwrap().tune(3_510_000);
        run(&mut rigs, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B28);
        // ... until rig 1 is selected again.
        assert!(rigs.toggle());
        run(&mut rigs, &mut t, &mut entry, 600);
        assert_eq!(
            (rigs.number(), entry.band, entry.mode),
            (1, Band::B3_5, Mode::Cw)
        );
    }

    #[test]
    fn a_band_typed_before_the_new_rig_reports_is_sent_to_it() {
        let mut rigs = Rigs::default();
        rigs.slot_mut(1).connect(config());
        let mut entry = Entry::new(Band::B14, Mode::Ssb, Power::P010);
        let mut t = Instant::now();
        run(&mut rigs, &mut t, &mut entry, 600);
        // Rig 2 is selected the moment it is connected, before its first status.
        rigs.slot_mut(2).connect(config());
        assert!(rigs.toggle());
        let st = Station::default();
        rigs.tick(t, &mut entry, &st);
        assert!(rigs.state().is_none_or(|s| s.freq().is_none()));
        entry.band = Band::B28;
        run(&mut rigs, &mut t, &mut entry, 600);
        let freq = rigs.slot_mut(2).sim.as_ref().unwrap().with(|r| r.freq_a);
        assert_eq!((entry.band, freq / 1_000_000), (Band::B28, 28));
    }

    #[test]
    fn shift_x_keeps_the_only_rig() {
        let mut rigs = Rigs::default();
        rigs.slot_mut(1).connect(config());
        assert!(!rigs.toggle());
        assert_eq!(rigs.number(), 1);
        // A panel may still select rig 2, as zLog's `actionSelectRig2` does.
        assert!(rigs.select(2));
        assert!(!rigs.select(2) && !rigs.select(3));
        assert_eq!(
            (rigs.number(), rigs.section(), rigs.connected()),
            (2, "rig2", false)
        );
    }
}
