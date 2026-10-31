#![allow(dead_code)]

pub mod bench;
pub mod gen;
pub mod reference;

use std::path::PathBuf;
use std::time::Duration;

use zlog_cfg::UserDefinedDef;
use zlog_core::{Contest, ContestMode, Log, ScoreSheet};

pub const IDS: &[&str] = &["sample", "days"];

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn setup() {
    zlog_lua::install();
    zlog_cfg::script::host::patience(Duration::from_secs(20));
}

pub fn ported(id: &str) -> UserDefinedDef {
    setup();
    let (def, missing) = zlog_cfg::file::load_cfg(&fixture(&format!("{id}.cfg"))).unwrap();
    assert_eq!(missing, None);
    assert!(def.has_script() && def.script_error().is_none(), "{id}");
    def
}

pub fn builtin(id: &str) -> UserDefinedDef {
    match id {
        "sample" => reference::sample(),
        "days" => reference::days(),
        _ => unreachable!(),
    }
}

pub fn known(log: &mut Log, c: &mut dyn Contest) -> ScoreSheet {
    let sheet = zlog_core::rescore(log, c).unwrap();
    assert!(!c.score_unknown(), "{}", c.name());
    sheet
}

pub fn scored(
    def: &UserDefinedDef,
    mode: ContestMode,
    qsos: Vec<zlog_core::Qso>,
) -> (Log, ScoreSheet) {
    let mut c = def.build_rules(mode);
    let mut l = Log {
        qsos,
        ..Log::default()
    };
    let sheet = known(&mut l, &mut c);
    (l, sheet)
}
