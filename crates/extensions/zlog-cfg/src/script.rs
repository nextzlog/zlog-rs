pub mod host;

use std::sync::{Arc, RwLock};

use zlog_core::{Band, Mode, Qso};

pub type Inputs = Arc<Vec<Arc<ScriptQso>>>;

pub fn same(a: &Inputs, b: &Inputs) -> bool {
    Arc::ptr_eq(a, b)
        || (a.len() == b.len()
            && a.iter()
                .zip(b.iter())
                .all(|(x, y)| Arc::ptr_eq(x, y) || x == y))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptQso {
    pub call: String,
    pub band: Band,
    pub mode: Mode,
    pub rcvd: String,
    pub sent: String,
    pub name: String,
    pub note: String,
    pub srst: u16,
    pub rrst: u16,
    pub tx: u8,
    pub time: (i32, u32, u32, u32, u32, u32),
    pub dupe: bool,
    pub counted: bool,
}

impl ScriptQso {
    pub fn is(&self, q: &Qso, dupe: bool, counted: bool) -> bool {
        self.dupe == dupe
            && self.counted == counted
            && self.band == q.band
            && self.mode == q.mode
            && self.srst == q.rst_sent
            && self.rrst == q.rst_rcvd
            && self.tx == q.tx
            && self.call == q.callsign
            && self.rcvd == q.nr_rcvd
            && self.sent == q.nr_sent
            && self.name == q.operator
            && self.note == q.memo
            && self.time == q.time.to_ymd_hms()
    }

    pub fn of(q: &Qso, dupe: bool, counted: bool) -> Self {
        ScriptQso {
            call: q.callsign.clone(),
            band: q.band,
            mode: q.mode,
            rcvd: q.nr_rcvd.clone(),
            sent: q.nr_sent.clone(),
            name: q.operator.clone(),
            note: q.memo.clone(),
            srst: q.rst_sent,
            rrst: q.rst_rcvd,
            tx: q.tx,
            time: q.time.to_ymd_hms(),
            dupe,
            counted,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Judged {
    pub points: i32,
    pub multis: Vec<String>,
    pub invalid: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Sums {
    pub qsos: u32,
    pub points: u32,
    pub multis: u32,
}

pub trait ScoreScript: Send {
    fn judge(&self, qsos: &Inputs) -> Option<Arc<Vec<Judged>>>;

    fn total(&self, qsos: &Inputs, sums: Sums) -> Option<u64>;

    fn check(&self, qso: &ScriptQso) -> Option<Judged>;
}

pub trait ScriptProgram: Send + Sync {
    fn start(&self) -> Box<dyn ScoreScript>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub memory: usize,
    pub steps: u64,
    pub depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            memory: 32 << 20,
            steps: 2_000_000,
            depth: 200,
        }
    }
}

pub trait Vm: Send {
    fn reset(&mut self) -> Result<(), String>;

    fn add(&mut self, qso: &ScriptQso) -> Result<Judged, String>;

    fn check(&mut self, qso: &ScriptQso) -> Result<Judged, String>;

    fn total(&mut self, sums: Sums) -> Result<u64, String>;

    fn verify(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn save(&mut self) -> Result<serde_json::Value, String>;

    fn restore(&mut self, state: &serde_json::Value) -> Result<(), String>;
}

pub trait ScriptEngine: Send + Sync {
    fn language(&self) -> &str;

    fn versions(&self) -> &[u32];

    fn compile(
        &self,
        name: &str,
        source: &str,
        version: u32,
        limits: &Limits,
    ) -> Result<Box<dyn Vm>, String>;
}

static ENGINES: RwLock<Vec<Arc<dyn ScriptEngine>>> = RwLock::new(Vec::new());

pub fn add_engine(engine: Arc<dyn ScriptEngine>) {
    let mut all = ENGINES.write().unwrap_or_else(|e| e.into_inner());
    all.retain(|e| !e.language().eq_ignore_ascii_case(engine.language()));
    all.push(engine);
}

pub fn engine(language: &str) -> Option<Arc<dyn ScriptEngine>> {
    ENGINES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|e| e.language().eq_ignore_ascii_case(language))
        .cloned()
}

#[cfg(feature = "window")]
pub fn register_engine(_b: &mut zlog_ui::Builder, engine: Arc<dyn ScriptEngine>) {
    add_engine(engine);
}

pub struct Section<'a> {
    pub language: String,
    pub version: Option<String>,
    pub source: &'a str,
}

pub fn split(text: &str) -> (&str, Option<Section<'_>>) {
    let mut after_exit = false;
    let mut at = 0;
    for line in text.split_inclusive(['\n', '\r']) {
        let trimmed = line.trim();
        if after_exit && !trimmed.is_empty() {
            let mut words = trimmed.split_whitespace();
            let script = words
                .next()
                .is_some_and(|w| w.eq_ignore_ascii_case("script"));
            return match (script, words.next()) {
                (true, Some(language)) => (
                    &text[..at],
                    Some(Section {
                        language: language.to_ascii_lowercase(),
                        version: words.next().map(String::from),
                        source: {
                            let rest = &text[at + line.len()..];
                            match line.ends_with('\r') {
                                true => rest.strip_prefix('\n').unwrap_or(rest),
                                false => rest,
                            }
                        },
                    }),
                ),
                _ => (text, None),
            };
        }
        if !after_exit && crate::cfg::is_exit(line) {
            after_exit = true;
        }
        at += line.len();
    }
    (text, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_follows_a_script_line_right_after_exit() {
        let text = "#A\nmode on\nexit ;\nScript Lua\nfunction init() end\n";
        let (cfg, s) = split(text);
        assert_eq!(cfg, "#A\nmode on\nexit ;\n");
        let s = s.unwrap();
        assert_eq!(
            (s.language.as_str(), s.source),
            ("lua", "function init() end\n")
        );
        assert_eq!(
            split("#A\r\nEXIT ;\r\n\r\nSCRIPT LUA\r\nx")
                .1
                .unwrap()
                .source,
            "x"
        );
    }

    #[test]
    fn a_cr_only_file_has_its_script_found() {
        let (cfg, s) = split("#A\rEXIT ;\rSCRIPT LUA\rx\ry");
        assert_eq!(cfg, "#A\rEXIT ;\r");
        assert_eq!(s.unwrap().source, "x\ry");
    }

    #[test]
    fn other_layouts_are_no_script() {
        assert!(split("#A\nSCRIPT LUA\nexit ;\n").1.is_none());
        assert!(split("#A\nSCRIPT LUA\nx\n").1.is_none());
        assert!(split("#A\nexit\nSCRIPT LUA\nx\n").1.is_none());
        assert!(split("#A\nexit ;\nSCRIPT\n").1.is_none());
        assert!(split("#A\nexit ;\ndll a.dll\nSCRIPT LUA\n").1.is_none());
        assert_eq!(split("#A\nexit ;\nSCRIPT PY\nx").1.unwrap().language, "py");
        let s = split("#A\nexit ;\nscript lua 1\nx").1.unwrap();
        assert_eq!(s.version.as_deref(), Some("1"));
    }

    #[test]
    fn engines_are_found_by_language_in_any_case() {
        struct Nothing;
        impl ScriptEngine for Nothing {
            fn language(&self) -> &str {
                "Test-None"
            }
            fn versions(&self) -> &[u32] {
                &[1]
            }
            fn compile(&self, _: &str, _: &str, _: u32, _: &Limits) -> Result<Box<dyn Vm>, String> {
                Err("no".into())
            }
        }
        add_engine(Arc::new(Nothing));
        add_engine(Arc::new(Nothing));
        assert!(engine("TEST-NONE").is_some());
        assert!(engine("test-other").is_none());
    }

    struct Counting(u32);

    impl Vm for Counting {
        fn reset(&mut self) -> Result<(), String> {
            self.0 = 0;
            Ok(())
        }
        fn add(&mut self, qso: &ScriptQso) -> Result<Judged, String> {
            self.0 += 1;
            Ok(Judged {
                points: self.0 as i32,
                multis: vec![qso.rcvd.clone()],
                invalid: None,
            })
        }
        fn check(&mut self, qso: &ScriptQso) -> Result<Judged, String> {
            Ok(Judged {
                points: 0,
                multis: vec![qso.rcvd.clone()],
                invalid: None,
            })
        }
        fn total(&mut self, sums: Sums) -> Result<u64, String> {
            Ok(sums.points as u64 * 1000 + self.0 as u64)
        }
        fn save(&mut self) -> Result<serde_json::Value, String> {
            Ok(self.0.into())
        }
        fn restore(&mut self, state: &serde_json::Value) -> Result<(), String> {
            self.0 = state.as_u64().ok_or("bad")? as u32;
            Ok(())
        }
    }

    struct Dummy;

    impl ScriptEngine for Dummy {
        fn language(&self) -> &str {
            "DUMMY"
        }
        fn versions(&self) -> &[u32] {
            &[1, 2]
        }
        fn compile(&self, _: &str, _: &str, _: u32, _: &Limits) -> Result<Box<dyn Vm>, String> {
            Ok(Box::new(Counting(0)))
        }
    }

    fn scored(cfg: &str) -> (crate::UserDefinedDef, zlog_core::Log, zlog_core::ScoreSheet) {
        add_engine(Arc::new(Dummy));
        host::patience(std::time::Duration::from_secs(20));
        let def = crate::UserDefinedDef::from_cfg(cfg, |_| None);
        let mut c = def.build_rules(zlog_core::ContestMode::Mix);
        let mut log = zlog_core::Log {
            qsos: ["A", "B", "C"]
                .map(|call| Qso {
                    callsign: format!("JA1{call}"),
                    band: Band::B7,
                    mode: Mode::Cw,
                    nr_rcvd: call.into(),
                    ..Qso::default()
                })
                .to_vec(),
            ..zlog_core::Log::default()
        };
        let sheet = zlog_core::rescore(&mut log, &mut c).unwrap();
        (def, log, sheet)
    }

    #[test]
    fn a_second_engine_is_dispatched_by_the_script_line() {
        let (def, log, sheet) = scored("#D\nundefmulti on\nexit ;\nSCRIPT Dummy 2\nwhatever\n");
        assert!(def.has_script() && def.script_error().is_none());
        let points: Vec<u8> = log.qsos.iter().map(|q| q.points).collect();
        assert_eq!(points, [1, 2, 3]);
        assert_eq!(log.qsos[1].multi1, "B");
        assert_eq!(sheet.score, 6 * 1000 + 3);
    }

    #[test]
    fn an_unknown_language_or_version_falls_back_to_the_plain_cfg() {
        for line in ["SCRIPT NOPE", "SCRIPT DUMMY 3", "SCRIPT DUMMY x"] {
            let (def, log, sheet) =
                scored(&format!("#D\nundefmulti on\nexit ;\n{line}\nwhatever\n"));
            assert!(!def.has_script(), "{line}");
            assert!(def.script_error().unwrap().contains(&line[7..]), "{line}");
            let points: Vec<u8> = log.qsos.iter().map(|q| q.points).collect();
            assert_eq!(points, [1, 1, 1]);
            assert_eq!(sheet.score, 9);
        }
    }
}
