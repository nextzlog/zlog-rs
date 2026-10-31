use std::collections::HashMap;

use crate::Spot;

pub const REFRESH_SECS: i64 = 60;

const MOVED_HZ: u64 = 100;

const FORGET_SECS: i64 = 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sideband {
    Upper,
    Lower,
}

pub fn heard_hz(dial: u64, audio: f64, pitch: f64, sideband: Sideband) -> u64 {
    let offset = audio - pitch;
    let hz = match sideband {
        Sideband::Upper => dial as f64 + offset,
        Sideband::Lower => dial as f64 - offset,
    };
    hz.round().max(0.0) as u64
}

#[derive(Default)]
pub struct SpotFeed {
    last: HashMap<String, (u64, String, i64)>,
}

impl SpotFeed {
    pub fn offer(&mut self, spot: Spot) -> Option<Spot> {
        let key = spot.callsign.to_ascii_uppercase();
        if let Some((hz, number, time)) = self.last.get(&key) {
            let same = hz.abs_diff(spot.hz) < MOVED_HZ && *number == spot.number;
            if same && spot.time - time < REFRESH_SECS {
                return None;
            }
        }
        self.last
            .retain(|_, (_, _, t)| spot.time - *t < FORGET_SECS);
        self.last
            .insert(key, (spot.hz, spot.number.clone(), spot.time));
        Some(spot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heard_frequency_follows_the_sideband_and_pitch() {
        assert_eq!(heard_hz(7_074_000, 1234.4, 0.0, Sideband::Upper), 7_075_234);
        assert_eq!(
            heard_hz(7_010_000, 700.0, 600.0, Sideband::Upper),
            7_010_100
        );
        assert_eq!(
            heard_hz(7_010_000, 500.0, 600.0, Sideband::Upper),
            7_009_900
        );
        assert_eq!(
            heard_hz(7_010_000, 700.0, 600.0, Sideband::Lower),
            7_009_900
        );
        assert_eq!(
            heard_hz(14_085_000, 2125.0, 0.0, Sideband::Lower),
            14_082_875
        );
        assert_eq!(heard_hz(100, 500.0, 0.0, Sideband::Lower), 0);
    }

    fn spot(call: &str, hz: u64, number: &str, time: i64) -> Spot {
        Spot {
            callsign: call.into(),
            hz,
            number: number.into(),
            time,
            ..Spot::default()
        }
    }

    #[test]
    fn the_feed_passes_news_and_refreshes() {
        let mut f = SpotFeed::default();
        assert!(f.offer(spot("JA1ZLO", 7_010_000, "", 0)).is_some());
        assert!(f.offer(spot("ja1zlo", 7_010_050, "", 10)).is_none());
        assert!(f.offer(spot("JA1ZLO", 7_010_000, "13M", 20)).is_some());
        assert!(f.offer(spot("JA1ZLO", 7_010_200, "13M", 30)).is_some());
        assert!(f.offer(spot("JA1YWX", 7_010_200, "", 30)).is_some());
        assert!(f.offer(spot("JA1ZLO", 7_010_200, "13M", 89)).is_none());
        assert!(f.offer(spot("JA1ZLO", 7_010_200, "13M", 90)).is_some());
        f.offer(spot("JA1ZLO", 7_010_200, "13M", 90 + FORGET_SECS));
        assert_eq!(f.last.len(), 1);
    }
}
