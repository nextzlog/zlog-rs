//! The host side of one plugin: its helper process and the traffic with it.
//!
//! [`PluginHost::start`] returns at once; the helper is spawned and the plugin
//! launched on a thread of its own, so nothing in zLog RS waits for a plugin to
//! start. Messages sent meanwhile wait in order and reach the plugin once it is
//! launched. A helper that exits, breaks the protocol or fails to launch leaves the
//! plugin [`State::Disabled`]; the log is never touched by it.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use zlog_core::Qso;

use crate::protocol::{read_frame, write_frame, HookKind, Reply, ToHelper, ToHost, VERSION};
use crate::record::from_record;

/// How long a stopped helper may take to finish before it is killed.
pub const FINISH_GRACE: Duration = Duration::from_secs(1);

/// The streams to a running helper.
pub struct Connection {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child>,
}

/// A running helper that can be killed.
pub trait Child: Send {
    fn kill(&mut self);
}

impl Child for std::process::Child {
    fn kill(&mut self) {
        let _ = std::process::Child::kill(self);
        let _ = self.wait();
    }
}

/// Starts helpers; tests use one that runs a fake helper in a thread.
pub trait Spawner: Send + Sync {
    fn spawn(&self) -> Result<Connection, String>;
}

/// Runs this executable with the helper subcommand ([`crate::SUBCOMMAND`]).
pub struct ProcessSpawner;

impl Spawner for ProcessSpawner {
    fn spawn(&self) -> Result<Connection, String> {
        use std::process::{Command, Stdio};
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut child = Command::new(exe)
            .arg(crate::SUBCOMMAND)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| e.to_string())?;
        let reader = Box::new(child.stdout.take().unwrap());
        let writer = Box::new(child.stdin.take().unwrap());
        Ok(Connection {
            reader,
            writer,
            child: Box::new(child),
        })
    }
}

/// Answers the plugin's queries (`{V}`, `{F}`, ...) from what the host last saw.
pub type Answerer = Arc<dyn Fn(&str) -> String + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Starting,
    Ready,
    /// Not running, with the reason (failed to load, crashed, not responding).
    Disabled(String),
    /// Stopped on purpose.
    Stopped,
}

/// What a plugin asked of zLog through its callbacks.
#[derive(Debug, Clone, PartialEq)]
pub enum FromPlugin {
    Insert { depth: u8, qso: Qso },
    Delete { depth: u8, qso: Qso },
    Update { depth: u8, qso: Qso },
    Commit { qso: Qso, id: i32, mask: u64 },
    Dialog(String),
    Notify(String),
    Unsupported(String),
}

/// A component the plugin hooked (`zylo_allow_button`, `zylo_allow_editor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hook {
    pub kind: HookKind,
    pub name: String,
    pub id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The plugin is not running.
    Unavailable,
    /// No answer in time; it may still come.
    Timeout,
}

struct Shared {
    state: Mutex<State>,
    pending: Mutex<HashMap<u32, mpsc::Sender<Reply>>>,
    inbox: Mutex<Vec<FromPlugin>>,
    hooks: Mutex<Vec<Hook>>,
    format: Mutex<String>,
    child: Mutex<Option<Box<dyn Child>>>,
}

impl Shared {
    /// Leave `Starting`/`Ready` for `to`; a stop or an earlier failure stays.
    fn fail(&self, to: State) {
        let mut s = self.state.lock().unwrap();
        if matches!(*s, State::Starting | State::Ready) {
            *s = to;
        }
        drop(s);
        self.pending.lock().unwrap().clear();
    }

    fn kill(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            c.kill();
        }
    }
}

/// One plugin, run in a helper.
pub struct PluginHost {
    name: String,
    shared: Arc<Shared>,
    tx: mpsc::Sender<ToHelper>,
    seq: AtomicU32,
}

