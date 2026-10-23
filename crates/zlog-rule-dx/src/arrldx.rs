//! ARRL International DX Contest, both sides: DX stations work W/VE states and
//! provinces (`TARRLDXContestDX`, `UARRLDXMulti.pas`), W/VE stations work DXCC
//! entities (`TARRLDXContestW`, `UARRLWMulti.pas`); 3 points (`UARRLDXScore.pas`).

use zlog_core::{ContestMode, Power, Qso, QsoScore, Station};

use crate::lists::{self, find_state, State};
use crate::world::{My, Primary, Row, Rule, WorldRules};

/// `AdifContestId` of both sides. Departure: zLog writes `ARRLSX-SSB` for ARRL DX
/// phone, a typo fixed here.
fn adif_contest_id(mode: ContestMode) -> &'static str {
    match mode {
        ContestMode::Mix => "",
        ContestMode::Cw => "ARRLDX-CW",
        ContestMode::Phone => "ARRLDX-SSB",
        _ => "",
    }
}

/// The DX side: states and provinces per band.
pub(crate) struct ArrlDxDxRule;

impl Rule for ArrlDxDxRule {
    fn id(&self) -> &'static str {
        "arrldx-dx"
    }

    fn caption_key(&self) -> &'static str {
        "caption.arrldx_dx"
    }

    fn name(&self) -> &'static str {
        "ARRL International DX Contest (DX)"
    }

    fn waedc(&self) -> bool {
        false
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str {
        adif_contest_id(mode)
    }

    fn adif_exchange_field(&self) -> &'static str {
        "state"
    }

    fn states(&self) -> Vec<State> {
        lists::ardx_states()
    }

    // UARRLDXMulti.pas TARRLDXMulti.AddNoUpdate; UARRLDXScore.pas: 3 points.
    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, _my: &My, s: &mut QsoScore) {
        let st = find_state(&w.states, &q.nr_rcvd).map(|s| s.abbrev.clone());
        s.multi1 = st.clone().unwrap_or_default();
        if skip {
            return;
        }
        if let Some(st) = st {
            s.new_multi1 = w.mark(0, &st, (q.band, 0), false);
        }
        s.points = 3;
    }

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary> {
        let st = find_state(&w.states, &q.nr_rcvd)?;
        Some((0, st.abbrev.clone(), st.name.clone(), (q.band, 0)))
    }

    fn valid_number(&self, w: &WorldRules, q: &Qso) -> bool {
        find_state(&w.states, q.nr_rcvd.trim()).is_some()
    }

    /// `$N`: the power.
    fn sent_number(&self, _w: &WorldRules, power: Power, _station: &Station) -> String {
        match power {
            Power::P1000 => "1KW".into(),
            p => p.label().trim_end_matches('W').to_string(),
        }
    }

    /// The state (`TARRLDXMulti.ExtractMulti`).
    fn extract_multi(&self, w: &WorldRules, q: &Qso) -> String {
        self.primary(w, q).map(|p| p.1).unwrap_or_default()
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        w.state_rows(0)
    }
}

/// The W/VE side: DXCC entities per band.
pub(crate) struct ArrlDxWRule;

impl Rule for ArrlDxWRule {
    fn id(&self) -> &'static str {
        "arrldx-w"
    }

    fn caption_key(&self) -> &'static str {
        "caption.arrldx_w"
    }

    fn name(&self) -> &'static str {
        "ARRL International DX Contest (W/VE)"
    }

    fn waedc(&self) -> bool {
        false
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str {
        adif_contest_id(mode)
    }

    fn adif_exchange_field(&self) -> &'static str {
        "rx_pwr"
    }

    // UARRLWMulti.pas TARRLWMulti.AddNoUpdate (ALLASIANFLAG off).
    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, _my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        if !f.country.is_waedc() {
            s.multi1 = code.to_string();
        }
        if skip {
            return;
        }
        s.points = 3;
        // ARRL DX rules 2026 (v2.0) 5.2.2: W/VE count DXCC entities except
        // USA and Canada; zLog counts them too.
        if matches!(code, "K" | "VE") {
            return;
        }
        if !f.country.is_waedc() {
            s.new_multi1 = w.mark(0, code, (q.band, 0), false);
        }
    }

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary> {
        w.country_primary(q)
    }

    /// `$V`: the state or province.
    fn sent_number(&self, _w: &WorldRules, _power: Power, station: &Station) -> String {
        station.prov.clone()
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let mut v = w.country_rows(0, |_| true);
        let extra = w.extra_rows(0, &v);
        v.extend(extra);
        v
    }
}

