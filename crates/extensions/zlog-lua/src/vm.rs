use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use mlua::{ffi, Function, Lua, LuaOptions, MultiValue, StdLib, Table, Value, VmState};
use serde_json::{json, Value as Json};
use zlog_cfg::script::{Judged, Limits, ScriptQso, Sums, Vm};
use zlog_core::{Band, Mode};

const REMOVED: &[&str] = &[
    "print",
    "loadstring",
    "load",
    "require",
    "dofile",
    "loadfile",
    "getfenv",
    "setfenv",
    "collectgarbage",
    "gcinfo",
    "newproxy",
];

const STATE_NODES: usize = 1_000_000;

const MULTIS: usize = 2;

const DEPTH_EVERY: u64 = 16;

const MAKE: &str = "local call, sent, rcvd, name, note, band, mode, srst, rrst, tx, time, dupe, counted = ...
return table.freeze({call = call, sent = sent, rcvd = rcvd, name = name, note = note, band = band, mode = mode,
srst = srst, rrst = rrst, tx = tx, time = time, dupe = dupe, counted = counted})";

pub struct LuaVm {
    lua: Lua,
    make: Function,
    bands: Vec<Value>,
    modes: Vec<Value>,
    memory: usize,
    steps: Arc<AtomicU64>,
    init: Function,
    add: Function,
    total: Function,
    locals: Vec<(Function, Vec<Value>)>,
    sealed: HashSet<usize>,
    state: Value,
}

fn text(e: mlua::Error) -> String {
    match e {
        mlua::Error::CallbackError { cause, .. } => text((*cause).clone()),
        mlua::Error::RuntimeError(m) => m.lines().next().unwrap_or("").to_string(),
        mlua::Error::MemoryError(_) => "the script used more memory than allowed".into(),
        e => e.to_string().lines().next().unwrap_or("").to_string(),
    }
}

fn stopped(lua: &Lua, why: &str) -> mlua::Error {
    let at = lua
        .inspect_stack(0, |d| {
            let source = d.source();
            let name = source.short_src.as_deref().unwrap_or("?").to_string();
            format!("{name}:{}: ", d.current_line().unwrap_or(0))
        })
        .unwrap_or_default();
    mlua::Error::runtime(format!("{at}{why}"))
}

fn sandbox(limits: &Limits) -> mlua::Result<(Lua, Arc<AtomicU64>)> {
    let lua = Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8 | StdLib::BIT,
        LuaOptions::default(),
    )?;
    let g = lua.globals();
    for name in REMOVED {
        g.raw_set(*name, Value::Nil)?;
    }
    let math: Table = g.get("math")?;
    math.raw_set("random", Value::Nil)?;
    math.raw_set("randomseed", Value::Nil)?;
    g.raw_set("zlog", api(&lua)?)?;
    lua.sandbox(true)?;
    lua.set_memory_limit(limits.memory)?;
    let steps = Arc::new(AtomicU64::new(0));
    let counter = steps.clone();
    let (budget, depth) = (limits.steps, limits.depth);
    lua.set_interrupt(move |lua| {
        let n = counter.fetch_add(1, Ordering::Relaxed);
        if n >= budget {
            return Err(stopped(lua, "the script ran too long"));
        }
        if n.is_multiple_of(DEPTH_EVERY) && lua.inspect_stack(depth, |_| ()).is_some() {
            return Err(stopped(lua, "the script nested calls too deeply"));
        }
        Ok(VmState::Continue)
    });
    Ok((lua, steps))
}

fn api(lua: &Lua) -> mlua::Result<Table> {
    let z = lua.create_table()?;
    z.raw_set("api", 1)?;
    let bands = lua.create_sequence_from(Band::ALL.iter().map(|b| b.label()))?;
    bands.set_readonly(true);
    z.raw_set("bands", bands)?;
    let modes = lua.create_sequence_from(Mode::ALL.iter().map(|m| m.label()))?;
    modes.set_readonly(true);
    z.raw_set("modes", modes)?;
    z.set_readonly(true);
    Ok(z)
}

