//! The plugins of this program: which are enabled, which run, and what zLog last
//! told them. One [`Manager`] lives for the program ([`global`]); the panel, the
//! link of each open log and the contest files share it.
//!
//! As in zLog (`zyloRuntimeLaunch`), every enabled plugin runs while zLog RS runs,
//! but here it starts in the background when the window first shows, never before;
//! a plugin marked "rules only" starts only while a contest whose CFG names it
//! (`dll` line) is open.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::host::{Answerer, PluginHost, ProcessSpawner, Spawner, State};
use crate::library::{is_any_plugin_file, is_plugin_file};
use crate::rules::RulePlugin;
use crate::texts;

/// Version reported for `{V}`: plugins refuse to launch below their minimum zLog
/// version, so this is that of the zLog release whose ZyLO API is implemented.
pub const ZLOG_VERSION: &str = "2.9.9.0";

/// What a query (`zylo_allow_access`) can ask about (`Request`, UzLogExtension.pas).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// `{C}`: own callsign, upper case.
    pub call: String,
    /// `{B}`, `{M}`: band and mode of the entry row.
    pub band: String,
    pub mode: String,
    /// `{F}`: the log as a ZLO file.
    pub file: String,
}

impl Snapshot {
    /// The query with its placeholders replaced. Departure: zLog also expands the
    /// CW macros (`$B`, `$X`, ... by `SetStrNoAbbrev`); they are left as they are.
    pub fn answer(&self, query: &str) -> String {
        query
            .replace("{V}", ZLOG_VERSION)
            .replace("{F}", &self.file)
            .replace("{C}", &self.call.to_uppercase())
            .replace("{B}", &self.band)
            .replace("{M}", &self.mode)
    }
}

/// Time limits; shorter in tests.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// A verify or points request answered later than this leaves the contest
    /// calculating; it is rescored once the answers are in.
    pub verify: Duration,
    /// A request unanswered this long disables the plugin.
    pub hang: Duration,
    /// How long opening a contest waits for its rule plugin's multiplier list.
    pub cities: Duration,
    /// How long an import or export waits for its plugin.
    pub file: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            verify: Duration::from_millis(300),
            hang: Duration::from_secs(10),
            cities: Duration::from_secs(5),
            file: Duration::from_secs(60),
        }
    }
}

/// The contest ID of a CFG scored by the plugin `name`.
pub fn rule_id(name: &str, contest: &str) -> String {
    format!("zylo:{name}:{contest}")
}

/// The rule plugin of the open contest.
#[derive(Clone)]
pub struct RuleSlot {
    pub host: Arc<PluginHost>,
    pub rule: Arc<RulePlugin>,
    pub contest: String,
    pub cfg: String,
    /// The ID of the contest it scores ([`zlog_core::ContestDef::id`]).
    pub id: String,
    /// Started for this contest (stopped with it), not an enabled plugin running anyway.
    pub owned: bool,
}

pub struct Manager {
    pub dir: PathBuf,
    /// File names of the enabled plugins (zLog `[zylo] DLLs`).
    pub enabled: BTreeSet<String>,
    /// Enabled plugins that start only with their contest.
    pub rules_only: BTreeSet<String>,
    /// File-dialog filters learnt from plugins, by plugin, so formats are offered
    /// before the plugin has started.
    pub formats: BTreeMap<String, String>,
    hosts: BTreeMap<String, Arc<PluginHost>>,
    rule: Option<RuleSlot>,
    /// Rule plugins attached for contests not (or no longer) open: contests chosen
    /// in the Open form, and earlier rules something still uses. One becomes
    /// [`Manager::rule`] when its contest is opened ([`Manager::select_rule`]);
    /// owned hosts are stopped once nothing uses their slot.
    retired: Vec<RuleSlot>,
    snapshot: Arc<Mutex<Snapshot>>,
    /// Messages for the status bar, taken by the link.
    messages: Vec<String>,
    /// Recent messages, shown in the panel.
    pub history: Vec<String>,
    reported: HashSet<String>,
    spawner: Arc<dyn Spawner>,
    pub timing: Timing,
    pub started: bool,
    pub language: String,
}

/// The default plugin folder: `zylo` in the data folder of zLog RS
/// ([`zlog_ui::APP_DIR`]).
pub fn default_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("zylo")
}

