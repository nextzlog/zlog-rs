//! ALL JA0 (`TJA0Contest`, `TJA0ContestZero`, UzLogContest.pas:1952-2014) with its
//! multipliers (`TJA0Multi`, UJA0Multi.pas) and points (`TJA0Score`, UJA0Score.pas).

use std::collections::{BTreeMap, BTreeSet};

use zlog_core::{
    Band, Contest, ContestMode, Log, Power, Qso, QsoScore, ScoreSheet, Scoring, Station,
};

use crate::common::{count_qso, dupe_flags, empty_rows, in_contest, serial_str, sum};

/// Bands of the score window (`JA0Band`, UJA0Score.pas:11); 14 MHz is not one.
pub const JA0_BANDS: [Band; 5] = [Band::B1_9, Band::B3_5, Band::B7, Band::B21, Band::B28];

/// ALL JA0 in progress; `ja0` is the section for JA0 stations (`TJA0ContestZero`).
pub struct Ja0Contest {
    ja0: bool,
    mode: ContestMode,
    /// Worked prefixes per band (`PXListX`).
    worked: BTreeMap<Band, BTreeSet<String>>,
}

impl Ja0Contest {
    pub fn new(ja0: bool, mode: ContestMode) -> Self {
        Ja0Contest {
            ja0,
            mode,
            worked: BTreeMap::new(),
        }
    }

    /// Multiplier of a callsign (`TJA0Multi.GetPrefix`, UJA0Multi.pas:117): JA0 stations
    /// count the first three characters; others count JA0 stations by the second
    /// character and the last letter of the suffix, `A*C` for JA0ABC.
    pub fn prefix(&self, call: &str) -> String {
        let c: Vec<char> = call.chars().collect();
        if self.ja0 {
            return c.iter().take(3).collect();
        }
        if !c.contains(&'0') {
            return String::new();
        }
        let alpha = |ch: char| ch.is_ascii_uppercase();
        if c.len() >= 6 {
            let last = if alpha(c[5]) { c[5] } else { c[4] };
            format!("{}*{}", c[1], last)
        } else if c.len() == 5 && alpha(c[4]) {
            format!("{}*{}", c[1], c[4])
        } else {
            String::new()
        }
    }
}

impl Contest for Ja0Contest {
    fn name(&self) -> &str {
        if self.ja0 {
            "ALL JA0 (JA0)"
        } else {
            "ALL JA0 (Others)"
        }
    }

    fn band_range(&self) -> (Band, Band) {
        (Band::B1_9, Band::B28)
    }

    fn contest_mode(&self) -> ContestMode {
        self.mode
    }

    /// `$S`, the serial number, which this call does not know: empty. See
    /// [`Ja0Contest::sent_number_with_serial`].
    fn sent_number(&self, _band: Band, _power: Power, _station: &Station) -> String {
        String::new()
    }

    fn exchange_from(&self, _band: Band, _past: &Qso) -> Option<String> {
        None
    }

    /// zLog's `TJA0Multi.ValidMulti` (a positive number) is declared `reintroduce`,
    /// so the main window calls `TBasicMulti.ValidMulti`, which accepts any number.
    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }

    fn score(&mut self, log: &Log) -> Scoring {
        self.worked.clear();
        let dupes = dupe_flags(log, false, true);
        let mut rows = empty_rows(Band::range(Band::B1_9, Band::B28));
        let qsos = log
            .qsos
            .iter()
            .zip(dupes)
            .map(|(q, dupe)| {
                let mut s = QsoScore {
                    dupe,
                    ..QsoScore::default()
                };
                if !in_contest(q.band, self.band_range(), false) || q.invalid {
                    return s;
                }
                // TJA0Multi.AddNoUpdate: dupes keep no multiplier text.
                if !dupe {
                    s.multi1 = self.prefix(&q.callsign);
                    if self.mode.accepts(q.mode) && !s.multi1.is_empty() {
                        s.new_multi1 = self
                            .worked
                            .entry(q.band)
                            .or_default()
                            .insert(s.multi1.clone());
                    }
                }
                let row = rows.entry(q.band).or_default();
                let valid = count_qso(row, self.mode, q);
                if s.new_multi1 {
                    row.multi += 1;
                }
                // TJA0Score.AddNoUpdate: 3 points for a callsign with 0, else 1.
                if JA0_BANDS.contains(&q.band) && !dupe && valid {
                    s.points = if q.callsign.contains('0') { 3 } else { 1 };
                    row.points += s.points as u32;
                }
                s
            })
            .collect();
        let bands: BTreeMap<Band, _> = rows
            .into_iter()
            .filter(|(b, _)| JA0_BANDS.contains(b))
            .collect();
        let total = sum(bands.values().copied());
        Scoring {
            qsos,
            sheet: ScoreSheet {
                bands,
                total,
                score: total.points as u64 * total.multi as u64,
                with_coeff: None,
            },
        }
    }

    fn multi_table(&self) -> Vec<(String, String, Vec<Band>)> {
        let mut all: BTreeMap<&str, Vec<Band>> = BTreeMap::new();
        for (b, set) in &self.worked {
            for p in set {
                all.entry(p).or_default().push(*b);
            }
        }
        all.into_iter()
            .map(|(p, b)| (p.to_string(), String::new(), b))
            .collect()
    }
}

impl Ja0Contest {
    /// The sent number with the QSO's serial number (`$S`, three digits).
    pub fn sent_number_with_serial(&self, serial: i32) -> String {
        serial_str(serial)
    }
}
