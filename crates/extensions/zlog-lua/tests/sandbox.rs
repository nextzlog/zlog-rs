use zlog_cfg::script::{Limits, ScriptQso, Sums, Vm};
use zlog_core::{Band, Mode};

fn qso() -> ScriptQso {
    ScriptQso {
        call: "JA1ZLO".into(),
        band: Band::B7,
        mode: Mode::Cw,
        rcvd: "10HS".into(),
        sent: "599".into(),
        name: String::new(),
        note: String::new(),
        srst: 599,
        rrst: 599,
        tx: 0,
        time: (2026, 8, 1, 9, 30, 0),
        dupe: false,
        counted: true,
    }
}

fn vm(source: &str) -> Result<Box<dyn Vm>, String> {
    zlog_lua::LuaVm::compile("t", source, &Limits::default()).map(|v| Box::new(v) as Box<dyn Vm>)
}

fn in_add(body: &str) -> String {
    let source = format!(
        "function init() return {{}} end\nfunction add(state, qso)\n{body}\nreturn state, {{}}\nend\nfunction total(state, sums) return 0 end"
    );
    let mut v = vm(&source).unwrap();
    v.reset().unwrap();
    v.add(&qso()).expect_err(body)
}

#[test]
fn nothing_outside_the_sandbox_is_reachable() {
    for body in [
        "os.time()",
        "io.open('x')",
        "require('x')",
        "loadstring('return 1')()",
        "load('return 1')()",
        "dofile('x')",
        "getfenv(1)",
        "setfenv(1, {})",
        "print('x')",
        "collectgarbage()",
        "math.random()",
        "math.randomseed(1)",
        "debug.traceback()",
        "coroutine.create(print)",
        "newproxy()",
    ] {
        let e = in_add(body);
        assert!(e.contains("t:3:"), "{body}: {e}");
    }
}

#[test]
fn shared_tables_cannot_be_changed() {
    for body in [
        "leak = 1",
        "string.upper = nil",
        "zlog.api = 2",
        "zlog.bands[1] = 'x'",
        "qso.call = 'X'",
        "table.insert = nil",
        "init = nil",
    ] {
        let e = in_add(body);
        assert!(
            e.contains("readonly") || e.contains("read-only"),
            "{body}: {e}"
        );
    }
}

#[test]
fn runaway_scripts_are_stopped() {
    let cases = [
        ("while true do end", "ran too long"),
        (
            "local function f() return f() + 1 end f()",
            "nested calls too deeply",
        ),
        ("local s = string.rep('x', 2 ^ 30)", "memory"),
        ("local s = 'x' for i = 1, 40 do s = s .. s end", "memory"),
        ("local t = table.create(2e7, 1)", "memory"),
        ("error('mine')", "t:3: mine"),
    ];
    for (body, want) in cases {
        let e = in_add(body);
        assert!(e.contains(want), "{body}: {e}");
    }
}

#[test]
fn broken_scripts_are_reported_with_their_line() {
    let e = vm("function init() return {} end\nfunction add(s, q) return s, { end")
        .err()
        .unwrap();
    assert!(e.contains("t:2"), "{e}");
    let e = vm("function init() return {} end").err().unwrap();
    assert!(e.contains("add"), "{e}");
    let e = vm("while true do end").err().unwrap();
    assert!(e.contains("too long"), "{e}");
}

fn run(add: &str, total: &str) -> (Result<(), String>, Result<u64, String>) {
    let source = format!(
        "function init() return {{n = 0}} end\nfunction add(state, qso)\n{add}\nend\nfunction total(state, sums)\n{total}\nend"
    );
    let mut v = vm(&source).unwrap();
    v.reset().unwrap();
    let added = v.add(&qso()).map(|_| ());
    let sums = Sums {
        qsos: 1,
        points: 2,
        multis: 3,
    };
    (added, v.total(sums))
}

#[test]
fn bad_results_are_errors() {
    for add in [
        "return state",
        "return state, 1",
        "return state, {score = 1.5}",
        "return state, {score = 'x'}",
        "return state, {mul1 = 1}",
        "return state, {score = 1e12}",
        "return state, {invalid = 1}",
        "return state, {mul1 = 'a\\0b'}",
        "return state, {mul2 = 'a\\nb'}",
    ] {
        let (added, _) = run(add, "return 0");
        assert!(added.is_err(), "{add}");
    }
    for total in [
        "return -1",
        "return 0.5",
        "return 'x'",
        "return nil",
        "state.n = 1 return 0",
    ] {
        let (_, t) = run("return state, {}", total);
        assert!(t.is_err(), "{total}");
    }
    let (_, t) = run(
        "return state, {extra = 1}",
        "return sums.score * sums.mults + sums.qsos",
    );
    assert_eq!(t, Ok(7));
}

