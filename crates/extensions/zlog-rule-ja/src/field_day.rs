use zlog_core::Band;

use crate::jarl::{NumberRule, Rules, ACAG_DAT, XPO_DAT};

pub(crate) const RULES: Rules = Rules {
    name: "Field Day",
    period: 18,
    bands: (Band::B1_9, Band::HIGHEST),
    lists: &[XPO_DAT, ACAG_DAT],
    number: NumberRule::ByBand,
    sent: "$Q$P",
    coeff: true,
    shf_points: 1,
};

#[cfg(test)]
mod tests {
    use crate::jarl::tests::*;

    #[test]
    fn field_day_uses_city_from_2400() {
        let c = JarlContest::FieldDay.build_rules(ContestMode::Mix);
        assert!(c.valid_number(&qso("X", Band::B430, Mode::Fm, "10M")));
        assert!(!c.valid_number(&qso("X", Band::B2400, Mode::Fm, "10M")));
        assert!(c.valid_number(&qso("X", Band::B2400, Mode::Fm, "100101M")));
        assert!(JarlContest::FieldDay.uses_coeff());
    }

    #[test]
    fn field_day_check_multi_wants_the_right_length() {
        let c = JarlContest::FieldDay.build_rules(ContestMode::Mix);
        let at = |band, nr: &str| c.check_multi(&qso("X", band, Mode::Cw, nr));
        assert_eq!(at(Band::B430, "100101M"), MultiCheck::Invalid);
        assert_eq!(at(Band::B2400, "10M"), MultiCheck::Invalid);
        assert!(matches!(at(Band::B430, "10M"), MultiCheck::Known { .. }));
        assert!(matches!(
            at(Band::B2400, "100101"),
            MultiCheck::Known { .. }
        ));
    }

    #[test]
    fn field_day_score_with_coefficient() {
        let mut log = Log {
            qsos: vec![
                qso("JA1AAA", Band::B7, Mode::Cw, "10H"),
                qso("JA1BBB", Band::B7, Mode::Cw, "11H"),
                qso("JA1CCC", Band::B10, Mode::Cw, "12H"),
                qso("JA1DDD", Band::B10G, Mode::Fm, "100101M"),
                qso("JA1EEE", Band::B10_4G, Mode::Fm, "100102M"),
            ],
            ..Log::default()
        };
        log.set_score_coeff(2.0);
        let mut c = JarlContest::FieldDay.build_rules(ContestMode::Mix);
        let sheet = rescore(&mut log, &mut c).unwrap();
        let t = sheet.total;
        assert_eq!((t.points, t.multi, sheet.score), (3, 3, 9));
        assert!(sheet.with_coeff.is_some());
    }
}
