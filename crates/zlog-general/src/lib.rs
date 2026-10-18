//! General logging as a rule extension: every band and mode, any number,
//! one point per QSO that is not a dupe, no multipliers.

use zlog_core::{Band, Contest, ContestDef, ContestMode, Extension, Log, Qso, Scoring, Station};
use zlog_scoring::{renew_by_band, BandRules};

zlog_core::texts!();

/// The extension that adds the General contest.
pub struct General;

impl Extension for General {
    fn name(&self) -> &str {
        "General logging"
    }

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        vec![Box::new(GeneralDef)]
    }
}

pub struct GeneralDef;

impl ContestDef for GeneralDef {
    fn id(&self) -> &str {
        "general"
    }

    fn caption(&self, language: &str) -> String {
        texts().get(language, "caption").into()
    }

    fn group(&self, language: &str) -> String {
        texts().get(language, "group").into()
    }

    fn modes(&self) -> &[ContestMode] {
        &[ContestMode::All]
    }

    fn build(&self, _mode: ContestMode) -> Box<dyn Contest> {
        Box::new(GeneralRules)
    }
}

/// The rules of the General contest.
pub struct GeneralRules;

impl Contest for GeneralRules {
    fn name(&self) -> &str {
        "General"
    }

    fn band_range(&self) -> (Band, Band) {
        (Band::B1_9, Band::HIGHEST)
    }

    fn uses_warc(&self) -> bool {
        true
    }

    fn contest_mode(&self) -> ContestMode {
        ContestMode::All
    }

    fn sent_number(&self, _band: Band, _power: zlog_core::Power, _station: &Station) -> String {
        String::new()
    }

    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }

    fn score(&mut self, log: &Log) -> Scoring {
        renew_by_band(log, self, None)
    }
}

impl BandRules for GeneralRules {
    /// The score is the number of points, since there are no multipliers.
    fn total(&self, points: u32, _multi: u32) -> u64 {
        points as u64
    }

    fn reset(&mut self) {}

    fn add_multi(&mut self, _q: &Qso, _dupe: bool) -> (String, bool) {
        (String::new(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn one_point_per_qso_without_dupes() {
        let qso = |call: &str| Qso {
            callsign: call.into(),
            ..Qso::default()
        };
        let mut log = Log {
            qsos: vec![qso("JA1AAA"), qso("JA1AAA"), qso("K1ABC")],
            ..Log::default()
        };
        let sheet = zlog_core::rescore(&mut log, &mut GeneralRules).unwrap();
        assert_eq!(
            (sheet.total.qsos, sheet.total.points, sheet.score),
            (3, 2, 2)
        );
    }
}
