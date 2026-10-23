//! Rig control in the window: one rig, a real one on a serial port or the built-in
//! simulator, kept in step with the entry row by zLog's rules (`zlog_rig::sync`).
//!
//! - The rig's frequency sets the band of the entry row, its mode the mode.
//! - A band chosen in the entry row tunes the rig to the last frequency used there, or
//!   to the band plan's lower limit for the mode, and in SSB sets the mode (the
//!   sideband) (`TMainForm.actionBandChangeExecute`, `main.pas`). A band the rig does
//!   not cover is not tuned (`TRig.SetBand`), the SSB mode is still sent, and the next
//!   status from the rig pulls the entry row back to the rig's band
//!   (`TRigControl.OnUpdateStatusProc`, `URigControl.pas`).
//! - A mode chosen in the entry row only sets the rig's mode; with the option
//!   `SetInitFreqWhenChangeMode` (off by default) it also tunes as for a band change.
//!   With the RTTY option `DontChangeRigMode` a change to RTTY leaves the rig alone
//!   (`TMainForm.actionChangeModeExecute`).
//! - With `IgnoreRigMode` the entry row does not follow the rig's mode
//!   (`TRigControl.OnUpdateStatusProc`).
//! - A change the rig reports because it was told to make it is not applied back.

use std::time::Instant;

use zlog_core::{Band, Mode, Station};
use zlog_rig::sync::{band_of, rig_mode, FreqMemory};
use zlog_rig::{Rig, RigState};
use zlog_rig_cat::{Dialect, Family, MODELS};
use zlog_rig_sim::{SimHandle, SimProtocol, SimRadio};
use zlog_ui::Section;

use crate::entry::Entry;

/// Port name standing for the simulator in the settings.
pub const SIMULATOR: &str = "simulator";

/// What the `rig` section of the settings says.
#[derive(Debug, Clone, PartialEq)]
pub struct RigConfig {
    /// Index into [`MODELS`].
    pub model: usize,
    /// A serial port name, or [`SIMULATOR`].
    pub port: String,
    pub baud: u32,
    /// Transverter offset in kHz added to the rig's frequency (`TransverterOffset`);
    /// 0 for none.
    pub transverter_khz: u64,
    /// `SetInitFreqWhenChangeMode`: a mode change also tunes the rig.
    pub init_freq_on_mode: bool,
    /// `IgnoreRigMode` (off by default): the entry row does not follow the rig's mode.
    pub ignore_rig_mode: bool,
    /// RTTY `DontChangeRigMode` (off by default): a mode change to RTTY is not sent.
    pub rtty_keeps_rig_mode: bool,
}

impl RigConfig {
    pub fn load(s: &Section) -> Self {
        let model = s
            .get("model")
            .and_then(|m| MODELS.iter().position(|x| x.name == m))
            .unwrap_or(1);
        RigConfig {
            model,
            port: s.get("port").cloned().unwrap_or_else(|| SIMULATOR.into()),
            baud: s
                .get("baud")
                .and_then(|b| b.parse().ok())
                .unwrap_or(MODELS[model].baud),
            transverter_khz: s
                .get("transverter_khz")
                .and_then(|b| b.parse().ok())
                .unwrap_or(0),
            init_freq_on_mode: s.get("init_freq_on_mode").is_some_and(|v| v == "true"),
            ignore_rig_mode: s.get("ignore_rig_mode").is_some_and(|v| v == "true"),
            rtty_keeps_rig_mode: s.get("rtty_keeps_rig_mode").is_some_and(|v| v == "true"),
        }
    }

    pub fn save(&self, s: &mut Section) {
        s.insert("model".into(), MODELS[self.model].name.into());
        s.insert("port".into(), self.port.clone());
        s.insert("baud".into(), self.baud.to_string());
        s.insert("transverter_khz".into(), self.transverter_khz.to_string());
        s.insert(
            "init_freq_on_mode".into(),
            self.init_freq_on_mode.to_string(),
        );
        s.insert("ignore_rig_mode".into(), self.ignore_rig_mode.to_string());
        s.insert(
            "rtty_keeps_rig_mode".into(),
            self.rtty_keeps_rig_mode.to_string(),
        );
    }
}