impl PluginHost {
    /// Start the plugin at `path` in a helper; returns at once, in
    /// [`State::Starting`].
    pub fn start(
        name: &str,
        path: &str,
        spawner: Arc<dyn Spawner>,
        answer: Answerer,
    ) -> Arc<PluginHost> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::Starting),
            pending: Mutex::new(HashMap::new()),
            inbox: Mutex::new(Vec::new()),
            hooks: Mutex::new(Vec::new()),
            format: Mutex::new(String::new()),
            child: Mutex::new(None),
        });
        let (tx, rx) = mpsc::channel::<ToHelper>();
        let host = Arc::new(PluginHost {
            name: name.to_string(),
            shared: shared.clone(),
            tx: tx.clone(),
            seq: AtomicU32::new(0),
        });
        let path = path.to_string();
        std::thread::spawn(move || {
            let conn = match spawner.spawn() {
                Ok(c) => c,
                Err(e) => return shared.fail(State::Disabled(e)),
            };
            let Connection {
                reader,
                mut writer,
                child,
            } = conn;
            *shared.child.lock().unwrap() = Some(child);
            if *shared.state.lock().unwrap() == State::Stopped {
                return shared.kill();
            }
            let reading = shared.clone();
            std::thread::spawn(move || read_loop(reader, reading, tx, answer));
            let first = [
                ToHelper::Hello { version: VERSION },
                ToHelper::Launch { path },
            ];
            for m in first.into_iter().chain(rx.iter()) {
                let finish = m == ToHelper::Finish;
                if write_frame(&mut writer, &m.encode()).is_err() || finish {
                    break;
                }
            }
            // Dropping the writer closes the helper's input.
        });
        host
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn state(&self) -> State {
        self.shared.state.lock().unwrap().clone()
    }

    fn usable(&self) -> bool {
        matches!(self.state(), State::Starting | State::Ready)
    }

    /// Send a one-way message; queued until the plugin is launched, dropped when it
    /// is not running.
    pub fn send(&self, m: ToHelper) {
        if self.usable() {
            let _ = self.tx.send(m);
        }
    }

    /// Send a request; its answer arrives on the returned channel.
    pub fn request(&self, make: impl FnOnce(u32) -> ToHelper) -> Option<mpsc::Receiver<Reply>> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        // Registered before the check: a helper failing in between either is seen
        // here or clears the entry, and the receiver then fails at once instead of
        // waiting out its timeout.
        self.shared.pending.lock().unwrap().insert(seq, tx);
        if !self.usable() {
            self.shared.pending.lock().unwrap().remove(&seq);
            return None;
        }
        let _ = self.tx.send(make(seq));
        Some(rx)
    }

    /// Send a request and wait for its answer.
    pub fn call(
        &self,
        make: impl FnOnce(u32) -> ToHelper,
        timeout: Duration,
    ) -> Result<Reply, CallError> {
        let rx = self.request(make).ok_or(CallError::Unavailable)?;
        match rx.recv_timeout(timeout) {
            Ok(r) => Ok(r),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(CallError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(CallError::Unavailable),
        }
    }

    /// Wait until the plugin is launched or has failed; true when ready.
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            match self.state() {
                State::Ready => return true,
                State::Starting if Instant::now() < end => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => return false,
            }
        }
    }

    /// What the plugin asked for since the last call.
    pub fn take_inbox(&self) -> Vec<FromPlugin> {
        std::mem::take(&mut *self.shared.inbox.lock().unwrap())
    }

    pub fn hooks(&self) -> Vec<Hook> {
        self.shared.hooks.lock().unwrap().clone()
    }

    /// The file-dialog filter of `zylo_query_format`, empty for none.
    pub fn format(&self) -> String {
        self.shared.format.lock().unwrap().clone()
    }

    /// Finish the plugin: `zylo_finish_event`, then the helper is killed after
    /// [`FINISH_GRACE`] if it has not ended. Nothing waits for it.
    pub fn stop(&self) {
        let was_running = self.usable();
        *self.shared.state.lock().unwrap() = State::Stopped;
        self.shared.pending.lock().unwrap().clear();
        if was_running {
            let _ = self.tx.send(ToHelper::Finish);
        }
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            std::thread::sleep(FINISH_GRACE);
            shared.kill();
        });
    }

    /// Stop a plugin that misbehaves (not responding) at once.
    pub fn disable(&self, reason: &str) {
        self.shared.fail(State::Disabled(reason.to_string()));
        self.shared.kill();
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        if self.usable() {
            self.stop();
        }
    }
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    shared: Arc<Shared>,
    tx: mpsc::Sender<ToHelper>,
    answer: Answerer,
) {
    let mut hello = false;
    let why = loop {
        let frame = match read_frame(&mut reader) {
            Ok(Some(f)) => f,
            Ok(None) => break "the helper stopped".to_string(),
            Err(e) => break format!("the helper failed: {e}"),
        };
        let m = match ToHost::decode(&frame) {
            Ok(m) => m,
            Err(e) => break format!("the helper failed: {e}"),
        };
        if !hello {
            match m {
                ToHost::Hello { version } if version == VERSION => {
                    hello = true;
                    continue;
                }
                _ => break "the helper speaks another protocol version".to_string(),
            }
        }
        let push = |f: FromPlugin| shared.inbox.lock().unwrap().push(f);
        match m {
            ToHost::Hello { .. } => {}
            ToHost::Launched { error, format } => {
                if !error.is_empty() {
                    break error;
                }
                *shared.format.lock().unwrap() = format;
                let mut s = shared.state.lock().unwrap();
                if *s == State::Starting {
                    *s = State::Ready;
                }
            }
            ToHost::Insert { depth, record } => push(FromPlugin::Insert {
                depth,
                qso: from_record(&record),
            }),
            ToHost::Delete { depth, record } => push(FromPlugin::Delete {
                depth,
                qso: from_record(&record),
            }),
            ToHost::Update { depth, record } => push(FromPlugin::Update {
                depth,
                qso: from_record(&record),
            }),
            ToHost::Commit {
                record, id, mask, ..
            } => push(FromPlugin::Commit {
                qso: from_record(&record),
                id,
                mask,
            }),
            ToHost::Dialog { text } => push(FromPlugin::Dialog(text)),
            ToHost::Notify { text } => push(FromPlugin::Notify(text)),
            ToHost::Unsupported { what } => push(FromPlugin::Unsupported(what)),
            ToHost::Query { seq, text } => {
                let _ = tx.send(ToHelper::Answer {
                    seq,
                    text: answer(&text),
                });
            }
            ToHost::Hook { kind, name, id } => {
                shared.hooks.lock().unwrap().push(Hook { kind, name, id })
            }
            ToHost::Reply { seq, reply } => {
                if let Some(w) = shared.pending.lock().unwrap().remove(&seq) {
                    let _ = w.send(reply);
                }
            }
        }
    };
    shared.fail(State::Disabled(why));
    shared.kill();
}

