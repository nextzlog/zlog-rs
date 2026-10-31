use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use zlog_core::zlo::read_records;
use zlog_core::{Contest, Format, Log, LogFormat, Qso};

use crate::manager::Manager;
use crate::protocol::{Reply, ToHelper};
use crate::texts;

pub fn parse_filter(filter: &str) -> Vec<(String, Vec<String>)> {
    let parts: Vec<&str> = filter.split('|').collect();
    parts
        .chunks(2)
        .filter(|c| c.len() == 2)
        .filter_map(|c| {
            let exts: Vec<String> = c[1]
                .split(';')
                .filter_map(|p| p.trim().strip_prefix("*."))
                .filter(|e| !e.is_empty() && *e != "*")
                .map(|e| e.to_lowercase())
                .collect();
            (!c[0].trim().is_empty()).then(|| (c[0].trim().to_string(), exts))
        })
        .collect()
}

pub struct PluginFormat {
    plugin: String,
    name: String,
    extensions: Vec<&'static str>,
    manager: Arc<Mutex<Manager>>,
}

pub fn formats(manager: &Arc<Mutex<Manager>>) -> Vec<Box<dyn LogFormat>> {
    let m = manager.lock().unwrap();
    let mut v: Vec<Box<dyn LogFormat>> = Vec::new();
    for (plugin, filter) in &m.formats {
        if !m.enabled.contains(plugin) {
            continue;
        }
        for (name, exts) in parse_filter(filter) {
            v.push(Box::new(PluginFormat {
                plugin: plugin.clone(),
                name,
                extensions: exts
                    .into_iter()
                    .map(|e| &*Box::leak(e.into_boxed_str()))
                    .collect(),
                manager: manager.clone(),
            }));
        }
    }
    v
}

fn temp(ext: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("zlog-zylo-io-{}-{n}.{ext}", std::process::id()))
}

impl PluginFormat {
    fn convert(&self, make: impl FnOnce(u32) -> ToHelper) -> Result<(), String> {
        let (host, timing, lang) = {
            let mut m = self.manager.lock().unwrap();
            (m.host_for_file(&self.plugin), m.timing, m.language.clone())
        };
        let fail = || texts().format(&lang, "warn.format_failed", &[&self.plugin]);
        let host = host.ok_or_else(fail)?;
        if !host.wait_ready(timing.file) {
            return Err(fail());
        }
        match host.call(make, timing.file) {
            Ok(Reply::Bool(true)) => Ok(()),
            _ => Err(fail()),
        }
    }
}

impl LogFormat for PluginFormat {
    fn name(&self) -> &str {
        &self.name
    }

    fn extensions(&self) -> &[&str] {
        &self.extensions
    }

    fn can_import(&self) -> bool {
        true
    }

    fn can_export(&self) -> bool {
        true
    }

    fn import(&self, bytes: &[u8]) -> Result<Vec<Qso>, String> {
        let source = temp(self.extensions.first().copied().unwrap_or("txt"));
        let target = temp("zlo");
        std::fs::write(&source, bytes).map_err(|e| e.to_string())?;
        let done = self.convert(|seq| ToHelper::Import {
            seq,
            source: source.display().to_string(),
            target: target.display().to_string(),
        });
        let out = std::fs::read(&target);
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&target);
        done?;
        let records = read_records(&out.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        Ok(records.records.into_iter().skip(1).collect())
    }

    fn export(&self, log: &Log, _contest: &dyn Contest) -> Result<Vec<u8>, String> {
        let target = temp("zlo");
        std::fs::write(&target, log.to_bytes(Format::Zlo)).map_err(|e| e.to_string())?;
        let done = self.convert(|seq| ToHelper::Export {
            seq,
            target: target.display().to_string(),
            format: self.name.clone(),
        });
        let out = std::fs::read(&target);
        let _ = std::fs::remove_file(&target);
        done?;
        out.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_split_into_formats() {
        assert_eq!(
            parse_filter("ADIF (*.adi)|*.adi;*.ADIF|Cabrillo|*.cbr|broken"),
            vec![
                (
                    "ADIF (*.adi)".to_string(),
                    vec!["adi".into(), "adif".into()]
                ),
                ("Cabrillo".to_string(), vec!["cbr".into()]),
            ]
        );
        assert!(parse_filter("").is_empty());
    }
}
