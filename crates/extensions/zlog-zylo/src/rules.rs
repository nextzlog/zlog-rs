use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zlog_cfg::file::{cfg_title, decode, load_cfg};
use zlog_cfg::user::{RuleHook, Verdict};
use zlog_cfg::DatEntry;
use zlog_core::{ContestFile, Log, OpenedContest, Qso, QsoScore};

use crate::host::{PluginHost, State};
use crate::manager::{Manager, Timing};
use crate::protocol::{Record, Reply, ToHelper};
use crate::record::{from_record, to_record};
use crate::texts;

pub fn dll_name(cfg: &str) -> Option<String> {
    cfg.lines()
        .filter_map(|l| {
            let t = l.trim();
            let head = t.get(..3)?;
            let rest = &t[3..];
            (head.eq_ignore_ascii_case("dll") && rest.starts_with([' ', '\t']))
                .then(|| rest.trim().to_string())
        })
        .filter(|n| !n.is_empty())
        .map(|n| {
            let n = n.replace('\\', "/");
            Path::new(&n)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&n)
                .to_string()
        })
        .next_back()
}

pub fn parse_cities(text: &str) -> Vec<DatEntry> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let code = parts.next()?.to_string();
            Some(DatEntry {
                code,
                name: parts.next().unwrap_or("").to_string(),
            })
        })
        .collect()
}

enum LateKey {
    Verify(Box<Record>),
    Total(u32, u32),
}

struct Late {
    key: LateKey,
    since: Instant,
    rx: Receiver<Reply>,
}

pub struct RulePlugin {
    host: Arc<PluginHost>,
    timing: Timing,
    verdicts: Mutex<HashMap<Record, Record>>,
    totals: Mutex<HashMap<(u32, u32), u64>>,
    late: Mutex<Option<Late>>,
    calculating: AtomicBool,
    rescore: AtomicBool,
}

fn verify_key(q: &Qso, dupe: bool) -> Record {
    let mut q = q.clone();
    q.dupe = dupe;
    q.points = 0;
    q.multi1.clear();
    q.multi2.clear();
    q.new_multi1 = false;
    q.new_multi2 = false;
    to_record(&q)
}

fn verdict(r: &Record) -> Option<Verdict> {
    let q = from_record(r);
    if q.multi1.is_empty() {
        return None;
    }
    Some(Verdict {
        points: q.points as i32,
        multi1: q.multi1,
        multi2: q.multi2,
    })
}

impl RulePlugin {
    pub fn new(host: Arc<PluginHost>, timing: Timing) -> Self {
        RulePlugin {
            host,
            timing,
            verdicts: Mutex::new(HashMap::new()),
            totals: Mutex::new(HashMap::new()),
            late: Mutex::new(None),
            calculating: AtomicBool::new(false),
            rescore: AtomicBool::new(false),
        }
    }

    pub fn host(&self) -> &Arc<PluginHost> {
        &self.host
    }

    pub fn calculating(&self) -> bool {
        self.calculating.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    fn invalidate(&self) {
        self.verdicts.lock().unwrap().clear();
        self.totals.lock().unwrap().clear();
    }

    pub fn forget(&self, q: &Qso) {
        let mut verdicts = self.verdicts.lock().unwrap();
        for dupe in [false, true] {
            verdicts.remove(&verify_key(q, dupe));
        }
        drop(verdicts);
        self.totals.lock().unwrap().clear();
    }

    fn check_late(&self) -> bool {
        let mut late = self.late.lock().unwrap();
        let Some(l) = late.as_ref() else {
            return false;
        };
        match (l.rx.try_recv(), &l.key) {
            (Ok(Reply::Record(r)), LateKey::Verify(key)) => {
                self.verdicts.lock().unwrap().insert(**key, r);
                *late = None;
                false
            }
            (Ok(Reply::Int(t)), LateKey::Total(p, m)) => {
                self.totals
                    .lock()
                    .unwrap()
                    .insert((*p, *m), t.max(0) as u64);
                *late = None;
                false
            }
            (Ok(_), _) | (Err(TryRecvError::Disconnected), _) => {
                *late = None;
                false
            }
            (Err(TryRecvError::Empty), _) => {
                if l.since.elapsed() > self.timing.hang {
                    *late = None;
                    drop(late);
                    self.host.disable("not responding");
                    false
                } else {
                    true
                }
            }
        }
    }

    pub fn take_rescore(&self) -> bool {
        let waiting = self.check_late();
        let state = self.host.state();
        if self.calculating() && !waiting && state != State::Starting {
            self.calculating.store(false, Ordering::SeqCst);
            self.rescore.store(state == State::Ready, Ordering::SeqCst);
        }
        self.rescore.swap(false, Ordering::SeqCst)
    }

    fn not_now(&self) {
        if matches!(self.host.state(), State::Starting | State::Ready) {
            self.calculating.store(true, Ordering::SeqCst);
        }
    }

    fn verify_record(&self, key: Record) -> Option<Record> {
        if let Some(r) = self.verdicts.lock().unwrap().get(&key) {
            return Some(*r);
        }
        if self.check_late() || self.host.state() != State::Ready {
            self.not_now();
            return None;
        }
        let rx = self
            .host
            .request(|seq| ToHelper::Verify { seq, record: key })?;
        match rx.recv_timeout(self.timing.verify) {
            Ok(Reply::Record(r)) => {
                self.verdicts.lock().unwrap().insert(key, r);
                Some(r)
            }
            Ok(_) => None,
            Err(_) => {
                *self.late.lock().unwrap() = Some(Late {
                    key: LateKey::Verify(Box::new(key)),
                    since: Instant::now(),
                    rx,
                });
                self.not_now();
                None
            }
        }
    }
}

impl RuleHook for RulePlugin {
    fn verify(&self, q: &Qso, dupe: bool) -> Option<Verdict> {
        self.verify_record(verify_key(q, dupe))
            .as_ref()
            .and_then(verdict)
    }