fn function(lua: &Lua, name: &str) -> Result<Function, String> {
    match lua.globals().raw_get::<Value>(name).map_err(text)? {
        Value::Function(f) => Ok(f),
        _ => Err(format!("the script defines no function {name}")),
    }
}

impl LuaVm {
    pub fn compile(name: &str, source: &str, limits: &Limits) -> Result<LuaVm, String> {
        let (lua, steps) = sandbox(limits).map_err(text)?;
        let make = lua
            .load(MAKE)
            .set_name("=zlog")
            .into_function()
            .map_err(text)?;
        let labels = |all: &mut dyn Iterator<Item = &'static str>| {
            all.map(|l| lua.create_string(l).map(Value::String))
                .collect::<mlua::Result<Vec<_>>>()
                .map_err(text)
        };
        let bands = labels(&mut Band::ALL.iter().map(|b| b.label()))?;
        let modes = labels(&mut Mode::ALL.iter().map(|m| m.label()))?;
        lua.load(source)
            .set_name(format!("={name}"))
            .exec()
            .map_err(text)?;
        let init = function(&lua, "init")?;
        let add = function(&lua, "add")?;
        let total = function(&lua, "total")?;
        let (sealed, locals) = seal(&lua).map_err(text)?;
        steps.store(0, Ordering::Relaxed);
        Ok(LuaVm {
            make,
            bands,
            modes,
            lua,
            memory: limits.memory,
            steps,
            init,
            add,
            total,
            locals,
            sealed,
            state: Value::Nil,
        })
    }

    fn fresh(&self) {
        self.steps.store(0, Ordering::Relaxed);
    }

    fn qso(&self, q: &ScriptQso) -> mlua::Result<Value> {
        let (y, mo, d, h, mi, s) = q.time;
        self.make.call((
            q.call.as_str(),
            q.sent.as_str(),
            q.rcvd.as_str(),
            q.name.as_str(),
            q.note.as_str(),
            self.bands[q.band as usize].clone(),
            self.modes[q.mode as usize].clone(),
            q.srst,
            q.rrst,
            q.tx,
            format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}"),
            q.dupe,
            q.counted,
        ))
    }

    fn unlimited<T>(&self, f: impl FnOnce() -> T) -> T {
        let _ = self.lua.set_memory_limit(0);
        let out = f();
        let _ = self.lua.set_memory_limit(self.memory);
        out
    }

    fn call_add(&self, state: Value, q: &ScriptQso) -> Result<(Value, Judged), String> {
        self.fresh();
        let qso = self.unlimited(|| self.qso(q)).map_err(text)?;
        let called = self.add.call::<(Value, Value)>((state, qso));
        self.unchanged()?;
        let (state, verdict) = called.map_err(text)?;
        let judged = self.unlimited(|| verdict_of(verdict))?;
        Ok((state, judged))
    }

    fn unchanged(&self) -> Result<(), String> {
        for (f, kept) in &self.locals {
            if upvalues(&self.lua, f).map_err(text)? != *kept {
                return Err(
                    "the script changed a chunk-level local variable; keep changing data in the state"
                        .into(),
                );
            }
        }
        Ok(())
    }

    fn walk(&self) -> Result<(), String> {
        self.unlimited(|| walk(&self.state, &self.sealed, |_, _| Ok(())))
    }

    fn copy(&self) -> Result<Value, String> {
        self.unlimited(|| {
            let mut made = HashMap::<usize, Table>::new();
            let mut copy = |v: &Value| -> mlua::Result<Value> {
                let Value::Table(t) = v else {
                    return Ok(v.clone());
                };
                let c = match made.entry(t.to_pointer() as usize) {
                    std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert(self.lua.create_table()?).clone()
                    }
                };
                Ok(Value::Table(c))
            };
            let root = copy(&self.state).map_err(text)?;
            walk(&self.state, &self.sealed, |t, pairs| {
                let Value::Table(c) = copy(&Value::Table(t.clone()))? else {
                    return Ok(());
                };
                for (k, v) in pairs {
                    c.raw_set(copy(k)?, copy(v)?)?;
                }
                c.set_readonly(t.is_readonly());
                Ok(())
            })?;
            Ok(root)
        })
    }
}