fn sim_protocol(family: Family) -> SimProtocol {
    match family {
        Family::Icom(a) => SimProtocol::Icom(a),
        Family::Text(Dialect::Kenwood) => SimProtocol::Text {
            digits: 11,
            yaesu: false,
        },
        Family::Text(Dialect::Yaesu9 | Dialect::Ftdx101) => SimProtocol::Text {
            digits: 9,
            yaesu: true,
        },
        Family::Text(Dialect::Yaesu8) => SimProtocol::Text {
            digits: 8,
            yaesu: true,
        },
    }
}

#[derive(Default)]
pub struct RigControl {
    pub config: Option<RigConfig>,
    rig: Option<Rig>,
    /// The simulated radio, when the port is the simulator.
    pub sim: Option<SimHandle>,
    memory: FreqMemory,
    /// Band and mode last agreed between the rig and the entry row.
    synced: Option<(Band, Mode)>,
    /// What the rig was just told to go to: until it reports it (or a second has
    /// passed), its older status is not applied to the entry row. zLog gets the same
    /// by setting the rig's band and mode at once in `TRig.SetBand` and `TRig.SetMode`.
    expect: Option<Expect>,
    pub message: Option<String>,
}

impl RigControl {
    pub fn connected(&self) -> bool {
        self.rig.is_some()
    }

    pub fn state(&self) -> Option<RigState> {
        self.rig.as_ref().map(|r| r.state)
    }

    pub fn error(&self) -> Option<String> {
        self.rig
            .as_ref()
            .and_then(|r| r.error.as_ref())
            .map(|e| format!("{e:?}"))
    }

    pub fn connect(&mut self, config: RigConfig) {
        let model = MODELS[config.model];
        let transport: Result<Box<dyn zlog_rig::Transport>, String> = if config.port == SIMULATOR {
            let sim = SimHandle::new(SimRadio::new(sim_protocol(model.family)));
            let t = sim.transport();
            self.sim = Some(sim);
            Ok(Box::new(t))
        } else {
            self.sim = None;
            let stop = if model.two_stop_bits {
                zlog_rig::serial::StopBits::Two
            } else {
                zlog_rig::serial::StopBits::One
            };
            zlog_rig::serial::SerialTransport::open(&config.port, config.baud, stop)
                .map(|t| Box::new(t) as Box<dyn zlog_rig::Transport>)
        };
        match transport {
            Ok(t) => {
                let mut rig = Rig::new(model.protocol(), t, config.baud);
                rig.freq_offset = config.transverter_khz * 1000;
                self.rig = Some(rig);
                self.message = None;
            }
            Err(e) => self.message = Some(e),
        }
        self.synced = None;
        self.expect = None;
        self.config = Some(config);
    }

    pub fn disconnect(&mut self) {
        self.rig = None;
        self.sim = None;
        self.synced = None;
        self.expect = None;
    }

    /// Go to a frequency picked in a panel (a spot clicked), as `TMainForm.QSY`: the
    /// entry row moves to the band and mode at once, then the rig is tuned and always
    /// told the entry row's mode. zLog then also retunes to the band's initial
    /// frequency when `SetInitFreqWhenChangeMode` is on, which would undo the spot's
    /// frequency; that is not followed here.
    pub fn qsy(&mut self, hz: u64, mode: Option<Mode>, entry: &mut Entry, station: &Station) {
        if let Some(band) = band_of(hz) {
            if entry.band != band {
                entry.band = band;
                entry.power = station.power_for(band);
            }
        }
        if let Some(m) = mode {
            entry.set_mode(m);
        }
        let Some(rig) = self.rig.as_mut() else {
            return;
        };
        rig.set_freq(hz);
        rig.set_mode(rig_mode(entry.mode, entry.band));
        // The rig has been told: the new row is not a change to send again.
        self.synced = Some((entry.band, entry.mode));
        self.expect = Expect::new(band_of(hz), Some(entry.mode), entry.band);
    }

