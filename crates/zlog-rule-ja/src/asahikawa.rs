//! Asahikawa contest (`aktest`): a CFG and DAT file run as a user-defined contest, with its own
//! acceptance and points as a [`RuleHook`].

use std::sync::Arc;

use zlog_core::{Band, ContestDef, Qso};

use zlog_cfg::common::{allowed, digits_then, rule_table, CW_TO_AM};
use zlog_cfg::user::{RuleHook, UserDefinedDef, Verdict};

pub const ID: &str = "aktest";

/// The contest with its rules.
pub fn def() -> UserDefinedDef {
    UserDefinedDef::from_cfg(include_str!("../data/aktest.cfg"), |_| None)
        .with_entries(rule_table(include_str!("../data/aktest.dat")))
        .with_hook(ID, Arc::new(Asahikawa))
}

/// The definition, boxed for the registry.
pub fn boxed() -> Box<dyn ContestDef> {
    Box::new(def())
}

/// 3.5-430 MHz, CW to AM, number `^(\d{4,})(M?)$`; 9 points for a member
/// (`M`) or Asahikawa (0102), 6 in Hokkaido (01...), else 3; the city is the multiplier.
pub struct Asahikawa;

impl RuleHook for Asahikawa {
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict> {
        if !allowed(q, dupe, (Band::B3_5 as u8, Band::B430 as u8), CW_TO_AM) {
            return None;
        }
        let (mul, mem) = digits_then(&q.nr_rcvd, 4, &["", "M"])?;
        let points = match () {
            _ if mem == "M" || mul == "0102" => 9,
            _ if mul.starts_with("01") => 6,
            _ => 3,
        };
        Some(Verdict {
            points,
            multi1: mul.into(),
            multi2: String::new(),
        })
    }
}
