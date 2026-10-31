use zlog_core::{Band, ContestMode, Power, Qso, QsoScore, Station};

use crate::lists::{self, extract_iota};
use crate::world::{My, Primary, Row, Rule, WorldRules, MIX_CW_PH_ALL};

pub(crate) struct IotaRule;

impl Rule for IotaRule {
    fn id(&self) -> &'static str {
        "iota"
    }

    fn caption_key(&self) -> &'static str {
        "caption.iota"
    }

    fn name(&self) -> &'static str {
        "IOTA Contest"
    }

    fn modes(&self) -> &'static [ContestMode] {
        MIX_CW_PH_ALL
    }

    fn bands(&self) -> (Band, Band) {
        (Band::B3_5, Band::B28)
    }

    fn different_mode(&self) -> bool {
        true
    }

    fn serial(&self) -> bool {
        true
    }

    fn period(&self) -> u32 {
        24
    }

    fn start_hour(&self) -> Option<u32> {
        Some(12)
    }

    fn adif_contest_id(&self, _mode: ContestMode) -> &'static str {
        "RSGB-IOTA"
    }

    fn islands(&self) -> Vec<(String, String)> {
        lists::islands()
    }

    fn read_sent(&self, my: &mut My, sent: &str) {
        my.iota = extract_iota(sent);
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, my: &My, s: &mut QsoScore) {
        let r = extract_iota(&q.nr_rcvd);
        s.points = match (my.iota.is_empty(), r.is_empty()) {
            (_, false) if r == my.iota => 5,
            (_, false) => 15,
            (false, true) => 5,
            (true, true) => 2,
        };
        s.multi1 = r.clone();
        if skip || r.is_empty() {
            return;
        }
        s.new_multi1 = w.mark(0, &r, (q.band, q.mode as u8), false);
    }

    fn primary(&self, w: &WorldRules, q: &Qso) -> Option<Primary> {
        let r = extract_iota(&q.nr_rcvd);
        if r.is_empty() {
            return None;
        }
        let name = w
            .islands
            .iter()
            .find(|(k, _)| *k == r)
            .map(|(_, n)| n.clone())?;
        Some((0, r, name, (q.band, q.mode as u8)))
    }

    fn valid_number(&self, _w: &WorldRules, _q: &Qso) -> bool {
        true
    }

    fn sent_number(&self, w: &WorldRules, _power: Power, station: &Station) -> String {
        w.serial_text(station)
    }

    fn exchange_from(&self, _w: &WorldRules, past: &Qso) -> Option<String> {
        (!extract_iota(&past.nr_rcvd).is_empty()).then(|| past.nr_rcvd.clone())
    }

    fn multi_table(&self, w: &WorldRules) -> Vec<Row> {
        let v: Vec<Row> = w
            .islands
            .iter()
            .map(|(k, n)| (k.clone(), n.clone(), w.worked_bands(0, k)))
            .collect();
        let mut v2 = v.clone();
        v2.extend(w.extra_rows(0, &v));
        v2
    }
}

#[cfg(test)]
mod tests {

    use crate::world::tests::*;

    #[test]
    fn iota() {
        let mut first = q("JA2AAA", Band::B14, Mode::Cw, "001AS007");
        first.nr_sent = "001AS007".into();
        let mut l = log(
            "JA1ZLO",
            vec![
                first,
                q("DL1AA", Band::B14, Mode::Cw, "005"),
                q("G4AAA", Band::B14, Mode::Cw, "010EU005"),
                q("G4AAA", Band::B14, Mode::Ssb, "011EU005"),
            ],
        );
        let (c, s) = run(Iota, ContestMode::Mix, &mut l);
        assert_eq!(totals(&s), (4, 40, 3, 120));
        assert!(matches!(
            c.check_multi(&q("X", Band::B21, Mode::Cw, "EU005")),
            MultiCheck::Known { needed: true, .. }
        ));
    }

    #[test]
    fn iota_world_station_points() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("DL1AA", Band::B14, Mode::Cw, "001"),
                q("G4AAA", Band::B14, Mode::Cw, "002EU005"),
                q("G4AAA", Band::B21, Mode::Cw, "003EU005"),
                q("G4AAA", Band::B21, Mode::Cw, "004EU005"),
                q("G4AAA", Band::B10, Mode::Cw, "005EU005"),
            ],
        );
        let (_, s) = run(Iota, ContestMode::Mix, &mut l);
        assert_eq!(totals(&s), (4, 32, 2, 64));
        assert!(l.qsos[3].dupe && l.qsos[3].points == 0);
    }

    #[test]
    fn iota_mode_category_and_unknown_reference() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("G4AAA", Band::B14, Mode::Ssb, "001EU005"),
                q("G4AAA", Band::B14, Mode::Cw, "001XX999"),
            ],
        );
        let (c, s) = run(Iota, ContestMode::Cw, &mut l);
        assert_eq!(s.total.multi, 0);
        assert!(c.valid_number(&q("X", Band::B14, Mode::Cw, "")));
    }
}
