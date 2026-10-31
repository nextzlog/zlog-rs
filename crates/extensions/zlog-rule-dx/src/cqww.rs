use zlog_core::{ContestMode, Mode, Power, Qso, QsoScore, Station};

use crate::lists::{self, find_state, States};
use crate::world::{cq_zone, int_of, split_zone_state, My, Primary, Row, Rule, WorldRules};

pub(crate) struct CqwwRule;

impl Rule for CqwwRule {
    fn id(&self) -> &'static str {
        "cqww"
    }

    fn caption_key(&self) -> &'static str {
        "caption.cqww"
    }

    fn name(&self) -> &'static str {
        "CQWW DX Contest"
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str {
        match mode {
            ContestMode::Mix => "",
            ContestMode::Cw => "CQWW-CW",
            ContestMode::Phone => "CQWW-SSB",
            _ => "CQWW-RTTY",
        }
    }

    fn adif_exchange_field(&self) -> &'static str {
        "cqz"
    }

    fn states(&self) -> &'static States {
        lists::ardx_states()
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        let cont = f.continent();
        let band = q.band;
        let (zone, state) = split_zone_state(&q.nr_rcvd);
        s.multi1 = zone.clone();
        if skip {
            return;
        }
        if w.mode == ContestMode::Rtty && q.mode == Mode::Rtty && !state.is_empty() {
            if let Some(st) = find_state(w.states, &state) {
                let abbrev = st.abbrev.clone();
                s.multi3 = abbrev.clone();
                s.new_multi3 = w.mark(2, &abbrev, (band, 0), false);
            }
        }
        if let Some(z) = int_of(&zone).filter(|z| (1..=40).contains(z)) {
            s.new_multi1 = w.mark(0, &z.to_string(), (band, 0), false);
        }
        s.multi2 = code.to_string();
        s.new_multi2 = w.mark(1, code, (band, 0), false);
        s.points = if q.mode == Mode::Rtty {
            if my.continent != cont {
                3
            } else if my.country == code {
                1
            } else {
                2
            }
        } else if my.country == code {
            0
        } else if my.continent == cont {
            if my.continent == "NA" {
                2
            } else {
                1
            }
        } else {
            3
        };
    }

    fn primary(&self, _w: &WorldRules, q: &Qso) -> Option<Primary> {
        let z = cq_zone(&q.nr_rcvd)?;
        Some((0, z.to_string(), format!("Zone {z}"), (q.band, 0)))
    }

    fn valid_number(&self, w: &WorldRules, q: &Qso) -> bool {
        let nr = q.nr_rcvd.trim();
        if cq_zone(nr).is_none() {
            return false;
        }
        if w.mode != ContestMode::Rtty {
            return true;
        }
        let (_, state) = split_zone_state(nr);
        let code = &w.cty.lookup(&q.callsign, true).country.code;
        let known = find_state(w.states, &state).is_some();
        if matches!(code.as_str(), "K" | "N" | "W" | "VE") {
            known
        } else {
            state.is_empty() || known
        }
    }

    fn sent_number(&self, w: &WorldRules, _power: Power, station: &Station) -> String {
        w.cty.guess_cq_zone(&station.callsign, self.waedc())
    }

    fn exchange_from(&self, w: &WorldRules, past: &Qso) -> Option<String> {
        w.zone_exchange(past)
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let mut v = w.zone_rows(40, "Zone");
        v.extend(w.country_rows(1, |_| true));
        v.extend(w.state_rows(2).into_iter().filter(|r| !r.2.is_empty()));
        v
    }
}

#[cfg(test)]
mod tests {

    use crate::world::tests::*;

    #[test]
    fn cqww() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("JA2AAA", Band::B14, Mode::Cw, "25"),
                q("BY1AA", Band::B14, Mode::Cw, "24"),
                q("W1AW", Band::B14, Mode::Cw, "5"),
                q("W1AW", Band::B14, Mode::Cw, "5"),
                q("K6XX", Band::B21, Mode::Cw, "3"),
                q("DL1AA", Band::B14, Mode::Ssb, "14"),
            ],
        );
        let (_, s) = run(Cqww, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (5, 7, 8, 56));
        assert!(l.qsos[3].dupe);
        assert_eq!(l.qsos[2].multi2, "K");
    }

    #[test]
    fn cqww_rtty_states() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B14, Mode::Rtty, "5CT"),
                q("K1AR", Band::B14, Mode::Rtty, "5MA"),
                q("JA2AAA", Band::B14, Mode::Rtty, "25"),
            ],
        );
        let (c, s) = run(Cqww, ContestMode::Rtty, &mut l);
        assert_eq!(totals(&s), (3, 7, 6, 42));
        assert_eq!(l.qsos[1].multi3, "MA");
        assert!(c.valid_number(&q("W1AW", Band::B14, Mode::Rtty, "5CT")));
        assert!(!c.valid_number(&q("W1AW", Band::B14, Mode::Rtty, "5")));
        assert!(c.valid_number(&q("DL1AA", Band::B14, Mode::Rtty, "14")));
    }

    #[test]
    fn cqww_band_edges_and_invalid() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B1_9, Mode::Cw, "5"),
                q("W1AW", Band::B28, Mode::Cw, "5"),
                q("W1AW", Band::B10, Mode::Cw, "5"),
                q("W1AW", Band::B50, Mode::Cw, "5"),
                invalid(q("DL1AA", Band::B14, Mode::Cw, "14")),
                q("DL1AA", Band::B14, Mode::Cw, "41"),
            ],
        );
        let (c, s) = run(Cqww, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (3, 9, 5, 45));
        assert!(!s.bands.contains_key(&Band::B10));
        assert!(!l.qsos[4].dupe && l.qsos[4].points == 0);
        assert!(!c.valid_number(&q("DL1AA", Band::B14, Mode::Cw, "41")));
        assert!(!c.valid_number(&q("DL1AA", Band::B14, Mode::Cw, "")));
    }
}
