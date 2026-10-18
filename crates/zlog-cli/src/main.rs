//! `zlog dump FILE` lists the contacts of a ZLO/ZLOX file.
//! `zlog score FILE CONTEST [MODE]` prints the score sheet (CONTEST: an id such as `allja`;
//! MODE: mix, cw, ph).

use std::process::ExitCode;

use zlog_core::{ContestMode, Log, Registry};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["dump", file] => dump(file),
        ["score", file, contest] => score(file, contest, "mix"),
        ["score", file, contest, mode] => score(file, contest, mode),
        _ => Err("usage: zlog dump FILE | zlog score FILE CONTEST [mix|cw|ph]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zlog: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dump(file: &str) -> Result<(), String> {
    let (log, format) = Log::load(file).map_err(|e| e.to_string())?;
    println!(
        "# {format:?}, {} QSOs, start {}",
        log.qsos.len(),
        log.start_time()
    );
    for q in &log.qsos {
        println!(
            "{}  {:<12} {:>3} {:<8} {:>3} {:<8} {:>5} {:<4} {:<8} {}",
            q.time,
            q.callsign,
            q.rst_sent,
            q.nr_sent,
            q.rst_rcvd,
            q.nr_rcvd,
            q.band,
            q.mode,
            q.operator,
            q.memo
        );
    }
    Ok(())
}

fn score(file: &str, contest: &str, mode: &str) -> Result<(), String> {
    let (mut log, _) = Log::load(file).map_err(|e| e.to_string())?;
    let registry = registry();
    let defs = registry.contests();
    let def = defs
        .iter()
        .find(|d| d.id() == contest)
        .ok_or_else(|| format!("unknown contest {contest}"))?;
    let mode = match mode {
        "mix" => ContestMode::Mix,
        "cw" => ContestMode::Cw,
        "ph" => ContestMode::Phone,
        _ => return Err(format!("unknown mode {mode}")),
    };
    let mut rules = def.build(mode);
    let sheet = zlog_core::rescore(&mut log, rules.as_mut())?;

    println!("{:>6} {:>6} {:>6} {:>6}", "MHz", "QSOs", "Points", "Multi");
    for (band, row) in &sheet.bands {
        println!(
            "{:>6} {:>6} {:>6} {:>6}",
            band.label(),
            row.qsos,
            row.points,
            row.multi
        );
    }
    let t = sheet.total;
    println!(
        "{:>6} {:>6} {:>6} {:>6}",
        "Total", t.qsos, t.points, t.multi
    );
    println!("Score  {}", sheet.score);
    if let Some((coeff, s)) = sheet.with_coeff {
        println!("Score with coefficient {coeff:.2}: {s:.0}");
    }
    Ok(())
}

/// Extensions linked into this binary; each extension crate is a Cargo feature.
fn registry() -> Registry {
    let registry = Registry::new();
    #[cfg(feature = "general")]
    let registry = registry.with(zlog_general::General);
    #[cfg(feature = "jarl")]
    let registry = registry.with(zlog_jarl::Jarl);
    #[cfg(feature = "japan")]
    let registry = registry.with(zlog_japan::Japan);
    #[cfg(feature = "world")]
    let registry = registry.with(zlog_world::World);
    #[cfg(feature = "qxsl")]
    let registry = registry.with(zlog_qxsl::Qxsl);
    registry
}
