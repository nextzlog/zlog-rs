use std::collections::BTreeSet;
use std::path::Path;

use crate::file::{decode, find_file};
use crate::script::{engine, split, Limits};
use crate::Config;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub fn check(path: &Path) -> Report {
    let mut r = Report::default();
    let text = match std::fs::read(path) {
        Ok(b) => decode(&b),
        Err(e) => {
            r.errors.push(format!("{}: {e}", path.display()));
            return r;
        }
    };
    let config = Config::parse(&text);
    r.notes.push(format!("contest {}", config.contest_name));
    for cmd in &config.ignored {
        r.warnings.push(format!("unknown or ignored command {cmd}"));
    }
    if !config.dat.is_empty() {
        let dir = path.parent().unwrap_or(Path::new("."));
        match find_file(dir, &config.dat).map(std::fs::read) {
            None => r.errors.push(format!("DAT {} not found", config.dat)),
            Some(Err(e)) => r.errors.push(format!("DAT {}: {e}", config.dat)),
            Some(Ok(bytes)) => dat(&config.dat, &decode(&bytes), &mut r),
        }
    }
    script(&text, &config.contest_name, &mut r);
    r
}

fn dat(name: &str, text: &str, r: &mut Report) {
    let mut seen = BTreeSet::new();
    let mut count = 0;
    for (n, line) in text
        .lines()
        .enumerate()
        .skip(1)
        .take_while(|(_, l)| !l.to_lowercase().contains("end of file"))
    {
        count += 1;
        let code = line.split(' ').next().unwrap_or("").trim();
        if code.is_empty() {
            r.errors
                .push(format!("DAT {name} line {}: no multiplier code", n + 1));
        } else if !seen.insert(code.to_string()) {
            r.warnings
                .push(format!("DAT {name} line {}: duplicate code {code}", n + 1));
        }
    }
    match count {
        0 => r.errors.push(format!("DAT {name}: no entries")),
        n => r.notes.push(format!("DAT {name}: {n} entries")),
    }
}

fn script(text: &str, name: &str, r: &mut Report) {
    let Some(section) = split(text).1 else {
        return;
    };
    let language = section.language.to_uppercase();
    let version = section.version.as_deref().unwrap_or("1");
    r.notes.push(format!("SCRIPT {language} {version}"));
    let Some(engine) = engine(&section.language) else {
        r.warnings.push(format!(
            "SCRIPT {language}: no engine in this build; not compiled"
        ));
        return;
    };
    let Some(version) = version
        .parse()
        .ok()
        .filter(|v| engine.versions().contains(v))
    else {
        r.errors.push(format!(
            "SCRIPT {language} {version}: unsupported API version"
        ));
        return;
    };
    if let Err(e) = engine.compile(name, section.source, version, &Limits::default()) {
        r.errors.push(format!("SCRIPT {language}: {e}"));
    }
}