#[cfg(test)]
pub mod fake {
    //! A fake helper: a thread speaking the protocol, scripted by a closure.

    use super::*;

    /// What the fake does with each message after launch; it may answer by
    /// returning messages.
    pub type Script = Arc<dyn Fn(&ToHelper) -> Vec<ToHost> + Send + Sync>;

    struct Killed(Arc<Mutex<bool>>);

    impl Child for Killed {
        fn kill(&mut self) {
            *self.0.lock().unwrap() = true;
        }
    }

    pub struct FakeSpawner {
        pub script: Script,
        /// Delay before the answer to `Launch`.
        pub launch_delay: Duration,
        pub launch_error: String,
        pub format: String,
        /// Every message the fake received, in order.
        pub seen: Arc<Mutex<Vec<ToHelper>>>,
        pub killed: Arc<Mutex<bool>>,
    }

    impl FakeSpawner {
        pub fn new(script: impl Fn(&ToHelper) -> Vec<ToHost> + Send + Sync + 'static) -> Self {
            FakeSpawner {
                script: Arc::new(script),
                launch_delay: Duration::ZERO,
                launch_error: String::new(),
                format: String::new(),
                seen: Arc::new(Mutex::new(Vec::new())),
                killed: Arc::new(Mutex::new(false)),
            }
        }
    }

    impl Spawner for FakeSpawner {
        fn spawn(&self) -> Result<Connection, String> {
            let (host_r, mut fake_w) = std::io::pipe().map_err(|e| e.to_string())?;
            let (mut fake_r, host_w) = std::io::pipe().map_err(|e| e.to_string())?;
            let (script, delay, error, format, seen, killed) = (
                self.script.clone(),
                self.launch_delay,
                self.launch_error.clone(),
                self.format.clone(),
                self.seen.clone(),
                self.killed.clone(),
            );
            std::thread::spawn(move || {
                let mut send = |m: ToHost| write_frame(&mut fake_w, &m.encode()).is_ok();
                send(ToHost::Hello { version: VERSION });
                while let Ok(Some(f)) = read_frame(&mut fake_r) {
                    if *killed.lock().unwrap() {
                        return;
                    }
                    let m = ToHelper::decode(&f).unwrap();
                    seen.lock().unwrap().push(m.clone());
                    let out = match &m {
                        ToHelper::Hello { .. } => vec![],
                        ToHelper::Launch { .. } => {
                            std::thread::sleep(delay);
                            vec![ToHost::Launched {
                                error: error.clone(),
                                format: format.clone(),
                            }]
                        }
                        ToHelper::Finish => return,
                        m => script(m),
                    };
                    for o in out {
                        if !send(o) {
                            return;
                        }
                    }
                }
            });
            Ok(Connection {
                reader: Box::new(host_r),
                writer: Box::new(host_w),
                child: Box::new(Killed(self.killed.clone())),
            })
        }
    }