fn upvalues(lua: &Lua, f: &Function) -> mlua::Result<Vec<Value>> {
    let values = unsafe {
        lua.exec_raw::<MultiValue>(f.clone(), |s| {
            let mut n = 1;
            while ffi::lua_checkstack(s, 1) != 0 && !ffi::lua_getupvalue(s, 1, n).is_null() {
                n += 1;
            }
            ffi::lua_remove(s, 1);
        })
    }?;
    Ok(values.into_vec())
}

type Locals = Vec<(Function, Vec<Value>)>;

fn seal(lua: &Lua) -> mlua::Result<(HashSet<usize>, Locals)> {
    let mut seen = HashSet::new();
    let mut locals = Vec::new();
    let mut stack = vec![Value::Table(lua.globals())];
    while let Some(v) = stack.pop() {
        if !seen.insert(v.to_pointer() as usize) {
            continue;
        }
        match &v {
            Value::Table(t) => {
                t.set_readonly(true);
                if let Some(m) = t.metatable() {
                    stack.push(Value::Table(m));
                }
                for pair in t.pairs::<Value, Value>() {
                    let (k, v) = pair?;
                    stack.push(k);
                    stack.push(v);
                }
            }
            Value::Function(f) if f.info().what != "C" => {
                let ups = upvalues(lua, f)?;
                stack.extend(ups.iter().cloned());
                locals.push((f.clone(), ups));
            }
            _ => {}
        }
    }
    Ok((seen, locals))
}

fn plain(v: Value, stack: &mut Vec<Table>) -> Result<(), String> {
    match v {
        Value::Table(t) => stack.push(t),
        Value::Nil | Value::Boolean(_) | Value::Integer(_) => {}
        Value::Number(n) if n.is_finite() => {}
        Value::Number(_) => {
            return Err("the state holds a number that is not finite (inf or nan)".into())
        }
        Value::String(s) if s.to_str().is_ok() => {}
        Value::String(_) => return Err("the state holds text that is not UTF-8".into()),
        _ => return Err("the state may hold only nil, booleans, numbers, text and tables".into()),
    }
    Ok(())
}

fn walk(
    state: &Value,
    sealed: &HashSet<usize>,
    mut each: impl FnMut(&Table, &[(Value, Value)]) -> mlua::Result<()>,
) -> Result<(), String> {
    let mut seen = HashSet::new();
    let mut stack = Vec::new();
    let mut pairs = Vec::new();
    plain(state.clone(), &mut stack)?;
    while let Some(t) = stack.pop() {
        let p = t.to_pointer() as usize;
        if !seen.insert(p) {
            continue;
        }
        if sealed.contains(&p) {
            return Err(
                "the state holds a table reached from a global or chunk-level local; keep only its own tables in the state".into(),
            );
        }
        if t.metatable().is_some() {
            return Err("the state holds a table with a metatable".into());
        }
        if seen.len() > STATE_NODES {
            return Err("the state is too large".into());
        }
        pairs.clear();
        t.for_each::<Value, Value>(|k, v| {
            pairs.push((k, v));
            Ok(())
        })
        .map_err(text)?;
        each(&t, &pairs).map_err(text)?;
        for (k, v) in pairs.drain(..) {
            plain(k, &mut stack)?;
            plain(v, &mut stack)?;
        }
    }
    Ok(())
}

fn integer(v: &Value, what: &str) -> Result<Option<f64>, String> {
    match v {
        Value::Nil => Ok(None),
        Value::Integer(i) => Ok(Some(*i as f64)),
        Value::Number(n) if n.fract() == 0.0 && n.is_finite() => Ok(Some(*n)),
        _ => Err(format!("{what} must be an integer")),
    }
}

