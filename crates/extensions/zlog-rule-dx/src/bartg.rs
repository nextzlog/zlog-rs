use zlog_core::{Band, BandScore, ContestMode, Mode, Power, Qso, QsoScore, Station};

use crate::cty::call_area;
use crate::world::{area_rows, listed_area, My, Primary, Row, Rule, WorldRules, RTTY};

pub(crate) struct BartgRule;

impl Rule for BartgRule {
    fn id(&self) -> &'static str {
        "bartg"
    }

    fn caption_key(&self) -> &'static str {
        "caption.bartg"
    }

    fn name(&self) -> &'static str {
        "BARTG HF RTTY"
    }

    fn modes(&self) -> &'static [ContestMode] {
        RTTY
    }

    fn bands(&self) -> (Band, Band) {
        (Band::B3_5, Band::B28)
    }

    fn waedc(&self) -> bool {
        false
    }

    fn serial(&self) -> bool {
        true
    }

    fn sent_with_time(&self) -> bool {
        true
    }

    fn adif_contest_id(&self, _mode: ContestMode) -> &'static str {
        "BARTG-RTTY"
    }

    fn adif_extra(&self, w: &WorldRules, q: &Qso) -> Option<(String, String)> {
        let f = w.cty.lookup(&q.callsign, self.waedc());
        Some(("cont".into(), f.continent().to_string()))
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, _my: &My, s: &mut QsoScore) {
        if skip || q.mode != Mode::Rtty {
            return;
        }
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        let cont = f.continent();
        let band = q.band;
        let area = call_area(&q.callsign, code);
        let mut new = Vec::new();
        if !f.country.is_waedc() && w.mark(0, code, (band, 0), false) {
            new.push(code.to_string());
        }
        if !area.is_empty() && listed_area(&area) && w.mark(0, &area, (band, 0), false) {
            new.push(area);
        }
        s.multi1 = new.join(",");
        s.new_multi1 = !new.is_empty();
        if !cont.is_empty() && w.mark(1, cont, (band, 0), true) {
            s.multi2 = cont.to_string();
            s.new_multi2 = true;
        }
        s.points = 1;
    }

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary> {
        w.country_primary(q)
    }

    fn sent_number(&self, w: &WorldRules, _power: Power, station: &Station) -> String {
        w.serial_text(station)
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let mut v = w.country_rows(0, |_| true);
        for code in ["K", "JA", "VE", "VK"] {
            for (k, n) in area_rows(code).into_iter().flatten() {
                let b = w.worked_bands(0, &k);
                v.push((k, n, b));
            }
        }
        let extra = w.extra_rows(0, &v);
        v.extend(extra);
        v
    }

    fn continent_factor(&self) -> bool {
        true
    }

    fn total(&self, total: &BandScore, _qtcs: u32, continents: u64) -> u64 {
        total.points as u64 * total.multi as u64 * continents
    }
}

#[cfg(test)]
mod tests {

    use crate::world::tests::*;

    #[test]
    fn bartg() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B14, Mode::Rtty, "001"),
                q("K1AR", Band::B14, Mode::Rtty, "002"),
                q("W6AA", Band::B14, Mode::Rtty, "003"),
                q("DL1AA", Band::B21, Mode::Rtty, "004"),
                q("DL1AA/MM", Band::B7, Mode::Rtty, "005"),
            ],
        );
        let (_, s) = run(Bartg, ContestMode::Rtty, &mut l);
        assert_eq!(totals(&s), (5, 5, 4, 40));
        assert_eq!(l.qsos[0].multi1, "K,K1");
    }

    #[test]
    fn bartg_multi_table_keeps_countries_and_appends_call_areas() {
        let mut l = log("JA1ZLO", vec![q("W6AA", Band::B14, Mode::Rtty, "001")]);
        let (c, _) = run(Bartg, ContestMode::Rtty, &mut l);
        let t = c.multi_table();
        let keys: Vec<&str> = t.iter().map(|r| r.0.as_str()).collect();
        assert!(keys.contains(&"K") && keys.contains(&"JA"));
        let k0 = keys.iter().position(|k| *k == "K0").unwrap();
        assert_eq!(&keys[k0..k0 + 2], &["K0", "K1"]);
        assert_eq!(keys.last(), Some(&"VK8"));
        let k6 = t.iter().find(|r| r.0 == "K6").unwrap();
        assert_eq!(k6.2, vec![Band::B14]);
    }

    #[test]
    fn bartg_bands_mode_and_dupes() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("K1AR", Band::B14, Mode::Cw, "001"),
                q("W1AW", Band::B10, Mode::Rtty, "002"),
                q("W1AW", Band::B14, Mode::Rtty, "003"),
                q("W1AW", Band::B14, Mode::Rtty, "004"),
                q("W1AW", Band::B21, Mode::Rtty, "005"),
            ],
        );
        let (_, s) = run(Bartg, ContestMode::Rtty, &mut l);
        assert_eq!((s.total.points, s.total.multi, s.score), (2, 4, 8));
        assert!(l.qsos[3].dupe && l.qsos[3].points == 0);
    }
}
