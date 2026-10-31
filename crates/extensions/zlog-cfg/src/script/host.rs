use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zlog_core::{Band, Mode};

use super::{
    same, Inputs, Judged, Limits, ScoreScript, ScriptEngine, ScriptProgram, ScriptQso, Sums, Vm,
};

pub const VERSION: u32 = 1;

pub const SUBCOMMAND: &str = "script-host";

pub const SNAPSHOT_EVERY: usize = 100;

const WAIT: Duration = Duration::from_millis(25);

const WAIT_CHECK: Duration = Duration::from_millis(10);

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

const OS_MEMORY: u64 = 128 << 20;

const CRASHES: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    InProcess,
    Child { program: PathBuf, args: Vec<String> },
}

static PLACEMENT: RwLock<Placement> = RwLock::new(Placement::InProcess);

static GENERATION: AtomicU64 = AtomicU64::new(0);

static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn child_ids() -> Vec<u32> {
    zlog_process::running()
}

static PATIENCE_MS: AtomicU64 = AtomicU64::new(0);

pub fn patience(wait: Duration) {
    PATIENCE_MS.store(wait.as_millis() as u64, Ordering::SeqCst);
}

fn waited(default: Duration) -> Duration {
    match PATIENCE_MS.load(Ordering::SeqCst) {
        0 => default,
        ms => Duration::from_millis(ms),
    }
}

pub fn place(placement: Placement) {
    *PLACEMENT.write().unwrap_or_else(|e| e.into_inner()) = placement;
}

