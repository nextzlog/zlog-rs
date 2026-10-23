//! ARRL 10-Meter Contest (`TARRL10Contest`, `UARRL10Multi.pas`, `UARRL10Score.pas`):
//! W/VE/XE states and provinces, ITU regions of /MM stations and DXCC entities,
//! by band and mode.

use zlog_core::{Band, ContestMode, Mode, Power, Qso, QsoScore, Station};

use crate::lists::{self, find_state, State};
use crate::world::{int_of, My, Primary, Row, Rule, WorldRules, MIX_CW_PH_ALL};

fn is_wve(code: &str) -> bool {
    matches!(code, "K" | "W" | "N" | "KH6" | "KL7" | "KL" | "VE")
}

pub(crate) struct Arrl10Rule;

impl Rule for Arrl10Rule {
    fn id(&self) -> &'static str {
        "arrl10"
    }

    fn caption_key(&self) -> &'static str {
        "caption.arrl10"
    }

    fn name(&self) -> &'static str {
        "ARRL 10m Contest"
    }

    fn modes(&self) -> &'static [ContestMode] {
        MIX_CW_PH_ALL
    }

    fn bands(&self) -> (Band, Band) {
        (Band::B28, Band::B28)
    }

    fn waedc(&self) -> bool {
        false
    }

    fn different_mode(&self) -> bool {
        true
    }

    fn serial(&self) -> bool {
        true
    }

    fn adif_contest_id(&self, _mode: ContestMode) -> &'static str {
        "ARRL-10"
    }

    fn states(&self) -> Vec<State> {
        lists::arrl10_states()
    }

    // UARRL10Multi.pas TARRL10Multi.AddNoUpdate; UARRL10Score.pas points.
    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, _my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        let slot = (q.band, if q.mode == Mode::Cw { 1 } else { 2 });
        if skip {
            return;
        }
        if is_wve(code) || q.callsign.contains("/MM") || code == "XE" {
            if let Some(st) = find_state(&w.states, &q.nr_rcvd) {
                let abbrev = st.abbrev.clone();
                s.multi1 = abbrev.clone();
                s.new_multi1 = w.mark(0, &abbrev, slot, false);
            }
        } else if !f.country.is_waedc() {
            s.multi1 = code.to_string();
            s.new_multi1 = w.mark(1, code, slot, false);
        }
        // ARRL 10 m rules v2.0 (2026) section 5.1: CW 4 points for every
        // station; zLog's 8 points for /N and /T novice calls are gone.
        s.points = match q.mode {
            Mode::Cw => 4,
            Mode::Ssb | Mode::Fm | Mode::Am => 2,
            _ => 0,
        };
    }

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary> {
        let f = w.cty.lookup(&q.callsign, self.waedc());
        let slot = (q.band, if q.mode == Mode::Cw { 1 } else { 2 });
        if is_wve(&f.country.code) || q.callsign.contains("/MM") || f.country.code == "XE" {
            let st = find_state(&w.states, &q.nr_rcvd)?;
            Some((0, st.abbrev.clone(), st.name.clone(), slot))
        } else {
            w.country_primary(q).map(|(_, k, n, _)| (1, k, n, slot))
        }
    }

    fn valid_number(&self, w: &WorldRules, q: &Qso) -> bool {
        let nr = q.nr_rcvd.trim();
        if nr.is_empty() {
            return false;
        }
        if q.callsign.contains("/MM") {
            return matches!(nr, "1" | "2" | "3");
        }
        let code = &w.cty.lookup(&q.callsign, false).country.code;
        if is_wve(code) || code == "XE" {
            find_state(&w.states, nr).is_some()
        } else {
            int_of(nr).is_some_and(|n| n > 0)
        }
    }

    /// ARRL 10 m rules: W, VE and XE send their state or province, /MM
    /// stations their ITU region ([`Station::itu_region`]),
    /// the others a serial.
    fn sent_number(&self, w: &WorldRules, _power: Power, station: &Station) -> String {
        let code = &w.cty.lookup(&station.callsign, self.waedc()).country.code;
        if station.callsign.to_uppercase().contains("/MM") {
            station.itu_region.clone()
        } else if is_wve(code) || code == "XE" {
            station.prov.clone()
        } else {
            w.serial_text(station)
        }
    }

    /// The state or country (`TARRL10Multi.ExtractMulti`).
    fn extract_multi(&self, w: &WorldRules, q: &Qso) -> String {
        self.primary(w, q).map(|p| p.1).unwrap_or_default()
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let mut v = w.state_rows(0);
        v.extend(w.country_rows(1, |c| !is_wve(&c.code) && c.code != "XE"));
        v
    }
}

