use std::path::Path;

use zlog_cfg::check::check;

fn cfg(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../apps/zlog-max/tests/regress/cfg")
        .join(name)
}

#[test]
fn a_good_cfg_has_no_errors() {
    let r = check(&cfg("good.cfg"));
    assert!(r.ok(), "{r:?}");
    assert_eq!(r.warnings, ["unknown or ignored command BOGUSCMD"]);
}

#[test]
fn a_script_without_an_engine_is_a_warning() {
    let r = check(&cfg("script.cfg"));
    assert!(r.ok());
    assert!(r.warnings.iter().any(|w| w.contains("no engine")));
}

#[test]
fn dat_problems_are_errors() {
    assert!(!check(&cfg("missing-dat.cfg")).ok());
    assert!(!check(&cfg("broken-dat.cfg")).ok());
    assert!(!check(Path::new("/nonexistent.cfg")).ok());
}
