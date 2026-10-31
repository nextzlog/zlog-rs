use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn dat(version: &str) -> String {
    let mut s = String::new();
    for i in 0..MIN_COUNTRIES {
        s.push_str(&format!(
            "Land {i}: 01: 01: EU: 0.00: 0.00: 0.0: X{i}:\n    X{i},=VER{version};\n"
        ));
    }
    s
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zlog-cty-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("zlog").join("cty.dat")
}

#[test]
fn texts_complete() {
    assert!(texts().missing().is_empty());
}

#[test]
fn ja_texts_are_short() {
    let long: Vec<_> = include_str!("../lang/ja.lang")
        .lines()
        .filter_map(|l| l.split_once(" = "))
        .filter(|(_, v)| v.chars().count() > 40)
        .collect();
    assert!(long.is_empty(), "{long:?}");
}

#[test]
fn the_version_is_read_from_the_ver_entry() {
    assert_eq!(version(&dat("20261020")).as_deref(), Some("20261020"));
    assert_eq!(version("Japan: 25: 45: AS: 0: 0: 0: JA:\n JA;"), None);
    assert_eq!(version("=VER2026"), None);
    let callsign = format!(
        "Canada: 05: 09: NA: 0: 0: 0: VE:\n    C3,=VERSION;\n{}",
        dat("20261023")
    );
    assert_eq!(version(&callsign).as_deref(), Some("20261023"));
    assert_eq!(validate(&callsign), Ok("20261023".into()));
}

#[test]
fn html_and_garbage_are_rejected() {
    assert_eq!(validate(&dat("20261020")), Ok("20261020".into()));
    let html = format!(
        "<!DOCTYPE html><html><body>{}</body></html>",
        dat("20261020")
    );
    assert!(validate(&html).is_err());
    assert!(validate("Not Found").is_err());
    assert!(validate("").is_err());
    assert!(validate(&dat("20261020")[..2000]).is_err());
    assert!(validate(&dat("x")).is_err());
}

#[test]
fn install_writes_newer_files_only() {
    let path = temp("install");
    assert_eq!(
        install(&path, &dat("20261001")),
        Ok(Outcome::Updated("20261001".into()))
    );
    assert_eq!(installed_version(&path).as_deref(), Some("20261001"));
    assert_eq!(
        install(&path, &dat("20261001")),
        Ok(Outcome::Current("20261001".into()))
    );
    assert!(install(&path, "<html>").is_err());
    assert_eq!(installed_version(&path).as_deref(), Some("20261001"));
    assert_eq!(
        install(&path, &dat("20261020")),
        Ok(Outcome::Updated("20261020".into()))
    );
    let dir = path.parent().unwrap();
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
    std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
}

static GETS: AtomicUsize = AtomicUsize::new(0);
static RELOADS: AtomicUsize = AtomicUsize::new(0);

fn updater(name: &str, get: fn(&str) -> Fetch, reload: fn()) -> Updater {
    Updater {
        path: Some(temp(name)),
        get,
        reload,
        ..Updater::default()
    }
}

fn said(u: &mut Updater) -> Vec<String> {
    u.take_actions()
        .into_iter()
        .filter_map(|a| match a {
            EntryAction::Say(s) => Some(s.text),
            _ => None,
        })
        .collect()
}

#[test]
fn nothing_is_fetched_while_auto_check_is_off() {
    let mut u = updater("off", |_| panic!("network"), || {});
    u.load(&Section::new());
    for _ in 0..3 {
        u.step("en");
    }
    assert!(u.job.is_none() && u.tick_after().is_none());
    assert!(said(&mut u).is_empty());
}

#[test]
fn check_now_installs_and_reloads() {
    let mut u = updater(
        "now",
        |url| {
            assert_eq!(url, URL);
            GETS.fetch_add(1, Ordering::SeqCst);
            Fetch::ready(Ok(dat("20261020")))
        },
        || {
            RELOADS.fetch_add(1, Ordering::SeqCst);
        },
    );
    u.run_command(ID);
    u.step("en");
    assert_eq!(GETS.load(Ordering::SeqCst), 1);
    assert_eq!(RELOADS.load(Ordering::SeqCst), 1);
    let path = u.path.clone().unwrap();
    assert_eq!(installed_version(&path).as_deref(), Some("20261020"));
    assert!(said(&mut u).last().unwrap().contains("20261020"));
    assert!(!u.last.is_empty());
    let mut s = Section::new();
    u.save(&mut s);
    assert_eq!(s[AUTO], "0");
    assert_eq!(s[LAST], u.last);
    std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
}

#[test]
fn auto_check_runs_once_at_start() {
    let mut u = updater(
        "auto",
        |_| Fetch::ready(Ok("<html>".into())),
        || panic!("reload"),
    );
    let mut s = Section::new();
    s.insert(AUTO.into(), "1".into());
    u.load(&s);
    assert!(u.tick_after().is_some());
    u.step("ja");
    let lines = said(&mut u);
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains(texts().get("ja", "invalid")));
    u.step("ja");
    assert!(u.job.is_none() && said(&mut u).is_empty());
}

#[test]
fn network_errors_are_reported() {
    let mut u = updater("error", |_| Fetch::ready(Err("offline".into())), || {});
    u.run_command(ID);
    u.step("en");
    assert!(said(&mut u).last().unwrap().ends_with("offline"));
}