#[test]
fn states_that_cannot_be_kept_are_errors() {
    let source = "function init() return {} end\nfunction add(state, qso) state.f = function() end return state, {} end\nfunction total(s, t) return 0 end";
    let mut v = vm(source).unwrap();
    v.reset().unwrap();
    assert!(v.add(&qso()).is_err() || v.save().is_err());
    let source = "function init() return {best = math.huge} end\nfunction add(state, qso) return state, {} end\nfunction total(s, t) return 0 end";
    let mut v = vm(source).unwrap();
    v.reset().unwrap();
    v.add(&qso()).unwrap();
    assert!(v.verify().unwrap_err().contains("not finite"));
}

#[test]
fn chunk_level_locals_cannot_keep_state() {
    let tail = "function init() return {} end\nfunction total(s, t) return 0 end\n";
    for add in [
        "local seen = {}\nfunction add(s, q) local p = seen[q.call] and 0 or 1 seen[q.call] = true return s, {score = p} end",
        "local n = 0\nfunction add(s, q) n += 1 return s, {score = n} end",
        "local box = {inner = {}}\nlocal function mark(q) box.inner[q.call] = 1 end\nfunction add(s, q) mark(q) return s, {} end",
    ] {
        let mut v = vm(&format!("{add}\n{tail}")).unwrap();
        v.reset().unwrap();
        assert!(v.check(&qso()).is_err(), "{add}");
        assert!(v.add(&qso()).is_err(), "{add}");
    }
    let mut v = vm(&format!(
        "local points = {{JA1ZLO = 3}}\nfunction add(s, q) return s, {{score = points[q.call]}} end\n{tail}"
    ))
    .unwrap();
    v.reset().unwrap();
    assert_eq!(v.add(&qso()).unwrap().points, 3);
}

#[test]
fn shared_and_cyclic_tables_keep_their_identity() {
    let source = "function init() local t = {n = 0} local s = {a = t, b = t} s.me = s return s end\nfunction add(s, q) s.a.n += 1 return s, {score = s.b.n + (s.me == s and 10 or 0)} end\nfunction total(s, t) return s.me.b.n end";
    let mut a = vm(source).unwrap();
    a.reset().unwrap();
    a.add(&qso()).unwrap();
    let saved = a.save().unwrap();
    let mut b = vm(source).unwrap();
    b.restore(&saved).unwrap();
    assert_eq!(b.save().unwrap(), saved);
    assert_eq!(a.check(&qso()).unwrap().points, 12);
    assert_eq!(a.add(&qso()).unwrap(), b.add(&qso()).unwrap());
    assert_eq!(b.total(Sums::default()).unwrap(), 2);
}

#[test]
fn check_of_a_large_state_is_fast() {
    let source = "function init() local s = {} for i = 1, 100000 do s[i] = {i} end return s end\nfunction add(s, q) return s, {score = #s} end\nfunction total(s, t) return 0 end";
    let mut v = zlog_lua::LuaVm::compile(
        "t",
        source,
        &Limits {
            steps: u64::MAX,
            memory: 256 << 20,
            ..Limits::default()
        },
    )
    .unwrap();
    v.reset().unwrap();
    let start = std::time::Instant::now();
    for _ in 0..10 {
        v.check(&qso()).unwrap();
    }
    let each = start.elapsed() / 10;
    let start = std::time::Instant::now();
    let saved = v.save().unwrap();
    v.restore(&saved).unwrap();
    println!(
        "check with 200k state nodes: {each:?}, JSON round trip {:?}",
        start.elapsed()
    );
    let limit = if cfg!(debug_assertions) { 2000 } else { 500 };
    assert!(each < std::time::Duration::from_millis(limit), "{each:?}");
}

#[test]
fn saved_states_restore_exactly() {
    let source = "function init() return {n = 0, keys = {}, [5] = true, [1.5] = 'x'} end\nfunction add(state, qso) state.n += 1 state.keys[qso.call .. state.n] = state.n return state, {score = state.n} end\nfunction total(s, t) return s.n end";
    let mut a = vm(source).unwrap();
    a.reset().unwrap();
    for _ in 0..5 {
        a.add(&qso()).unwrap();
    }
    let saved = a.save().unwrap();
    let mut b = vm(source).unwrap();
    b.restore(&saved).unwrap();
    assert_eq!(b.save().unwrap(), saved);
    assert_eq!(a.add(&qso()).unwrap(), b.add(&qso()).unwrap());
    let sums = Sums::default();
    assert_eq!(a.total(sums).unwrap(), 6);
    assert_eq!(b.total(sums).unwrap(), 6);
}

const PLAIN: &str = "\nfunction add(s, q) return s, {} end\nfunction total(s, t) return 0 end";