#[cfg(test)]
mod tests {
    //! Expected values follow the Delphi code cited per test.

    use crate::world::tests::*;

    #[test]
    fn arrl_dx() {
        // UARRLDXMulti.pas (states per band), UARRLDXScore.pas (3 points).
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B14, Mode::Cw, "CT"),
                q("K1AR", Band::B14, Mode::Cw, "MA"),
                q("K1XX", Band::B14, Mode::Cw, "CT"),
                q("W1AW", Band::B21, Mode::Cw, "CT"),
            ],
        );
        let (c, s) = run(ArrlDxDx, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (4, 12, 3, 36));
        assert!(!c.valid_number(&q("W1AW", Band::B14, Mode::Cw, "XX")));
        // UARRLWMulti.pas (countries per band) for the W/VE side.
        let mut l = log(
            "W1AW",
            vec![
                q("JA1ZLO", Band::B14, Mode::Cw, "100"),
                q("JA2AAA", Band::B14, Mode::Cw, "KW"),
                q("IT9AAA", Band::B14, Mode::Cw, "100"), // no WAE list: Italy
                q("KH6AA", Band::B14, Mode::Cw, "100"),  // Hawaii is DX
                q("VE3AA", Band::B14, Mode::Cw, "ON"),   // rules 5.2.2: no multiplier
            ],
        );
        let (_, s) = run(ArrlDxW, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (5, 15, 3, 45));
        assert_eq!(l.qsos[2].multi1, "I");
    }

    #[test]
    fn arrl_dx_dx_dupes_and_band_edges() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B1_9, Mode::Cw, "CT"),
                q("W1AW", Band::B1_9, Mode::Cw, "CT"), // dupe
                q("W1AW", Band::B10, Mode::Cw, "CT"),  // WARC: out
                q("W1AW", Band::B50, Mode::Cw, "CT"),  // out
                q("K1AR", Band::B28, Mode::Cw, "MA"),
            ],
        );
        let (_, s) = run(ArrlDxDx, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (3, 6, 2, 12));
    }

    #[test]
    fn arrl_dx_dx_mode_and_unknown_state() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B14, Mode::Ssb, "CT"), // phone in a CW log
                q("K1AR", Band::B14, Mode::Cw, "ZZ"),  // 3 points, no state
                q("W1AW", Band::B14, Mode::Cw, "CT"),  // dupe of the phone QSO
            ],
        );
        let (c, s) = run(ArrlDxDx, ContestMode::Cw, &mut l);
        // Dupes ignore the mode (`AcceptDifferentMode` off).
        assert_eq!(totals(&s), (2, 3, 0, 0));
        assert!(l.qsos[2].dupe);
        assert!(c.valid_number(&q("W1AW", Band::B14, Mode::Cw, "CT")));
    }

    #[test]
    fn arrl_dx_w_countries_per_band() {
        let mut l = log(
            "W1AW",
            vec![
                q("DL1AA", Band::B14, Mode::Cw, "100"),
                q("DL2AA", Band::B14, Mode::Cw, "100"), // DL not new
                q("DL1AA", Band::B21, Mode::Cw, "100"), // DL on 21
                q("DL1AA", Band::B21, Mode::Cw, "100"), // dupe
                q("XE1AA", Band::B18, Mode::Cw, "100"), // WARC: out
            ],
        );
        let (c, s) = run(ArrlDxW, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (4, 9, 2, 18));
        assert!(!c.valid_number(&q("DL1AA", Band::B14, Mode::Cw, "")));
    }

    #[test]
    fn arrl_dx_w_mode_category() {
        let mut l = log(
            "W1AW",
            vec![
                q("DL1AA", Band::B14, Mode::Cw, "100"),
                q("JA1ZLO", Band::B14, Mode::Ssb, "100"),
            ],
        );
        let (_, s) = run(ArrlDxW, ContestMode::Phone, &mut l);
        assert_eq!(totals(&s), (1, 3, 1, 3));
    }
}
