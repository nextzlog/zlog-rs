//! zLog-style scoring for rule extensions: QSOs, points and multipliers per band,
//! score = total points x total multipliers (`TBasicScore`, `RenewScoreAndMulti`).
//!
//! A contest implements [`BandRules`] and calls [`renew_by_band`] from its
//! `Contest::renew`. Contests with other scoring simply do not use this crate.
//!
//! The crate also exports the extension [`Base`], whose base rules (every band and
//! mode, one point per QSO) are the smallest example of [`BandRules`].

use std::collections::BTreeMap;

mod base;
pub use base::{Base, BaseDef, BaseRules};

zlog_core::texts!();

use zlog_core::{Band, BandScore, Contest, Log, Mode, Qso, QsoScore, ScoreSheet, Scoring};

/// What a band-scored contest adds to [`Contest`].
///
/// [`renew_by_band`] calls [`BandRules::reset`] once, then [`BandRules::add_multi`]
/// once per in-band, valid QSO in log order.
pub trait BandRules: Contest {
    /// QSO points on a band (`PointTable`).
    fn points(&self, band: Band) -> u8 {
        let _ = band;
        1
    }

    /// Whether a band has a row in the score window and counts in the totals; 10.4G
    /// does not, as in zLog's score window (its QSOs keep their own points).
    fn score_band(&self, band: Band) -> bool {
        let (lo, hi) = self.band_range();
        band >= lo && band <= hi && (self.uses_warc() || !band.is_warc()) && band != Band::B10_4G
    }

    /// Score from total points and multipliers; zLog multiplies them.
    fn total(&self, points: u32, multi: u32) -> u64 {
        points as u64 * multi as u64
    }

    /// Forget all worked multipliers.
    fn reset(&mut self);

    /// The multiplier text of a QSO and whether it is new on its band; remember it
    /// when it counts (`AddNoUpdate`). Dupes are passed in so they can be skipped.
    fn add_multi(&mut self, q: &Qso, dupe: bool) -> (String, bool);
}

/// Score the log by band: dupes, multipliers, points and the score sheet.
///
/// `coeff` is the station coefficient for contests that use one (Field Day).
pub fn renew_by_band(log: &Log, rules: &mut impl BandRules, coeff: Option<f64>) -> Scoring {
    rules.reset();
    let dupes = log.dupe_flags();
    let (lo, hi) = rules.band_range();
    // Every band of the contest has a row, zeros included, as in zLog's score window.
    let mut rows: BTreeMap<Band, BandScore> = Band::range(lo, hi)
        .filter(|b| rules.score_band(*b))
        .map(|b| (b, BandScore::default()))
        .collect();
    let mode = rules.contest_mode();

    let qsos = log
        .qsos
        .iter()
        .zip(dupes)
        .map(|(q, dupe)| {
            let mut s = QsoScore {
                dupe,
                ..QsoScore::default()
            };
            let in_contest =
                q.band >= lo && q.band <= hi && (rules.uses_warc() || !q.band.is_warc());
            if !in_contest || q.invalid {
                return s;
            }
            (s.multi1, s.new_multi1) = rules.add_multi(q, dupe);

            // As in zLog (TBasicScore.AddNoUpdate), a dupe still counts as a QSO but scores nothing.
            let valid = mode.counts(q.mode);
            let row = rows.entry(q.band).or_default();
            if valid {
                row.qsos += 1;
                if q.mode == Mode::Cw {
                    row.cw_qsos += 1;
                }
            }
            if s.new_multi1 {
                row.multi += s.multi1.split(',').count() as u32;
            }
            if valid && !dupe {
                s.points = rules.points(q.band);
                row.points += s.points as u32;
            }
            s
        })
        .collect();

    let bands: BTreeMap<Band, BandScore> = rows
        .into_iter()
        .filter(|(b, _)| rules.score_band(*b))
        .collect();
    let total = bands.values().fold(BandScore::default(), |a, b| BandScore {
        qsos: a.qsos + b.qsos,
        cw_qsos: a.cw_qsos + b.cw_qsos,
        points: a.points + b.points,
        multi: a.multi + b.multi,
    });
    let score = rules.total(total.points, total.multi);
    let with_coeff = coeff.filter(|c| *c > 0.0).map(|c| (c, score as f64 * c));
    Scoring {
        qsos,
        sheet: ScoreSheet {
            bands,
            total,
            score,
            with_coeff,
        },
    }
}

/// Expand a zLog sent-number template: `$V` prefecture, `$Q` QTH, `$P` power code
/// (empty for power classes without a code, as zLog's `NewPowerStr`).
pub fn expand_template(template: &str, prov: &str, qth: &str, power: zlog_core::Power) -> String {
    let code = power.code().map(String::from).unwrap_or_default();
    template
        .replace("$V", prov)
        .replace("$Q", qth)
        .replace("$P", &code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template() {
        use zlog_core::Power;
        assert_eq!(expand_template("$V$P", "10", "100101", Power::P005), "10M");
        assert_eq!(
            expand_template("$Q$P", "10", "100101", Power::P010),
            "100101H"
        );
        assert_eq!(expand_template("$V$P", "10", "", Power::P100), "10");
    }
}