fn string(v: Value, what: &str) -> Result<String, String> {
    match v {
        Value::Nil => Ok(String::new()),
        Value::String(s) => s
            .to_str()
            .map(|s| s.to_string())
            .map_err(|_| format!("{what} must be UTF-8 text")),
        _ => Err(format!("{what} must be a string")),
    }
}

fn verdict_of(v: Value) -> Result<Judged, String> {
    let Value::Table(t) = v else {
        return Err("add must return the state and a verdict table".into());
    };
    let invalid = match t.raw_get::<Value>("invalid").map_err(text)? {
        Value::Nil => None,
        r => Some(string(r, "verdict.invalid")?),
    };
    let score =
        integer(&t.raw_get::<Value>("score").map_err(text)?, "verdict.score")?.unwrap_or(0.0);
    if !(i32::MIN as f64..=i32::MAX as f64).contains(&score) {
        return Err("verdict.score is out of range".into());
    }
    let mut multis = Vec::with_capacity(MULTIS);
    for key in ["mul1", "mul2"] {
        let m = string(t.raw_get::<Value>(key).map_err(text)?, key)?;
        if m.chars().any(char::is_control) {
            return Err(format!("verdict.{key} must not hold control characters"));
        }
        multis.push(m);
    }
    Ok(Judged {
        points: score as i32,
        multis,
        invalid,
    })
}

fn rank(v: &Value) -> u8 {
    match v {
        Value::Boolean(_) => 0,
        Value::Integer(_) | Value::Number(_) => 1,
        Value::String(_) => 2,
        _ => 3,
    }
}

fn number(v: &Value) -> f64 {
    match v {
        Value::Integer(i) => *i as f64,
        Value::Number(n) => *n,
        _ => 0.0,
    }
}

fn key_order(a: &Value, b: &Value) -> std::cmp::Ordering {
    rank(a).cmp(&rank(b)).then_with(|| match (a, b) {
        (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),
        (Value::String(a), Value::String(b)) => a.as_bytes().cmp(&b.as_bytes()),
        (Value::Table(_), _) | (_, Value::Table(_)) => std::cmp::Ordering::Equal,
        (a, b) => number(a).total_cmp(&number(b)),
    })
}

struct Encoder {
    ids: HashMap<usize, usize>,
    order: Vec<Table>,
}

impl Encoder {
    fn value(&mut self, v: &Value) -> Result<Json, String> {
        Ok(match v {
            Value::Nil => Json::Null,
            Value::Boolean(b) => Json::Bool(*b),
            Value::Integer(i) => json!(*i),
            Value::Number(n) => serde_json::Number::from_f64(*n)
                .map(Json::Number)
                .ok_or("the state holds a number that is not finite (inf or nan)")?,
            Value::String(s) => Json::String(
                s.to_str()
                    .map_err(|_| "the state holds text that is not UTF-8")?
                    .to_string(),
            ),
            Value::Table(t) => {
                let next = self.order.len();
                let id = *self.ids.entry(t.to_pointer() as usize).or_insert(next);
                if id == next {
                    self.order.push(t.clone());
                }
                json!({ "r": id })
            }
            _ => {
                return Err(
                    "the state may hold only nil, booleans, numbers, text and tables".into(),
                )
            }
        })
    }
}

fn encode_state(state: &Value) -> Result<Json, String> {
    let mut e = Encoder {
        ids: HashMap::new(),
        order: Vec::new(),
    };
    let root = e.value(state)?;
    let mut tables = Vec::new();
    while let Some(t) = e.order.get(tables.len()).cloned() {
        let mut pairs = t
            .pairs::<Value, Value>()
            .collect::<mlua::Result<Vec<_>>>()
            .map_err(text)?;
        pairs.sort_by(|a, b| key_order(&a.0, &b.0));
        let mut out = Vec::with_capacity(pairs.len());
        for (k, v) in &pairs {
            out.push(Json::Array(vec![e.value(k)?, e.value(v)?]));
        }
        tables.push(Json::Array(out));
    }
    Ok(json!({ "root": root, "tables": tables }))
}

