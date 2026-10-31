use std::collections::BTreeMap;

use crate::types::Power;

use crate::log::Log;
use crate::qso::Qso;
use crate::types::{Band, ContestMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiCheck {
    Invalid,
    Known {
        code: String,
        name: String,
        needed: bool,
        worked_on: Vec<Band>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Station {
    pub callsign: String,
    pub operator: String,
    pub prov: String,
    pub city: String,
    pub power: BTreeMap<Band, Power>,
    pub age: String,
    pub cq_zone: String,
    pub iaru_zone: String,
    pub itu_region: String,
    pub iota: String,
    pub tx_nr: u8,
    pub handle_cw: String,
    pub handle_ph: String,
}

impl Station {
    pub fn default_power(band: Band) -> Power {
        match band {
            b if b <= Band::B50 => Power::P010,
            Band::B144 | Band::B430 => Power::P005,
            Band::B1200 => Power::P002,
            _ => Power::P001,
        }
    }

    pub fn power_for(&self, band: Band) -> Power {
        self.power
            .get(&band)
            .copied()
            .unwrap_or_else(|| Self::default_power(band))
    }

    pub fn set_power_everywhere(&mut self, power: Power) {
        self.power = Band::ALL.iter().map(|b| (*b, power)).collect();
    }
}

pub trait ContestDef: Send + Sync {
    fn id(&self) -> &str;

    fn caption(&self, language: &str) -> String;

    fn group(&self, language: &str) -> String {
        let _ = language;
        "Other".into()
    }

    fn modes(&self) -> &[ContestMode] {
        &[ContestMode::Mix, ContestMode::Cw, ContestMode::Phone]
    }

    fn uses_coeff(&self) -> bool {
        false
    }

    fn uses_utc(&self) -> bool {
        false
    }

    fn build(&self, mode: ContestMode) -> Box<dyn Contest>;
}

pub trait Contest {
    fn name(&self) -> &str;

    fn band_range(&self) -> (Band, Band);

    fn uses_warc(&self) -> bool {
        false
    }

    fn contest_mode(&self) -> ContestMode;

    fn period_hours(&self) -> u32 {
        0
    }

    fn uses_period(&self) -> bool {
        self.period_hours() > 0
    }

    fn start_hour(&self) -> Option<u32> {
        None
    }

    fn adif_fields(&self, q: &Qso) -> Vec<(String, String)> {
        let _ = q;
        Vec::new()
    }

    fn sent_number(&self, band: Band, power: Power, station: &Station) -> String;

    fn sent_template(&self, station: &Station) -> Option<String> {
        let _ = station;
        None
    }

    fn sent_with_time(&self) -> bool {
        false
    }

    fn next_serial_on(&self, band: Band) -> Option<i32> {
        let _ = band;
        None
    }

    fn next_serial_of_tx(&self, band: Band, tx_nr: u8) -> Option<i32> {
        let _ = tx_nr;
        self.next_serial_on(band)
    }

    fn exchange_from(&self, band: Band, past: &Qso) -> Option<String> {
        let _ = band;
        Some(past.nr_rcvd.clone())
    }

    fn valid_number(&self, q: &Qso) -> bool;

    fn score(&mut self, log: &Log) -> Scoring;

    fn score_unknown(&self) -> bool {
        false
    }

    fn check_multi(&self, q: &Qso) -> MultiCheck {
        let _ = q;
        MultiCheck::Invalid
    }

    fn exchange_label(&self, _language: &str) -> Option<String> {
        None
    }

    fn extract_multi(&self, q: &Qso) -> String {
        q.nr_rcvd.clone()
    }

    fn multi_table(&self) -> Vec<(String, String, Vec<Band>)> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QsoScore {
    pub dupe: bool,
    pub points: u8,
    pub multi1: String,
    pub new_multi1: bool,
    pub multi2: String,
    pub new_multi2: bool,
    pub multi3: String,
    pub new_multi3: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scoring {
    pub qsos: Vec<QsoScore>,
    pub sheet: ScoreSheet,
}

pub fn rescore(log: &mut Log, contest: &mut dyn Contest) -> Result<ScoreSheet, String> {
    let scoring = contest.score(log);
    if scoring.qsos.len() != log.qsos.len() {
        return Err(format!(
            "{} scored {} QSOs of {}",
            contest.name(),
            scoring.qsos.len(),
            log.qsos.len()
        ));
    }
    for (q, s) in log.qsos.iter_mut().zip(scoring.qsos) {
        q.dupe = s.dupe;
        q.points = s.points;
        q.multi1 = s.multi1;
        q.new_multi1 = s.new_multi1;
        q.multi2 = s.multi2;
        q.new_multi2 = s.new_multi2;
        q.multi3 = s.multi3;
        q.new_multi3 = s.new_multi3;
    }
    Ok(scoring.sheet)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BandScore {
    pub qsos: u32,
    pub cw_qsos: u32,
    pub points: u32,
    pub multi: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScoreSheet {
    pub bands: BTreeMap<Band, BandScore>,
    pub total: BandScore,
    pub score: u64,
    pub with_coeff: Option<(f64, f64)>,
}
