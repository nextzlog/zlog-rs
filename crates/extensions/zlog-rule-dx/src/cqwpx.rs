use zlog_core::{Band, ContestMode, Mode, Power, Qso, QsoScore, Station};

use crate::cty::wpx_prefix;
use crate::world::{int_of, My, Primary, Row, Rule, WorldRules};

pub(crate) struct WpxRule;

impl Rule for WpxRule {
    fn id(&self) -> &'static str {
        "cqwpx"
    }

    fn caption_key(&self) -> &'static str {
        "caption.cqwpx"
    }

    fn name(&self) -> &'static str {
        "CQ WPX Contest"
    }

    fn serial(&self) -> bool {
        true
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str {
        match mode {
            ContestMode::Mix => "",
            ContestMode::Cw => "CQWPX-CW",
            ContestMode::Phone => "CQWPX-SSB",
            _ => "CQWPX-RTTY",
        }
    }

    fn adif_extra(&self, _w: &WorldRules, q: &Qso) -> Option<(String, String)> {
        Some(("pfx".into(), q.multi1.clone()))
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        let cont = f.continent();
        let band = q.band;
        let px = wpx_prefix(&q.callsign);
        s.multi1 = px.clone();
        if skip {
            return;
        }
        s.new_multi1 = w.mark(0, &px, (band, 0), true);
        let low = band <= Band::B7;
        s.points = if q.mode == Mode::Rtty {
            let base = if my.continent != cont {
                3
            } else if my.country == code {
                1
            } else {
                2
            };
            match band {
                Band::B14 | Band::B21 | Band::B28 => base,
                Band::B3_5 | Band::B7 => base * 2,
                _ => 0,
            }
        } else if my.country == code {
            1
        } else if my.continent == cont {
            match (my.continent == "NA", low) {
                (true, true) => 4,
                (true, false) | (false, true) => 2,
                (false, false) => 1,
            }
        } else if low {
            6
        } else {
            3
        };
    }

    fn primary(&self, _w: &WorldRules, q: &Qso) -> Option<Primary> {
        let px = wpx_prefix(&q.callsign);
        (!q.callsign.is_empty()).then(|| (0, px.clone(), px, (Band::Unknown, 0)))
    }

    fn valid_number(&self, _w: &WorldRules, q: &Qso) -> bool {
        int_of(q.nr_rcvd.trim()).is_some_and(|n| n >= 0)
    }

    fn sent_number(&self, w: &WorldRules, _power: Power, station: &Station) -> String {
        w.serial_text(station)
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        w.extra_rows(0, &[])
    }
}

#[cfg(test)]
mod tests {

    use crate::world::tests::*;

    #[test]
    fn wpx() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B7, Mode::Cw, "001"),
                q("W1AW", Band::B14, Mode::Cw, "002"),
                q("BY1AA", Band::B7, Mode::Cw, "003"),
                q("JA2AAA", Band::B14, Mode::Cw, "004"),
                q("JA1BBB", Band::B21, Mode::Cw, "005"),
            ],
        );
        l.qsos[4].nr_sent = "005".into();
        let (c, s) = run(Wpx, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (5, 13, 4, 52));
        assert!(!l.qsos[1].new_multi1);
        assert_eq!(c.next_serial(), 6);
        let st = Station::default();
        assert_eq!(c.sent_number(Band::B7, Power::P100, &st), "006");
        assert_eq!(c.exchange_from(Band::B7, &l.qsos[0]), None);
    }

    #[test]
    fn wpx_prefix_once_and_band_edges() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B14, Mode::Cw, "001"),
                q("W1BB", Band::B21, Mode::Cw, "002"),
                q("W1CC", Band::B7, Mode::Cw, "003"),
                q("W1AW", Band::B14, Mode::Cw, "004"),
                q("W1DD", Band::B18, Mode::Cw, "005"),
                q("W1EE", Band::B14, Mode::Ssb, "006"),
            ],
        );
        let (c, s) = run(Wpx, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (4, 12, 1, 12));
        assert!(l.qsos[3].dupe);
        assert!(c.valid_number(&q("X", Band::B14, Mode::Cw, "0")));
        assert!(!c.valid_number(&q("X", Band::B14, Mode::Cw, "5A")));
        assert!(!c.valid_number(&q("X", Band::B14, Mode::Cw, "")));
    }

    #[test]
    fn wpx_rtty_points() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B3_5, Mode::Rtty, "001"),
                q("BY1AA", Band::B14, Mode::Rtty, "002"),
                q("JA2AAA", Band::B7, Mode::Rtty, "003"),
                q("JA3AAA", Band::B28, Mode::Rtty, "004"),
            ],
        );
        let (_, s) = run(Wpx, ContestMode::Rtty, &mut l);
        assert_eq!(totals(&s), (4, 11, 4, 44));
    }
}
