use std::collections::HashMap;

use zlog_core::{Band, Mode};

pub use haml::Mode as RigMode;
pub use zlog_core::band_of;

pub trait LogMode {
    fn to_log(self) -> Mode;
    fn text(self) -> &'static str;
}

impl LogMode for RigMode {
    fn to_log(self) -> Mode {
        match self {
            RigMode::Lsb | RigMode::Usb => Mode::Ssb,
            RigMode::Cw | RigMode::CwR => Mode::Cw,
            RigMode::Am => Mode::Am,
            RigMode::Fm => Mode::Fm,
            RigMode::Rtty | RigMode::RttyR => Mode::Rtty,
            RigMode::Data => Mode::Ft8,
            RigMode::Dv => Mode::Dv,
            RigMode::Other => Mode::Other,
        }
    }

    fn text(self) -> &'static str {
        match self {
            RigMode::Other => "Other",
            m => m.label(),
        }
    }
}

const CW_LOWER: [u64; 22] = [
    1_801_000,
    3_510_000,
    7_010_000,
    10_100_000,
    14_050_000,
    18_068_000,
    21_050_000,
    24_890_000,
    28_050_000,
    50_050_000,
    144_050_000,
    430_050_000,
    1_294_000_000,
    2_424_000_000,
    5_760_000_000,
    10_240_000_000,
    10_450_000_000,
    24_000_000_000,
    47_000_000_000,
    77_500_000_000,
    134_000_000_000,
    248_000_000_000,
];
const SSB_LOWER: [u64; 22] = [
    1_850_000,
    3_535_000,
    7_060_000,
    0,
    14_250_000,
    18_110_000,
    21_350_000,
    24_930_000,
    28_600_000,
    50_350_000,
    144_250_000,
    430_250_000,
    1_294_000_000,
    2_424_000_000,
    5_760_000_000,
    10_240_000_000,
    10_450_000_000,
    24_000_000_000,
    47_000_000_000,
    77_500_000_000,
    134_000_000_000,
    248_000_000_000,
];
const FM_LOWER: [u64; 22] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    29_200_000,
    51_000_000,
    144_750_000,
    432_100_000,
    1_294_900_000,
    2_425_000_000,
    5_757_000_000,
    10_237_000_000,
    10_450_000_000,
    24_000_000_000,
    47_000_000_000,
    77_500_000_000,
    134_000_000_000,
    248_000_000_000,
];
const AM_LOWER: [u64; 22] = [
    1_850_000,
    3_535_000,
    7_060_000,
    0,
    14_250_000,
    0,
    21_350_000,
    0,
    28_600_000,
    50_350_000,
    144_250_000,
    430_250_000,
    1_294_900_000,
    2_427_000_000,
    5_757_000_000,
    10_237_000_000,
    10_450_000_000,
    24_000_000_000,
    47_000_000_000,
    77_500_000_000,
    134_000_000_000,
    248_000_000_000,
];
const RTTY_LOWER: [u64; 22] = [
    0,
    3_530_000,
    7_030_000,
    0,
    14_070_000,
    0,
    21_070_000,
    0,
    28_070_000,
    50_070_000,
    144_100_000,
    430_100_000,
    1_294_000_000,
    2_424_000_000,
    5_760_000_000,
    10_237_000_000,
    10_450_000_000,
    24_000_000_000,
    47_000_000_000,
    77_500_000_000,
    134_000_000_000,
    248_000_000_000,
];

pub fn lower_limit(band: Band, mode: Mode) -> Option<u64> {
    let table = match mode {
        Mode::Cw => &CW_LOWER,
        Mode::Ssb => &SSB_LOWER,
        Mode::Fm => &FM_LOWER,
        Mode::Am => &AM_LOWER,
        Mode::Rtty => &RTTY_LOWER,
        _ => return None,
    };
    table.get(band as usize).copied().filter(|hz| *hz > 0)
}

pub fn rig_mode(mode: Mode, band: Band) -> RigMode {
    match mode {
        Mode::Cw => RigMode::Cw,
        Mode::Ssb if band <= Band::B7 => RigMode::Lsb,
        Mode::Ssb => RigMode::Usb,
        Mode::Am => RigMode::Am,
        Mode::Fm => RigMode::Fm,
        Mode::Rtty => RigMode::Rtty,
        Mode::Dv => RigMode::Dv,
        Mode::Other => RigMode::Other,
        _ => RigMode::Data,
    }
}

#[derive(Debug, Default, Clone)]
pub struct FreqMemory(HashMap<(Band, Mode), u64>);

impl FreqMemory {
    pub fn remember(&mut self, hz: u64, mode: Mode) {
        if let Some(band) = band_of(hz) {
            self.0.insert((band, mode), hz);
        }
    }

