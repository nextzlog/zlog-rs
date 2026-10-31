use std::collections::HashMap;

use zlog_core::{Band, DelphiTime, Qso};

use crate::table::Row;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub callsign: String,
    pub exchange: String,
    pub name: String,
    pub extra: Vec<(String, String)>,
    pub source: usize,
    pub time: Option<DelphiTime>,
    pub band: Option<Band>,
}

#[derive(Debug, Clone, Default)]
pub struct Index {
    entries: HashMap<String, Vec<Entry>>,
    portables: HashMap<String, Vec<String>>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub text: String,
    pub agree: usize,
    pub of: usize,
    pub source: usize,
    pub via: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Found<'a> {
    pub entries: Vec<&'a Entry>,
    pub via: Option<String>,
}

fn looks_like_call(part: &str) -> bool {
    part.bytes().any(|b| b.is_ascii_digit())
        && part.bytes().any(|b| b.is_ascii_alphabetic())
        && part.bytes().last().is_some_and(|b| b.is_ascii_alphabetic())
}

pub fn base_call(call: &str) -> &str {
    let longest = |shaped: bool| {
        call.split('/')
            .filter(|p| !shaped || looks_like_call(p))
            .fold("", |best, p| if p.len() > best.len() { p } else { best })
    };
    match longest(true) {
        "" => longest(false),
        p => p,
    }
}

fn key(call: &str) -> String {
    call.trim().to_ascii_uppercase()
}

impl Index {
    pub fn add_source(&mut self, name: impl Into<String>) -> usize {
        self.sources.push(name.into());
        self.sources.len() - 1
    }

    pub fn add(&mut self, entry: Entry) {
        if entry.callsign.trim().is_empty() {
            return;
        }
        let k = key(&entry.callsign);
        if k.contains('/') && !self.entries.contains_key(&k) {
            let base = base_call(&k).to_string();
            if base != k {
                self.portables.entry(base).or_default().push(k.clone());
            }
        }
        self.entries.entry(k).or_default().push(entry);
    }

    pub fn add_qsos(&mut self, source: usize, qsos: &[Qso]) {
        for q in qsos {
            self.add(Entry {
                callsign: q.callsign.to_ascii_uppercase(),
                exchange: q.nr_rcvd.trim().to_ascii_uppercase(),
                source,
                time: Some(q.time),
                band: Some(q.band),
                ..Entry::default()
            });
        }
    }