/// The manager of this program.
pub fn global() -> Arc<Mutex<Manager>> {
    static M: OnceLock<Arc<Mutex<Manager>>> = OnceLock::new();
    M.get_or_init(|| Arc::new(Mutex::new(Manager::new(Arc::new(ProcessSpawner)))))
        .clone()
}

impl Manager {
    pub fn new(spawner: Arc<dyn Spawner>) -> Self {
        Manager {
            dir: default_dir(),
            enabled: BTreeSet::new(),
            rules_only: BTreeSet::new(),
            formats: BTreeMap::new(),
            hosts: BTreeMap::new(),
            rule: None,
            retired: Vec::new(),
            snapshot: Arc::new(Mutex::new(Snapshot::default())),
            messages: Vec::new(),
            history: Vec::new(),
            reported: HashSet::new(),
            spawner,
            timing: Timing::default(),
            started: false,
            language: "en".into(),
        }
    }

    pub fn snapshot(&self) -> Arc<Mutex<Snapshot>> {
        self.snapshot.clone()
    }

    fn answerer(&self) -> Answerer {
        let s = self.snapshot.clone();
        Arc::new(move |q: &str| s.lock().unwrap().answer(q))
    }

    fn path_of(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn launch(&self, name: &str) -> Arc<PluginHost> {
        let path = self.path_of(name).display().to_string();
        PluginHost::start(name, &path, self.spawner.clone(), self.answerer())
    }

    /// Start every enabled plugin that is not for rules only (once; later
    /// enabling starts plugins one by one).
    pub fn start_enabled(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let names: Vec<String> = self.enabled.iter().cloned().collect();
        for n in names {
            self.start_one(&n);
        }
    }

    fn start_one(&mut self, name: &str) {
        let runnable = is_plugin_file(Path::new(name))
            && !self.rules_only.contains(name)
            && !self.hosts.contains_key(name);
        if !runnable {
            return;
        }
        // A rule plugin already running for a contest runs on as this plugin,
        // not as a second process; its slots no longer own it.
        let running = self
            .rule
            .iter()
            .chain(&self.retired)
            .find(|r| r.host.name() == name && r.host.state() != State::Stopped)
            .map(|r| r.host.clone());
        let h = match running {
            Some(h) => {
                for r in self.rule.iter_mut().chain(&mut self.retired) {
                    if Arc::ptr_eq(&r.host, &h) {
                        r.owned = false;
                    }
                }
                h
            }
            None => self.launch(name),
        };
        self.hosts.insert(name.to_string(), h);
    }

    /// The plugin `name` no longer runs for itself: rule slots using its host take
    /// it over (stopped with the last of them), or it stops.
    fn let_go(&mut self, name: &str) {
        let Some(h) = self.hosts.remove(name) else {
            return;
        };
        let mut used = false;
        for r in self.rule.iter_mut().chain(&mut self.retired) {
            if Arc::ptr_eq(&r.host, &h) {
                r.owned = true;
                used = true;
            }
        }
        if !used {
            h.stop();
        }
    }

    /// Enable or disable a plugin (zLog `InstallDLL`, `DisableDLL`); takes effect at
    /// once, where zLog needs a restart to stop one.
    pub fn set_enabled(&mut self, name: &str, on: bool) {
        if on {
            self.enabled.insert(name.to_string());
            self.reported.remove(name);
            if self.started {
                self.start_one(name);
            }
        } else {
            self.enabled.remove(name);
            // The rule plugin of a contest in use keeps scoring until released.
            self.let_go(name);
        }
    }

    pub fn set_rules_only(&mut self, name: &str, on: bool) {
        if on {
            self.rules_only.insert(name.to_string());
            // The rule plugin of a contest in use keeps running, now with it.
            self.let_go(name);
        } else {
            self.rules_only.remove(name);
            if self.started && self.enabled.contains(name) {
                self.start_one(name);
            }
        }
    }

    /// Copy the plugin at `src` (from anywhere) into the plugin folder; returns its
    /// file name. It is not enabled: the operator enables it. A plugin of that
    /// name already there is stopped and replaced, the old file kept as
    /// `name.old`.
    pub fn add(&mut self, src: &Path) -> Result<String, String> {
        if !is_any_plugin_file(src) {
            return Err(format!(
                "{}: not a plugin (.dll, .dylib, .so)",
                src.display()
            ));
        }
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("{}: no file name", src.display()))?
            .to_string();
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let dest = self.dir.join(&name);
        // The very file already in the folder, however its path is written.
        let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
        if same(&dest, src) {
            return Ok(name);
        }
        // Copied beside it first, so a failed copy leaves the old plugin in place.
        let part = self.dir.join(format!("{name}.part"));
        std::fs::copy(src, &part).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            e.to_string()
        })?;
        if let Some(h) = self.hosts.remove(&name) {
            h.stop();
        }
        let old = self.dir.join(format!("{name}.old"));
        let had = dest.exists();
        let moved = if had {
            std::fs::rename(&dest, &old)
        } else {
            Ok(())
        }
        .and_then(|_| {
            std::fs::rename(&part, &dest).inspect_err(|_| {
                // The old plugin goes back where it was.
                if had {
                    let _ = std::fs::rename(&old, &dest);
                }
            })
        });
        if moved.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        // Started again, the new plugin or, after a failure, the old one.
        if self.started && self.enabled.contains(&name) {
            self.start_one(&name);
        }
        moved.map_err(|e| e.to_string())?;
        Ok(name)
    }

    /// Stop and disable the plugin `name` and take it out of the plugin folder,
    /// kept as `name.removed` (a library a stopping helper still holds cannot be
    /// deleted on Windows, and a mistake can be undone).
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        if Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name) {
            return Err(format!("{name}: not a file of the plugin folder"));
        }
        self.set_enabled(name, false);
        self.rules_only.remove(name);
        // Also when it runs only as the rule plugin of a contest.
        for r in self.rule.iter().chain(&self.retired) {
            if r.host.name() == name {
                r.host.stop();
            }
        }
        let path = self.dir.join(name);
        if !path.exists() {
            return Ok(());
        }
        let kept = self.dir.join(format!("{name}.removed"));
        let _ = std::fs::remove_file(&kept);
        std::fs::rename(&path, kept).map_err(|e| e.to_string())
    }

    /// Plugin files in the folder, with whether each is for this platform.
    pub fn files(&self) -> Vec<(String, bool)> {
        let mut v: Vec<(String, bool)> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_any_plugin_file(p))
            .filter_map(|p| {
                let n = p.file_name()?.to_str()?.to_string();
                Some((n, is_plugin_file(&p)))
            })
            .collect();
        for n in &self.enabled {
            if !v.iter().any(|(m, _)| m == n) {
                v.push((n.clone(), is_plugin_file(Path::new(n))));
            }
        }
        v.sort();
        v
    }

    /// The running host of a plugin, as utility or as the rule plugin.
    pub fn host(&self, name: &str) -> Option<Arc<PluginHost>> {
        self.hosts.get(name).cloned().or_else(|| {
            self.rule
                .as_ref()
                .filter(|r| r.host.name() == name)
                .map(|r| r.host.clone())
        })
    }

    /// The host for an import or export with an enabled plugin, started if needed.
    pub fn host_for_file(&mut self, name: &str) -> Option<Arc<PluginHost>> {
        if let Some(h) = self.host(name) {
            return Some(h);
        }
        if !self.enabled.contains(name) {
            return None;
        }
        let h = self.launch(name);
        self.hosts.insert(name.to_string(), h.clone());
        Some(h)
    }

    pub fn state(&self, name: &str) -> Option<State> {
        self.host(name).map(|h| h.state())
    }

    /// Start the plugin a CFG names (`zyloContestSwitch`) for the contest read from
    /// it. It becomes the rule plugin only when that contest is opened
    /// ([`Manager::select_rule`]): reading a CFG in the Open form changes nothing.
    pub fn attach_rule(
        &mut self,
        name: &str,
        contest: &str,
        cfg: &str,
    ) -> Result<RuleSlot, String> {
        let lang = self.language.clone();
        if !self.enabled.contains(name) || !is_plugin_file(Path::new(name)) {
            return Err(texts().format(&lang, "warn.rule_missing", &[&name]));
        }
        let (host, owned) = match self.hosts.get(name) {
            Some(h) => (h.clone(), false),
            None => match self
                .rule
                .iter()
                .chain(&self.retired)
                .find(|r| r.host.name() == name && r.host.state() != State::Stopped)
            {
                Some(r) => (r.host.clone(), true),
                None => (self.launch(name), true),
            },
        };
        let slot = RuleSlot {
            rule: Arc::new(RulePlugin::new(host.clone(), self.timing)),
            host,
            contest: contest.to_string(),
            cfg: cfg.to_string(),
            id: rule_id(name, contest),
            owned,
        };
        self.retired.push(slot.clone());
        self.prune_retired();
        Ok(slot)
    }

    /// The contest `id` is opened: its rule plugin, the latest attached for it,
    /// becomes the rule plugin; `None` for a contest without one (the rule plugin
    /// then stays as it was, released with its own contest).
    pub fn select_rule(&mut self, id: &str) -> Option<RuleSlot> {
        // A slot attached since the current one (the CFG read again) is newer.
        let Some(i) = self.retired.iter().rposition(|r| r.id == id) else {
            self.prune_retired();
            return self.rule.as_ref().filter(|r| r.id == id).cloned();
        };
        let slot = self.retired.remove(i);
        if let Some(old) = self.rule.replace(slot.clone()) {
            self.retired.push(old);
        }
        self.prune_retired();
        Some(slot)
    }

    /// Stop the retired rule plugins nothing uses any more.
    fn prune_retired(&mut self) {
        let (unused, kept) = std::mem::take(&mut self.retired)
            .into_iter()
            .partition(|r| Arc::strong_count(&r.rule) == 1);
        self.retired = kept;
        for r in unused {
            self.stop_owned(&r);
        }
    }

    /// Stop the host of `slot`, owned by it, unless another rule slot shares it.
    fn stop_owned(&self, slot: &RuleSlot) {
        let shared = self
            .rule
            .iter()
            .chain(&self.retired)
            .any(|r| Arc::ptr_eq(&r.host, &slot.host));
        if slot.owned && !shared {
            slot.host.stop();
        }
    }

    pub fn rule(&self) -> Option<RuleSlot> {
        self.rule.clone()
    }

    /// The contest of `rule` was closed: stop its plugin unless it runs anyway or a
    /// newer contest took over.
    pub fn release_rule(&mut self, rule: &Arc<RulePlugin>) {
        if self
            .rule
            .as_ref()
            .is_some_and(|r| Arc::ptr_eq(&r.rule, rule))
        {
            let r = self.rule.take().unwrap();
            self.stop_owned(&r);
        } else if let Some(i) = self.retired.iter().position(|r| Arc::ptr_eq(&r.rule, rule)) {
            let r = self.retired.remove(i);
            self.stop_owned(&r);
        }
        self.prune_retired();
    }

    /// Every running plugin, the rule plugin included, each once.
    pub fn targets(&self) -> Vec<Arc<PluginHost>> {
        let mut v: Vec<Arc<PluginHost>> = self.hosts.values().cloned().collect();
        if let Some(r) = &self.rule {
            if !v.iter().any(|h| Arc::ptr_eq(h, &r.host)) {
                v.push(r.host.clone());
            }
        }
        v
    }

    pub fn say(&mut self, message: String) {
        self.history.push(message.clone());
        let n = self.history.len();
        if n > 50 {
            self.history.drain(..n - 50);
        }
        self.messages.push(message);
    }

    /// Learn formats of plugins that became ready and warn once about plugins that
    /// were disabled.
    pub fn watch(&mut self) {
        let lang = self.language.clone();
        for h in self.targets() {
            match h.state() {
                State::Ready => {
                    let f = h.format();
                    if !f.is_empty() {
                        self.formats.insert(h.name().to_string(), f);
                    }
                }
                State::Disabled(why) if self.reported.insert(h.name().to_string()) => {
                    self.say(texts().format(&lang, "warn.disabled", &[&h.name(), &why]));
                }
                _ => {}
            }
        }
    }

    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    /// Restore from the settings section (zLog's keys `path` and `DLLs`).
    pub fn load(&mut self, section: &zlog_ui::Section) {
        let list = |k: &str| -> BTreeSet<String> {
            section
                .get(k)
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        // zLog keeps file names only (`GetDLLsINI`).
                        .map(|s| s.rsplit(['/', '\\']).next().unwrap_or(s).to_string())
                        .collect()
                })
                .unwrap_or_default()
        };
        if let Some(p) = section.get("path").filter(|p| !p.is_empty()) {
            self.dir = PathBuf::from(p);
        }
        self.enabled = list("DLLs");
        self.rules_only = list("rules");
        self.formats = section
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("format.")?.to_string(), v.clone())))
            .collect();
    }

    pub fn save(&self, section: &mut zlog_ui::Section) {
        section.insert("path".into(), self.dir.display().to_string());
        let join = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(",");
        section.insert("DLLs".into(), join(&self.enabled));
        section.insert("rules".into(), join(&self.rules_only));
        for (k, v) in &self.formats {
            section.insert(format!("format.{k}"), v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::fake::FakeSpawner;

    fn name() -> String {
        format!("a.{}", crate::library::EXTENSION)
    }

    #[test]
    fn queries_are_answered_from_the_snapshot() {
        let s = Snapshot {
            call: "ja1zlo".into(),
            band: "7".into(),
            mode: "CW".into(),
            file: "/tmp/x.zlo".into(),
        };
        assert_eq!(s.answer("{C} {B} {M} {F}"), "JA1ZLO 7 CW /tmp/x.zlo");
        assert_eq!(s.answer("{V}"), ZLOG_VERSION);
    }

    #[test]
    fn settings_round_trip_and_only_enabled_plugins_start() {
        let mut sec = zlog_ui::Section::new();
        sec.insert("path".into(), "/p".into());
        sec.insert("DLLs".into(), format!("C:\\x\\{}, b.dll", name()));
        sec.insert("format.b.dll".into(), "B|*.b".into());
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.load(&sec);
        assert!(m.enabled.contains(&name()));
        assert_eq!(m.formats["b.dll"], "B|*.b");
        let mut out = zlog_ui::Section::new();
        m.save(&mut out);
        let mut again = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        again.load(&out);
        assert_eq!(again.enabled, m.enabled);
        assert_eq!(again.dir, PathBuf::from("/p"));

        assert!(m.targets().is_empty());
        m.start_enabled();
        // Only the plugin for this platform starts.
        let running: Vec<String> = m.targets().iter().map(|h| h.name().to_string()).collect();
        assert_eq!(running, vec![name()]);
        m.set_enabled(&name(), false);
        assert!(m.targets().is_empty());
    }

    #[test]
    fn rules_only_plugins_start_with_their_contest_and_stop_with_it() {
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.enabled.insert(name());
        m.rules_only.insert(name());
        m.start_enabled();
        assert!(m.targets().is_empty());
        assert!(m.attach_rule("missing.dll", "X", "x.cfg").is_err());
        let slot = m.attach_rule(&name(), "TEST", "t.cfg").unwrap();
        assert!(slot.owned);
        assert!(m.targets().is_empty(), "read, not opened");
        m.select_rule(&slot.id).unwrap();
        assert_eq!(m.targets().len(), 1);
        m.release_rule(&slot.rule);
        assert!(m.targets().is_empty());
        assert_eq!(slot.host.state(), State::Stopped);
    }

    /// Z1: reading a CFG (the Open form) never changes the rule plugin; only the
    /// contest opened does, and a plugin in use survives the next CFG read.
    #[test]
    fn only_the_opened_contest_selects_its_rule_plugin() {
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.enabled.insert(name());
        m.rules_only.insert(name());
        // Read A, open a built-in contest: no rule plugin; A's is stopped once let go.
        let a = m.attach_rule(&name(), "A", "a.cfg").unwrap();
        assert!(m.select_rule("builtin").is_none());
        assert!(m.rule().is_none());
        let a_host = a.host.clone();
        drop(a);
        // Read A, then B, open B: B's rule.
        let a = m.attach_rule(&name(), "A", "a.cfg").unwrap();
        let b = m.attach_rule(&name(), "B", "b.cfg").unwrap();
        // The running host of the slot let go is reused, not started again.
        assert!(Arc::ptr_eq(&a.host, &a_host) && Arc::ptr_eq(&b.host, &a_host));
        let open = m.select_rule(&b.id).unwrap();
        assert!(Arc::ptr_eq(&open.rule, &b.rule));
        drop((a, b));
        // Read A again and cancel: B stays the rule plugin, still running.
        let preview = m.attach_rule(&name(), "A", "a.cfg").unwrap();
        drop(preview);
        assert!(Arc::ptr_eq(&m.rule().unwrap().rule, &open.rule));
        assert_ne!(open.host.state(), State::Stopped);
        m.release_rule(&open.rule);
        assert!(m.rule().is_none());
        assert_eq!(open.host.state(), State::Stopped);
    }

    /// Z5: marking the open contest's plugin "rules only" keeps it scoring; removing
    /// a plugin stops it also as a rule plugin.
    #[test]
    fn rules_only_keeps_the_open_rule_and_remove_stops_it() {
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.dir = std::env::temp_dir().join(format!("zlog-zylo-ro-{}", std::process::id()));
        m.enabled.insert(name());
        m.start_enabled();
        let slot = m.attach_rule(&name(), "TEST", "t.cfg").unwrap();
        m.select_rule(&slot.id).unwrap();
        assert!(!slot.owned);
        m.set_rules_only(&name(), true);
        assert_ne!(slot.host.state(), State::Stopped);
        m.remove(&name()).unwrap();
        assert_eq!(slot.host.state(), State::Stopped);
    }

    /// U4: a rule plugin running for the open contest is not started twice when
    /// "rules only" is turned off.
    #[test]
    fn rules_only_off_reuses_the_running_rule_plugin() {
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.enabled.insert(name());
        m.rules_only.insert(name());
        m.start_enabled();
        let slot = m.attach_rule(&name(), "TEST", "t.cfg").unwrap();
        m.select_rule(&slot.id).unwrap();
        m.set_rules_only(&name(), false);
        assert!(Arc::ptr_eq(&m.host(&name()).unwrap(), &slot.host));
        assert_eq!(m.targets().len(), 1);
        // Released, it runs on as an enabled plugin.
        m.release_rule(&slot.rule);
        assert_ne!(slot.host.state(), State::Stopped);
    }

    /// U5: disabling the plugin of the open contest keeps it scoring until released.
    #[test]
    fn disabling_keeps_the_open_rule_plugin_until_released() {
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.enabled.insert(name());
        m.start_enabled();
        let slot = m.attach_rule(&name(), "TEST", "t.cfg").unwrap();
        m.select_rule(&slot.id).unwrap();
        m.set_enabled(&name(), false);
        assert_ne!(slot.host.state(), State::Stopped);
        m.release_rule(&slot.rule);
        assert_eq!(slot.host.state(), State::Stopped);
    }

    /// U6: a retired slot sharing the host takes it over too.
    #[test]
    fn rules_only_hands_the_host_to_retired_slots() {
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.enabled.insert(name());
        m.start_enabled();
        let preview = m.attach_rule(&name(), "A", "a.cfg").unwrap();
        m.set_rules_only(&name(), true);
        assert_ne!(preview.host.state(), State::Stopped, "still referenced");
        let host = preview.host.clone();
        drop(preview);
        m.prune_retired();
        assert_eq!(host.state(), State::Stopped, "stopped once let go");
    }

    /// U7: the slot attached last for a contest is selected.
    #[test]
    fn the_latest_slot_of_a_contest_is_selected() {
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.enabled.insert(name());
        let first = m.attach_rule(&name(), "A", "a.cfg").unwrap();
        m.select_rule(&first.id).unwrap();
        let again = m.attach_rule(&name(), "A", "a2.cfg").unwrap();
        let open = m.select_rule(&again.id).unwrap();
        assert!(Arc::ptr_eq(&open.rule, &again.rule));
    }

    #[test]
    fn plugins_are_added_from_anywhere_and_removed_with_a_copy_kept() {
        let base = std::env::temp_dir().join(format!("zlog-zylo-add-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (from, dir) = (base.join("downloads"), base.join("plugins"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("a.dll"), "v1").unwrap();
        std::fs::write(from.join("notes.txt"), "x").unwrap();
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.dir = dir.clone();
        assert_eq!(m.add(&from.join("a.dll")).unwrap(), "a.dll");
        assert!(m.add(&from.join("notes.txt")).is_err());
        assert!(!m.enabled.contains("a.dll"), "added, not enabled");
        std::fs::write(from.join("a.dll"), "v2").unwrap();
        m.add(&from.join("a.dll")).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.dll.old")).unwrap(),
            "v1"
        );
        m.set_enabled("a.dll", true);
        m.remove("a.dll").unwrap();
        assert!(!m.enabled.contains("a.dll"));
        assert!(!dir.join("a.dll").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("a.dll.removed")).unwrap(),
            "v2"
        );
        assert!(m.files().is_empty());
        // The file already in the folder, by another path: left as it is.
        std::fs::write(dir.join("b.dll"), "b").unwrap();
        let other = dir.join("..").join("plugins").join("b.dll");
        assert_eq!(m.add(&other).unwrap(), "b.dll");
        assert_eq!(std::fs::read_to_string(dir.join("b.dll")).unwrap(), "b");
        assert!(!dir.join("b.dll.old").exists());
        // A source that cannot be read leaves the plugin there.
        assert!(m.add(&from.join("gone.dll")).is_err());
        assert!(m.remove("../downloads/a.dll").is_err());
        std::fs::remove_dir_all(base).unwrap();
    }
}