#[cfg(test)]
mod tests {
    //! Expected values follow the Delphi code cited per test.

    use crate::world::tests::*;

    #[test]
    fn arrl10() {
        // UARRL10Multi.pas (mode slots), UARRL10Score.pas (CW 4, /N 8, phone 2).
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B28, Mode::Cw, "CT"),
                q("W1AW", Band::B28, Mode::Ssb, "CT"),
                q("K1AA/N", Band::B28, Mode::Cw, "MA"),
                q("DL1AA", Band::B28, Mode::Cw, "123"),
                q("XE1AA", Band::B28, Mode::Ssb, "TLAX"),
            ],
        );
        let (c, s) = run(Arrl10, ContestMode::Mix, &mut l);
        // ARRL 10 m rules 2026 v2.0 5.1: no novice bonus (zLog gives /N 8 points):
        // 4 + 2 + 4 + 4 + 2 = 16; CT cw, CT ph, MA cw, DL cw, TLX ph.
        assert_eq!(totals(&s), (5, 16, 5, 80));
        assert!(c.valid_number(&q("X/MM", Band::B28, Mode::Cw, "2")));
        assert!(!c.valid_number(&q("DL1AA", Band::B28, Mode::Cw, "CT")));
        let me = Station {
            callsign: "W1AW".into(),
            prov: "CT".into(),
            ..Station::default()
        };
        assert_eq!(c.sent_number(Band::B28, Power::P100, &me), "CT");
    }

    #[test]
    fn arrl10_only_28_mhz_and_dupes() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B21, Mode::Cw, "CT"), // out
                q("W1AW", Band::B50, Mode::Cw, "CT"), // out
                q("W1AW", Band::B28, Mode::Cw, "CT"), // 4, CT cw
                q("W1AW", Band::B28, Mode::Cw, "CT"), // dupe
                q("K1AR", Band::B28, Mode::Cw, "CT"), // 4, CT cw not new
            ],
        );
        let (_, s) = run(Arrl10, ContestMode::Mix, &mut l);
        assert_eq!(totals(&s), (3, 8, 1, 8));
    }

    #[test]
    fn arrl10_digital_scores_nothing_and_mm_needs_a_state() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("DL1AA", Band::B28, Mode::Ssb, "001"),  // 2, DL ph
                q("DL2AA", Band::B28, Mode::Rtty, "002"), // not in a mixed CW/phone log
                q("W1AW/MM", Band::B28, Mode::Cw, "2"),   // 4, ITU region 2 cw
                q("W2AW/MM", Band::B28, Mode::Cw, "9"),   // 4, no region 9
            ],
        );
        let (_, s) = run(Arrl10, ContestMode::Mix, &mut l);
        assert_eq!((s.total.points, s.total.multi), (10, 2));
        assert!(l.qsos[3].multi1.is_empty());
    }

    #[test]
    fn arrl10_sent_state_or_province() {
        let c = rules(Arrl10, ContestMode::Mix);
        let st = |call: &str| Station {
            callsign: call.into(),
            prov: "ON".into(),
            ..Station::default()
        };
        assert_eq!(c.sent_number(Band::B28, Power::P100, &st("VE3AA")), "ON");
        assert_eq!(c.sent_number(Band::B28, Power::P100, &st("XE1AA")), "ON");
        assert_eq!(c.sent_number(Band::B28, Power::P100, &st("W1AW")), "ON");
        // /MM sends the ITU region of the station settings, empty when not set.
        assert_eq!(c.sent_number(Band::B28, Power::P100, &st("W1AW/MM")), "");
        let mm = Station {
            itu_region: "3".into(),
            ..st("JA1ZLO/MM")
        };
        assert_eq!(c.sent_number(Band::B28, Power::P100, &mm), "3");
        assert_eq!(c.sent_number(Band::B28, Power::P100, &st("JA1ZLO")), "001");
        assert_eq!(c.sent_number(Band::B28, Power::P100, &st("")), "001");
    }
}
