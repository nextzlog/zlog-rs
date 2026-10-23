//! The link between the book and the plugins: changes of the log go to every
//! running plugin as insert and delete events (`zyloLogUpdated`), and what the
//! plugins ask for comes back as queued commands from [`Link::poll`].
//!
//! Rules (zLog's, and where they differ):
//!
//! - Every change reaches every plugin, whoever made it (the operator, the network,
//!   a plugin), as zLog does; an edit is a delete of the old QSO and an insert of
//!   the new one. Sorting is no event, as in zLog.
//! - A plugin never changes the log during a call: its insert, delete, update and
//!   commit callbacks become [`Command`]s, sent at the next poll. A plugin that
//!   changes the log in reaction to an event of its own change (todo#132: insert in
//!   `OnInsertEvent`) would loop forever in zLog; here each change carries how many
//!   plugin changes led to it, and from [`MAX_DEPTH`] on the plugins are no longer
//!   told, which ends the chain with a warning.
//! - A commit is not told to any plugin, as zLog suppresses events during it
//!   (`zyloNotifySuppressDepth`).
//! - `zylo_attach_event` makes reiwa-style plugins read the QSOs from `{F}`; that is
//!   a ZLO copy of the log written for them, so they are not sent twice.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use zlog_core::{Command, Context, Format, Link, LogEvent, Observer, Qso, QsoId};

use crate::host::{FromPlugin, PluginHost};
use crate::manager::Manager;
use crate::protocol::ToHelper;
use crate::record::{apply_commit, to_record};
use crate::rules::RulePlugin;
use crate::texts;

/// Plugin changes in a row after which plugins are no longer told of the next.
pub const MAX_DEPTH: u8 = 8;

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;

/// An event expected from a command this link sent.
#[derive(Debug, Clone, Copy)]
struct Expected {
    depth: u8,
    /// Not told to plugins (a commit, or the link's own rescoring).
    quiet: bool,
}

pub struct ZyloLink {
    manager: Arc<Mutex<Manager>>,
    expected: VecDeque<Expected>,
    attached: Vec<Arc<PluginHost>>,
    rule: Option<Arc<RulePlugin>>,
    rule_host: Option<Arc<PluginHost>>,
    contest: String,
    cfg: String,
    open: bool,
    file: Option<PathBuf>,
    warned_loop: bool,
    warned_calc: bool,
    unsupported: HashSet<String>,
}

impl ZyloLink {
    pub fn new(manager: Arc<Mutex<Manager>>) -> Self {
        ZyloLink {
            manager,
            expected: VecDeque::new(),
            attached: Vec::new(),
            rule: None,
            rule_host: None,
            contest: String::new(),
            cfg: String::new(),
            open: false,
            file: None,
            warned_loop: false,
            warned_calc: false,
            unsupported: HashSet::new(),
        }
    }

    fn language(&self) -> String {
        self.manager.lock().unwrap().language.clone()
    }

    /// Write the ZLO copy of the log the plugins read as `{F}`.
    fn write_file(&mut self, cx: &Context<'_>) {
        let path = self.file.get_or_insert_with(|| {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::env::temp_dir().join(format!("zlog-zylo-{}-{n}.zlo", std::process::id()))
        });
        if std::fs::write(&*path, cx.log().to_bytes(Format::Zlo)).is_ok() {
            let snapshot = self.manager.lock().unwrap().snapshot();
            snapshot.lock().unwrap().file = path.display().to_string();
        }
    }

    /// Attach plugins that run but have not seen this contest (`TDLL.ContestOpened`,
    /// and `AssignEvent` for the rule plugin).
    fn attach_new(&mut self, cx: &Context<'_>) {
        if !self.open {
            return;
        }
        let targets = self.manager.lock().unwrap().targets();
        let new: Vec<_> = targets
            .into_iter()
            .filter(|t| !self.attached.iter().any(|a| Arc::ptr_eq(a, t)))
            .collect();
        if new.is_empty() {
            return;
        }
        self.write_file(cx);
        // zLog's `UTCOffset` is the bias of Windows: minus the offset from UTC.
        let minutes = -cx.log().utc_offset_minutes().unwrap_or(0);
        for h in new {
            h.send(ToHelper::Offset { minutes });
            h.send(ToHelper::Attach {
                contest: self.contest.clone(),
                cfg: self.cfg.clone(),
            });
            if self.rule_host.as_ref().is_some_and(|r| Arc::ptr_eq(r, &h)) {
                h.send(ToHelper::Assign {
                    contest: self.contest.clone(),
                    cfg: self.cfg.clone(),
                });
            }
            self.attached.push(h);
        }
    }

    /// The depth of the change being reported, or `None` when plugins are not told.
    fn depth_of(&mut self, cx: &mut Context<'_>) -> Option<u8> {
        if !cx.caused_by_me() {
            return Some(0);
        }
        let e = self.expected.pop_front()?;
        if e.quiet {
            return None;
        }
        if e.depth >= MAX_DEPTH {
            if !self.warned_loop {
                self.warned_loop = true;
                cx.say(texts().format(&self.language(), "warn.loop", &[&MAX_DEPTH]));
            }
            return None;
        }
        Some(e.depth)
    }

