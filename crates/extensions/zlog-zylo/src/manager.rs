use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::host::{Answerer, PluginHost, ProcessSpawner, Spawner, State};
use crate::library::{is_any_plugin_file, is_plugin_file};
use crate::rules::RulePlugin;
use crate::texts;

pub const ZLOG_VERSION: &str = "2.9.9.0";

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub call: String,
    pub band: String,
    pub mode: String,
    pub file: String,
}

impl Snapshot {
    pub fn answer(&self, query: &str) -> String {
        query
            .replace("{V}", ZLOG_VERSION)
            .replace("{F}", &self.file)
            .replace("{C}", &self.call.to_uppercase())
            .replace("{B}", &self.band)
            .replace("{M}", &self.mode)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub verify: Duration,
    pub hang: Duration,
    pub cities: Duration,
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

pub fn rule_id(name: &str, contest: &str) -> String {
    format!("zylo:{name}:{contest}")
}

#[derive(Clone)]
pub struct RuleSlot {
    pub host: Arc<PluginHost>,
    pub rule: Arc<RulePlugin>,
    pub contest: String,
    pub cfg: String,
    pub id: String,
    pub owned: bool,
}

pub struct Manager {
    pub dir: PathBuf,
    pub enabled: BTreeSet<String>,
    pub rules_only: BTreeSet<String>,
    pub formats: BTreeMap<String, String>,
    hosts: BTreeMap<String, Arc<PluginHost>>,
    rule: Option<RuleSlot>,
    retired: Vec<RuleSlot>,
    snapshot: Arc<Mutex<Snapshot>>,
    messages: Vec<String>,
    pub history: Vec<String>,
    reported: HashSet<String>,
    spawner: Arc<dyn Spawner>,
    pub timing: Timing,
    pub started: bool,
    pub language: String,
}

pub fn default_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("zylo")
}

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

    pub fn set_enabled(&mut self, name: &str, on: bool) {
        if on {
            self.enabled.insert(name.to_string());
            self.reported.remove(name);
            if self.started {
                self.start_one(name);
            }
        } else {
            self.enabled.remove(name);
            self.let_go(name);
        }
    }