    fn total(&self, points: u32, multis: u32, _log: &Log, _scored: &[QsoScore]) -> u64 {
        let product = points as u64 * multis as u64;
        if let Some(t) = self.totals.lock().unwrap().get(&(points, multis)) {
            return *t;
        }
        if self.check_late() || self.host.state() != State::Ready {
            self.not_now();
            return product;
        }
        let Some(rx) = self.host.request(|seq| ToHelper::Points {
            seq,
            points: points as i64,
            multis: multis as i64,
        }) else {
            return product;
        };
        match rx.recv_timeout(self.timing.verify) {
            Ok(Reply::Int(t)) => {
                let t = t.max(0) as u64;
                self.totals.lock().unwrap().insert((points, multis), t);
                t
            }
            Ok(_) | Err(RecvTimeoutError::Disconnected) => product,
            Err(RecvTimeoutError::Timeout) => {
                *self.late.lock().unwrap() = Some(Late {
                    key: LateKey::Total(points, multis),
                    since: Instant::now(),
                    rx,
                });
                self.not_now();
                product
            }
        }
    }
}

pub struct ZyloCfgFile {
    pub manager: Arc<Mutex<Manager>>,
}

fn read_dll_name(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    dll_name(&decode(&bytes))
}

impl ContestFile for ZyloCfgFile {
    fn name(&self, language: &str) -> String {
        texts().get(language, "file.cfg").into()
    }

    fn extensions(&self) -> &[&str] {
        &["cfg"]
    }

    fn accepts(&self, path: &Path) -> bool {
        let cfg = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("cfg"));
        cfg && read_dll_name(path).is_some()
    }

    fn title(&self, path: &Path) -> Option<String> {
        cfg_title(path)
    }

