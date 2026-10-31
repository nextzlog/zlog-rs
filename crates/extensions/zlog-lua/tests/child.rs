mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zlog_cfg::script::host::{self, Placement};
use zlog_cfg::script::{Inputs, ScriptProgram, Sums};

const ENTRY: &str = "script_host_entry";

static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn script_host_entry() {
    if std::env::var_os("ZLOG_LUA_TEST_HOST").is_some() && std::env::args().any(|a| a == ENTRY) {
        zlog_lua::install();
        host::serve_stdio();
        std::process::exit(0);
    }
}

fn in_child() -> std::sync::MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    support::setup();
    std::env::set_var("ZLOG_LUA_TEST_HOST", "1");
    host::place(Placement::Child {
        program: std::env::current_exe().unwrap(),
        args: ["--exact", ENTRY, "--nocapture", "--test-threads=1"]
            .map(String::from)
            .to_vec(),
    });
    guard
}

fn source_program(source: &str) -> Arc<dyn ScriptProgram> {
    let engine = zlog_cfg::script::engine("lua").unwrap();
    host::program(engine, "test", source, 1)
}

fn upto(all: &[Arc<zlog_cfg::script::ScriptQso>], n: usize) -> Inputs {
    Arc::new(all[..n].to_vec())
}

const SUMS: Sums = Sums {
    qsos: 1,
    points: 2,
    multis: 3,
};

#[test]
fn the_child_host_is_fast_enough() {
    let _g = in_child();
    for id in support::IDS {
        let t = support::bench::measure(id);
        println!(
            "child {id}: start {:.3} ms, append {:.4} ms, full replay {:.3} ms",
            t.start, t.append, t.replay
        );
        if !cfg!(debug_assertions) {
            assert!(t.append <= 1.0, "{id} append {} ms", t.append);
            assert!(t.replay <= 50.0, "{id} replay {} ms", t.replay);
        }
    }
}

fn rss_kb(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    s.lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[test]
fn a_killed_child_is_restarted_and_replayed() {
    let _g = in_child();
    let all = support::bench::inputs("sample", 400);
    let want = {
        host::place(Placement::InProcess);
        let s = support::bench::program("sample").start();
        let v = s.judge(&upto(&all, 400)).unwrap();
        (v, s.total(&upto(&all, 400), SUMS).unwrap())
    };
    drop(_g);
    let _g = in_child();
    let s = support::bench::program("sample").start();
    s.judge(&upto(&all, 350)).unwrap();
    let before = host::child_ids();
    assert_eq!(before.len(), 1);
    if let Some(kb) = rss_kb(before[0]) {
        println!("child peak memory {kb} kB");
    }
    std::process::Command::new("kill")
        .args(["-9", &before[0].to_string()])
        .status()
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let v = s.judge(&upto(&all, 400));
    let v = match v {
        Some(v) => v,
        None => s.judge(&upto(&all, 400)).expect("recovered"),
    };
    assert_eq!(v, want.0);
    assert_eq!(s.total(&upto(&all, 400), SUMS), Some(want.1));
    let after = host::child_ids();
    assert_eq!(after.len(), 1);
    assert_ne!(after, before);
}

#[test]
fn hostile_scripts_stop_in_the_child_and_the_host_lives_on() {
    let _g = in_child();
    let all = support::bench::inputs("sample", 10);
    for (body, why) in [
        ("while true do end", "too long"),
        ("local s = 'x' while true do s = s .. s end", "memory"),
        ("local t = {} for i = 1, 1e9 do t[i] = i end", ""),
        ("local t = table.create(2e7, true)", "memory"),
        ("local s = string.rep('x', 2 ^ 30)", "memory"),
        ("local function f(n) return f(n + 1) + 1 end f(1)", "deeply"),
        ("os.exit(1)", "index nil"),
    ] {
        host::take_notices();
        let p = source_program(&format!(
            "function init() return {{}} end\nfunction add(s, q)\n{body}\nreturn s, {{}} end\nfunction total(s, t) return 0 end"
        ));
        let s = p.start();
        let t = Instant::now();
        assert!(s.judge(&upto(&all, 10)).is_none(), "{body}");
        assert!(t.elapsed() < Duration::from_secs(20));
        let notices = host::take_notices().join("\n");
        assert!(notices.contains(why), "{body}: {notices}");
        assert!(
            why.is_empty() || why == "memory" || notices.contains("test:"),
            "{body}: {notices}"
        );
    }
    let s = support::bench::program("sample").start();
    assert!(s.judge(&upto(&all, 10)).is_some());
}

const COUNTING: &str = "function init() return {n = 0} end\nfunction add(s, q) s.n += 1 if q.rcvd == '?' then error('no number') end return s, {score = 1} end\nfunction total(s, t) return s.n end";

fn until<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "no answer");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_crash_before_the_total_replays_the_whole_log() {
    let _g = in_child();
    let all = support::bench::inputs("sample", 350);
    let s = source_program(COUNTING).start();
    until(|| s.judge(&upto(&all, 350)));
    let before = host::child_ids();
    std::process::Command::new("kill")
        .args(["-9", &before[0].to_string()])
        .status()
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let total = until(|| {
        s.judge(&upto(&all, 350))?;
        s.total(&upto(&all, 350), SUMS)
    });
    assert_eq!(total, 350);
}

#[test]
fn an_error_while_typing_shows_nothing_and_keeps_the_script() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    support::setup();
    host::place(Placement::InProcess);
    let all = support::bench::inputs("sample", 3);
    let s = source_program(COUNTING).start();
    until(|| s.judge(&upto(&all, 3)));
    let mut typing = (*all[0]).clone();
    typing.rcvd = "?".into();
    for _ in 0..5 {
        assert_eq!(s.check(&typing), None);
    }
    assert!(until(|| s.check(&all[1])).invalid.is_none());
    assert_eq!(until(|| s.total(&upto(&all, 3), SUMS)), 3);
}
