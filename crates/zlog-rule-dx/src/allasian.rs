//! All Asian DX Contest, Asian side (`TAllAsianContest`, `UARRLWMulti.pas` with
//! ALLASIANFLAG on): DXCC entities per band, points by band and continent.

use zlog_core::{Band, ContestMode, Power, Qso, QsoScore, Station};

use crate::world::{My, Primary, Row, Rule, WorldRules, CW_PH};

pub(crate) struct AllAsianRule;

impl Rule for AllAsianRule {
    fn id(&self) -> &'static str {
        "allasian"
    }

    fn caption_key(&self) -> &'static str {
        "caption.allasian"
    }

    fn name(&self) -> &'static str {
        "All Asian DX Contest (Asia)"
    }

    fn modes(&self) -> &'static [ContestMode] {
        CW_PH
    }

    fn waedc(&self) -> bool {
        false
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str {
        match mode {
            ContestMode::Mix => "",
            ContestMode::Cw => "ALL-ASIAN-DX-CW",
            ContestMode::Phone => "ALL-ASIAN-DX-SSB",
            _ => "",
        }
    }

    fn adif_exchange_field(&self) -> &'static str {
        "age"
    }

    // UARRLWMulti.pas TARRLWMulti.AddNoUpdate (ALLASIANFLAG on).
    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        if !f.country.is_waedc() {
            s.multi1 = code.to_string();
        }
        if skip {
            return;
        }
        if my.country == code {
            // zLog exits before marking (`AddNoUpdate`, `C.Country = MyCountry`);
            // All Asian DX rules (JARL 2024): "Contact among same entities will
            // neither count as a point nor a multiplier."
            s.points = 0;
            return;
        }
        let asia = f.country.continent == "AS";
        s.points = match (q.band, asia) {
            (Band::B1_9, true) => 3,
            (Band::B3_5 | Band::B28, true) => 2,
            (_, true) => 1,
            (Band::B1_9, false) => 9,
            (Band::B3_5 | Band::B28, false) => 6,
            (_, false) => 3,
        };
        if !f.country.is_waedc() {
            s.new_multi1 = w.mark(0, code, (q.band, 0), false);
        }
    }

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary> {
        w.country_primary(q)
    }

    /// `$A`: the age ([`Station::age`], UzLogContest.pas:2583).
    fn sent_number(&self, _w: &WorldRules, _power: Power, station: &Station) -> String {
        station.age.clone()
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let mut v = w.country_rows(0, |_| true);
        let extra = w.extra_rows(0, &v);
        v.extend(extra);
        v
    }

    fn counts_every_mode(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    //! Expected values follow the Delphi code cited per test.

    use crate::world::tests::*;

    #[test]
    fn all_asian() {
        // UARRLWMulti.pas AddNoUpdate with ALLASIANFLAG.
        let mut l = log(
            "JA1ZLO",
            vec![
                q("JA2AAA", Band::B14, Mode::Cw, "50"),
                q("BY1AA", Band::B1_9, Mode::Cw, "40"),
                q("BY1AA", Band::B28, Mode::Cw, "40"),
                q("DL1AA", Band::B7, Mode::Cw, "30"),
                q("W1AW", Band::B3_5, Mode::Cw, "30"),
            ],
        );
        let (_, s) = run(AllAsian, ContestMode::Cw, &mut l);
        // 0 + 3 + 2 + 3 + 6 = 14; BY, BY, DL, K (own country JA: no multiplier, per
        // zLog and the JARL rules).
        assert_eq!(totals(&s), (5, 14, 4, 56));
        assert!(!l.qsos[0].new_multi1);
    }

    #[test]
    fn all_asian_band_edges_and_dupes() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("DL1AA", Band::B28, Mode::Cw, "30"), // 6
                q("DL1AA", Band::B28, Mode::Cw, "30"), // dupe
                q("DL1AA", Band::B10, Mode::Cw, "30"), // WARC: out
                q("DL1AA", Band::B50, Mode::Cw, "30"), // out
                q("DL1AA", Band::B21, Mode::Cw, "30"), // 3, DL on 21
            ],
        );
        let (c, s) = run(AllAsian, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (3, 9, 2, 18));
        assert!(!c.valid_number(&q("DL1AA", Band::B14, Mode::Cw, "")));
    }

    #[test]
    fn all_asian_phone_entry() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("BY1AA", Band::B14, Mode::Ssb, "40"), // 1
                q("BY2AA", Band::B14, Mode::Cw, "40"),  // CW in a phone log: no score
            ],
        );
        let (_, s) = run(AllAsian, ContestMode::Phone, &mut l);
        assert_eq!((s.total.points, s.total.multi, s.score), (1, 1, 1));
    }
}