pub fn placement() -> Placement {
    PLACEMENT.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

pub fn take_notices() -> Vec<String> {
    std::mem::take(&mut *NOTICES.lock().unwrap_or_else(|e| e.into_inner()))
}

fn bump() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

type Time = (i32, u32, u32, u32, u32, u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireQso(
    String,
    String,
    String,
    String,
    String,
    u8,
    u8,
    u16,
    u16,
    u8,
    Time,
    bool,
    bool,
);

impl WireQso {
    fn of(q: &ScriptQso) -> Self {
        WireQso(
            q.call.clone(),
            q.sent.clone(),
            q.rcvd.clone(),
            q.name.clone(),
            q.note.clone(),
            q.band as u8,
            q.mode as u8,
            q.srst,
            q.rrst,
            q.tx,
            q.time,
            q.dupe,
            q.counted,
        )
    }

    fn qso(&self) -> Result<ScriptQso, String> {
        Ok(ScriptQso {
            call: self.0.clone(),
            sent: self.1.clone(),
            rcvd: self.2.clone(),
            name: self.3.clone(),
            note: self.4.clone(),
            band: Band::from_byte(self.5).ok_or("bad band")?,
            mode: Mode::from_byte(self.6).ok_or("bad mode")?,
            srst: self.7,
            rrst: self.8,
            tx: self.9,
            time: self.10,
            dupe: self.11,
            counted: self.12,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireJudged(i32, Vec<String>, Option<String>);

impl WireJudged {
    fn of(j: Judged) -> Self {
        WireJudged(j.points, j.multis, j.invalid)
    }

    fn judged(self) -> Judged {
        Judged {
            points: self.0,
            multis: self.1,
            invalid: self.2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    Load {
        version: u32,
        language: String,
        api: u32,
        name: String,
        source: String,
    },
    Reset,
    Restore(Value),
    Add(Vec<WireQso>, bool),
    Check(WireQso),
    Total((u32, u32, u32)),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Hello(u32),
    Ok,
    Judged(Vec<WireJudged>, Option<Value>),
    One(WireJudged),
    Score(u64),
    Error(String),
}

fn compile(language: &str, name: &str, source: &str, api: u32) -> Result<Box<dyn Vm>, String> {
    let engine = super::engine(language).ok_or_else(|| format!("no engine for {language}"))?;
    engine.compile(name, source, api, &Limits::default())
}

fn answer(vm: &mut Option<Box<dyn Vm>>, request: Request) -> Response {
    let result = match (request, vm.as_mut()) {
        (
            Request::Load {
                version,
                language,
                api,
                name,
                source,
            },
            _,
        ) => {
            if version != VERSION {
                Err(format!("protocol {version} is not {VERSION}"))
            } else {
                compile(&language, &name, &source, api).map(|v| {
                    *vm = Some(v);
                    Response::Ok
                })
            }
        }
        (_, None) => Err("no script loaded".into()),
        (Request::Reset, Some(vm)) => vm.reset().map(|_| Response::Ok),
        (Request::Restore(state), Some(vm)) => vm.restore(&state).map(|_| Response::Ok),
        (Request::Add(qsos, save), Some(vm)) => qsos
            .iter()
            .map(|q| vm.add(&q.qso()?).map(WireJudged::of))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|v| {
                let state = if save {
                    Some(vm.save()?)
                } else {
                    vm.verify()?;
                    None
                };
                Ok(Response::Judged(v, state))
            }),
        (Request::Check(q), Some(vm)) => q
            .qso()
            .and_then(|q| vm.check(&q))
            .map(|j| Response::One(WireJudged::of(j))),
        (Request::Total((qsos, points, multis)), Some(vm)) => vm
            .total(Sums {
                qsos,
                points,
                multis,
            })
            .map(Response::Score),
    };
    result.unwrap_or_else(Response::Error)
}

fn hello() -> String {
    serde_json::to_string(&Response::Hello(VERSION)).unwrap_or_default()
}

pub fn serve(input: impl BufRead, mut output: impl Write) {
    let _ = writeln!(output, "{}", hello());
    let _ = output.flush();
    let mut vm = None;
    for line in input.lines() {
        let Ok(line) = line else {
            break;
        };
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(r) => answer(&mut vm, r),
            Err(e) => Response::Error(format!("bad request: {e}")),
        };
        let text =
            serde_json::to_string(&response).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
        if writeln!(output, "{text}")
            .and_then(|_| output.flush())
            .is_err()
        {
            break;
        }
    }
}

pub fn serve_stdio() {
    serve(std::io::stdin().lock(), std::io::stdout().lock());
}

enum Fail {
    Script(String),
    Crash(String),
}

enum Peer {
    Local(Option<Box<dyn Vm>>),
    Remote(zlog_process::Helper),
}

impl Peer {
    fn start(placement: &Placement) -> Result<Peer, String> {
        match placement {
            Placement::InProcess => Ok(Peer::Local(None)),
            Placement::Child { program, args } => {
                zlog_process::Helper::start(&zlog_process::Spec {
                    program: program.clone(),
                    args: args.clone(),
                    memory: OS_MEMORY,
                    timeout: CALL_TIMEOUT,
                    greeting: hello(),
                })
                .map(Peer::Remote)
                .map_err(|e| format!("{}: {e}", program.display()))
            }
        }
    }

    fn call(&mut self, request: Request) -> Result<Response, Fail> {
        let response = match self {
            Peer::Local(vm) => answer(vm, request),
            Peer::Remote(helper) => {
                let text =
                    serde_json::to_string(&request).map_err(|e| Fail::Script(e.to_string()))?;
                let line = helper
                    .request(&text)
                    .map_err(|e| Fail::Crash(e.to_string()))?;
                serde_json::from_str(&line).map_err(|e| Fail::Crash(format!("bad answer: {e}")))?
            }
        };
        match response {
            Response::Error(e) => Err(Fail::Script(e)),
            r => Ok(r),
        }
    }
}

fn unexpected(r: Response) -> Fail {
    Fail::Crash(format!("unexpected answer {r:?}"))
}

#[derive(Default)]
struct Inner {
    want: Option<Inputs>,
    want_total: Option<Sums>,
    want_check: Option<ScriptQso>,
    done: Option<(Inputs, Arc<Vec<Judged>>)>,
    total_done: Option<(Inputs, Sums, u64)>,
    check_done: Option<(ScriptQso, Option<Judged>)>,
    disabled: Option<String>,
    late: bool,
    quit: bool,
}

#[derive(Default)]
struct Shared {
    inner: Mutex<Inner>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait_for<T>(
        &self,
        timeout: Duration,
        mut ready: impl FnMut(&mut Inner) -> Option<T>,
    ) -> Option<T> {
        let mut g = self.lock();
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if g.disabled.is_some() {
                return None;
            }
            if let Some(v) = ready(&mut g) {
                return Some(v);
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                g.late = true;
                return None;
            }
            g = self
                .changed
                .wait_timeout(g, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

struct Program {
    language: String,
    api: u32,
    name: String,
    source: String,
}

pub fn program(
    engine: Arc<dyn ScriptEngine>,
    name: &str,
    source: &str,
    api: u32,
) -> Arc<dyn ScriptProgram> {
    Arc::new(Program {
        language: engine.language().to_string(),
        api,
        name: name.to_string(),
        source: source.to_string(),
    })
}

impl ScriptProgram for Program {
    fn start(&self) -> Box<dyn ScoreScript> {
        let shared = Arc::new(Shared::default());
        let mut worker = Worker {
            shared: shared.clone(),
            load: Request::Load {
                version: VERSION,
                language: self.language.clone(),
                api: self.api,
                name: self.name.clone(),
                source: self.source.clone(),
            },
            placement: placement(),
            peer: None,
            applied: Vec::new(),
            verdicts: Arc::default(),
            snapshots: Vec::new(),
            restarts: zlog_process::Restarts::new(CRASHES - 1),
        };
        std::thread::spawn(move || worker.run());
        Box::new(Session { shared })
    }
}

struct Session {
    shared: Arc<Shared>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.lock().quit = true;
        self.shared.changed.notify_all();
    }
}

impl ScoreScript for Session {
    fn judge(&self, qsos: &Inputs) -> Option<Arc<Vec<Judged>>> {
        {
            let mut g = self.shared.lock();
            if g.disabled.is_some() {
                return None;
            }
            if let Some((i, v)) = &g.done {
                if same(i, qsos) {
                    return Some(v.clone());
                }
            }
            if g.want.as_ref().is_none_or(|w| !same(w, qsos)) {
                g.want = Some(qsos.clone());
                g.done = None;
                g.total_done = None;
            }
        }
        self.shared.changed.notify_all();
        self.shared.wait_for(waited(WAIT), |g| match &g.done {
            Some((i, v)) if same(i, qsos) => Some(v.clone()),
            _ => None,
        })
    }

    fn total(&self, qsos: &Inputs, sums: Sums) -> Option<u64> {
        {
            let mut g = self.shared.lock();
            if g.disabled.is_some() {
                return None;
            }
            if let Some((i, s, t)) = &g.total_done {
                if *s == sums && same(i, qsos) {
                    return Some(*t);
                }
            }
            if !g.done.as_ref().is_some_and(|(i, _)| same(i, qsos)) {
                g.late = true;
                return None;
            }
            g.want_total = Some(sums);
        }
        self.shared.changed.notify_all();
        self.shared.wait_for(waited(WAIT), |g| match &g.total_done {
            Some((i, s, t)) if *s == sums && same(i, qsos) => Some(*t),
            _ => None,
        })
    }

    fn check(&self, qso: &ScriptQso) -> Option<Judged> {
        {
            let mut g = self.shared.lock();
            if g.disabled.is_some() || g.done.is_none() {
                return None;
            }
            if let Some((q, j)) = &g.check_done {
                if q == qso {
                    return j.clone();
                }
            }
            g.want_check = Some(qso.clone());
        }
        self.shared.changed.notify_all();
        self.shared
            .wait_for(waited(WAIT_CHECK), |g| match &g.check_done {
                Some((q, j)) if q == qso => Some(j.clone()),
                _ => None,
            })
            .flatten()
    }
}

enum Job {
    Judge(Inputs),
    Total(Inputs, Sums),
    Check(ScriptQso),
}

struct Worker {
    shared: Arc<Shared>,
    load: Request,
    placement: Placement,
    peer: Option<Peer>,
    applied: Vec<Arc<ScriptQso>>,
    verdicts: Arc<Vec<Judged>>,
    snapshots: Vec<(usize, Value)>,
    restarts: zlog_process::Restarts,
}

impl Worker {
    fn next_job(&self) -> Option<Job> {
        let mut g = self.shared.lock();
        loop {
            if g.quit || g.disabled.is_some() {
                return None;
            }
            let done = g.done.as_ref().map(|(i, _)| i.clone());
            if let Some(w) = &g.want {
                if done.as_ref().is_none_or(|d| !same(d, w)) {
                    return Some(Job::Judge(w.clone()));
                }
            }
            if let (Some(s), Some(d)) = (g.want_total, &done) {
                return Some(Job::Total(d.clone(), s));
            }
            if let (Some(q), Some(_)) = (g.want_check.clone(), &done) {
                return Some(Job::Check(q));
            }
            g = self
                .shared
                .changed
                .wait(g)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    fn run(&mut self) {
        let first = self.peer().map(|_| ());
        self.settle(first);
        while let Some(job) = self.next_job() {
            let done = self.work(&job);
            self.settle(done);
        }
    }

    fn settle(&mut self, result: Result<(), Fail>) {
        match result {
            Ok(()) => {}
            Err(Fail::Script(e)) => self.disable(e),
            Err(Fail::Crash(e)) => {
                self.peer = None;
                if !self.restarts.failed() {
                    self.disable(format!("the script host failed {CRASHES} times: {e}"));
                }
            }
        }
    }

    fn disable(&mut self, why: String) {
        self.peer = None;
        NOTICES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("Scoring script stopped for this session: {why}"));
        self.shared.lock().disabled = Some(why);
        self.shared.changed.notify_all();
        bump();
    }

    fn publish(&self, store: impl FnOnce(&mut Inner)) {
        let late = {
            let mut g = self.shared.lock();
            store(&mut g);
            std::mem::take(&mut g.late)
        };
        self.shared.changed.notify_all();
        if late {
            bump();
        }
    }

    fn peer(&mut self) -> Result<&mut Peer, Fail> {
        if self.peer.is_none() {
            let mut peer = Peer::start(&self.placement).map_err(Fail::Crash)?;
            match peer.call(self.load.clone())? {
                Response::Ok => {}
                r => return Err(unexpected(r)),
            }
            let back = self.snapshots.last().cloned();
            match &back {
                Some((_, state)) => peer.call(Request::Restore(state.clone()))?,
                None => peer.call(Request::Reset)?,
            };
            let at = back.map_or(0, |(i, _)| i);
            self.applied.truncate(at);
            Arc::make_mut(&mut self.verdicts).truncate(at);
            self.peer = Some(peer);
            self.restarts.started();
            let mut g = self.shared.lock();
            g.done = None;
            g.total_done = None;
            g.check_done = None;
        }
        self.peer
            .as_mut()
            .ok_or(Fail::Crash("no script host".into()))
    }

    fn work(&mut self, job: &Job) -> Result<(), Fail> {
        match job {
            Job::Judge(target) => {
                if let Some(v) = self.judge(target)? {
                    let target = target.clone();
                    self.publish(|g| {
                        g.done = Some((target, v));
                        g.check_done = None;
                    });
                }
            }
            Job::Total(inputs, sums) => {
                let sums = *sums;
                self.peer()?;
                if self.applied.len() != inputs.len() {
                    self.shared.lock().done = None;
                    return Ok(());
                }
                let r = self
                    .peer()?
                    .call(Request::Total((sums.qsos, sums.points, sums.multis)))?;
                let Response::Score(t) = r else {
                    return Err(unexpected(r));
                };
                let inputs = inputs.clone();
                self.publish(|g| {
                    if g.want_total == Some(sums) {
                        g.want_total = None;
                    }
                    g.total_done = Some((inputs, sums, t));
                });
            }
            Job::Check(q) => {
                self.peer()?;
                if self.shared.lock().done.is_none() {
                    return Ok(());
                }
                let j = match self.peer()?.call(Request::Check(WireQso::of(q))) {
                    Ok(Response::One(j)) => Some(j.judged()),
                    Ok(r) => return Err(unexpected(r)),
                    Err(Fail::Script(_)) => None,
                    Err(e) => return Err(e),
                };
                let q = q.clone();
                self.publish(|g| {
                    if g.want_check.as_ref() == Some(&q) {
                        g.want_check = None;
                    }
                    g.check_done = Some((q, j));
                });
            }
        }
        Ok(())
    }

    fn stale(&self, target: &Inputs) -> bool {
        let g = self.shared.lock();
        g.quit || g.want.as_ref().is_some_and(|w| !Arc::ptr_eq(w, target))
    }

    fn judge(&mut self, target: &Inputs) -> Result<Option<Arc<Vec<Judged>>>, Fail> {
        self.peer()?;
        let same = self
            .applied
            .iter()
            .zip(target.iter())
            .take_while(|(a, b)| Arc::ptr_eq(a, b) || a == b)
            .count();
        if same < self.applied.len() {
            self.snapshots.retain(|(i, _)| *i <= same);
            let back = self.snapshots.last().cloned();
            let peer = self.peer()?;
            match &back {
                Some((_, state)) => peer.call(Request::Restore(state.clone()))?,
                None => peer.call(Request::Reset)?,
            };
            let at = back.map_or(0, |(i, _)| i);
            self.applied.truncate(at);
            Arc::make_mut(&mut self.verdicts).truncate(at);
        }
        while self.applied.len() < target.len() {
            let from = self.applied.len();
            let to = target
                .len()
                .min((from / SNAPSHOT_EVERY + 1) * SNAPSHOT_EVERY);
            let batch = target[from..to].iter().map(|q| WireQso::of(q)).collect();
            let save = to.is_multiple_of(SNAPSHOT_EVERY);
            let r = self.peer()?.call(Request::Add(batch, save))?;
            let Response::Judged(v, state) = r else {
                return Err(unexpected(r));
            };
            if v.len() != to - from {
                return Err(Fail::Crash("wrong number of verdicts".into()));
            }
            self.applied.extend_from_slice(&target[from..to]);
            Arc::make_mut(&mut self.verdicts).extend(v.into_iter().map(WireJudged::judged));
            if let Some(state) = state {
                self.snapshots.push((to, state));
            }
            if self.stale(target) {
                return Ok(None);
            }
        }
        Ok(Some(self.verdicts.clone()))
    }
}

#[cfg(feature = "window")]
pub fn register(b: &mut zlog_ui::Builder) {
    b.subcommand(SUBCOMMAND, serve_stdio);
    if let Ok(program) = std::env::current_exe() {
        place(Placement::Child {
            program,
            args: vec![SUBCOMMAND.to_string()],
        });
    }
}
