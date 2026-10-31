use zlog_core::{Band, Contest, ContestDef, ContestMode, Extension, Log, Qso, Scoring, Station};

use crate::{renew_by_band, texts, BandRules};

pub struct Base;

impl Extension for Base {
    fn name(&self) -> &str {
        "Base rules"
    }

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        vec![Box::new(BaseDef)]
    }
}

pub struct BaseDef;

impl ContestDef for BaseDef {
    fn id(&self) -> &str {
        "base"
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
        Box::new(BaseRules)
    }
}

pub struct BaseRules;

impl Contest for BaseRules {
    fn name(&self) -> &str {
        "Base"
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

    fn sent_template(&self, _station: &Station) -> Option<String> {
        Some(String::new())
    }

    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }

    fn score(&mut self, log: &Log) -> Scoring {
        renew_by_band(log, self, None)
    }
}

impl BandRules for BaseRules {
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
    fn the_sent_template_is_empty_like_zlog() {
        let t = BaseRules.sent_template(&Station::default());
        assert_eq!(t.as_deref(), Some(""));
    }

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
        let sheet = zlog_core::rescore(&mut log, &mut BaseRules).unwrap();
        assert_eq!(
            (sheet.total.qsos, sheet.total.points, sheet.score),
            (3, 2, 2)
        );
    }

    #[test]
    fn every_band_warc_and_mode_counts() {
        let qso = |call: &str, band, mode| Qso {
            callsign: call.into(),
            band,
            mode,
            ..Qso::default()
        };
        let mut log = Log {
            qsos: vec![
                qso("JA1AAA", Band::B1_9, zlog_core::Mode::Cw),
                qso("JA1AAA", Band::B10, zlog_core::Mode::Cw),
                qso("JA1AAA", Band::B248G, zlog_core::Mode::Ft8),
                qso("JA1AAA", Band::B10, zlog_core::Mode::Ssb),
            ],
            ..Log::default()
        };
        let sheet = zlog_core::rescore(&mut log, &mut BaseRules).unwrap();
        assert_eq!(
            (sheet.total.points, sheet.total.multi, sheet.score),
            (3, 0, 3)
        );
        assert!(log.qsos[3].dupe);
    }

    #[test]
    fn invalid_qsos_and_empty_log() {
        let mut log = Log::default();
        let sheet = zlog_core::rescore(&mut log, &mut BaseRules).unwrap();
        assert_eq!(sheet.score, 0);
        log.qsos.push(Qso {
            callsign: "JA1AAA".into(),
            invalid: true,
            ..Qso::default()
        });
        let sheet = zlog_core::rescore(&mut log, &mut BaseRules).unwrap();
        assert_eq!(sheet.score, 0);
        assert!(BaseRules.valid_number(&Qso::default()));
        assert_eq!(BaseRules.period_hours(), 0);
        assert!(BaseRules.adif_fields(&Qso::default()).is_empty());
    }
}
