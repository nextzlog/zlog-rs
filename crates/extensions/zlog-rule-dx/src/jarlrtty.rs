use zlog_core::{Band, ContestMode, Mode, Power, Qso, QsoScore, Station};

use crate::cty::call_area;
use crate::world::{area_rows, listed_area, My, Primary, Row, Rule, WorldRules, RTTY};

pub(crate) struct JarlRttyRule;

impl Rule for JarlRttyRule {
    fn id(&self) -> &'static str {
        "jarlrtty"
    }

    fn caption_key(&self) -> &'static str {
        "caption.jarlrtty"
    }

    fn name(&self) -> &'static str {
        "JARL World Wide RTTY"
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

    fn adif_contest_id(&self, _mode: ContestMode) -> &'static str {
        "JARL-WW-RTTY"
    }

    fn adif_exchange_field(&self) -> &'static str {
        "age"
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, my: &My, s: &mut QsoScore) {
        if skip || q.mode != Mode::Rtty {
            return;
        }
        if q.callsign.contains("/MM") {
            s.points = 2;
            return;
        }
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        let cont = f.continent();
        if f.is_unknown() {
            s.points = 3;
            return;
        }
        let key = if matches!(code, "JA" | "K" | "VE" | "VK") {
            let a = call_area(&q.callsign, code);
            if listed_area(&a) {
                a
            } else {
                String::new()
            }
        } else if f.country.is_waedc() {
            String::new()
        } else {
            code.to_string()
        };
        s.points = if my.continent == cont { 2 } else { 3 };
        s.new_multi1 = w.mark(0, &key, (q.band, 0), false);
        s.multi1 = key;
    }

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary> {
        w.country_primary(q)
    }

    fn sent_number(&self, _w: &WorldRules, _power: Power, station: &Station) -> String {
        station.age.clone()
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let mut v = Vec::new();
        for row in w.country_rows(0, |_| true) {
            match area_rows(&row.0) {
                Some(areas) => v.extend(areas.map(|(k, n)| {
                    let b = w.worked_bands(0, &k);
                    (k, n, b)
                })),
                None => v.push(row),
            }
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
    fn jarl_rtty() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("JA2AAA", Band::B14, Mode::Rtty, "40"),
                q("JA2BBB", Band::B14, Mode::Rtty, "30"),
                q("W1AW", Band::B14, Mode::Rtty, "50"),
                q("BY1AA", Band::B14, Mode::Rtty, "20"),
                q("DL1AA", Band::B21, Mode::Rtty, "60"),
                q("DL2AA/MM", Band::B7, Mode::Rtty, "70"),
                q("W1AW", Band::B14, Mode::Rtty, "50"),
                q("JA3AAA", Band::B14, Mode::Cw, "40"),
            ],
        );
        let (_, s) = run(JarlRtty, ContestMode::Rtty, &mut l);
        assert_eq!((s.total.points, s.total.multi, s.score), (14, 4, 56));
        assert_eq!(l.qsos[0].multi1, "JA2");
        assert_eq!(l.qsos[2].multi1, "K1");
        assert!(l.qsos[5].multi1.is_empty());
    }

    #[test]
    fn jarl_rtty_unknown_call_scores_3_without_multi() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("ZZ9ZZ", Band::B14, Mode::Rtty, "40"),
                q("ZZ8ZZ", Band::B14, Mode::Rtty, "40"),
                q("ZZ9ZZ", Band::B14, Mode::Rtty, "40"),
            ],
        );
        let (_, s) = run(JarlRtty, ContestMode::Rtty, &mut l);
        assert_eq!((s.total.points, s.total.multi, s.score), (6, 0, 0));
        assert_eq!(l.qsos[0].points, 3);
        assert!(l.qsos[0].multi1.is_empty() && !l.qsos[0].new_multi1);
        assert_eq!(l.qsos[2].points, 0);
    }

    #[test]
    fn jarl_rtty_mm_scores_2_and_marks_nothing() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("DL1AA/MM", Band::B14, Mode::Rtty, "40"),
                q("DL2AA", Band::B14, Mode::Rtty, "40"),
                q("ZZ9ZZ/MM", Band::B14, Mode::Rtty, "40"),
            ],
        );
        let (_, s) = run(JarlRtty, ContestMode::Rtty, &mut l);
        assert_eq!((s.total.points, s.total.multi), (2 + 3 + 2, 1));
        assert!(l.qsos[1].new_multi1);
    }

    #[test]
    fn jarl_rtty_bands_and_vk0() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B1_9, Mode::Rtty, "40"),
                q("W1AW", Band::B10, Mode::Rtty, "40"),
                q("W1AW", Band::B28, Mode::Rtty, "40"),
                q("W1AW", Band::B3_5, Mode::Rtty, "40"),
                q("VK0AA", Band::B14, Mode::Rtty, "40"),
                q("W1AW", Band::B50, Mode::Rtty, "40"),
            ],
        );
        let (_, s) = run(JarlRtty, ContestMode::Rtty, &mut l);
        assert_eq!(totals(&s), (3, 3 + 3 + 3, 2, 18));
        assert!(l.qsos[0].multi1.is_empty() && l.qsos[0].points == 0);
        assert!(l.qsos[4].multi1.is_empty());
    }

    #[test]
    fn jarl_rtty_and_all_asian_send_the_age() {
        let st = station_aged("45");
        for (kind, mode) in [(JarlRtty, ContestMode::Rtty), (AllAsian, ContestMode::Cw)] {
            let c = rules(kind, mode);
            assert_eq!(c.sent_number(Band::B14, Power::P100, &st), "45");
            assert_eq!(
                c.sent_number(Band::B14, Power::P100, &Station::default()),
                ""
            );
        }
    }

    #[test]
    fn jarl_rtty_multi_table_has_call_areas() {
        let mut l = log("JA1ZLO", vec![q("JA2AAA", Band::B14, Mode::Rtty, "40")]);
        let (c, _) = run(JarlRtty, ContestMode::Rtty, &mut l);
        let t = c.multi_table();
        let keys: Vec<&str> = t.iter().map(|r| r.0.as_str()).collect();
        for gone in ["JA", "K", "VE", "VK", "VK0", "VK9"] {
            assert!(!keys.contains(&gone), "{gone}");
        }
        let areas = keys
            .iter()
            .filter(|k| {
                ["JA", "K", "VE", "VK"]
                    .iter()
                    .any(|c| k.strip_prefix(c).is_some_and(|d| d.len() == 1))
            })
            .count();
        assert_eq!(areas, 38);
        assert_eq!(&keys[..2], &["JA0", "JA1"]);
        let ja2 = t.iter().find(|r| r.0 == "JA2").unwrap();
        assert_eq!(ja2.1, "Japan (Area 2)");
        assert_eq!(ja2.2, vec![Band::B14]);
        assert!(keys.contains(&"DL"));
    }

    #[test]
    fn adif_fields_of_jarl_rtty() {
        let x = q("W1AW", Band::B14, Mode::Rtty, "52");
        for mode in [ContestMode::Rtty, ContestMode::Mix] {
            let v = adif(JarlRtty, mode, &x);
            assert_eq!(field(&v, "contest_id").as_deref(), Some("JARL-WW-RTTY"));
            assert_eq!(field(&v, "age").as_deref(), Some("52"));
        }
    }
}