#[test]
fn deep_states_survive_json_and_never_overflow_the_stack() {
    let source = format!(
        "function init() local s = {{}} for i = 1, 300000 do s = {{n = s}} end return s end{PLAIN}"
    );
    let mut v = vm(&source).unwrap();
    v.reset().unwrap();
    v.verify().unwrap();
    v.check(&qso()).unwrap();
    v.total(Sums::default()).unwrap();
    let saved = v.save().unwrap();
    let text = serde_json::to_string(&saved).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    v.restore(&parsed).unwrap();
    assert_eq!(v.save().unwrap(), saved);
}

#[test]
fn deep_globals_do_not_overflow_the_stack() {
    let source = format!(
        "G = {{}} for i = 1, 300000 do G = {{n = G}} end\nfunction init() return {{}} end{PLAIN}"
    );
    vm(&source).unwrap();
}

#[test]
fn states_must_be_plain_data_of_their_own() {
    for (init, why) in [
        ("return setmetatable({}, {})", "metatable"),
        (
            "return {inner = setmetatable({}, {__index = function() return 7 end})}",
            "metatable",
        ),
        ("return {k = K}", "global"),
        ("return {k = L}", "global"),
        ("return {s = string}", "global"),
        ("return {b = zlog.bands}", "global"),
        ("return {f = function() end}", "only nil"),
    ] {
        let source = format!("K = {{}}\nlocal L = {{}}\nfunction init() {init} end{PLAIN}");
        let mut v = vm(&source).unwrap();
        v.reset().unwrap();
        assert!(v.check(&qso()).unwrap_err().contains(why), "{init}");
        assert!(v.verify().unwrap_err().contains(why), "{init}");
        assert!(v.save().unwrap_err().contains(why), "{init}");
        assert!(
            v.total(Sums::default()).unwrap_err().contains(why),
            "{init}"
        );
    }
    let source = format!("function init() return {{t = table.freeze({{1}})}} end{PLAIN}");
    let mut v = vm(&source).unwrap();
    v.reset().unwrap();
    v.check(&qso()).unwrap();
    v.verify().unwrap();
}

#[test]
fn check_agrees_with_add_on_random_states() {
    const PARTS: &[&str] = &[
        "s.a = {}",
        "s.b = s.a",
        "s.c = K",
        "s.d = setmetatable({}, MT)",
        "s.e = {x = 3}",
        "s[s.e or 1] = 5",
        "s.me = s",
        "s.q = q",
        "s[1.5] = true",
        "s.f = table.freeze({y = 2})",
    ];
    let probe = "local p = 0\nif s.a ~= nil and s.a == s.b then p += 1 end\nif s.c == K then p += 2 end\nif s.d ~= nil and s.d.z == 9 then p += 4 end\nif s.e ~= nil and s[s.e] == 5 then p += 8 end\nif s.me == s then p += 16 end\nif s.f ~= nil and s.f.y == 2 then p += 32 end\nif s.q ~= nil then p += 64 end";
    let mut seed = 0x2545_f491_u64;
    for _ in 0..200 {
        let mut body = Vec::new();
        for part in PARTS {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            if seed >> 62 == 0 {
                body.push(*part);
            }
        }
        let body = body.join("\n");
        let source = format!(
            "K = {{}}\nMT = {{__index = function() return 9 end}}\nfunction init() return {{}} end\nfunction add(s, q)\n{body}\n{probe}\nreturn s, {{score = p}}\nend\nfunction total(s, t) return 0 end"
        );
        let mut v = vm(&source).unwrap();
        v.reset().unwrap();
        let first = v.add(&qso()).unwrap();
        let kept = v.verify();
        let checked = v.check(&qso());
        let added = v.add(&qso()).and_then(|j| v.verify().map(|_| j));
        assert_eq!(kept.is_ok(), checked.is_ok(), "{body}");
        if let Ok(c) = checked {
            assert_eq!(Ok(c), added, "{body}");
        }
        let _ = first;
    }
}

#[test]
fn saved_keys_are_in_a_total_order() {
    let source = format!(
        "function init() return {{[1.0000001] = 'a', [1.0000002] = 'b', [true] = 1, x = 2, [-0.5] = 3}} end{PLAIN}"
    );
    let mut v = vm(&source).unwrap();
    v.reset().unwrap();
    let saved = v.save().unwrap();
    let keys: Vec<String> = saved["tables"][0]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p[0].to_string())
        .collect();
    assert_eq!(keys, ["true", "-0.5", "1.0000001", "1.0000002", "\"x\""]);
}

#[test]
fn totals_of_wide_states_are_fast() {
    let source = format!(
        "function init() local s = {{}} for i = 1, 50000 do s[i] = {{}} end return s end{PLAIN}"
    );
    let mut v = zlog_lua::LuaVm::compile(
        "t",
        &source,
        &Limits {
            steps: u64::MAX,
            memory: 256 << 20,
            ..Limits::default()
        },
    )
    .unwrap();
    v.reset().unwrap();
    let start = std::time::Instant::now();
    v.total(Sums::default()).unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}