    pub fn set_rules_only(&mut self, name: &str, on: bool) {
        if on {
            self.rules_only.insert(name.to_string());
            self.let_go(name);
        } else {
            self.rules_only.remove(name);
            if self.started && self.enabled.contains(name) {
                self.start_one(name);
            }
        }
    }

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
        let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
        if same(&dest, src) {
            return Ok(name);
        }
        let part = zlog_core::file::stage(&dest, |f| {
            std::io::copy(&mut std::fs::File::open(src)?, f).map(drop)
        })
        .map_err(|e| e.to_string())?;
        self.let_go(&name);
        let old = self.dir.join(format!("{name}.old"));
        let had = dest.exists();
        let moved = if had {
            std::fs::rename(&dest, &old)
        } else {
            Ok(())
        }
        .and_then(|_| {
            part.persist(&dest)
                .map(drop)
                .map_err(|e| e.error)
                .inspect_err(|_| {
                    if had {
                        let _ = std::fs::rename(&old, &dest);
                    }
                })
        });
        if self.started && self.enabled.contains(&name) {
            self.start_one(&name);
        }
        moved.map_err(|e| e.to_string())?;
        Ok(name)
    }

    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        if Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name) {
            return Err(format!("{name}: not a file of the plugin folder"));
        }
        self.set_enabled(name, false);
        self.rules_only.remove(name);
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

    pub fn host(&self, name: &str) -> Option<Arc<PluginHost>> {
        self.hosts.get(name).cloned().or_else(|| {
            self.rule
                .as_ref()
                .filter(|r| r.host.name() == name)
                .map(|r| r.host.clone())
        })
    }

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

    pub fn select_rule(&mut self, id: &str) -> Option<RuleSlot> {
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

    fn prune_retired(&mut self) {
        let (unused, kept) = std::mem::take(&mut self.retired)
            .into_iter()
            .partition(|r| Arc::strong_count(&r.rule) == 1);
        self.retired = kept;
        for r in unused {
            self.stop_owned(&r);
        }
    }

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

    pub fn finish_all(&mut self, grace: Duration) {
        let until = std::time::Instant::now() + grace;
        let mut all: Vec<Arc<PluginHost>> = Vec::new();
        let rules = self.rule.take().into_iter().chain(self.retired.drain(..));
        for h in std::mem::take(&mut self.hosts)
            .into_values()
            .chain(rules.map(|r| r.host))
        {
            if !all.iter().any(|o| Arc::ptr_eq(o, &h)) {
                h.stop();
                all.push(h);
            }
        }
        for h in all {
            h.finish(until);
        }
    }

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
    use crate::protocol::ToHelper;

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
        let running: Vec<String> = m.targets().iter().map(|h| h.name().to_string()).collect();
        assert_eq!(running, vec![name()]);
        m.set_enabled(&name(), false);
        assert!(m.targets().is_empty());
    }

    #[test]
    fn at_exit_every_plugin_is_told_to_finish_within_the_grace() {
        let fake = FakeSpawner::new(|_| vec![]);
        let seen = fake.seen.clone();
        let mut m = Manager::new(Arc::new(fake));
        m.enabled.insert(name());
        m.start_enabled();
        let h = m.targets()[0].clone();
        assert!(h.wait_ready(Duration::from_secs(5)));
        let t = std::time::Instant::now();
        m.finish_all(crate::host::FINISH_GRACE);
        assert!(t.elapsed() <= crate::host::FINISH_GRACE + Duration::from_millis(500));
        assert_eq!(seen.lock().unwrap().last(), Some(&ToHelper::Finish));
        assert!(m.targets().is_empty() && h.state() == State::Stopped);
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

    #[test]
    fn only_the_opened_contest_selects_its_rule_plugin() {
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.enabled.insert(name());
        m.rules_only.insert(name());
        let a = m.attach_rule(&name(), "A", "a.cfg").unwrap();
        assert!(m.select_rule("builtin").is_none());
        assert!(m.rule().is_none());
        let a_host = a.host.clone();
        drop(a);
        let a = m.attach_rule(&name(), "A", "a.cfg").unwrap();
        let b = m.attach_rule(&name(), "B", "b.cfg").unwrap();
        assert!(Arc::ptr_eq(&a.host, &a_host) && Arc::ptr_eq(&b.host, &a_host));
        let open = m.select_rule(&b.id).unwrap();
        assert!(Arc::ptr_eq(&open.rule, &b.rule));
        drop((a, b));
        let preview = m.attach_rule(&name(), "A", "a.cfg").unwrap();
        drop(preview);
        assert!(Arc::ptr_eq(&m.rule().unwrap().rule, &open.rule));
        assert_ne!(open.host.state(), State::Stopped);
        m.release_rule(&open.rule);
        assert!(m.rule().is_none());
        assert_eq!(open.host.state(), State::Stopped);
    }

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
        m.release_rule(&slot.rule);
        assert_ne!(slot.host.state(), State::Stopped);
    }

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
    fn re_adding_keeps_the_open_rule_host_and_restarts_the_others() {
        let base = std::env::temp_dir().join(format!("zlog-zylo-readd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (from, dir) = (base.join("downloads"), base.join("plugins"));
        std::fs::create_dir_all(&from).unwrap();
        let src = from.join(name());
        std::fs::write(&src, "v1").unwrap();
        let mut m = Manager::new(Arc::new(FakeSpawner::new(|_| vec![])));
        m.dir = dir;
        m.add(&src).unwrap();
        m.set_enabled(&name(), true);
        m.start_enabled();
        let first = m.host(&name()).unwrap();
        std::fs::write(&src, "v2").unwrap();
        m.add(&src).unwrap();
        assert_eq!(first.state(), State::Stopped);
        let second = m.host(&name()).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert_ne!(second.state(), State::Stopped);
        let slot = m.attach_rule(&name(), "TEST", "t.cfg").unwrap();
        m.select_rule(&slot.id).unwrap();
        assert!(Arc::ptr_eq(&slot.host, &second));
        std::fs::write(&src, "v3").unwrap();
        m.add(&src).unwrap();
        assert_ne!(slot.host.state(), State::Stopped);
        assert!(Arc::ptr_eq(&m.host(&name()).unwrap(), &slot.host));
        m.release_rule(&slot.rule);
        assert_ne!(slot.host.state(), State::Stopped);
        m.set_enabled(&name(), false);
        assert_eq!(slot.host.state(), State::Stopped);
        std::fs::remove_dir_all(base).unwrap();
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
        std::fs::write(dir.join("b.dll"), "b").unwrap();
        let other = dir.join("..").join("plugins").join("b.dll");
        assert_eq!(m.add(&other).unwrap(), "b.dll");
        assert_eq!(std::fs::read_to_string(dir.join("b.dll")).unwrap(), "b");
        assert!(!dir.join("b.dll.old").exists());
        assert!(m.add(&from.join("gone.dll")).is_err());
        assert!(m.remove("../downloads/a.dll").is_err());
        std::fs::remove_dir_all(base).unwrap();
    }
}