    pub fn add_rows(&mut self, source: usize, rows: Vec<Row>) {
        for r in rows {
            self.add(Entry {
                callsign: r.callsign,
                exchange: r.exchange,
                name: r.name,
                extra: r.extra,
                source,
                ..Entry::default()
            });
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn lookup(&self, call: &str) -> Found<'_> {
        let call = key(call);
        if call.is_empty() {
            return Found::default();
        }
        if let Some(v) = self.entries.get(&call) {
            return Found {
                entries: v.iter().collect(),
                via: None,
            };
        }
        let base = base_call(&call);
        if base != call {
            if let Some(v) = self.entries.get(base) {
                return Found {
                    entries: v.iter().collect(),
                    via: Some(base.to_string()),
                };
            }
        }
        let Some(calls) = self.portables.get(&call) else {
            return Found::default();
        };
        let mut entries: Vec<&Entry> = calls.iter().flat_map(|c| &self.entries[c]).collect();
        entries.sort_by_key(|e| e.source);
        Found {
            entries,
            via: Some(calls.join(", ")),
        }
    }

    pub fn exchange(&self, call: &str) -> Option<Pick> {
        let f = self.lookup(call);
        pick(&f.entries, |e| &e.exchange).map(|p| Pick { via: f.via, ..p })
    }

    pub fn name(&self, call: &str) -> Option<Pick> {
        let f = self.lookup(call);
        pick(&f.entries, |e| &e.name).map(|p| Pick { via: f.via, ..p })
    }
}

pub fn pick(entries: &[&Entry], field: impl Fn(&Entry) -> &String) -> Option<Pick> {
    let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
    let mut of = 0;
    for (i, e) in entries.iter().enumerate() {
        let v = field(e).trim();
        if v.is_empty() {
            continue;
        }
        of += 1;
        let c = counts.entry(v.to_ascii_uppercase()).or_insert((0, i));
        *c = (c.0 + 1, i);
    }
    let (agree, last) = counts.into_values().max()?;
    Some(Pick {
        text: field(entries[last]).trim().to_string(),
        agree,
        of,
        source: entries[last].source,
        via: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(call: &str, exch: &str, name: &str) -> Row {
        Row {
            callsign: call.into(),
            exchange: exch.into(),
            name: name.into(),
            extra: Vec::new(),
        }
    }

    #[test]
    fn looks_up_ignoring_case_and_portable_parts() {
        let mut ix = Index::default();
        let s = ix.add_source("a.txt");
        ix.add_rows(s, vec![row("JA1ZLO", "10M", "Taro"), row("", "1", "")]);
        assert_eq!(ix.len(), 1);
        assert_eq!(ix.lookup("ja1zlo").entries.len(), 1);
        assert!(ix.lookup("ja1zlo").via.is_none());
        let f = ix.lookup("JA1ZLO/1");
        assert_eq!(f.entries[0].exchange, "10M");
        assert_eq!(f.via.as_deref(), Some("JA1ZLO"));
        assert_eq!(
            ix.exchange("JA1ZLO/1").unwrap().via.as_deref(),
            Some("JA1ZLO")
        );
        assert_eq!(ix.lookup("KH6/JA1ZLO").entries.len(), 1);
        assert!(ix.lookup("JA1ZL").entries.is_empty());
        assert!(ix.lookup(" ").entries.is_empty());
        assert_eq!(base_call("KH6/JA1ZLO/P"), "JA1ZLO");
    }

    #[test]
    fn the_base_call_is_the_part_shaped_like_a_callsign() {
        assert_eq!(base_call("KH6/K1A"), "K1A");
        assert_eq!(base_call("K1A/QRPP"), "K1A");
        assert_eq!(base_call("JA1ZLO/1"), "JA1ZLO");
        assert_eq!(base_call("QRPP"), "QRPP");
    }

    #[test]
    fn stored_portable_calls_are_found_from_the_base_call_and_marked() {
        let mut ix = Index::default();
        let s = ix.add_source("a.txt");
        ix.add_rows(
            s,
            vec![row("JA1ZLO/1", "10M", ""), row("JA1ZLO/1", "10M", "")],
        );
        let p = ix.exchange("JA1ZLO").unwrap();
        assert_eq!((p.text.as_str(), p.agree), ("10M", 2));
        assert_eq!(p.via.as_deref(), Some("JA1ZLO/1"));
        assert!(ix.exchange("JA1ZLO/1").unwrap().via.is_none());
    }

    #[test]
    fn the_exchange_most_entries_give_wins_and_ties_go_to_the_latest() {
        let mut ix = Index::default();
        let log = ix.add_source("old.zlox");
        let q = |nr: &str| Qso {
            callsign: "JA1ZLO".into(),
            nr_rcvd: nr.into(),
            ..Qso::default()
        };
        ix.add_qsos(log, &[q("10M"), q("10m"), q("11M"), q("")]);
        let p = ix.exchange("JA1ZLO").unwrap();
        assert_eq!((p.text.as_str(), p.agree, p.of), ("10M", 2, 3));
        let file = ix.add_source("hist.txt");
        ix.add_rows(file, vec![row("JA1ZLO", "11M", "")]);
        let p = ix.exchange("JA1ZLO").unwrap();
        assert_eq!((p.text.as_str(), p.agree, p.source), ("11M", 2, file));
        assert!(ix.name("JA1ZLO").is_none());
        ix.add_rows(
            file,
            vec![row("JG1VPP", "", "Taro"), row("JG1VPP", "", "TARO")],
        );
        ix.add_rows(file, vec![row("JG1VPP", "", "Jiro")]);
        let p = ix.name("JG1VPP").unwrap();
        assert_eq!((p.text.as_str(), p.agree), ("TARO", 2));
        assert!(ix.exchange("JG1VPP").is_none());
    }
}