fn decode(lua: &Lua, j: &Json) -> Result<Value, String> {
    let bad = || "bad state".to_string();
    let tables = j.get("tables").and_then(Json::as_array).ok_or_else(bad)?;
    if tables.len() > STATE_NODES {
        return Err("the state is too large".into());
    }
    let made = tables
        .iter()
        .map(|_| lua.create_table())
        .collect::<mlua::Result<Vec<_>>>()
        .map_err(text)?;
    let value = |j: &Json| -> Result<Value, String> {
        Ok(match j {
            Json::Null => Value::Nil,
            Json::Bool(b) => Value::Boolean(*b),
            Json::Number(n) => Value::Number(n.as_f64().ok_or_else(bad)?),
            Json::String(s) => Value::String(lua.create_string(s).map_err(text)?),
            Json::Object(m) => {
                let r = m.get("r").and_then(Json::as_u64).ok_or_else(bad)?;
                let t = usize::try_from(r).ok().and_then(|r| made.get(r));
                Value::Table(t.ok_or("bad reference")?.clone())
            }
            Json::Array(_) => return Err(bad()),
        })
    };
    for (t, pairs) in made.iter().zip(tables) {
        for p in pairs.as_array().ok_or_else(bad)? {
            let kv = p.as_array().filter(|a| a.len() == 2).ok_or("bad pair")?;
            let k = value(&kv[0])?;
            if k.is_nil() {
                return Err("bad pair".into());
            }
            t.raw_set(k, value(&kv[1])?).map_err(text)?;
        }
    }
    value(j.get("root").ok_or_else(bad)?)
}

impl LuaVm {
    fn call_total(&self, sums: Sums) -> Result<u64, String> {
        self.fresh();
        let t = self
            .unlimited(|| {
                let t = self.lua.create_table()?;
                t.raw_set("score", sums.points)?;
                t.raw_set("mults", sums.multis)?;
                t.raw_set("qsos", sums.qsos)?;
                t.set_readonly(true);
                Ok::<_, mlua::Error>(t)
            })
            .map_err(text)?;
        let called = self.total.call::<Value>((self.state.clone(), t));
        self.unchanged()?;
        let v = called.map_err(text)?;
        match integer(&v, "total")? {
            Some(n) if (0.0..9.0e15).contains(&n) => Ok(n as u64),
            _ => Err("total must return a non-negative integer".into()),
        }
    }
}

impl Vm for LuaVm {
    fn reset(&mut self) -> Result<(), String> {
        self.fresh();
        let called = self.init.call::<Value>(());
        self.unchanged()?;
        self.state = called.map_err(text)?;
        Ok(())
    }

    fn add(&mut self, qso: &ScriptQso) -> Result<Judged, String> {
        let state = std::mem::replace(&mut self.state, Value::Nil);
        let (state, judged) = self.call_add(state, qso)?;
        self.state = state;
        Ok(judged)
    }

    fn check(&mut self, qso: &ScriptQso) -> Result<Judged, String> {
        let copy = self.copy()?;
        Ok(self.call_add(copy, qso)?.1)
    }

    fn total(&mut self, sums: Sums) -> Result<u64, String> {
        let mut open = Vec::new();
        self.unlimited(|| {
            walk(&self.state, &self.sealed, |t, _| {
                if !t.is_readonly() {
                    open.push(t.clone());
                }
                Ok(())
            })
        })?;
        for t in &open {
            t.set_readonly(true);
        }
        let result = self.call_total(sums);
        for t in &open {
            t.set_readonly(false);
        }
        result
    }

    fn verify(&mut self) -> Result<(), String> {
        self.walk()
    }

    fn save(&mut self) -> Result<Json, String> {
        self.walk()?;
        self.unlimited(|| encode_state(&self.state))
    }

    fn restore(&mut self, state: &Json) -> Result<(), String> {
        self.state = self.unlimited(|| decode(&self.lua, state))?;
        Ok(())
    }
}
