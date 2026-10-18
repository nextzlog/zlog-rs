//! Rig control in the window: one rig, a real one on a serial port or the built-in
//! simulator, kept in step with the entry row by zLog's rules (`zlog_rig::sync`).
//!
//! - The rig's frequency sets the band of the entry row, its mode the mode.
//! - A band or mode chosen in the entry row tunes the rig to the last frequency used
//!   there, or to the band's lower edge for the mode.
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
}

impl RigConfig {
    pub fn from_section(s: &Section) -> Self {
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
        }
    }

    pub fn to_section(&self, s: &mut Section) {
        s.insert("model".into(), MODELS[self.model].name.into());
        s.insert("port".into(), self.port.clone());
        s.insert("baud".into(), self.baud.to_string());
    }
}

fn sim_protocol(family: Family) -> SimProtocol {
    match family {
        Family::Icom(a) => SimProtocol::Icom(a),
        Family::Text(Dialect::Kenwood) => SimProtocol::Text {
            digits: 11,
            yaesu: false,
        },
        Family::Text(Dialect::Yaesu9) => SimProtocol::Text {
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
                self.rig = Some(Rig::new(model.protocol(), t, config.baud));
                self.message = None;
            }
            Err(e) => self.message = Some(e),
        }
        self.synced = None;
        self.config = Some(config);
    }

    pub fn disconnect(&mut self) {
        self.rig = None;
        self.sim = None;
        self.synced = None;
    }

    /// Run the rig and keep it and the entry row in step; true when the row changed.
    pub fn tick(&mut self, now: Instant, entry: &mut Entry, station: &Station) -> bool {
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        rig.tick(now);
        let st = rig.state;
        let from_rig = st.freq().and_then(band_of).zip(st.mode.map(|m| m.to_log()));
        if let Some(hz) = st.freq() {
            if let Some(m) = st.mode {
                self.memory.remember(hz, m.to_log());
            }
        }
        let typed = (entry.band, entry.mode);
        match self.synced {
            // The operator changed band or mode in the entry row: tune the rig.
            Some(prev) if prev != typed => {
                if let Some(hz) = self.memory.tune(entry.band, entry.mode) {
                    rig.set_freq(hz);
                }
                rig.set_mode(rig_mode(entry.mode, entry.band));
                self.synced = Some(typed);
                false
            }
            // The rig moved: follow it, unless it is still on its way to what we asked.
            _ => match from_rig {
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

        // Shift+B in the entry row: the rig goes to 28 MHz, lower edge for CW.
        entry.band = Band::B28;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(rc.sim.as_ref().unwrap().with(|r| r.freq_a), 28_000_000);
        assert_eq!(entry.band, Band::B28);

        // Back to 21 MHz: the rig returns to where it was.
        entry.band = Band::B21;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(rc.sim.as_ref().unwrap().with(|r| r.freq_a), 21_030_000);
    }
}
