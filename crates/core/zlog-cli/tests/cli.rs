use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn zlog(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zlog-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regress")
}

fn log(name: &str) -> String {
    fixtures().join("logs").join(name).display().to_string()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zlog-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn regression_cases_match_byte_for_byte() {
    let out = zlog(&["regress", &fixtures().display().to_string()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!text.contains("FAIL"), "{text}");
}

#[test]
fn regress_reports_a_mismatch_and_fails() {
    let dir = scratch("broken");
    let case = dir.join("case");
    std::fs::create_dir_all(&case).unwrap();
    std::fs::write(case.join("args"), format!("dump\n{}\n", log("log.zlo"))).unwrap();
    std::fs::write(case.join("expected"), "wrong").unwrap();
    let out = zlog(&["regress", &dir.display().to_string()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("FAIL case"));
}

#[test]
fn regress_without_cases_fails() {
    let dir = scratch("empty");
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(
        zlog(&["regress", &dir.display().to_string()]).status.code(),
        Some(1)
    );
}

#[test]
fn convert_zlo_to_zlox_and_back_is_lossless() {
    let zlox = scratch("back.zlox");
    let zlo = scratch("back.zlo");
    let z = zlox.display().to_string();
    assert!(zlog(&["convert", &log("log.zlo"), &z]).status.success());
    assert!(zlog(&["convert", &z, &zlo.display().to_string()])
        .status
        .success());
    assert_eq!(
        std::fs::read(zlo).unwrap(),
        std::fs::read(log("log.zlo")).unwrap()
    );
}

#[test]
fn convert_honours_explicit_formats() {
    let out = scratch("named.bin");
    let o = out.display().to_string();
    let run = zlog(&["convert", &log("log.zlox"), &o, "--to", "ZLO"]);
    assert!(run.status.success());
    assert_eq!(
        std::fs::read(out).unwrap(),
        std::fs::read(log("log.zlo")).unwrap()
    );
}

#[test]
fn convert_rejects_unknown_formats_and_missing_files() {
    let o = scratch("x.unknown").display().to_string();
    let run = zlog(&["convert", &log("log.zlo"), &o]);
    assert_eq!(run.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("unknown format"));
    let o = scratch("x.zlo").display().to_string();
    assert_eq!(
        zlog(&["convert", "/nonexistent.zlo", &o]).status.code(),
        Some(1)
    );
}

#[test]
fn score_prints_the_sheet() {
    let run = zlog(&["score", &log("log.zlox"), "base"]);
    assert!(run.status.success());
    assert!(String::from_utf8_lossy(&run.stdout).contains("Score  6"));
}

#[test]
fn score_rejects_unknown_contests_and_modes() {
    assert_eq!(
        zlog(&["score", &log("log.zlox"), "nope"]).status.code(),
        Some(1)
    );
    assert_eq!(
        zlog(&["score", &log("log.zlox"), "base", "rtty"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn dump_lists_the_qsos() {
    let run = zlog(&["dump", &log("log.zlo")]);
    assert!(run.status.success());
    assert!(String::from_utf8_lossy(&run.stdout).starts_with("# Zlo, 7 QSOs"));
}

#[test]
fn usage_errors_exit_with_two() {
    assert_eq!(zlog(&[]).status.code(), Some(2));
    assert_eq!(zlog(&["bogus"]).status.code(), Some(2));
    assert_eq!(zlog(&["convert", "only-one"]).status.code(), Some(2));
    assert!(zlog(&["--help"]).status.success());
}