    fn tell(&self, insert: bool, depth: u8, q: &Qso) {
        let record = to_record(q);
        for h in &self.attached {
            h.send(if insert {
                ToHelper::Insert { depth, record }
            } else {
                ToHelper::Delete { depth, record }
            });
        }
        if let Some(r) = &self.rule {
            r.invalidate();
        }
    }
}

impl Observer for ZyloLink {
    fn name(&self) -> &str {
        "ZyLO"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        match event {
            LogEvent::Opened { contest } => {
                let slot = {
                    let mut m = self.manager.lock().unwrap();
                    m.start_enabled();
                    m.select_rule(contest)
                };
                (self.contest, self.cfg) = match &slot {
                    Some(s) => (s.contest.clone(), s.cfg.clone()),
                    None => (contest.clone(), String::new()),
                };
                self.rule = slot.as_ref().map(|s| s.rule.clone());
                self.rule_host = slot.map(|s| s.host);
                self.open = true;
                self.attach_new(cx);
            }
            LogEvent::Inserted { index } => {
                if let Some(d) = self.depth_of(cx) {
                    let q = cx.log().qsos[*index].clone();
                    self.tell(true, d, &q);
                }
            }
            LogEvent::Updated { index, before } => {
                if let Some(d) = self.depth_of(cx) {
                    let q = cx.log().qsos[*index].clone();
                    self.tell(false, d, before);
                    self.tell(true, d, &q);
                }
            }
            LogEvent::Deleted { qso, .. } => {
                if let Some(d) = self.depth_of(cx) {
                    self.tell(false, d, qso);
                }
            }
            LogEvent::Sorted | LogEvent::Resynced => {}
            LogEvent::Closed => {
                for h in self.attached.drain(..) {
                    h.send(ToHelper::Detach {
                        contest: self.contest.clone(),
                        cfg: self.cfg.clone(),
                    });
                }
                if let Some(r) = self.rule.take() {
                    self.manager.lock().unwrap().release_rule(&r);
                }
                self.rule_host = None;
                self.open = false;
                if let Some(f) = self.file.take() {
                    let _ = std::fs::remove_file(f);
                }
            }
        }
    }
}

impl Link for ZyloLink {
    fn poll(&mut self, cx: &mut Context<'_>) {
        // Events of the commands sent at the last poll have all been delivered.
        self.expected.clear();
        let (messages, lang) = {
            let mut m = self.manager.lock().unwrap();
            m.watch();
            (m.take_messages(), m.language.clone())
        };
        for msg in messages {
            cx.say(msg);
        }
        self.attach_new(cx);
        let mut ids: Option<HashSet<QsoId>> = None;
        let attached = self.attached.clone();
        for h in &attached {
            for f in h.take_inbox() {
                let ids = ids.get_or_insert_with(|| cx.log().qsos.iter().map(QsoId::of).collect());
                match f {
                    FromPlugin::Insert { depth, qso } => {
                        ids.insert(QsoId::of(&qso));
                        cx.send(Command::Insert(qso));
                        self.expected.push_back(Expected {
                            depth,
                            quiet: false,
                        });
                    }
                    // A QSO is named by its ID (`Reserve3` without the edit counter).
                    FromPlugin::Delete { depth, qso } => {
                        let id = QsoId::of(&qso);
                        if ids.remove(&id) {
                            cx.send(Command::Delete(id));
                            self.expected.push_back(Expected {
                                depth,
                                quiet: false,
                            });
                        }
                    }
                    FromPlugin::Update { depth, qso } => {
                        let id = QsoId::of(&qso);
                        if ids.contains(&id) {
                            cx.send(Command::Update(id, qso));
                            self.expected.push_back(Expected {
                                depth,
                                quiet: false,
                            });
                        }
                    }
                    FromPlugin::Commit { qso, id, mask } => {
                        let id = QsoId(id);
                        let base = cx.log().qsos.iter().find(|q| QsoId::of(q) == id);
                        if let (Some(base), true) = (base, mask != 0 && ids.contains(&id)) {
                            let q = apply_commit(base, &qso, mask);
                            cx.send(Command::Update(id, q));
                            self.expected.push_back(Expected {
                                depth: 0,
                                quiet: true,
                            });
                        }
                    }
                    // Departure: zLog shows a modal dialog and a toast; here both go
                    // to the status bar and the panel, and the plugin does not wait.
                    FromPlugin::Dialog(t) | FromPlugin::Notify(t) => {
                        let t = format!("{}: {t}", h.name());
                        self.manager.lock().unwrap().history.push(t.clone());
                        cx.say(t);
                    }
                    FromPlugin::Unsupported(what) => {
                        if self.unsupported.insert(format!("{}/{what}", h.name())) {
                            cx.say(texts().format(&lang, "warn.unsupported", &[&h.name(), &what]));
                        }
                    }
                }
            }
        }
        if let Some(r) = self.rule.clone() {
            if r.calculating() && !self.warned_calc {
                self.warned_calc = true;
                cx.say(texts().get(&lang, "warn.calculating").to_string());
            }
            // A rescore delivers no event, so nothing is expected back.
            if r.take_rescore() {
                self.warned_calc = false;
                cx.send(Command::Rescore);
            }
        }
    }
}
