use zlog_core::{ContestMode, Power, Qso, QsoScore, Station};

use crate::world::{int_of, My, Primary, Row, Rule, WorldRules, MIX_CW_PH_ALL};

pub(crate) struct IaruRule;

impl Rule for IaruRule {
    fn id(&self) -> &'static str {
        "iaru"
    }

    fn caption_key(&self) -> &'static str {
        "caption.iaru"
    }

    fn name(&self) -> &'static str {
        "IARU HF Championship"
    }

    fn modes(&self) -> &'static [ContestMode] {
        MIX_CW_PH_ALL
    }

    fn different_mode(&self) -> bool {
        true
    }

    fn period(&self) -> u32 {
        24
    }

    fn start_hour(&self) -> Option<u32> {
        Some(12)
    }

    fn adif_contest_id(&self, _mode: ContestMode) -> &'static str {
        "IARU-HF"
    }

    fn adif_exchange_field(&self) -> &'static str {
        "ituz"
    }

    fn read_sent(&self, my: &mut My, sent: &str) {
        if let Some(z) = int_of(sent).filter(|z| (1..=90).contains(z)) {
            my.itu = z.to_string();
        }
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let cont = f.continent();
        s.multi1 = q.nr_rcvd.clone();
        if skip {
            return;
        }
        let (key, hq) = match int_of(&q.nr_rcvd).filter(|z| (1..=90).contains(z)) {
            Some(z) => (z.to_string(), false),
            None => (q.nr_rcvd.clone(), true),
        };
        s.new_multi1 = w.mark(0, &key, (q.band, 0), false);
        s.points = if my.itu == key || hq {
            1
        } else if my.continent == cont {
            3
        } else {
            5
        };
    }

    fn primary(&self, _w: &WorldRules, q: &Qso) -> Option<Primary> {
        let key = match int_of(&q.nr_rcvd).filter(|z| (1..=90).contains(z)) {
            Some(z) => z.to_string(),
            None if q.nr_rcvd.is_empty() => return None,
            None => q.nr_rcvd.clone(),
        };
        Some((0, key.clone(), key, (q.band, 0)))
    }

    fn sent_number(&self, w: &WorldRules, _power: Power, station: &Station) -> String {
        w.cty.guess_itu_zone(&station.callsign, self.waedc())
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let mut v = w.zone_rows(90, "ITU Zone");
        for (k, n) in [
            ("AC", "Admin. Council"),
            ("R1", "Reg. Exec. Committee"),
            ("R2", "Reg. Exec. Committee"),
            ("R3", "Reg. Exec. Committee"),
        ] {
            v.push((k.into(), n.into(), w.worked_bands(0, k)));
        }
        let extra = w.extra_rows(0, &v);
        v.extend(extra);
        v
    }
}

#[cfg(test)]
mod tests {

    use crate::world::tests::*;

    #[test]
    fn iaru() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("JA2AAA", Band::B14, Mode::Cw, "45"),
                q("JA2AAA", Band::B14, Mode::Ssb, "45"),
                q("BY1AA", Band::B14, Mode::Cw, "44"),
                q("W1AW", Band::B14, Mode::Cw, "ARRL"),
                q("DL1AA", Band::B21, Mode::Cw, "28"),
            ],
        );
        let (_, s) = run(Iaru, ContestMode::Mix, &mut l);
        assert_eq!(totals(&s), (5, 11, 4, 44));
        assert!(!l.qsos[1].dupe);
    }

    #[test]
    fn iaru_band_edges_and_invalid() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B1_9, Mode::Cw, "8"),
                q("W1AW", Band::B24, Mode::Cw, "8"),
                invalid(q("W1AW", Band::B14, Mode::Cw, "8")),
                q("W1AW", Band::B14, Mode::Cw, "8"),
                q("W1AW", Band::B14, Mode::Cw, "8"),
            ],
        );
        let (c, s) = run(Iaru, ContestMode::Mix, &mut l);
        assert_eq!(totals(&s), (3, 10, 2, 20));
        assert!(l.qsos[4].dupe);
        assert!(!c.valid_number(&q("W1AW", Band::B14, Mode::Cw, "")));
    }

    #[test]
    fn iaru_mode_category() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B14, Mode::Cw, "8"),
                q("W1AW", Band::B14, Mode::Ssb, "8"),
            ],
        );
        let (_, s) = run(Iaru, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (1, 5, 1, 5));
    }
}
