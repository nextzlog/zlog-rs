use zlog_core::{
    Band, Contest, ContestMode, Log, Mode, Power, Qso, QsoScore, ScoreSheet, Scoring, Station,
};

use zlog_cfg::common::{dupe_flags, empty_rows, in_contest, sum};

pub struct NypContest {
    mode: ContestMode,
}

impl NypContest {
    pub fn new(mode: ContestMode) -> Self {
        NypContest { mode }
    }
}

impl Contest for NypContest {
    fn adif_fields(&self, _q: &Qso) -> Vec<(String, String)> {
        vec![("contest_id".into(), "JA_DOMESTIC".into())]
    }

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

    fn sent_number(&self, _band: Band, _power: Power, station: &Station) -> String {
        if self.mode == ContestMode::Phone {
            station.handle_ph.clone()
        } else {
            station.handle_cw.clone()
        }
    }

    fn sent_template(&self, _station: &Station) -> Option<String> {
        Some("$H".into())
    }

    fn exchange_from(&self, _band: Band, _past: &Qso) -> Option<String> {
        None
    }

    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }

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
