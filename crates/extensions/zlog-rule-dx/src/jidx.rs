use zlog_core::{Band, ContestMode, Power, Qso, QsoScore, Station};

use crate::lists::PREFECTURES;
use crate::world::{cq_zone, int_of, My, Primary, Row, Rule, WorldRules};

fn jidx_points(band: Band) -> u8 {
    match band {
        Band::B1_9 => 4,
        Band::B3_5 | Band::B28 => 2,
        b if b >= Band::B7 && b <= Band::B21 => 1,
        _ => 0,
    }
}

fn adif_contest_id(mode: ContestMode) -> &'static str {
    match mode {
        ContestMode::Mix => "",
        ContestMode::Cw => "JIDX-CW",
        ContestMode::Phone => "JIDX-SSB",
        _ => "",
    }
}

const PERIOD: u32 = 30;
const START_HOUR: u32 = 7;

pub(crate) struct JidxRule;

impl Rule for JidxRule {
    fn id(&self) -> &'static str {
        "jidx"
    }

    fn caption_key(&self) -> &'static str {
        "caption.jidx"
    }

    fn name(&self) -> &'static str {
        "JIDX Contest (JA)"
    }

    fn period(&self) -> u32 {
        PERIOD
    }

    fn start_hour(&self) -> Option<u32> {
        Some(START_HOUR)
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str {
        adif_contest_id(mode)
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, _my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        let band = q.band;
        s.multi1 = q.nr_rcvd.clone();
        if skip || code == "JA" {
            return;
        }
        if let Some(z) = int_of(&q.nr_rcvd).filter(|z| (1..=40).contains(z)) {
            s.new_multi1 = w.mark(0, &z.to_string(), (band, 0), false);
        }
        s.points = jidx_points(band);
        if code.is_empty() || f.country.is_waedc() || code.starts_with("JD") {
            return;
        }
        s.multi2 = code.to_string();
        s.new_multi2 = w.mark(1, code, (band, 0), false);
    }

    fn primary(&self, _w: &WorldRules, q: &Qso) -> Option<Primary> {
        let z = cq_zone(&q.nr_rcvd)?;
        Some((0, z.to_string(), format!("Zone {z}"), (q.band, 0)))
    }

    fn valid_number(&self, _w: &WorldRules, q: &Qso) -> bool {
        cq_zone(q.nr_rcvd.trim()).is_some()
    }

    fn sent_number(&self, _w: &WorldRules, _power: Power, station: &Station) -> String {
        station.prov.clone()
    }

    fn exchange_from(&self, w: &WorldRules, past: &Qso) -> Option<String> {
        w.zone_exchange(past)
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let mut v = w.zone_rows(40, "Zone");
        v.extend(w.country_rows(1, |_| true));
        v
    }
}

pub(crate) struct JidxDxRule;

impl Rule for JidxDxRule {
    fn id(&self) -> &'static str {
        "jidx-dx"
    }

    fn caption_key(&self) -> &'static str {
        "caption.jidx_dx"
    }

    fn name(&self) -> &'static str {
        "JIDX Contest (DX)"
    }

    fn period(&self) -> u32 {
        PERIOD
    }

    fn start_hour(&self) -> Option<u32> {
        Some(START_HOUR)
    }

    fn adif_contest_id(&self, mode: ContestMode) -> &'static str {
        adif_contest_id(mode)
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, _my: &My, s: &mut QsoScore) {
        let cty = w.cty.clone();
        let f = cty.lookup(&q.callsign, self.waedc());
        let code = f.country.code.as_str();
        s.multi1 = q.nr_rcvd.clone();
        let japan = code == "JA" || code.starts_with("JD");
        if skip || (!f.is_unknown() && !japan) {
            return;
        }
        if let Some(p) = int_of(&q.nr_rcvd).filter(|p| (1..=50).contains(p)) {
            s.new_multi1 = w.mark(0, &p.to_string(), (q.band, 0), false);
        }
        s.points = jidx_points(q.band);
    }

    fn primary(&self, _w: &WorldRules, q: &Qso) -> Option<Primary> {
        let p = int_of(&q.nr_rcvd).filter(|p| (1..=50).contains(p))?;
        Some((
            0,
            p.to_string(),
            PREFECTURES[p as usize - 1].to_string(),
            (q.band, 0),
        ))
    }

    fn valid_number(&self, _w: &WorldRules, q: &Qso) -> bool {
        !q.band.is_warc() && int_of(q.nr_rcvd.trim()).is_some_and(|n| (1..=50).contains(&n))
    }

    fn sent_number(&self, _w: &WorldRules, _power: Power, station: &Station) -> String {
        station.prov.clone()
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        (1..=50)
            .map(|p| {
                let k = p.to_string();
                let b = w.worked_bands(0, &k);
                (k, PREFECTURES[p - 1].to_string(), b)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {

    use crate::world::tests::*;

    #[test]
    fn jidx() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B1_9, Mode::Cw, "5"),
                q("K6XX", Band::B1_9, Mode::Cw, "3"),
                q("DL1AA", Band::B7, Mode::Cw, "14"),
                q("JA2AAA", Band::B7, Mode::Cw, "25"),
            ],
        );
        let (_, s) = run(Jidx, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (4, 9, 5, 45));
        let mut l = log(
            "W1AW",
            vec![
                q("JA1ZLO", Band::B28, Mode::Cw, "10"),
                q("JA1BBB", Band::B28, Mode::Cw, "10"),
                q("JD1AA", Band::B14, Mode::Cw, "48"),
            ],
        );
        let (c, s) = run(JidxDx, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (3, 5, 2, 10));
        assert!(!c.valid_number(&q("JA1ZLO", Band::B14, Mode::Cw, "51")));
        assert_eq!(c.multi_table()[47].1, "Ogasawara");
    }

    #[test]
    fn jidx_band_edges_and_dupes() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("W1AW", Band::B28, Mode::Cw, "5"),
                q("W1AW", Band::B28, Mode::Cw, "5"),
                q("W1AW", Band::B10, Mode::Cw, "5"),
                q("W1AW", Band::B50, Mode::Cw, "5"),
                q("W1AW", Band::B14, Mode::Cw, "41"),
            ],
        );
        let (_, s) = run(Jidx, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (3, 3, 3, 9));
    }

    #[test]
    fn jidx_dx_band_edges_and_non_japan() {
        let mut l = log(
            "W1AW",
            vec![
                q("JA1ZLO", Band::B1_9, Mode::Cw, "10"),
                q("JA1ZLO", Band::B1_9, Mode::Cw, "10"),
                q("DL1AA", Band::B14, Mode::Cw, "10"),
                q("JA1ZLO", Band::B10, Mode::Cw, "10"),
                q("JA1ZLO", Band::B21, Mode::Cw, "51"),
            ],
        );
        let (_, s) = run(JidxDx, ContestMode::Cw, &mut l);
        assert_eq!((s.total.points, s.total.multi, s.score), (5, 1, 5));
    }

    #[test]
    fn jidx_period_is_30_hours() {
        for kind in [Jidx, JidxDx] {
            let c = rules(kind, ContestMode::Cw);
            assert_eq!(c.period_hours(), 30);
            assert_eq!(c.start_hour(), Some(7));
        }
    }
}
