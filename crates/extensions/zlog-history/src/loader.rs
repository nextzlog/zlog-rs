use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use zlog_core::{Log, LogFormat, Registry};

use crate::config::Source;
use crate::index::Index;
use crate::table;

pub type Formats = fn() -> Registry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Missing(String),
    Unknown,
    Broken(String),
}

#[derive(Debug, Clone, Default)]
pub struct Built {
    pub index: Index,
    pub problems: Vec<(String, Problem)>,
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn read(
    index: &mut Index,
    n: usize,
    path: &str,
    bytes: &[u8],
    formats: &[Box<dyn LogFormat>],
) -> Result<(), Problem> {
    match extension(path).as_str() {
        "zlo" | "zlox" => {
            let (log, _) = Log::from_bytes(bytes).map_err(|e| Problem::Broken(e.to_string()))?;
            index.add_qsos(n, &log.qsos);
        }
        "txt" | "csv" => {
            let rows = table::parse(&table::decode(bytes))
                .map_err(|e| Problem::Broken(format!("{e:?}")))?;
            index.add_rows(n, rows);
        }
        ext => {
            let format = formats
                .iter()
                .filter(|f| f.can_import())
                .find(|f| f.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))
                .ok_or(Problem::Unknown)?;
            index.add_qsos(n, &format.import(bytes).map_err(Problem::Broken)?);
        }
    }
    Ok(())
}

pub fn build(sources: &[Source], formats: &[Box<dyn LogFormat>]) -> Built {
    let mut built = Built::default();
    for s in sources {
        let name = Path::new(&s.path)
            .file_name()
            .map_or(s.path.clone(), |n| n.to_string_lossy().into_owned());
        let n = built.index.add_source(name);
        let result = std::fs::read(&s.path)
            .map_err(|e| Problem::Missing(e.to_string()))
            .and_then(|bytes| read(&mut built.index, n, &s.path, &bytes, formats));
        if let Err(p) = result {
            built.problems.push((s.path.clone(), p));
        }
    }
    built
}

pub struct Loading {
    receiver: Receiver<Built>,
}

impl Loading {
    pub fn start(sources: Vec<Source>, formats: Option<Formats>) -> Loading {
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let known = ["zlo", "zlox", "txt", "csv"];
            let others = sources
                .iter()
                .any(|s| !known.contains(&extension(&s.path).as_str()));
            let registry = formats.filter(|_| others).map(|f| f()).unwrap_or_default();
            let _ = sender.send(build(&sources, registry.formats()));
        });
        Loading { receiver }
    }

    pub fn poll(&self) -> Option<Result<Built, ()>> {
        match self.receiver.try_recv() {
            Ok(b) => Some(Ok(b)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("zlog-history-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn source(path: &Path) -> Source {
        Source {
            path: path.display().to_string(),
            contest: String::new(),
        }
    }

    struct Plain;

    impl LogFormat for Plain {
        fn name(&self) -> &str {
            "plain"
        }
        fn extensions(&self) -> &[&str] {
            &["log"]
        }
        fn can_import(&self) -> bool {
            true
        }
        fn import(&self, bytes: &[u8]) -> Result<Vec<zlog_core::Qso>, String> {
            let text = String::from_utf8_lossy(bytes);
            let (call, nr) = text.trim().split_once(' ').ok_or("no space")?;
            Ok(vec![zlog_core::Qso {
                callsign: call.into(),
                nr_rcvd: nr.into(),
                ..Default::default()
            }])
        }
    }

    #[test]
    fn reads_logs_tables_and_other_formats_and_reports_the_rest() {
        let d = dir("build");
        let mut log = Log::default();
        log.qsos.push(zlog_core::Qso {
            callsign: "JA1ZLO".into(),
            nr_rcvd: "10M".into(),
            ..Default::default()
        });
        let zlox = d.join("old.ZLOX");
        log.save(&zlox, zlog_core::Format::Zlox).unwrap();
        let txt = d.join("hist.txt");
        std::fs::write(&txt, "CALL,NAME,EXCH\nJG1VPP,Kaz,13M\n").unwrap();
        let other = d.join("x.log");
        std::fs::write(&other, "JA1YWX 01H").unwrap();
        let unknown = d.join("x.adx");
        std::fs::write(&unknown, "").unwrap();
        let broken = d.join("bad.csv");
        std::fs::write(&broken, "NAME\nTaro\n").unwrap();
        let missing = d.join("none.zlo");
        let sources: Vec<Source> = [&zlox, &txt, &other, &unknown, &broken, &missing]
            .into_iter()
            .map(|p| source(p))
            .collect();
        let formats: Vec<Box<dyn LogFormat>> = vec![Box::new(Plain)];
        let b = build(&sources, &formats);
        assert_eq!(b.index.exchange("JA1ZLO").unwrap().text, "10M");
        assert_eq!(b.index.name("JG1VPP").unwrap().text, "Kaz");
        assert_eq!(b.index.exchange("JA1YWX").unwrap().text, "01H");
        assert_eq!(b.index.sources[0], "old.ZLOX");
        let kinds: Vec<_> = b.problems.iter().map(|(_, p)| p.clone()).collect();
        assert_eq!(kinds[0], Problem::Unknown);
        assert!(matches!(kinds[1], Problem::Broken(_)));
        assert!(matches!(kinds[2], Problem::Missing(_)));
        assert_eq!(kinds.len(), 3);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn builds_on_a_thread_even_when_every_file_is_missing() {
        let d = dir("thread");
        let loading = Loading::start(vec![source(&d.join("gone.txt"))], None);
        let built = loop {
            if let Some(b) = loading.poll() {
                break b.unwrap();
            }
            std::thread::yield_now();
        };
        assert!(built.index.is_empty());
        assert_eq!(built.problems.len(), 1);
        std::fs::remove_dir_all(d).unwrap();
    }
}
