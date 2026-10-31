use std::collections::{BTreeMap, BTreeSet};

use zlog_core::{
    Band, Contest, ContestMode, Log, Power, Qso, QsoScore, ScoreSheet, Scoring, Station,
};

use zlog_cfg::common::{count_qso, dupe_flags, empty_rows, in_contest, serial_str, sum, Serials};

pub const JA0_BANDS: [Band; 5] = [Band::B1_9, Band::B3_5, Band::B7, Band::B21, Band::B28];

pub struct Ja0Contest {
    ja0: bool,
    mode: ContestMode,
    worked: BTreeMap<Band, BTreeSet<String>>,
    serials: Serials,
}

impl Ja0Contest {
    pub fn new(ja0: bool, mode: ContestMode) -> Self {
        Ja0Contest {
            ja0,
            mode,
            worked: BTreeMap::new(),
            serials: Serials::new(1),
        }
    }

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
    fn adif_fields(&self, _q: &Qso) -> Vec<(String, String)> {
        vec![("contest_id".into(), "JA_DOMESTIC".into())]
    }

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

    fn sent_number(&self, _band: Band, _power: Power, station: &Station) -> String {
        serial_str(self.serials.next(None, station.tx_nr))
    }

    fn sent_template(&self, _station: &Station) -> Option<String> {
        Some("$S".into())
    }

    fn next_serial_on(&self, band: Band) -> Option<i32> {
        self.next_serial_of_tx(band, 0)
    }

    fn next_serial_of_tx(&self, _band: Band, tx_nr: u8) -> Option<i32> {
        Some(self.serials.next(None, tx_nr))
    }

    fn exchange_from(&self, _band: Band, _past: &Qso) -> Option<String> {
        None
    }

    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }

    fn score(&mut self, log: &Log) -> Scoring {
        self.worked.clear();
        self.serials = Serials::scan(log, 1);
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