    /// Run the rig while the entry row works with the other one: it is polled and its
    /// band memory kept, but the entry row is left alone (zLog polls every rig).
    pub fn poll(&mut self, now: Instant) {
        let Some(rig) = self.rig.as_mut() else {
            return;
        };
        rig.tick(now);
        if let (Some(hz), Some(m)) = (rig.state.freq(), rig.state.mode) {
            self.memory.remember(hz, m.to_log());
        }
    }

    /// Forget what was agreed with the entry row, so that the row follows the rig at
    /// its next status (`TMainForm.SwitchRig` calls `UpdateBandAndMode`).
    pub fn follow(&mut self) {
        self.synced = None;
        self.expect = None;
    }

    /// [`follow`](Self::follow) once the rig has reported: before its first status
    /// the row `typed` is taken as agreed, so that a band or mode typed meanwhile is
    /// sent to the rig (zLog's `UpdateBandAndMode` has nothing to follow then) and an
    /// untouched row still follows the first status.
    pub fn follow_from(&mut self, typed: (Band, Mode)) {
        let reported = self
            .rig
            .as_ref()
            .is_some_and(|r| r.state.freq().is_some() && r.state.mode.is_some());
        self.follow();
        if !reported {
            self.synced = Some(typed);
        }
    }

    /// Run the rig and keep it and the entry row in step; true when the row changed.
    pub fn tick(&mut self, now: Instant, entry: &mut Entry, station: &Station) -> bool {
        let model = self.config.as_ref().map(|c| MODELS[c.model]);
        let option = |f: fn(&RigConfig) -> bool| self.config.as_ref().is_some_and(f);
        let init_freq_on_mode = option(|c| c.init_freq_on_mode);
        let ignore_rig_mode = option(|c| c.ignore_rig_mode);
        let rtty_keeps_rig_mode = option(|c| c.rtty_keeps_rig_mode);
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        rig.tick(now);
        let st = rig.state;
        // With `IgnoreRigMode` the rig's mode is taken to be the entry row's.
        let from_rig = st.freq().and_then(band_of).zip(st.mode.map(|m| {
            if ignore_rig_mode {
                entry.mode
            } else {
                m.to_log()
            }
        }));
        if let Some(hz) = st.freq() {
            if let Some(m) = st.mode {
                self.memory.remember(hz, m.to_log());
            }
        }
        let typed = (entry.band, entry.mode);
        match self.synced {
            // The operator changed band or mode in the entry row: tell the rig.
            Some(prev) if prev != typed => {
                let band_changed = prev.0 != typed.0;
                // `TRig.SetBand`: only bands the rig covers.
                let covered = model.is_none_or(|m| m.covers(entry.band));
                // RTTY `DontChangeRigMode`: `actionChangeModeExecute` exits before
                // telling the rig anything.
                let hold = !band_changed && entry.mode == Mode::Rtty && rtty_keeps_rig_mode;
                let mut tuned = None;
                if (band_changed || init_freq_on_mode) && covered && !hold {
                    if let Some(hz) = self.memory.tune(entry.band, entry.mode) {
                        rig.set_freq(hz);
                        tuned = band_of(hz);
                    }
                }
                // A band change sets the mode only for SSB (the sideband), covered or
                // not (`actionBandChangeExecute`); a mode change always sets it.
                let mut moded = None;
                if (!band_changed && !hold) || entry.mode == Mode::Ssb {
                    rig.set_mode(rig_mode(entry.mode, entry.band));
                    moded = Some(entry.mode);
                }
                self.synced = Some(typed);
                self.expect = Expect::new(tuned, moded, entry.band);
                false
            }
            // The rig moved: follow it, unless it is still on its way to what we asked.
            // A band the rig does not cover is left on its next status, since the
            // rig's band differs (`TRigControl.OnUpdateStatusProc`).
            _ => match from_rig {
                Some(seen) if self.waiting(seen, now) => false,
                Some(seen) if Some(seen) != self.synced && seen != typed => {
                    if entry.band != seen.0 {
                        entry.band = seen.0;
                        entry.power = station.power_for(seen.0);
                    }
                    entry.set_mode(seen.1);
                    self.synced = Some(seen);
                    true
                }
                Some(seen) => {
                    self.synced = Some(seen);
                    false
                }
                None => false,
            },
        }
    }
}

