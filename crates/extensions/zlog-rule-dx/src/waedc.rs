use zlog_core::{Band, BandScore, ContestMode, Mode, Power, Qso, QsoScore, Station};

use crate::world::{My, Primary, Row, Rule, WorldRules};

fn wae_factor(band: Band) -> u32 {
    match band {
        Band::B3_5 => 4,
        Band::B7 => 3,
        Band::B14 | Band::B21 | Band::B28 => 2,
        _ => 0,
    }
}

pub(crate) struct WaeRule;

impl Rule for WaeRule {
    fn id(&self) -> &'static str {
        "waedc"
    }

    fn caption_key(&self) -> &'static str {
        "caption.waedc"
    }

    fn name(&self) -> &'static str {
        "WAEDC Contest"
    }

    fn bands(&self) -> (Band, Band) {
        (Band::B3_5, Band::B28)
    }

    fn serial(&self) -> bool {
        true
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str {
        match mode {
            ContestMode::Mix => "",
            ContestMode::Cw => "DARC-WAEDC-CW",
            ContestMode::Phone => "DARC-WAEDC-SSB",
            _ => "DARC-WAEDC-RTTY",
        }
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, _my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        if f.country.continent != "EU" && q.mode != Mode::Rtty {
            s.multi1 = "Non-EU".into();
            return;
        }
        s.multi1 = code.to_string();
        if skip {
            return;
        }
        s.new_multi1 = w.mark(0, code, (q.band, 0), false);
        s.points = 1;
    }

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary> {
        let f = w.cty.lookup(&q.callsign, self.waedc());
        w.country_primary(q).filter(|_| f.country.continent == "EU")
    }

    fn sent_number(&self, w: &WorldRules, _power: Power, station: &Station) -> String {
        w.serial_text(station)
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        w.country_rows(0, |c| c.continent == "EU")
    }

    fn multi_weight(&self, band: Band) -> u32 {
        wae_factor(band)
    }

    fn counts_qtcs(&self) -> bool {
        true
    }

    fn total(&self, total: &BandScore, qtcs: u32, _continents: u64) -> u64 {
        (total.points + qtcs) as u64 * total.multi as u64
    }
}

#[cfg(test)]
mod tests {

    use crate::world::tests::*;

    #[test]
    fn waedc() {
        let mut tagged = q("DL1AA", Band::B7, Mode::Cw, "001");
        tagged.memo = "[QTC 10]".into();
        let mut l = log(
            "JA1ZLO",
            vec![
                q("DL1AA", Band::B3_5, Mode::Cw, "001"),
                tagged,
                q("DJ1BB", Band::B7, Mode::Cw, "002"),
                q("W1AW", Band::B7, Mode::Cw, "003"),
                q("IT9AA", Band::B14, Mode::Cw, "004"),
            ],
        );
        let (_, s) = run(Wae, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (5, 4, 9, 45));
        assert_eq!(l.qsos[3].multi1, "Non-EU");
    }

    #[test]
    fn waedc_band_edges_and_dupes() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("DL1AA", Band::B1_9, Mode::Cw, "001"),
                q("DL1AA", Band::B28, Mode::Cw, "002"),
                q("DL1AA", Band::B28, Mode::Cw, "003"),
                q("DL1AA", Band::B10, Mode::Cw, "004"),
            ],
        );
        let (_, s) = run(Wae, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (2, 1, 2, 2));
    }

    #[test]
    fn waedc_rtty_non_europe_is_a_multiplier() {
        let mut tagged = q("DL1AA", Band::B14, Mode::Rtty, "002");
        tagged.memo = "[QTC 5]".into();
        let mut l = log(
            "JA1ZLO",
            vec![q("W1AW", Band::B14, Mode::Rtty, "001"), tagged],
        );
        let (_, s) = run(Wae, ContestMode::Rtty, &mut l);
        assert_eq!(totals(&s), (2, 2, 4, 12));
        assert_eq!(l.qsos[0].multi1, "K");
    }

    #[test]
    fn wae_rtty_counts_non_eu_entities() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B14, Mode::Rtty, "001"),
                q("VE3AA", Band::B14, Mode::Rtty, "002"),
                q("K1AR", Band::B14, Mode::Rtty, "003"),
                q("W1AW", Band::B14, Mode::Rtty, "004"),
                q("DL1AA", Band::B14, Mode::Rtty, "005"),
            ],
        );
        let (_, s) = run(Wae, ContestMode::Rtty, &mut l);
        assert_eq!(l.qsos[0].multi1, "K");
        assert!(l.qsos[0].new_multi1 && l.qsos[1].new_multi1);
        assert!(!l.qsos[2].new_multi1);
        assert_eq!(l.qsos[3].points, 0);
        assert!(l.qsos[4].new_multi1);
        assert_eq!(s.total.points, 4);
    }

    #[test]
    fn wae_cw_still_ignores_non_eu() {
        let mut l = log("JA1ZLO", vec![q("W1AW", Band::B14, Mode::Cw, "001")]);
        let (_, s) = run(Wae, ContestMode::Cw, &mut l);
        assert_eq!(l.qsos[0].multi1, "Non-EU");
        assert_eq!(totals(&s), (1, 0, 0, 0));
    }
}
