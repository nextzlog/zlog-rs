//! High-school contest (`hstest`): a CFG and DAT file run as a user-defined contest, with its own
//! acceptance and points as a [`RuleHook`].

use std::collections::BTreeSet;
use std::sync::Arc;

use zlog_core::{Band, ContestDef, Log, Mode, Qso, QsoScore};

use zlog_cfg::common::{allowed, digits_then, rule_table, CW_TO_AM};
use zlog_cfg::user::{RuleHook, UserDefinedDef, Verdict};

pub const ID: &str = "hstest";

/// The contest with its rules.
pub fn def() -> UserDefinedDef {
    UserDefinedDef::from_cfg(include_str!("../data/hstest.cfg"), |_| None)
        .with_entries(rule_table(include_str!("../data/hstest.dat")))
        .with_hook(ID, Arc::new(HighSchool))
}

/// The definition, boxed for the registry.
pub fn boxed() -> Box<dyn ContestDef> {
    Box::new(def())
}

/// 7, 21, 50, 144, 430 MHz, CW to AM, number `^(\d{2,})(HS|C)$`; CW 3
/// points, else 1; the number is the multiplier, `HS` the second one; the score adds
/// the number of high-school stations worked (`hsmults`) to the multipliers.
pub struct HighSchool;

impl RuleHook for HighSchool {
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict> {
        let band_ok = [Band::B7, Band::B21, Band::B50, Band::B144, Band::B430].contains(&q.band);
        if !band_ok || !allowed(q, dupe, (0, u8::MAX), CW_TO_AM) {
            return None;
        }
        let (mul, tail) = digits_then(&q.nr_rcvd, 2, &["HS", "C"])?;
        Some(Verdict {
            points: if q.mode == Mode::Cw { 3 } else { 1 },
            multi1: mul.into(),
            multi2: if tail == "HS" {
                "HS".into()
            } else {
                String::new()
            },
        })
    }

    /// `score * (mults + hsmults)`, hsmults being the distinct callsigns of QSOs whose
    /// second multiplier is HS.
    fn total(&self, points: u32, multis: u32, log: &Log, scored: &[QsoScore]) -> u64 {
        let hs: BTreeSet<&str> = log
            .qsos
            .iter()
            .zip(scored)
            .filter(|(_, s)| s.multi2 == "HS")
            .map(|(q, _)| q.callsign.as_str())
            .collect();
        points as u64 * (multis as u64 + hs.len() as u64)
    }
}
