use std::process::Output;

use zlog_testing::cli::{fixture, fixtures, scratch, stdout};

fn zlog(args: &[&str]) -> Output {
    zlog_testing::cli::run(env!("CARGO_BIN_EXE_zlog"), args)
}

#[test]
fn regression_cases_match_byte_for_byte() {
    let out = zlog(&["regress", &fixtures().display().to_string()]);
    assert!(
        out.status.success(),
        "{}{}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stdout(&out).contains("FAIL"));
}

#[test]
fn check_cfg_accepts_a_good_cfg_and_reports_ignored_commands() {
    let out = zlog(&["check-cfg", &fixture("cfg/good.cfg")]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("warning: unknown or ignored command BOGUSCMD"));
    assert!(stdout(&out).contains("note: DAT test.dat: 7 entries"));
}

#[test]
fn check_cfg_compiles_the_lua_script_with_the_registered_engine() {
    let out = zlog(&["check-cfg", &fixture("cfg/script.cfg")]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(stdout(&out).contains("note: SCRIPT LUA 1"));
    assert!(!stdout(&out).contains("no engine"));
    let out = zlog(&["check-cfg", &fixture("cfg/bad-lua.cfg")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("error: SCRIPT LUA: syntax error"));
    let out = zlog(&["check-cfg", &fixture("cfg/bad-version.cfg")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("unsupported API version"));
}

#[test]
fn check_cfg_fails_on_dat_problems() {
    let out = zlog(&["check-cfg", &fixture("cfg/missing-dat.cfg")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("error: DAT nowhere.dat not found"));
    let out = zlog(&["check-cfg", &fixture("cfg/broken-dat.cfg")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("line 3: no multiplier code"));
    assert!(stdout(&out).contains("line 4: duplicate code 10"));
    assert_eq!(
        zlog(&["check-cfg", "/nonexistent.cfg"]).status.code(),
        Some(1)
    );
}

#[test]
fn convert_imports_qxsl_formats() {
    let adi = scratch("log.adi");
    let zlox = scratch("back.zlox");
    let log = fixture("logs/log.zlox");
    assert!(zlog(&["convert", &log, &adi]).status.success());
    assert!(zlog(&["convert", &adi, &zlox]).status.success());
    let dump = zlog(&["dump", &zlox]);
    assert!(stdout(&dump).starts_with("# Zlox, 7 QSOs"));
}

#[test]
fn convert_lists_the_known_formats_on_error() {
    let out = zlog(&["convert", &fixture("logs/log.zlox"), &scratch("x.nope")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ADIF (ADI)"));
}

#[test]
fn summary_writes_both_kinds() {
    let settings = fixture("logs/settings.toml");
    let log = fixture("logs/log.zlox");
    let em = scratch("log.em");
    let out = zlog(&["summary", &settings, &log, &em, "--contest", "allja"]);
    assert!(out.status.success());
    assert!(std::fs::read(&em).unwrap().starts_with(b"<SUMMARYSHEET"));
    let cbr = scratch("log.cbr");
    let out = zlog(&["summary", &settings, &log, &cbr, "--contest", "allja"]);
    assert!(out.status.success());
    assert!(std::fs::read(&cbr).unwrap().starts_with(b"START-OF-LOG"));
}

#[test]
fn summary_reports_errors() {
    let log = fixture("logs/log.zlox");
    let empty = scratch("empty.toml");
    std::fs::write(&empty, "").unwrap();
    let out = zlog(&[
        "summary",
        &empty,
        &log,
        &scratch("a.em"),
        "--contest",
        "allja",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("missing.signature"));
    let settings = fixture("logs/settings.toml");
    let unknown = [
        "summary",
        &settings,
        &log,
        &scratch("a.xyz"),
        "--contest",
        "allja",
    ];
    assert_eq!(zlog(&unknown).status.code(), Some(1));
    let contest = [
        "summary",
        &settings,
        &log,
        &scratch("a.em"),
        "--contest",
        "nope",
    ];
    assert_eq!(zlog(&contest).status.code(), Some(1));
    let bad = scratch("bad.toml");
    std::fs::write(&bad, "[x").unwrap();
    let toml = [
        "summary",
        &bad,
        &log,
        &scratch("a.em"),
        "--contest",
        "allja",
    ];
    assert_eq!(zlog(&toml).status.code(), Some(1));
    assert_eq!(zlog(&["summary", &settings]).status.code(), Some(2));
}

#[test]
fn allja1_zlo_from_the_original_zlog_decodes_every_record() {
    use zlog_core::{Band, Format, Log, Mode};
    let (log, format) = Log::load(fixture("logs/allja1.zlo")).unwrap();
    assert_eq!(format, Format::Zlo);
    assert_eq!(log.qsos.len(), 24);
    assert_eq!(log.utc_offset_minutes(), Some(540));
    let row = |i: usize| {
        let q = &log.qsos[i];
        format!(
            "{} {} {} {} {}|{}|{}|{} {} {}|{}|{} {} {}|{}|{} {} {}",
            q.time,
            q.callsign,
            q.band,
            q.mode,
            q.rst_sent,
            q.rst_rcvd,
            q.nr_sent,
            q.nr_rcvd,
            q.serial,
            q.multi1,
            q.multi2,
            q.new_multi1,
            q.points,
            q.dupe,
            q.operator,
            q.memo,
            q.power2,
            q.reserve3
        )
    };
    let expected = [
        (
            0,
            "2014-06-01 09:32:00 JA1YAD 14 SSB 59|59||100110 0 100110||true 1 false|| 2 1600",
        ),
        (
            1,
            "2014-06-01 09:35:00 JA1YWX 21 CW 599|599||100105 0 100105||true 1 false|| 2 12700",
        ),
        (
            3,
            "2014-06-01 10:02:00 JA1YXP 50 AM 59|59||134404 0 134404||true 1 false|| 2 35100",
        ),
        (
            5,
            "2014-06-01 10:51:00 JR1ZTT 21 SSB 59|59||1420 0 1420||false 0 true|| 2 58700",
        ),
        (
            6,
            "2014-06-01 10:57:00 JA1ZLO 50 SSB 59|59||10 0 10||false 1 false|| 2 61400",
        ),
        (
            11,
            "2014-06-01 11:24:00 JA3YDS 28 FM 59|59||22 0 22||true 1 false|| 2 110200",
        ),
        (
            13,
            "2014-06-01 16:02:00 JA1ZLO 1.9 CW 599|599||100110 0 100110||true 1 false|| 2 136600",
        ),
        (
            21,
            "2014-06-01 20:33:00 JA1ZGP 7 SSB 59|59||1009 0 1009||false 1 false|| 2 216300",
        ),
        (
            23,
            "2014-06-01 23:53:00 JA1YYE 7 SSB 59|59||13009 0 13009||true 1 false|| 2 232000",
        ),
    ];
    for (i, row_text) in expected {
        assert_eq!(row(i), row_text, "record {i}");
    }
    assert!(log.qsos.iter().all(|q| q.band != Band::Unknown
        && q.mode != Mode::Other
        && q.multi1 == q.nr_rcvd
        && q.points == u8::from(!q.dupe)));
    assert_eq!(log.qsos.iter().filter(|q| q.dupe).count(), 2);
}

#[test]
fn allja1_zlo_survives_a_zlox_round_trip() {
    let zlo = fixture("logs/allja1.zlo");
    let zlox = scratch("allja1.zlox");
    let back = scratch("allja1-back.zlo");
    assert!(zlog(&["convert", &zlo, &zlox]).status.success());
    assert!(zlog(&["convert", &zlox, &back]).status.success());
    let (a, b) = (std::fs::read(&zlo).unwrap(), std::fs::read(&back).unwrap());
    assert_eq!(a.len(), b.len());
    let load = |p: &str| zlog_core::Log::load(p).unwrap().0;
    let (a, x, b) = (load(&zlo), load(&zlox), load(&back));
    assert_eq!(a.qsos, x.qsos);
    assert_eq!(a.qsos, b.qsos);
    assert_eq!(a.header, b.header);
}

#[test]
fn usage_errors_exit_with_two() {
    assert_eq!(zlog(&["summary"]).status.code(), Some(2));
}
