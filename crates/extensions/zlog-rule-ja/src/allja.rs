use zlog_core::Band;

use crate::jarl::{NumberRule, Rules, XPO_DAT};

pub(crate) const RULES: Rules = Rules {
    name: "ALL JA",
    period: 24,
    bands: (Band::B1_9, Band::B50),
    lists: &[XPO_DAT],
    number: NumberRule::Prefecture,
    sent: "$V$P",
    coeff: false,
    shf_points: 1,
};

#[cfg(test)]
mod tests {
    use crate::jarl::tests::*;

    #[test]
    fn allja_score() {
        let mut log = Log {
            qsos: vec![
                qso("JA1AAA", Band::B7, Mode::Cw, "10H"),
                qso("JA1BBB", Band::B7, Mode::Cw, "10M"),
                qso("JA1AAA", Band::B7, Mode::Ssb, "10H"),
                qso("JA8CCC", Band::B14, Mode::Cw, "101P"),
                qso("JA2DDD", Band::B10, Mode::Cw, "18H"),
                qso("JA3EEE", Band::B144, Mode::Fm, "25M"),
            ],
            ..Log::default()
        };
        let mut c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        let sheet = rescore(&mut log, &mut c).unwrap();
        let t = sheet.total;
        assert_eq!((t.qsos, t.points, t.multi), (4, 3, 2));
        assert_eq!(sheet.score, 6);
        assert!(log.qsos[2].dupe);
        assert_eq!(log.qsos[3].multi1, "101");
    }

    #[test]
    fn allja_validates_numbers() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        assert!(c.valid_number(&qso("X", Band::B7, Mode::Cw, "10H")));
        assert!(c.valid_number(&qso("X", Band::B7, Mode::Cw, "101P")));
        assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "10")));
        assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "49H")));
    }

    #[test]
    fn allja_mode_category_and_band_edges() {
        let mut log = Log {
            qsos: vec![
                qso("JA1AAA", Band::B1_9, Mode::Cw, "10H"),
                qso("JA1BBB", Band::B50, Mode::Ssb, "11H"),
                qso("JA1CCC", Band::B50, Mode::Cw, "12H"),
                qso("JA1DDD", Band::B144, Mode::Cw, "13H"),
                qso("JA1EEE", Band::B18, Mode::Cw, "14H"),
            ],
            ..Log::default()
        };
        let mut c = JarlContest::AllJa.build_rules(ContestMode::Cw);
        let sheet = rescore(&mut log, &mut c).unwrap();
        assert_eq!(
            (sheet.total.points, sheet.total.multi, sheet.score),
            (2, 2, 4)
        );
        assert_eq!(log.qsos[1].points, 0);
        assert!(!log.qsos[1].new_multi1);
    }

    #[test]
    fn allja_invalid_qso_is_neither_scored_nor_a_dupe() {
        let mut first = qso("JA1AAA", Band::B7, Mode::Cw, "10H");
        first.invalid = true;
        let mut log = Log {
            qsos: vec![first, qso("JA1AAA", Band::B7, Mode::Cw, "10H")],
            ..Log::default()
        };
        let mut c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        let sheet = rescore(&mut log, &mut c).unwrap();
        assert_eq!((sheet.total.points, sheet.total.multi), (1, 1));
        assert!(!log.qsos[1].dupe && log.qsos[1].new_multi1);
        assert_eq!(log.qsos[0].points, 0);
    }
}
