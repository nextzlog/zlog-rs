//! Local contests from ZyLO rule plugins (github.com/nextzlog/zylo,
//! `src/plugins/rules`, commit 1832a6f, 2025-07-07): Asahikawa (aktest), High-school
//! (hstest), Tokai Marathon (tmtest) and YL (yltest).
//!
//! Each is the plugin's own CFG and DAT file (in `data/zylo/`) run as a user-defined
//! contest, with the plugin's `OnAcceptEvent` and `OnPointsEvent` as a [`RuleHook`].
//! Copyright (C) 2020-2021 JA1ZLO, MIT License (see `data/zylo/NOTICE.md`).
//!
//! The plugins use ZyLO's old byte values for modes and bands (`reiwa` enums:
//! CW, SSB, FM, AM, RTTY, OTHER; 1.9 MHz to `G10UP` = 15), which zLog reads as its
//! current `TMode` and `TBand` bytes; the ranges below are those bytes.

use std::collections::BTreeSet;
use std::sync::Arc;

use zlog_core::{Band, ContestDef, Log, Mode, Qso, QsoScore};

use crate::user::{DatEntry, RuleHook, UserDefinedDef, Verdict};

macro_rules! data {
    ($n:literal) => {
        (
            include_str!(concat!("data/zylo/", $n, ".cfg")),
            include_str!(concat!("data/zylo/", $n, ".dat")),
        )
    };
}

/// The built-in ZyLO contests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZyloContest {
    Asahikawa,
    HighSchool,
    TokaiMarathon,
    Yl,
}

impl ZyloContest {
    pub const ALL: [ZyloContest; 4] = [
        ZyloContest::Asahikawa,
        ZyloContest::HighSchool,
        ZyloContest::TokaiMarathon,
        ZyloContest::Yl,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ZyloContest::Asahikawa => "aktest",
            ZyloContest::HighSchool => "hstest",
            ZyloContest::TokaiMarathon => "tmtest",
            ZyloContest::Yl => "yltest",
        }
    }

    /// The contest as a user-defined contest with the plugin's rules.
    pub fn def(self) -> UserDefinedDef {
        let (cfg, dat) = match self {
            ZyloContest::Asahikawa => data!("aktest"),
            ZyloContest::HighSchool => data!("hstest"),
            ZyloContest::TokaiMarathon => data!("tmtest"),
            ZyloContest::Yl => data!("yltest"),
        };
        let hook: Arc<dyn RuleHook> = match self {
            ZyloContest::Asahikawa => Arc::new(Asahikawa),
            ZyloContest::HighSchool => Arc::new(HighSchool),
            ZyloContest::TokaiMarathon => Arc::new(TokaiMarathon),
            ZyloContest::Yl => Arc::new(Yl),
        };
        UserDefinedDef::from_cfg(cfg, |_| None)
            .with_entries(plugin_table(dat))
            .with_hook(self.id(), hook)
    }

    pub fn defs() -> Vec<Box<dyn ContestDef>> {
        Self::ALL
            .into_iter()
            .map(|c| Box::new(c.def()) as Box<dyn ContestDef>)
            .collect()
    }
}

/// A plugin's multiplier table (`CitiesCallBack`, UzLogExtension.pas:393): every line,
/// the title too, split at blanks into code and name (its first word).
pub fn plugin_table(text: &str) -> Vec<DatEntry> {
    text.lines()
        .map(|l| {
            let mut words = l.split_whitespace();
            DatEntry {
                code: words.next().unwrap_or("").to_string(),
                name: words.next().unwrap_or("").to_string(),
            }
        })
        .collect()
}

/// ZyLO's default checks (`OnVerifyEvent`, reiwa/zylo.go:880-899): not a dupe, band
/// and mode allowed.
fn allowed(q: &Qso, dupe: bool, bands: (u8, u8), modes: (u8, u8)) -> bool {
    let (b, m) = (q.band as u8, q.mode as u8);
    !dupe && (bands.0..=bands.1).contains(&b) && (modes.0..=modes.1).contains(&m)
}

/// `^\d{n,}` followed by one of `tails` (or nothing if `tails` is empty) to the end:
/// the digits and the tail.
fn digits_then<'a>(s: &'a str, n: usize, tails: &[&str]) -> Option<(&'a str, &'a str)> {
    let k = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if k < n {
        return None;
    }
    let (d, rest) = s.split_at(k);
    (tails.is_empty() && rest.is_empty() || tails.contains(&rest)).then_some((d, rest))
}

/// `^\w{3,}` on the callsign.
fn call_ok(call: &str) -> bool {
    call.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .count()
        >= 3
}

const CW_TO_AM: (u8, u8) = (Mode::Cw as u8, Mode::Am as u8);

/// aktest.go: 3.5-430 MHz, CW to AM, number `^(\d{4,})(M?)$`; 9 points for a member
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

/// hstest.go: 7, 21, 50, 144, 430 MHz, CW to AM, number `^(\d{2,})(HS|C)$`; CW 3
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
    /// second multiplier is HS (hstest.go:42-52, 81-83).
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

/// tmtest.go: 50 MHz to byte 15 (10.1 GHz), modes bytes 0-5, callsign `^\w{3,}`,
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

    /// `score * mults * len(days)`; every QSO in the log counts for its day
    /// (`onInsertEvent`, tmtest.go:33-35).
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

/// yltest.go: 1.9-1200 MHz, CW to AM, callsign `^\w{3,}`, received and sent numbers
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
        // any valid four-digit number scores as an OM (yltest.go:68-77).
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
