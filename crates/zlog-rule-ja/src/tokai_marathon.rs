//! Tokai Marathon (`tmtest`): a CFG and DAT file run as a user-defined contest, with its own
//! acceptance and points as a [`RuleHook`].

use std::collections::BTreeSet;
use std::sync::Arc;

use zlog_core::{Band, ContestDef, Log, Qso, QsoScore};

use zlog_cfg::common::{allowed, call_ok, digits_then, rule_table};
use zlog_cfg::user::{RuleHook, UserDefinedDef, Verdict};

pub const ID: &str = "tmtest";

/// The contest with its rules.
pub fn def() -> UserDefinedDef {
    UserDefinedDef::from_cfg(include_str!("../data/tmtest.cfg"), |_| None)
        .with_entries(rule_table(include_str!("../data/tmtest.dat")))
        .with_hook(ID, Arc::new(TokaiMarathon))
}

/// The definition, boxed for the registry.
pub fn boxed() -> Box<dyn ContestDef> {
    Box::new(def())
}

/// 50 MHz to byte 15 (10.1 GHz), modes bytes 0-5, callsign `^\w{3,}`,
/// number `^\d{3,}$`; points 1 up to 430 MHz, 2 on 1200, 5 on 2400, 10 on 5600, 20
/// on 10 GHz; the last letter of the callsign (portable part removed) is the
/// multiplier; the score is points x multipliers x days with QSOs.
pub struct TokaiMarathon;

impl RuleHook for TokaiMarathon {
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict> {
        if !allowed(q, dupe, (Band::B50 as u8, Band::B10G as u8), (0, 5))
            || !call_ok(&q.callsign)
            || digits_then(&q.nr_rcvd, 3, &[]).is_none()
        {
            return None;
        }
        let points = match q.band {
            Band::B50 | Band::B144 | Band::B430 => 1,
            Band::B1200 => 2,
            Band::B2400 => 5,
            Band::B5600 => 10,
            Band::B10G => 20,
            _ => 0,
        };
        let call = q.callsign.split('/').next().unwrap_or("");
        Some(Verdict {
            points,
            multi1: call.chars().last().map(String::from).unwrap_or_default(),
            multi2: String::new(),
        })
    }

    /// `score * mults * len(days)`; every QSO in the log counts for its day.
    fn total(&self, points: u32, multis: u32, log: &Log, _scored: &[QsoScore]) -> u64 {
        let days: BTreeSet<(i32, u32, u32)> = log
            .qsos
            .iter()
            .map(|q| {
                let (y, m, d, ..) = q.time.to_ymd_hms();
                (y, m, d)
            })
            .collect();
        points as u64 * multis as u64 * days.len() as u64
    }
}
