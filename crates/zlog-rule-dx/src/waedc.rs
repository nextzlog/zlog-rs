//! Worked All Europe DX Contest (`TWAEContest`, `UWAEMulti.pas`, `UWAEScore.pas`):
//! European entities per band with a band factor, QTCs add to the points.

use zlog_core::{Band, BandScore, ContestMode, Mode, Power, Qso, QsoScore, Station};

use crate::world::{My, Primary, Row, Rule, WorldRules};

/// WAEDC multiplier bonus per band (`UWAEScore.BandFactor`).
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

    // UWAEMulti.pas TWAEMulti.AddNoUpdate; UWAEScore.pas: 1 point.
    // WAE rules 2025 section 12: in RTTY everybody works everybody and every
    // entity, European or not, is a multiplier for everybody (departure:
    // zLog counts Europe only).
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

    /// `$S`: the serial.
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

    /// WAE rules 2025 section 8: (QSO points + QTCs) x multipliers; zLog takes all
    /// QSOs (`TotQSO`, dupes included) instead of the points.
    fn total(&self, total: &BandScore, qtcs: u32, _continents: u64) -> u64 {
        (total.points + qtcs) as u64 * total.multi as u64
    }
}

#[cfg(test)]
mod tests {
    //! Expected values follow the Delphi code cited per test.

    use crate::world::tests::*;

    #[test]
    fn waedc() {
        // UWAEMulti.pas (Europe only), UWAEScore.pas: (QSOs + QTCs) x factor x multis.
        let mut tagged = q("DL1AA", Band::B7, Mode::Cw, "001");
        tagged.memo = "[QTC 10]".into();
        let mut l = log(
            "JA1ZLO",
            vec![
                q("DL1AA", Band::B3_5, Mode::Cw, "001"), // 80 m factor 4
                tagged,                                  // 40 m factor 3, one QTC
                q("DJ1BB", Band::B7, Mode::Cw, "002"),
                q("W1AW", Band::B7, Mode::Cw, "003"), // not Europe
                q("IT9AA", Band::B14, Mode::Cw, "004"), // *IT9, factor 2
            ],
        );
        let (_, s) = run(Wae, ContestMode::Cw, &mut l);
        // points 4, QTCs 1; multis 4 + 3 + 2 = 9.
        assert_eq!(totals(&s), (5, 4, 9, 45));
        assert_eq!(l.qsos[3].multi1, "Non-EU");
    }

    #[test]
    fn waedc_band_edges_and_dupes() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("DL1AA", Band::B1_9, Mode::Cw, "001"), // below 80 m: out
                q("DL1AA", Band::B28, Mode::Cw, "002"),  // factor 2
                q("DL1AA", Band::B28, Mode::Cw, "003"),  // dupe
                q("DL1AA", Band::B10, Mode::Cw, "004"),  // WARC: out
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
            vec![
                q("W1AW", Band::B14, Mode::Rtty, "001"), // 1, K factor 2
                tagged,                                  // 1, DL factor 2, a QTC
            ],
        );
        let (_, s) = run(Wae, ContestMode::Rtty, &mut l);
        // WAE rules 2025 section 12: (2 points + 1 QTC) x 4.
        assert_eq!(totals(&s), (2, 2, 4, 12));
        assert_eq!(l.qsos[0].multi1, "K");
    }

    #[test]
    fn wae_rtty_counts_non_eu_entities() {
        // WAE rules 2025 section 12: every entity is a multiplier in RTTY.
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B14, Mode::Rtty, "001"),
                q("VE3AA", Band::B14, Mode::Rtty, "002"),
                q("K1AR", Band::B14, Mode::Rtty, "003"), // K again
                q("W1AW", Band::B14, Mode::Rtty, "004"), // dupe
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
