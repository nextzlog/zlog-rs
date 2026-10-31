use crate::Band;

const BANDS_KHZ: &[(Band, u64, u64)] = &[
    (Band::B1_9, 1_800, 1_999),
    (Band::B3_5, 3_000, 3_999),
    (Band::B7, 6_900, 7_999),
    (Band::B10, 9_900, 11_000),
    (Band::B14, 13_900, 14_999),
    (Band::B18, 17_500, 18_999),
    (Band::B21, 20_900, 21_999),
    (Band::B24, 23_500, 24_999),
    (Band::B28, 27_800, 29_999),
    (Band::B50, 49_000, 59_000),
    (Band::B144, 140_000, 149_999),
    (Band::B430, 400_000, 450_000),
    (Band::B1200, 1_200_000, 1_299_999),
    (Band::B2400, 2_400_000, 2_499_999),
    (Band::B5600, 5_600_000, 5_899_999),
    (Band::B10G, 10_000_000, 10_449_999),
    (Band::B10_4G, 10_450_000, 10_499_999),
    (Band::B24G, 24_000_000, 24_049_999),
    (Band::B47G, 47_000_000, 47_199_999),
    (Band::B77G, 77_500_000, 77_999_999),
    (Band::B135G, 134_000_000, 135_999_999),
    (Band::B248G, 248_000_000, 249_999_999),
];

pub fn band_of(hz: u64) -> Option<Band> {
    let khz = hz / 1000;
    BANDS_KHZ
        .iter()
        .find(|(_, lo, hi)| (*lo..=*hi).contains(&khz))
        .map(|(b, _, _)| *b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_of_follows_freq_to_band() {
        for (band, lo, hi) in BANDS_KHZ {
            assert_eq!(band_of(lo * 1000), Some(*band), "{band:?} low");
            assert_eq!(band_of(hi * 1000 + 999), Some(*band), "{band:?} high");
            assert_ne!(band_of(lo * 1000 - 1), Some(*band), "{band:?} below");
            assert_ne!(band_of((hi + 1) * 1000), Some(*band), "{band:?} above");
        }
        assert_eq!(band_of(3_999_000), Some(Band::B3_5));
        assert_eq!(band_of(3_000_000), Some(Band::B3_5));
        assert_eq!(band_of(6_950_000), Some(Band::B7));
        assert_eq!(band_of(29_999_999), Some(Band::B28));
        assert_eq!(band_of(10_460_000_000), Some(Band::B10_4G));
        assert_eq!(band_of(0), None);
        assert_eq!(band_of(1_799_999), None);
        assert_eq!(band_of(u64::MAX), None);
    }
}
