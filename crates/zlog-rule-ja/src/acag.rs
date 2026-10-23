//! ACAG (`TACAGContest`, UzLogContest.pas): cities, counties and wards (`ACAG.DAT`)
//! on every band.

use zlog_core::Band;

use crate::jarl::{NumberRule, Rules, ACAG_DAT};

pub(crate) const RULES: Rules = Rules {
    name: "ACAG",
    period: 24,
    bands: (Band::B1_9, Band::HIGHEST),
    lists: &[ACAG_DAT],
    number: NumberRule::City,
    sent: "$Q$P",
    coeff: false,
    shf_points: 1,
};

#[cfg(test)]
mod tests {
    use crate::jarl::tests::*;

    #[test]
    fn acag_score() {
        let mut log = Log {
            qsos: vec![
                qso("JA1AAA", Band::B7, Mode::Cw, "100101H"),
                qso("JA1BBB", Band::B7, Mode::Cw, "100101M"), // city not new
                qso("JA1AAA", Band::B7, Mode::Cw, "100101H"), // dupe
                qso("JA1AAA", Band::B14, Mode::Cw, "100101H"), // new on 14
                qso("JA1CCC", Band::B24, Mode::Cw, "100102H"), // WARC: out
                qso("JA1DDD", Band::B5600, Mode::Fm, "100102M"),
            ],
            ..Log::default()
        };
        let mut c = JarlContest::Acag.build_rules(ContestMode::Mix);
        let sheet = rescore(&mut log, &mut c).unwrap();
        let t = sheet.total;
        assert_eq!((t.points, t.multi, sheet.score), (4, 3, 12));
        assert!(log.qsos[2].dupe);
    }

    #[test]
    fn acag_validates_city_numbers() {
        let c = JarlContest::Acag.build_rules(ContestMode::Mix);
        assert!(c.valid_number(&qso("X", Band::B7, Mode::Cw, "100101H")));
        assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "10H")));
        assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "100101")));
        assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "999999H")));
        assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "")));
    }
}
