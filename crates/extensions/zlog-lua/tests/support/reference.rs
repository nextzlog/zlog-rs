use std::collections::BTreeSet;
use std::sync::Arc;

use zlog_cfg::user::{RuleHook, UserDefinedDef, Verdict};
use zlog_core::{Band, Log, Mode, Qso, QsoScore};

fn plain(id: &str, hook: Arc<dyn RuleHook>) -> UserDefinedDef {
    let read = |name: &str| std::fs::read_to_string(super::fixture(name)).unwrap();
    let cfg = read(&format!("{id}.cfg"));
    let dat = read(&format!("{id}.dat"));
    UserDefinedDef::from_cfg(zlog_cfg::script::split(&cfg).0, |_| Some(dat.clone()))
        .with_hook(id, hook)
}

fn allowed(q: &Qso, dupe: bool, modes: (u8, u8)) -> bool {
    let m = q.mode as u8;
    !dupe && (modes.0..=modes.1).contains(&m)
}

fn digits_then<'a>(s: &'a str, n: usize, tails: &[&str]) -> Option<(&'a str, &'a str)> {
    let k = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if k < n {
        return None;
    }
    let (d, rest) = s.split_at(k);
    (tails.is_empty() && rest.is_empty() || tails.contains(&rest)).then_some((d, rest))
}

fn call_ok(call: &str) -> bool {
    call.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .count()
        >= 3
}

const CW_TO_AM: (u8, u8) = (Mode::Cw as u8, Mode::Am as u8);

pub fn sample() -> UserDefinedDef {
    plain("sample", Arc::new(Sample))
}

struct Sample;

impl RuleHook for Sample {
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict> {
        let band_ok = [Band::B7, Band::B21, Band::B50, Band::B144, Band::B430].contains(&q.band);
        if !band_ok || !allowed(q, dupe, CW_TO_AM) {
            return None;
        }
        let (mul, tail) = digits_then(&q.nr_rcvd, 2, &["T", "C"])?;
        Some(Verdict {
            points: if q.mode == Mode::Cw { 3 } else { 1 },
            multi1: mul.into(),
            multi2: if tail == "T" {
                "T".into()
            } else {
                String::new()
            },
        })
    }

    fn total(&self, points: u32, multis: u32, log: &Log, scored: &[QsoScore]) -> u64 {
        let tagged: BTreeSet<&str> = log
            .qsos
            .iter()
            .zip(scored)
            .filter(|(_, s)| s.multi2 == "T")
            .map(|(q, _)| q.callsign.as_str())
            .collect();
        points as u64 * (multis as u64 + tagged.len() as u64)
    }
}

pub fn days() -> UserDefinedDef {
    plain("days", Arc::new(Days))
}

struct Days;

impl RuleHook for Days {
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict> {
        let points = match q.band {
            Band::B50 | Band::B144 | Band::B430 => 1,
            Band::B1200 => 2,
            Band::B2400 => 4,
            Band::B5600 => 8,
            Band::B10G => 16,
            _ => return None,
        };
        if !allowed(q, dupe, (0, 5))
            || !call_ok(&q.callsign)
            || digits_then(&q.nr_rcvd, 3, &[]).is_none()
        {
            return None;
        }
        let call = q.callsign.split('/').next().unwrap_or("");
        Some(Verdict {
            points,
            multi1: call.chars().last().map(String::from).unwrap_or_default(),
            multi2: String::new(),
        })
    }

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