    fn open(&self, path: &Path, language: &str) -> Result<OpenedContest, String> {
        let (def, missing_dat) = load_cfg(path)?;
        let name = read_dll_name(path).unwrap_or_default();
        let contest = def.config().contest_name.clone();
        let mut warnings = Vec::new();
        let attached = {
            let mut m = self.manager.lock().unwrap();
            m.language = language.to_string();
            m.attach_rule(&name, &contest, &path.display().to_string())
        };
        let slot = match attached {
            Ok(s) => s,
            Err(e) => {
                warnings.push(e);
                warnings.extend(
                    missing_dat.map(|d| texts().format(language, "warn.dat_missing", &[&d])),
                );
                return Ok(OpenedContest {
                    def: Box::new(def),
                    warnings,
                });
            }
        };
        let timing = self.manager.lock().unwrap().timing;
        let mut def = def;
        let mut table = false;
        if slot.host.wait_ready(timing.cities) {
            if let Ok(Reply::Text(t)) = slot
                .host
                .call(|seq| ToHelper::Cities { seq }, timing.cities)
            {
                let entries = parse_cities(&t);
                if !entries.is_empty() {
                    def = def.with_entries(entries);
                    table = true;
                }
            }
        } else {
            warnings.push(texts().format(language, "warn.rule_starting", &[&name]));
        }
        if !table {
            warnings
                .extend(missing_dat.map(|d| texts().format(language, "warn.dat_missing", &[&d])));
        }
        Ok(OpenedContest {
            def: Box::new(def.with_hook(&slot.id, slot.rule.clone())),
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::fake::{answerer, FakeSpawner};
    use std::time::Duration;

    #[test]
    fn a_cfg_with_a_dll_line_opens_with_the_plugins_table_and_scoring() {
        let dir = std::env::temp_dir().join(format!("zlog-zylo-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lib = format!("r.{}", crate::library::EXTENSION);
        let with = dir.join("with.cfg");
        std::fs::write(&with, format!("#RULED\nDLL {lib}\n")).unwrap();
        let without = dir.join("plain.cfg");
        std::fs::write(&without, "#PLAIN\n").unwrap();
        let s = FakeSpawner::new(|m| match m {
            ToHelper::Cities { seq } => vec![crate::protocol::ToHost::Reply {
                seq: *seq,
                reply: Reply::Text("100 Sapporo\n101 Asahikawa".into()),
            }],
            m => verifier(Duration::ZERO)(m),
        });
        let mut m = Manager::new(Arc::new(s));
        m.timing = fast();
        let m = Arc::new(Mutex::new(m));
        let file = ZyloCfgFile { manager: m.clone() };
        assert!(file.accepts(&with));
        assert!(!file.accepts(&without));
        let opened = file.open(&with, "en").unwrap();
        assert!(opened.warnings[0].contains(&lib));
        m.lock().unwrap().enabled.insert(lib.clone());
        let opened = file.open(&with, "en").unwrap();
        assert!(opened.warnings.is_empty(), "{:?}", opened.warnings);
        assert_eq!(opened.def.id(), format!("zylo:{lib}:RULED"));
        let mut c = opened.def.build(zlog_core::ContestMode::Mix);
        let log = Log {
            qsos: vec![qso("100")],
            ..Log::default()
        };
        let s = c.score(&log);
        assert_eq!(s.qsos[0].points, 3);
        assert_eq!(s.qsos[0].multi1, "100");
        assert_eq!(c.multi_table().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_dll_line_names_the_plugin() {
        assert_eq!(dll_name("#T\nDLL sample.dll\n"), Some("sample.dll".into()));
        assert_eq!(
            dll_name("dll\tC:\\z\\a.dll\nexit\ndll b.dll"),
            Some("b.dll".into())
        );
        assert_eq!(dll_name("; uses dll\ndllx a"), None);
        assert_eq!(dll_name("dll "), None);
    }

    fn plugin(
        delay: Duration,
        script: impl Fn(&ToHelper) -> Vec<crate::protocol::ToHost> + Send + Sync + 'static,
    ) -> (Arc<PluginHost>, Arc<FakeSpawner>) {
        let mut s = FakeSpawner::new(script);
        s.launch_delay = delay;
        let s = Arc::new(s);
        let h = PluginHost::start("r", "r", s.clone(), answerer());
        (h, s)
    }

    fn fast() -> Timing {
        Timing {
            verify: Duration::from_millis(50),
            hang: Duration::from_millis(400),
            cities: Duration::from_secs(2),
            file: Duration::from_secs(2),
        }
    }

    fn verifier(delay: Duration) -> impl Fn(&ToHelper) -> Vec<crate::protocol::ToHost> {
        move |m| match m {
            ToHelper::Verify { seq, record } => {
                std::thread::sleep(delay);
                let mut q = from_record(record);
                q.points = 3;
                q.multi1 = q.nr_rcvd.clone();
                vec![crate::protocol::ToHost::Reply {
                    seq: *seq,
                    reply: Reply::Record(to_record(&q)),
                }]
            }
            _ => vec![],
        }
    }

    fn qso(nr: &str) -> Qso {
        Qso {
            callsign: "JA1ZLO".into(),
            nr_rcvd: nr.into(),
            ..Qso::default()
        }
    }

    fn wait_rescore(r: &RulePlugin) {
        let end = Instant::now() + Duration::from_secs(5);
        while !r.take_rescore() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn verify_while_starting_is_calculating_then_rescored() {
        let (h, _) = plugin(Duration::from_millis(100), verifier(Duration::ZERO));
        let r = RulePlugin::new(h, fast());
        assert_eq!(r.verify(&qso("10"), false), None);
        assert!(r.calculating());
        wait_rescore(&r);
        assert!(!r.calculating());
        let v = r.verify(&qso("10"), false).unwrap();
        assert_eq!((v.points, v.multi1.as_str()), (3, "10"));
        assert_eq!(r.verify(&qso(""), false), None);
    }

    #[test]
    fn a_late_answer_is_kept_and_triggers_rescoring() {
        let (h, _) = plugin(Duration::ZERO, verifier(Duration::from_millis(150)));
        assert!(h.wait_ready(Duration::from_secs(5)));
        let r = RulePlugin::new(h, fast());
        assert_eq!(r.verify(&qso("20"), false), None);
        assert!(r.calculating());
        let t = Instant::now();
        assert_eq!(r.verify(&qso("21"), false), None);
        assert!(t.elapsed() < Duration::from_millis(40));
        wait_rescore(&r);
        assert_eq!(r.verify(&qso("20"), false).unwrap().points, 3);
    }

    #[test]
    fn a_plugin_that_never_answers_is_disabled() {
        let (h, s) = plugin(Duration::ZERO, |_| vec![]);
        assert!(h.wait_ready(Duration::from_secs(5)));
        let r = RulePlugin::new(h.clone(), fast());
        assert_eq!(r.verify(&qso("1"), false), None);
        std::thread::sleep(Duration::from_millis(450));
        assert!(!r.take_rescore());
        assert_eq!(h.state(), State::Disabled("not responding".into()));
        assert!(*s.killed.lock().unwrap());
        assert!(!r.calculating());
        assert_eq!(r.total(5, 2, &Log::default(), &[]), 10);
    }

    #[test]
    fn answers_are_kept_until_the_plugin_hears_of_a_change() {
        let (h, s) = plugin(Duration::ZERO, verifier(Duration::ZERO));
        assert!(h.wait_ready(Duration::from_secs(5)));
        let r = RulePlugin::new(h, fast());
        let asked = || {
            s.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|m| matches!(m, ToHelper::Verify { .. }))
                .count()
        };
        r.verify(&qso("1"), false).unwrap();
        let mut scored = qso("1");
        scored.points = 9;
        scored.multi1 = "old".into();
        r.verify(&scored, false).unwrap();
        assert_eq!(asked(), 1);
        r.verify(&qso("2"), false).unwrap();
        assert_eq!(asked(), 2);
        r.forget(&scored);
        r.verify(&qso("1"), false).unwrap();
        r.verify(&qso("2"), false).unwrap();
        assert_eq!(asked(), 3);
        r.verify(&qso("1"), true).unwrap();
        assert_eq!(asked(), 4);
        r.invalidate();
        r.verify(&qso("1"), false).unwrap();
        assert_eq!(asked(), 5);
    }

    #[test]
    fn a_change_forgets_the_totals() {
        let script = |m: &ToHelper| match m {
            ToHelper::Points { seq, .. } => vec![crate::protocol::ToHost::Reply {
                seq: *seq,
                reply: Reply::Int(7),
            }],
            _ => vec![],
        };
        let (h, s) = plugin(Duration::ZERO, script);
        assert!(h.wait_ready(Duration::from_secs(5)));
        let r = RulePlugin::new(h, fast());
        let asked = || {
            s.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|m| matches!(m, ToHelper::Points { .. }))
                .count()
        };
        assert_eq!(r.total(5, 2, &Log::default(), &[]), 7);
        assert_eq!(r.total(5, 2, &Log::default(), &[]), 7);
        assert_eq!(asked(), 1);
        r.forget(&qso("1"));
        assert_eq!(r.total(5, 2, &Log::default(), &[]), 7);
        assert_eq!(asked(), 2);
    }

    #[test]
    fn a_late_total_is_kept_and_the_rescore_settles() {
        let script = |m: &ToHelper| match m {
            ToHelper::Points {
                seq,
                points,
                multis,
            } => {
                std::thread::sleep(Duration::from_millis(100));
                vec![crate::protocol::ToHost::Reply {
                    seq: *seq,
                    reply: Reply::Int(points * multis + 1),
                }]
            }
            _ => vec![],
        };
        let (h, _) = plugin(Duration::ZERO, script);
        assert!(h.wait_ready(Duration::from_secs(5)));
        let r = RulePlugin::new(h, fast());
        assert_eq!(r.total(5, 2, &Log::default(), &[]), 10);
        assert!(r.calculating());
        wait_rescore(&r);
        let t = Instant::now();
        assert_eq!(r.total(5, 2, &Log::default(), &[]), 11);
        assert!(t.elapsed() < Duration::from_millis(40));
        assert!(!r.calculating());
        assert!(!r.take_rescore());
    }
}
