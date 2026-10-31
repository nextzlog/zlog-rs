use std::io::Write;
use std::path::Path;

use crate::{execute, Failure, MakeRegistry, Tools};

pub fn run<T: Tools>(
    registry: MakeRegistry,
    dir: &Path,
    update: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let mut cases: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("args").is_file())
        .collect();
    cases.sort();
    if cases.is_empty() {
        return Err(format!("{}: no cases", dir.display()));
    }
    let mut failed = Vec::new();
    for case in &cases {
        let name = case.file_name().unwrap_or_default().to_string_lossy();
        let verdict = check::<T>(registry, case, update);
        let line = match &verdict {
            Ok(()) => format!("ok   {name}"),
            Err(e) => format!("FAIL {name}: {e}"),
        };
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
        if verdict.is_err() {
            failed.push(name.into_owned());
        }
    }
    match failed.is_empty() {
        true => Ok(()),
        false => Err(format!("{} of {} cases failed", failed.len(), cases.len())),
    }
}

fn check<T: Tools>(registry: MakeRegistry, case: &Path, update: bool) -> Result<(), String> {
    let spec = std::fs::read_to_string(case.join("args")).map_err(|e| e.to_string())?;
    let name = case.file_name().unwrap_or_default().to_string_lossy();
    let temp = std::env::temp_dir().join(format!("zlog-regress-{}-{name}", std::process::id()));
    let mut output = None;
    let mut args = vec!["zlog".to_string()];
    for line in spec.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let arg = line
            .replace("{dir}", &case.to_string_lossy())
            .replace("{out}", &temp.to_string_lossy());
        if line.contains("{out}") {
            output.get_or_insert_with(|| arg.clone());
        }
        args.push(arg);
    }
    let mut stdout = Vec::new();
    let result = execute::<T>(registry, &args, &mut stdout);
    let actual = match &output {
        Some(path) => {
            let bytes = std::fs::read(path).unwrap_or_default();
            let _ = std::fs::remove_file(path);
            bytes
        }
        None => stdout,
    };
    match result {
        Ok(()) => {}
        Err(Failure::Usage(e)) => return Err(e.to_string()),
        Err(Failure::Failed(e)) => return Err(e),
    }
    let expected = case.join("expected");
    if update {
        return std::fs::write(&expected, &actual).map_err(|e| e.to_string());
    }
    let want = std::fs::read(&expected).map_err(|e| format!("expected: {e}"))?;
    match want == actual {
        true => Ok(()),
        false => {
            let at = want
                .iter()
                .zip(&actual)
                .position(|(a, b)| a != b)
                .unwrap_or(want.len().min(actual.len()));
            Err(format!(
                "differs at byte {at} (expected {} bytes, got {})",
                want.len(),
                actual.len()
            ))
        }
    }
}