    pub fn tune(&self, band: Band, mode: Mode) -> Option<u64> {
        if let Some(hz) = self.0.get(&(band, mode)) {
            return Some(*hz);
        }
        lower_limit(band, mode).or_else(|| lower_limit(band, Mode::Cw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_texts_keep_the_zlog_spelling() {
        assert_eq!(RigMode::Other.text(), "Other");
        assert_eq!(RigMode::CwR.text(), "CW-R");
    }

    #[test]
    fn bands_and_sidebands() {
        assert_eq!(band_of(7_012_000), Some(Band::B7));
        assert_eq!(band_of(8_000_000), None);
        assert_eq!(rig_mode(Mode::Ssb, Band::B7), RigMode::Lsb);
        assert_eq!(rig_mode(Mode::Ssb, Band::B14), RigMode::Usb);
    }

    #[test]
    fn memory_then_band_plan() {
        let mut m = FreqMemory::default();
        assert_eq!(m.tune(Band::B7, Mode::Cw), Some(7_010_000));
        assert_eq!(m.tune(Band::B7, Mode::Ssb), Some(7_060_000));
        m.remember(7_012_500, Mode::Cw);
        assert_eq!(m.tune(Band::B7, Mode::Cw), Some(7_012_500));
        assert_eq!(m.tune(Band::B7, Mode::Ssb), Some(7_060_000));
        m.remember(8_500_000, Mode::Cw);
        assert_eq!(m.0.len(), 1);
    }

    #[test]
    fn tune_uses_the_ja_band_plan() {
        let m = FreqMemory::default();
        let cases = [
            (Band::B1_9, Mode::Cw, 1_801_000),
            (Band::B1_9, Mode::Ssb, 1_850_000),
            (Band::B1_9, Mode::Rtty, 1_801_000),
            (Band::B3_5, Mode::Cw, 3_510_000),
            (Band::B3_5, Mode::Ssb, 3_535_000),
            (Band::B3_5, Mode::Rtty, 3_530_000),
            (Band::B3_5, Mode::Fm, 3_510_000),
            (Band::B10, Mode::Ssb, 10_100_000),
            (Band::B14, Mode::Cw, 14_050_000),
            (Band::B14, Mode::Ssb, 14_250_000),
            (Band::B14, Mode::Rtty, 14_070_000),
            (Band::B18, Mode::Am, 18_068_000),
            (Band::B21, Mode::Ssb, 21_350_000),
            (Band::B28, Mode::Fm, 29_200_000),
            (Band::B50, Mode::Cw, 50_050_000),
            (Band::B50, Mode::Ssb, 50_350_000),
            (Band::B50, Mode::Fm, 51_000_000),
            (Band::B144, Mode::Fm, 144_750_000),
            (Band::B144, Mode::Ssb, 144_250_000),
            (Band::B430, Mode::Fm, 432_100_000),
            (Band::B1200, Mode::Fm, 1_294_900_000),
            (Band::B2400, Mode::Am, 2_427_000_000),
            (Band::B248G, Mode::Cw, 248_000_000_000),
            (Band::B14, Mode::Ft8, 14_050_000),
            (Band::B7, Mode::Other, 7_010_000),
        ];
        for (band, mode, hz) in cases {
            assert_eq!(m.tune(band, mode), Some(hz), "{band:?} {mode:?}");
        }
        assert_eq!(m.tune(Band::Unknown, Mode::Cw), None);
        assert_eq!(m.tune(Band::Target, Mode::Ssb), None);
    }

    #[test]
    fn plan_limits_lie_in_their_bands() {
        for band in Band::ALL.iter().take(22) {
            for mode in [Mode::Cw, Mode::Ssb, Mode::Fm, Mode::Am, Mode::Rtty] {
                if let Some(hz) = lower_limit(*band, mode) {
                    assert_eq!(band_of(hz), Some(*band), "{band:?} {mode:?}");
                }
            }
        }
    }

    #[test]
    fn data_modes_map_to_data() {
        assert_eq!(rig_mode(Mode::Ft8, Band::B14), RigMode::Data);
        assert_eq!(rig_mode(Mode::Ft4, Band::B7), RigMode::Data);
        assert_eq!(rig_mode(Mode::Ssb, Band::B3_5), RigMode::Lsb);
        assert_eq!(rig_mode(Mode::Ssb, Band::B10), RigMode::Usb);
        assert_eq!(rig_mode(Mode::Dv, Band::B430), RigMode::Dv);
        assert_eq!(rig_mode(Mode::Other, Band::B7), RigMode::Other);
    }

    #[test]
    fn rig_modes_round_trip_through_the_log() {
        for (rig, log) in [
            (RigMode::Lsb, Mode::Ssb),
            (RigMode::Usb, Mode::Ssb),
            (RigMode::Cw, Mode::Cw),
            (RigMode::CwR, Mode::Cw),
            (RigMode::Am, Mode::Am),
            (RigMode::Fm, Mode::Fm),
            (RigMode::Rtty, Mode::Rtty),
            (RigMode::RttyR, Mode::Rtty),
            (RigMode::Data, Mode::Ft8),
            (RigMode::Dv, Mode::Dv),
            (RigMode::Other, Mode::Other),
        ] {
            assert_eq!(rig.to_log(), log, "{rig:?}");
        }
        for m in [
            Mode::Cw,
            Mode::Am,
            Mode::Fm,
            Mode::Rtty,
            Mode::Dv,
            Mode::Other,
        ] {
            assert_eq!(rig_mode(m, Band::B14).to_log(), m, "{m:?}");
        }
    }
}
