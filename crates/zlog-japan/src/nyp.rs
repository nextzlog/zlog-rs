//! NEW YEAR PARTY (`TNYP`, UzLogContest.pas:2018): no multipliers (`TBasicMulti`)
//! and a QSO count by band and mode (`TPediScore`, UPediScore.pas).

use zlog_core::{
    Band, Contest, ContestMode, Log, Mode, Power, Qso, QsoScore, ScoreSheet, Scoring, Station,
};

use crate::common::{dupe_flags, empty_rows, in_contest, sum};

pub struct NypContest {
    mode: ContestMode,
}

impl NypContest {
    pub fn new(mode: ContestMode) -> Self {
        NypContest { mode }
    }
}

impl Contest for NypContest {
    fn name(&self) -> &str {
        "NEW YEAR PARTY"
    }

    fn band_range(&self) -> (Band, Band) {
        (Band::B1_9, Band::HIGHEST)
    }

    fn uses_warc(&self) -> bool {
        true
    }

    fn contest_mode(&self) -> ContestMode {
        self.mode
    }

    /// `$H` repeats the number already typed; there is nothing to fill in.
    fn sent_number(&self, _band: Band, _power: Power, _station: &Station) -> String {
        String::new()
    }

    fn exchange_from(&self, _band: Band, _past: &Qso) -> Option<String> {
        None
    }

    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }

    /// Every QSO, dupes and any mode included, gets 1 point and is counted
    /// (`TPediScore.AddNoUpdate`). The window shows only counts; zLog's score of this
    /// form is 0, since it never adds to `Points[]`.
    fn score(&mut self, log: &Log) -> Scoring {
        let dupes = dupe_flags(log, true, true);
        let mut rows = empty_rows(Band::range(Band::B1_9, Band::HIGHEST));
        let qsos = log
            .qsos
            .iter()
            .zip(dupes)
            .map(|(q, dupe)| {
                let mut s = QsoScore {
                    dupe,
                    ..QsoScore::default()
                };
                if in_contest(q.band, self.band_range(), true) && !q.invalid {
                    s.points = 1;
                    let row = rows.entry(q.band).or_default();
                    row.qsos += 1;
                    if q.mode == Mode::Cw {
                        row.cw_qsos += 1;
                    }
                }
                s
            })
            .collect();
        let total = sum(rows.values().copied());
        Scoring {
            qsos,
            sheet: ScoreSheet {
                bands: rows,
                total,
                score: 0,
                with_coeff: None,
            },
        }
    }
}
