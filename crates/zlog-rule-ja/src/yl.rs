//! YL contest (`yltest`): a CFG and DAT file run as a user-defined contest, with its own
//! acceptance and points as a [`RuleHook`].

use std::sync::Arc;

use zlog_core::{Band, ContestDef, Qso};

use zlog_cfg::common::{allowed, call_ok, digits_then, rule_table, CW_TO_AM};
use zlog_cfg::user::{RuleHook, UserDefinedDef, Verdict};

pub const ID: &str = "yltest";

/// The contest with its rules.
pub fn def() -> UserDefinedDef {
    UserDefinedDef::from_cfg(include_str!("../data/yltest.cfg"), |_| None)
        .with_entries(rule_table(include_str!("../data/yltest.dat")))
        .with_hook(ID, Arc::new(Yl))
}

/// The definition, boxed for the registry.
pub fn boxed() -> Box<dyn ContestDef> {
    Box::new(def())
}

/// 1.9-1200 MHz, CW to AM, callsign `^\w{3,}`, received and sent numbers
/// `^\d{4,}$`. A number below 2000 is an OM, above 5000 a JL, else a YL; points by
/// the sent class: OM gets 5 from JL, 1 from YL, 0 from OM; YL gets 5 from JL or YL,
/// 1 from OM; JL gets 1. The first three characters of the callsign are the
/// multiplier, the received number the second one.
pub struct Yl;

/// `codeToClass`: 0 OM, 1 YL, 2 JL.
fn yl_class(code: i64) -> u8 {
    match code {
        c if c < 2000 => 0,
        c if c > 5000 => 2,
        _ => 1,
    }
}

impl RuleHook for Yl {
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict> {
        if !allowed(q, dupe, (Band::B1_9 as u8, Band::B1200 as u8), CW_TO_AM)
            || !call_ok(&q.callsign)
            || digits_then(&q.nr_rcvd, 4, &[]).is_none()
            || digits_then(&q.nr_sent, 4, &[]).is_none()
        {
            return None;
        }
        let rcvd = yl_class(q.nr_rcvd.parse().unwrap_or(i64::MAX));
        // As in the plugin, the sent *number* (not its class) selects the table, so
        // any valid four-digit number scores as an OM.
        let sent: i64 = q.nr_sent.parse().unwrap_or(0);
        let points = match sent {
            2 => 1,
            1 => [1, 5, 5][rcvd as usize],
            _ => [0, 1, 5][rcvd as usize],
        };
        Some(Verdict {
            points,
            multi1: q.callsign.chars().take(3).collect(),
            multi2: q.nr_rcvd.clone(),
        })
    }
}