    pub fn answerer() -> Answerer {
        Arc::new(|q: &str| q.replace("{C}", "JA1ZLO"))
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    fn wait_until(mut f: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !f() {
            assert!(Instant::now() < end, "timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn events_sent_while_starting_arrive_in_order_after_launch() {
        let mut s = FakeSpawner::new(|_| vec![]);
        s.launch_delay = Duration::from_millis(100);
        let seen = s.seen.clone();
        let h = PluginHost::start("a.dll", "a.dll", Arc::new(s), answerer());
        assert_eq!(h.state(), State::Starting);
        h.send(ToHelper::Offset { minutes: -540 });
        h.send(ToHelper::Button { id: 1 });
        assert!(h.wait_ready(Duration::from_secs(5)));
        wait_until(|| seen.lock().unwrap().len() == 4);
        let seen = seen.lock().unwrap();
        assert!(matches!(seen[1], ToHelper::Launch { .. }));
        assert_eq!(seen[2], ToHelper::Offset { minutes: -540 });
        assert_eq!(seen[3], ToHelper::Button { id: 1 });
    }

    #[test]
    fn requests_are_answered_and_queries_served_from_the_answerer() {
        let s = FakeSpawner::new(|m| match m {
            ToHelper::Points { seq, points, .. } => vec![
                ToHost::Query {
                    seq: 0,
                    text: "{C}".into(),
                },
                ToHost::Reply {
                    seq: *seq,
                    reply: Reply::Int(points * 2),
                },
            ],
            _ => vec![],
        });
        let seen = s.seen.clone();
        let h = PluginHost::start("a", "a", Arc::new(s), answerer());
        let r = h.call(
            |seq| ToHelper::Points {
                seq,
                points: 21,
                multis: 1,
            },
            Duration::from_secs(5),
        );
        assert_eq!(r, Ok(Reply::Int(42)));
        wait_until(|| {
            seen.lock().unwrap().iter().any(|m| {
                *m == ToHelper::Answer {
                    seq: 0,
                    text: "JA1ZLO".into(),
                }
            })
        });
    }

    #[test]
    fn a_failed_launch_or_a_crash_disables_the_plugin() {
        let mut s = FakeSpawner::new(|_| vec![]);
        s.launch_error = "missing zylo_verify_event".into();
        let h = PluginHost::start("a", "a", Arc::new(s), answerer());
        assert!(!h.wait_ready(Duration::from_secs(5)));
        assert_eq!(
            h.state(),
            State::Disabled("missing zylo_verify_event".into())
        );
        // A helper that dies on the first event.
        struct Dies;
        impl Spawner for Dies {
            fn spawn(&self) -> Result<Connection, String> {
                let s = FakeSpawner::new(|_| panic!("crash"));
                s.spawn()
            }
        }
        let h = PluginHost::start("b", "b", Arc::new(Dies), answerer());
        assert!(h.wait_ready(Duration::from_secs(5)));
        h.send(ToHelper::Button { id: 1 });
        wait_until(|| matches!(h.state(), State::Disabled(_)));
        assert_eq!(
            h.call(|seq| ToHelper::Cities { seq }, Duration::from_secs(1)),
            Err(CallError::Unavailable)
        );
    }

    #[test]
    fn callbacks_land_in_the_inbox_and_hooks_are_kept() {
        let s = FakeSpawner::new(|m| match m {
            ToHelper::Button { .. } => vec![
                ToHost::Notify { text: "hi".into() },
                ToHost::Hook {
                    kind: HookKind::Button,
                    name: "MainForm.X".into(),
                    id: 3,
                },
            ],
            _ => vec![],
        });
        let h = PluginHost::start("a", "a", Arc::new(s), answerer());
        h.send(ToHelper::Button { id: 1 });
        wait_until(|| !h.hooks().is_empty());
        assert_eq!(h.take_inbox(), vec![FromPlugin::Notify("hi".into())]);
        assert_eq!(h.hooks()[0].id, 3);
    }

    #[test]
    fn stop_finishes_then_kills() {
        let s = FakeSpawner::new(|_| vec![]);
        let (seen, killed) = (s.seen.clone(), s.killed.clone());
        let h = PluginHost::start("a", "a", Arc::new(s), answerer());
        assert!(h.wait_ready(Duration::from_secs(5)));
        h.stop();
        assert_eq!(h.state(), State::Stopped);
        wait_until(|| seen.lock().unwrap().last() == Some(&ToHelper::Finish));
        wait_until(|| *killed.lock().unwrap());
    }
}
