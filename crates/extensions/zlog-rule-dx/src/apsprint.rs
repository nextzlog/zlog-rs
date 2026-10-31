use zlog_core::{Band, ContestMode, Power, Qso, QsoScore, Station};

use crate::cty::wpx_prefix;
use crate::world::{int_of, My, Primary, Row, Rule, WorldRules, CW_PH_ALL};

pub(crate) struct ApSprintRule;

impl Rule for ApSprintRule {
    fn id(&self) -> &'static str {
        "apsprint"
    }

    fn caption_key(&self) -> &'static str {
        "caption.apsprint"
    }

    fn name(&self) -> &'static str {
        "Asia Pacific Sprint"
    }

    fn modes(&self) -> &'static [ContestMode] {
        CW_PH_ALL
    }

    fn bands(&self) -> (Band, Band) {
        (Band::B14, Band::B21)
    }

    fn serial(&self) -> bool {
        true
    }

    fn period(&self) -> u32 {
        0
    }

    fn start_hour(&self) -> Option<u32> {
        None
    }

    fn adif_contest_id(&self, _mode: ContestMode) -> &'static str {
        "AP-SPRINT"
    }

    fn add(&self, w: &mut WorldRules, q: &Qso, skip: bool, _my: &My, s: &mut QsoScore) {
        let px = wpx_prefix(&q.callsign);
        s.multi1 = px.clone();
        if skip {
            return;
        }
        s.new_multi1 = w.mark(0, &px, (q.band, 0), true);
        s.points = 1;
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
    fn ap_sprint() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("JA2AAA", Band::B7, Mode::Cw, "001"),
                q("JA2AAA", Band::B14, Mode::Cw, "002"),
                q("VK2AA", Band::B21, Mode::Cw, "003"),
            ],
        );
        let (_, s) = run(ApSprint, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (2, 2, 2, 4));
    }

    #[test]
    fn ap_sprint_prefix_once_and_dupes() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("VK2AA", Band::B14, Mode::Cw, "001"),
                q("VK2AA", Band::B14, Mode::Cw, "002"),
                q("VK2BB", Band::B21, Mode::Cw, "003"),
                q("VK2AA", Band::B28, Mode::Cw, "004"),
            ],
        );
        let (c, s) = run(ApSprint, ContestMode::Cw, &mut l);
        assert_eq!(totals(&s), (3, 2, 1, 2));
        assert!(!c.valid_number(&q("X", Band::B14, Mode::Cw, "A")));
    }

    #[test]
    fn ap_sprint_mode_category() {
        let mut l = log(
            "JA1ZLO",
            vec![
                q("VK2AA", Band::B14, Mode::Ssb, "001"),
                q("VK3AA", Band::B14, Mode::Cw, "002"),
            ],
        );
        let (_, s) = run(ApSprint, ContestMode::Phone, &mut l);
        assert_eq!((s.total.points, s.total.multi), (1, 1));
    }
}
