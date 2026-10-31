use zlog_core::{Band, BandScore, ContestMode, Power, Qso, QsoScore, Station};

use crate::world::{My, Primary, Row, Rule, WorldRules, EVERY};

pub(crate) struct PediRule;

impl Rule for PediRule {
    fn id(&self) -> &'static str {
        "pedi"
    }

    fn caption_key(&self) -> &'static str {
        "caption.pedi"
    }

    fn name(&self) -> &'static str {
        "Pedition mode"
    }

    fn modes(&self) -> &'static [ContestMode] {
        EVERY
    }

    fn bands(&self) -> (Band, Band) {
        (Band::B1_9, Band::HIGHEST)
    }

    fn different_mode(&self) -> bool {
        true
    }

    fn period(&self) -> u32 {
        0
    }

    fn start_hour(&self) -> Option<u32> {
        None
    }

    fn uses_utc(&self) -> bool {
        false
    }

    fn uses_warc(&self) -> bool {
        true
    }

    fn adif_contest_id(&self, _mode: ContestMode) -> &'static str {
        ""
    }

    fn add(&self, _w: &mut WorldRules, _q: &Qso, _skip: bool, _my: &My, s: &mut QsoScore) {
        s.points = 1;
    }

    fn primary(&self, _w: &WorldRules, _q: &Qso) -> Option<Primary> {
        None
    }

    fn valid_number(&self, _w: &WorldRules, _q: &Qso) -> bool {
        true
    }

    fn sent_number(&self, _w: &WorldRules, _power: Power, _station: &Station) -> String {
        String::new()
    }

    fn multi_table(&self, _w: &WorldRules) -> Vec<Row> {
        Vec::new()
    }

    fn dupes_score(&self) -> bool {
        true
    }

    fn counts_every_mode(&self) -> bool {
        true
    }

    fn total(&self, total: &BandScore, _qtcs: u32, _continents: u64) -> u64 {
        total.points as u64
    }
}

#[cfg(test)]
mod tests {

    use crate::world::tests::*;

    #[test]
    fn pedi() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B10, Mode::Cw, ""),
                q("W1AW", Band::B10, Mode::Ft8, ""),
                q("W1AW", Band::B144, Mode::Fm, ""),
            ],
        );
        let (_, s) = run(Pedi, ContestMode::All, &mut l);
        assert_eq!(totals(&s), (3, 3, 0, 3));
    }

    #[test]
    fn pedi_counts_every_band_and_dupes_score() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B1_9, Mode::Cw, ""),
                q("W1AW", Band::B1_9, Mode::Cw, ""),
                q("W1AW", Band::B248G, Mode::Fm, ""),
            ],
        );
        let (c, s) = run(Pedi, ContestMode::All, &mut l);
        assert_eq!(s.total.points, 3);
        assert!(c.valid_number(&q("X", Band::B14, Mode::Cw, "")));
        assert!(c.multi_table().is_empty());
    }

    #[test]
    fn pedi_invalid_qso_scores_nothing() {
        let mut l = log(
            "JA1ZLO",
            vec![
                invalid(q("W1AW", Band::B14, Mode::Cw, "")),
                q("W1AW", Band::B14, Mode::Cw, ""),
            ],
        );
        let (_, s) = run(Pedi, ContestMode::All, &mut l);
        assert_eq!(totals(&s), (1, 1, 0, 1));
    }
}