/// Band and mode the rig was told to go to (see [`RigControl::expect`]).
#[derive(Debug, Clone, Copy)]
struct Expect {
    band: Option<Band>,
    mode: Option<Mode>,
    until: Option<Instant>,
}

impl Expect {
    /// Modes the rig is sent no command for are not waited for.
    fn new(band: Option<Band>, mode: Option<Mode>, row_band: Band) -> Option<Expect> {
        let mode = mode.filter(|m| {
            !matches!(
                rig_mode(*m, row_band),
                zlog_rig::RigMode::Data | zlog_rig::RigMode::Other
            )
        });
        (band.is_some() || mode.is_some()).then_some(Expect {
            band,
            mode,
            until: None,
        })
    }
}

impl RigControl {
    /// Whether the rig is still on its way to what it was told; clears the wait once
    /// it has arrived or a second has passed.
    fn waiting(&mut self, seen: (Band, Mode), now: Instant) -> bool {
        let Some(e) = self.expect.as_mut() else {
            return false;
        };
        let until = *e
            .until
            .get_or_insert(now + std::time::Duration::from_secs(1));
        let arrived = e.band.is_none_or(|b| b == seen.0) && e.mode.is_none_or(|m| m == seen.1);
        if arrived || now >= until {
            self.expect = None;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use zlog_core::Power;

    fn run(rc: &mut RigControl, t: &mut Instant, entry: &mut Entry, ms: u64) {
        let st = Station::default();
        for _ in 0..ms / 10 {
            *t += Duration::from_millis(10);
            rc.tick(*t, entry, &st);
        }
    }

    #[test]
    fn entry_follows_the_simulated_rig_and_back() {
        let mut rc = RigControl::default();
        rc.connect(RigConfig {
            model: 1,
            port: SIMULATOR.into(),
            baud: 19200,
            transverter_khz: 0,
            init_freq_on_mode: false,
            ignore_rig_mode: false,
            rtty_keeps_rig_mode: false,
        });
        let mut entry = Entry::new(Band::B14, Mode::Ssb, Power::P010);
        let mut t = Instant::now();
        // The simulator starts on 7.012 MHz CW; the entry row follows.
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));

        // A mode key on the rig: the entry row follows, with phone RSTs.
        rc.sim.as_ref().unwrap().press_mode(zlog_rig::RigMode::Lsb);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.mode, entry.rst_sent.as_str()), (Mode::Ssb, "59"));
        rc.sim.as_ref().unwrap().press_mode(zlog_rig::RigMode::Cw);
        run(&mut rc, &mut t, &mut entry, 600);

        // The operator turns the dial to 21 MHz.
        rc.sim.as_ref().unwrap().tune(21_030_000);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B21);

        // Shift+B in the entry row: the rig goes to 28 MHz, the CW lower limit.
        entry.band = Band::B28;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(rc.sim.as_ref().unwrap().with(|r| r.freq_a), 28_050_000);
        assert_eq!(entry.band, Band::B28);

        // Back to 21 MHz: the rig returns to where it was.
        entry.band = Band::B21;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(rc.sim.as_ref().unwrap().with(|r| r.freq_a), 21_030_000);
    }

    fn model(name: &str) -> usize {
        MODELS.iter().position(|m| m.name == name).unwrap()
    }

    /// A rig on the simulator, settled with the entry row on 7.012 MHz CW.
    fn settled(name: &str, init_freq_on_mode: bool) -> (RigControl, Entry, Instant) {
        settled_with(name, |c| c.init_freq_on_mode = init_freq_on_mode)
    }

    fn settled_with(name: &str, f: impl FnOnce(&mut RigConfig)) -> (RigControl, Entry, Instant) {
        let mut rc = RigControl::default();
        let mut config = RigConfig {
            model: model(name),
            port: SIMULATOR.into(),
            baud: 19200,
            transverter_khz: 0,
            init_freq_on_mode: false,
            ignore_rig_mode: false,
            rtty_keeps_rig_mode: false,
        };
        f(&mut config);
        rc.connect(config);
        let mut entry = Entry::new(Band::B7, Mode::Cw, Power::P010);
        let mut t = Instant::now();
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        rc.sim.as_ref().unwrap().with(|r| r.received.clear());
        (rc, entry, t)
    }

    /// Commands the radio received other than polls (Icom 03/04, text IF/FA/OI/...).
    fn sets(rc: &RigControl) -> Vec<Vec<u8>> {
        rc.sim.as_ref().unwrap().with(|r| {
            r.received
                .iter()
                .filter(|c| match c.first() {
                    Some(0xFE) => matches!(c.get(4), Some(0x05 | 0x06)),
                    _ => c.len() > 4,
                })
                .cloned()
                .collect()
        })
    }

    fn icom_freq_sets(cmds: &[Vec<u8>]) -> usize {
        cmds.iter().filter(|c| c[4] == 0x05).count()
    }

    fn icom_mode_sets(cmds: &[Vec<u8>]) -> usize {
        cmds.iter().filter(|c| c[4] == 0x06).count()
    }

    #[test]
    fn a_mode_change_sets_only_the_mode() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (0, 1));
        let sim = rc.sim.as_ref().unwrap();
        assert_eq!(
            sim.with(|r| (r.freq_a, r.mode)),
            (7_012_000, zlog_rig::RigMode::Lsb)
        );
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Ssb));
    }

    #[test]
    fn a_mode_change_tunes_with_the_option() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", true);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
        assert_eq!(rc.sim.as_ref().unwrap().with(|r| r.freq_a), 7_060_000);
    }

    #[test]
    fn a_band_change_sets_the_mode_only_for_ssb() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.band = Band::B14;
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 0));
        assert_eq!(rc.sim.as_ref().unwrap().with(|r| r.freq_a), 14_050_000);

        // CW-R on the rig survives a band change.
        rc.sim.as_ref().unwrap().press_mode(zlog_rig::RigMode::CwR);
        run(&mut rc, &mut t, &mut entry, 600);
        entry.band = Band::B21;
        run(&mut rc, &mut t, &mut entry, 600);
        let sim = rc.sim.clone().unwrap();
        assert_eq!(sim.with(|r| r.mode), zlog_rig::RigMode::CwR);

        // In SSB the sideband follows the band.
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        sim.with(|r| r.received.clear());
        entry.band = Band::B3_5;
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
        assert_eq!(
            sim.with(|r| (r.freq_a, r.mode)),
            (3_535_000, zlog_rig::RigMode::Lsb)
        );
    }

    /// Regression: the row was "parked" on a band the rig does not cover; zLog does
    /// not tune the rig (`TRig.SetBand`) and pulls the row back to the rig's band on
    /// its next status (`TRigControl.OnUpdateStatusProc`).
    #[test]
    fn a_band_the_rig_does_not_cover_is_not_tuned_and_the_row_comes_back() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.band = Band::B144;
        run(&mut rc, &mut t, &mut entry, 1000);
        assert!(sets(&rc).is_empty());
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        // The operator turns the dial to another band: the row follows.
        rc.sim.as_ref().unwrap().tune(21_030_000);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B21);
        // A covered band is tuned.
        entry.band = Band::B28;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(rc.sim.as_ref().unwrap().with(|r| r.freq_a), 28_050_000);
        assert_eq!(entry.band, Band::B28);
    }

    /// Regression: in SSB a band change to a band the rig does not cover sent nothing;
    /// `actionBandChangeExecute` still sends the mode (the sideband of the new band).
    #[test]
    fn an_uncovered_band_in_ssb_still_sends_the_mode() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        let sim = rc.sim.clone().unwrap();
        sim.with(|r| r.received.clear());
        entry.band = Band::B144;
        run(&mut rc, &mut t, &mut entry, 1000);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (0, 1));
        // USB, as for 144 MHz; the rig stays on 7 MHz and the row comes back to it.
        assert_eq!(
            sim.with(|r| (r.freq_a, r.mode)),
            (7_012_000, zlog_rig::RigMode::Usb)
        );
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Ssb));
    }

    #[test]
    fn the_lowest_band_of_a_vhf_rig() {
        let mut rc = RigControl::default();
        rc.connect(RigConfig {
            model: model("IC-9700"),
            port: SIMULATOR.into(),
            baud: 19200,
            transverter_khz: 0,
            init_freq_on_mode: false,
            ignore_rig_mode: false,
            rtty_keeps_rig_mode: false,
        });
        rc.sim.as_ref().unwrap().with(|r| r.freq_a = 144_050_000);
        let mut entry = Entry::new(Band::B144, Mode::Cw, Power::P010);
        let mut t = Instant::now();
        run(&mut rc, &mut t, &mut entry, 600);
        rc.sim.as_ref().unwrap().with(|r| r.received.clear());
        entry.band = Band::B50;
        run(&mut rc, &mut t, &mut entry, 600);
        assert!(sets(&rc).is_empty());
        assert_eq!(entry.band, Band::B144);
        entry.band = Band::B430;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(rc.sim.as_ref().unwrap().with(|r| r.freq_a), 430_050_000);
    }

    #[test]
    fn transverter_offset_from_the_settings() {
        let mut rc = RigControl::default();
        rc.connect(RigConfig {
            model: model("IC-705"),
            port: SIMULATOR.into(),
            baud: 19200,
            transverter_khz: 1_150_000,
            init_freq_on_mode: false,
            ignore_rig_mode: false,
            rtty_keeps_rig_mode: false,
        });
        rc.sim.as_ref().unwrap().with(|r| r.freq_a = 144_050_000);
        let mut entry = Entry::new(Band::B7, Mode::Cw, Power::P010);
        let mut t = Instant::now();
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B1200);
        assert_eq!(rc.state().unwrap().freq(), Some(1_294_050_000));
    }

    #[test]
    fn data_modes_leave_the_rig_alone() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.set_mode(Mode::Ft8);
        run(&mut rc, &mut t, &mut entry, 600);
        assert!(sets(&rc).is_empty());
    }

    #[test]
    fn text_rig_band_change_goes_to_the_vfo_in_use() {
        let (mut rc, mut entry, mut t) = settled("TS-590", false);
        rc.sim.as_ref().unwrap().select_vfo(zlog_rig::Vfo::B);
        run(&mut rc, &mut t, &mut entry, 600);
        // VFO B is on 7.1 MHz: still 7 MHz, so the row stays.
        entry.band = Band::B14;
        run(&mut rc, &mut t, &mut entry, 600);
        let sim = rc.sim.as_ref().unwrap();
        assert_eq!(sim.with(|r| (r.freq_a, r.freq_b)), (7_012_000, 14_050_000));
        assert_eq!(sets(&rc), vec![b"FB00014050000;".to_vec()]);
    }

    /// `IgnoreRigMode`: the row follows the rig's band but not its mode; modes typed
    /// in the row still reach the rig.
    #[test]
    fn ignore_rig_mode_keeps_the_row_mode() {
        let (mut rc, mut entry, mut t) = settled_with("IC-7300", |c| c.ignore_rig_mode = true);
        let sim = rc.sim.clone().unwrap();
        sim.press_mode(zlog_rig::RigMode::Lsb);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        sim.tune(14_030_000);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B14, Mode::Cw));
        sim.with(|r| r.received.clear());
        entry.set_mode(Mode::Fm);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(icom_mode_sets(&sets(&rc)), 1);
        assert_eq!(sim.with(|r| r.mode), zlog_rig::RigMode::Fm);
        assert_eq!(entry.mode, Mode::Fm);
        // Without the option the row follows the rig's mode.
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        rc.sim.as_ref().unwrap().press_mode(zlog_rig::RigMode::Lsb);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Ssb);
    }

    /// RTTY `DontChangeRigMode`: a change to RTTY in the row tells the rig nothing,
    /// not even with `SetInitFreqWhenChangeMode`; other modes still go.
    #[test]
    fn rtty_keeps_rig_mode_option() {
        // Alone, the rig's next status brings the row back to CW, as in zLog.
        let (mut rc, mut entry, mut t) = settled_with("IC-7300", |c| {
            c.rtty_keeps_rig_mode = true;
        });
        entry.set_mode(Mode::Rtty);
        run(&mut rc, &mut t, &mut entry, 600);
        assert!(sets(&rc).is_empty());
        assert_eq!(entry.mode, Mode::Cw);
        // With `IgnoreRigMode` the row stays in RTTY.
        let (mut rc, mut entry, mut t) = settled_with("IC-7300", |c| {
            c.rtty_keeps_rig_mode = true;
            c.ignore_rig_mode = true;
            c.init_freq_on_mode = true;
        });
        entry.set_mode(Mode::Rtty);
        run(&mut rc, &mut t, &mut entry, 600);
        assert!(sets(&rc).is_empty());
        assert_eq!(entry.mode, Mode::Rtty);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
        // Without the option RTTY is sent.
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.set_mode(Mode::Rtty);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(icom_mode_sets(&sets(&rc)), 1);
        assert_eq!(
            rc.sim.as_ref().unwrap().with(|r| r.mode),
            zlog_rig::RigMode::Rtty
        );
    }

    /// Regression: a spot QSY with a rig left the row until the rig reported back, and
    /// sent no mode when the spot had none; `TMainForm.QSY` moves the row at once and
    /// always sends the row's mode.
    #[test]
    fn qsy_with_a_rig() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        let st = Station::default();
        rc.qsy(21_250_000, Some(Mode::Ssb), &mut entry, &st);
        assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Ssb));
        assert_eq!(entry.rst_sent, "59");
        // The row never goes back while the rig gets there.
        for _ in 0..60 {
            t += Duration::from_millis(10);
            rc.tick(t, &mut entry, &st);
            assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Ssb));
        }
        let sim = rc.sim.clone().unwrap();
        assert_eq!(
            sim.with(|r| (r.freq_a, r.mode)),
            (21_250_000, zlog_rig::RigMode::Usb)
        );
        // Sent once each: the row's change is not sent again as a band change.
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));

        // A spot without a mode: the row's mode is still sent, with the sideband.
        sim.with(|r| r.received.clear());
        rc.qsy(7_150_000, None, &mut entry, &st);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Ssb));
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
        assert_eq!(
            sim.with(|r| (r.freq_a, r.mode)),
            (7_150_000, zlog_rig::RigMode::Lsb)
        );

        // The same band: the frequency and mode go, the row keeps its band.
        sim.with(|r| r.received.clear());
        rc.qsy(7_020_000, Some(Mode::Cw), &mut entry, &st);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        assert_eq!(sim.with(|r| r.freq_a), 7_020_000);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
    }

    /// A band change in the row is not undone by the rig's status from before it got
    /// there; a rig that never gets there is followed after a second.
    #[test]
    fn a_band_change_does_not_bounce() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        let st = Station::default();
        entry.band = Band::B14;
        for _ in 0..60 {
            t += Duration::from_millis(10);
            rc.tick(t, &mut entry, &st);
            assert_eq!(entry.band, Band::B14);
        }
        // The radio is switched off and back on 7 MHz: the row follows it.
        let sim = rc.sim.clone().unwrap();
        sim.with(|r| r.power_on = false);
        entry.band = Band::B21;
        run(&mut rc, &mut t, &mut entry, 300);
        sim.with(|r| {
            r.power_on = true;
            r.freq_a = 7_020_000;
        });
        run(&mut rc, &mut t, &mut entry, 2500);
        assert_eq!(entry.band, Band::B7);
    }

    #[test]
    fn qsy_without_a_rig() {
        let mut rc = RigControl::default();
        let st = Station::default();
        let mut entry = Entry::new(Band::B7, Mode::Cw, Power::P010);
        rc.qsy(14_250_000, Some(Mode::Ssb), &mut entry, &st);
        assert_eq!((entry.band, entry.mode), (Band::B14, Mode::Ssb));
        assert_eq!(entry.power, st.power_for(Band::B14));
        // No mode: the mode stays.
        rc.qsy(21_030_000, None, &mut entry, &st);
        assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Ssb));
        // A frequency outside every band: the band stays, the mode still changes.
        rc.qsy(8_500_000, Some(Mode::Cw), &mut entry, &st);
        assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Cw));
        rc.qsy(0, None, &mut entry, &st);
        assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Cw));
        assert!(!rc.connected());
    }

    /// A rig in DV, or in a mode zLog does not know, puts the row in DV or Other;
    /// DV typed in the row reaches an Icom.
    #[test]
    fn dv_and_other_modes_from_and_to_an_icom() {
        let (mut rc, mut entry, mut t) = settled("IC-705", false);
        let sim = rc.sim.clone().unwrap();
        sim.press_mode(zlog_rig::RigMode::Dv);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Dv);
        sim.press_mode(zlog_rig::RigMode::Other);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Other);
        sim.press_mode(zlog_rig::RigMode::Cw);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Cw);
        sim.with(|r| r.received.clear());
        entry.set_mode(Mode::Dv);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(icom_mode_sets(&sets(&rc)), 1);
        assert_eq!(sim.with(|r| r.mode), zlog_rig::RigMode::Dv);
        assert_eq!(entry.mode, Mode::Dv);
    }

    /// A Kenwood in CW-R stays in CW-R when CW is chosen again in the row.
    #[test]
    fn kenwood_cw_reverse_survives_a_mode_change() {
        let (mut rc, mut entry, mut t) = settled("TS-590", false);
        let sim = rc.sim.clone().unwrap();
        sim.press_mode(zlog_rig::RigMode::CwR);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Cw);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(sim.with(|r| r.mode), zlog_rig::RigMode::Lsb);
        entry.set_mode(Mode::Cw);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(sim.with(|r| r.mode), zlog_rig::RigMode::CwR);
    }

    /// The FTDX101 on the simulator: the row follows VFO B when it receives on B.
    #[test]
    fn ftdx101_follows_the_receiving_vfo() {
        let (mut rc, mut entry, mut t) = settled("FTDX101", false);
        let sim = rc.sim.clone().unwrap();
        sim.with(|r| r.freq_b = 14_100_000);
        sim.select_vfo(zlog_rig::Vfo::B);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B14);
        assert!(sim.with(|r| r.received.iter().any(|c| c == b"FR;")));
        assert!(sim.with(|r| r.received.iter().all(|c| c != b"FT;")));
    }

    #[test]
    fn settings_round_trip_and_defaults() {
        let d = RigConfig::load(&Section::default());
        assert_eq!(MODELS[d.model].name, "IC-7300");
        assert_eq!((d.transverter_khz, d.init_freq_on_mode), (0, false));
        assert_eq!((d.ignore_rig_mode, d.rtty_keeps_rig_mode), (false, false));
        assert_eq!(d.port, SIMULATOR);
        let c = RigConfig {
            model: model("FT-710"),
            port: "/dev/ttyUSB0".into(),
            baud: 38400,
            transverter_khz: 116_000,
            init_freq_on_mode: true,
            ignore_rig_mode: true,
            rtty_keeps_rig_mode: true,
        };
        let mut s = Section::default();
        c.save(&mut s);
        assert_eq!(RigConfig::load(&s), c);
        // Malformed values fall back to the defaults.
        s.insert("transverter_khz".into(), "x".into());
        s.insert("init_freq_on_mode".into(), "yes".into());
        s.insert("ignore_rig_mode".into(), "1".into());
        s.insert("rtty_keeps_rig_mode".into(), "".into());
        s.insert("model".into(), "nonexistent".into());
        let m = RigConfig::load(&s);
        assert_eq!((m.transverter_khz, m.init_freq_on_mode), (0, false));
        assert_eq!((m.ignore_rig_mode, m.rtty_keeps_rig_mode), (false, false));
        assert_eq!(MODELS[m.model].name, "IC-7300");
    }
}
