//! zLog's rules between the rig and the entry row (`URigCtrlLib.pas`, `URigControl.pas`).
//!
//! - The band follows the rig's frequency (`UpdateFreqMem`, `BandPlan.FreqToBand`).
//! - On a band change from the entry row the rig goes to the last frequency used on
//!   that band and mode, else to the lower edge for the mode (`TRig.SetBand`).
//! - SSB is LSB up to 7 MHz and USB above.

use std::collections::HashMap;

use zlog_core::{Band, Mode};

use crate::RigMode;

/// Band edges in hertz, after the Japanese band plan; 10.4G shares the 10G entry.
const BANDS: &[(Band, u64, u64)] = &[
    (Band::B1_9, 1_800_000, 1_912_500),
    (Band::B3_5, 3_500_000, 3_805_000),
    (Band::B7, 7_000_000, 7_200_000),
    (Band::B10, 10_100_000, 10_150_000),
    (Band::B14, 14_000_000, 14_350_000),
    (Band::B18, 18_068_000, 18_168_000),
    (Band::B21, 21_000_000, 21_450_000),
    (Band::B24, 24_890_000, 24_990_000),
    (Band::B28, 28_000_000, 29_700_000),
    (Band::B50, 50_000_000, 54_000_000),
    (Band::B144, 144_000_000, 146_000_000),
    (Band::B430, 430_000_000, 440_000_000),
    (Band::B1200, 1_260_000_000, 1_300_000_000),
    (Band::B2400, 2_400_000_000, 2_450_000_000),
    (Band::B5600, 5_650_000_000, 5_850_000_000),
    (Band::B10G, 10_000_000_000, 10_250_000_000),
];

/// Lower edge of the phone segment, where it differs from the band's lower edge.
const PHONE_EDGES: &[(Band, u64)] = &[
    (Band::B3_5, 3_535_000),
    (Band::B7, 7_030_000),
    (Band::B14, 14_100_000),
    (Band::B21, 21_150_000),
    (Band::B28, 28_200_000),
    (Band::B50, 50_100_000),
    (Band::B144, 144_100_000),
    (Band::B430, 430_100_000),
];

pub fn band_of(hz: u64) -> Option<Band> {
    BANDS
        .iter()
        .find(|(_, lo, hi)| (*lo..=*hi).contains(&hz))
        .map(|(b, _, _)| *b)
}

/// SSB sideband by band: LSB at 7 MHz and below, USB above.
pub fn rig_mode(mode: Mode, band: Band) -> RigMode {
    match mode {
        Mode::Cw => RigMode::Cw,
        Mode::Ssb if band <= Band::B7 => RigMode::Lsb,
        Mode::Ssb => RigMode::Usb,
        Mode::Am => RigMode::Am,
        Mode::Fm => RigMode::Fm,
        Mode::Rtty => RigMode::Rtty,
        _ => RigMode::Data,
    }
}

/// Last frequency used per band and mode (`FFreqMem`).
#[derive(Debug, Default, Clone)]
pub struct FreqMemory(HashMap<(Band, Mode), u64>);

impl FreqMemory {
    pub fn remember(&mut self, hz: u64, mode: Mode) {
        if let Some(band) = band_of(hz) {
            self.0.insert((band, mode), hz);
        }
    }

    /// Where to tune for a band and mode: the remembered frequency, else the edge.
    pub fn tune(&self, band: Band, mode: Mode) -> Option<u64> {
        if let Some(hz) = self.0.get(&(band, mode)) {
            return Some(*hz);
        }
        let lower = BANDS
            .iter()
            .find(|(b, _, _)| *b == band)
            .map(|(_, lo, _)| *lo)?;
        let phone = !matches!(mode, Mode::Cw | Mode::Rtty);
        let edge = PHONE_EDGES
            .iter()
            .find(|(b, _)| *b == band)
            .map(|(_, hz)| *hz);
        Some(if phone { edge.unwrap_or(lower) } else { lower })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_and_sidebands() {
        assert_eq!(band_of(7_012_000), Some(Band::B7));
        assert_eq!(band_of(8_000_000), None);
        assert_eq!(rig_mode(Mode::Ssb, Band::B7), RigMode::Lsb);
        assert_eq!(rig_mode(Mode::Ssb, Band::B14), RigMode::Usb);
    }

    #[test]
    fn memory_then_edges() {
        let mut m = FreqMemory::default();
        assert_eq!(m.tune(Band::B7, Mode::Cw), Some(7_000_000));
        assert_eq!(m.tune(Band::B7, Mode::Ssb), Some(7_030_000));
        m.remember(7_012_500, Mode::Cw);
        assert_eq!(m.tune(Band::B7, Mode::Cw), Some(7_012_500));
    }
}
