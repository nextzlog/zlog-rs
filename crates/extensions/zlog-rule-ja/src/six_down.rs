use zlog_core::Band;

use crate::jarl::{NumberRule, Rules, ACAG_DAT, XPO_DAT};

pub(crate) const RULES: Rules = Rules {
    name: "6m AND DOWN",
    period: 18,
    bands: (Band::B50, Band::HIGHEST),
    lists: &[XPO_DAT, ACAG_DAT],
    number: NumberRule::ByBand,
    sent: "$Q$P",
    coeff: false,
    shf_points: 2,
};

#[cfg(test)]
mod tests {
    use crate::jarl::tests::*;

    #[test]
    fn six_down_points() {
        let c = JarlContest::SixDown.build_rules(ContestMode::Mix);
        assert_eq!(c.points(Band::B50), 1);
        assert_eq!(c.points(Band::B5600), 2);
        assert_eq!(c.points(Band::B10_4G), 0);
    }

    #[test]
    fn six_down_score() {
        let mut log = Log {
            qsos: vec![
                qso("JA1AAA", Band::B50, Mode::Cw, "10H"),
                qso("JA1AAA", Band::B144, Mode::Fm, "10M"),
                qso("JA1AAA", Band::B144, Mode::Ssb, "10M"),
                qso("JA1BBB", Band::B2400, Mode::Fm, "100101M"),
                qso("JA1CCC", Band::B10_4G, Mode::Fm, "100102M"),
                qso("JA1DDD", Band::B28, Mode::Cw, "10H"),
            ],
            ..Log::default()
        };
        let mut c = JarlContest::SixDown.build_rules(ContestMode::Mix);
        let sheet = rescore(&mut log, &mut c).unwrap();
        let t = sheet.total;
        assert_eq!((t.points, t.multi), (1 + 1 + 2, 3));
        assert!(log.qsos[2].dupe);
        assert_eq!(log.qsos[4].points, 0);
        assert_eq!(log.qsos[5].points, 0);
    }

    #[test]
    fn six_down_validates_numbers_by_band() {
        let c = JarlContest::SixDown.build_rules(ContestMode::Mix);
        assert!(c.valid_number(&qso("X", Band::B1200, Mode::Fm, "101P")));
        assert!(!c.valid_number(&qso("X", Band::B1200, Mode::Fm, "100101M")));
        assert!(c.valid_number(&qso("X", Band::B5600, Mode::Fm, "100101M")));
        assert!(!c.valid_number(&qso("X", Band::B5600, Mode::Fm, "100101")));
        assert!(!c.valid_number(&qso("X", Band::B50, Mode::Fm, "")));
        assert!(!c.valid_number(&qso("X", Band::B50, Mode::Fm, "99M")));
    }
}
