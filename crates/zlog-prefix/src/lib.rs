//! Country or area of the worked station, from the longest matching callsign prefix.
//!
//! Port of ZyLO's `prefix` plugin (MIT, JA1ZLO); `prefix.yaml` is its table.
//! When a QSO is logged, the country goes to the status bar and, if the QSO has no
//! entity yet, into its `entity` field. Japanese prefixes are left out, as in ZyLO.

use std::collections::HashMap;

use zlog_core::{Command, Context, Extension, LogEvent, Observer, QsoId};

const TABLE: &str = include_str!("prefix.yaml");

pub struct Prefix {
    table: HashMap<String, String>,
    longest: usize,
}

impl Default for Prefix {
    fn default() -> Self {
        let table: HashMap<String, String> = TABLE
            .lines()
            .filter_map(|line| {
                let (key, value) = line.split_once(": ")?;
                Some((key.trim_matches('\'').to_string(), value.trim().to_string()))
            })
            .filter(|(_, value)| value != "Japan")
            .collect();
        let longest = table.keys().map(String::len).max().unwrap_or(0);
        Prefix { table, longest }
    }
}

impl Prefix {
    /// Country for a callsign, from its longest known prefix.
    pub fn lookup(&self, callsign: &str) -> Option<&str> {
        let call = callsign.to_ascii_uppercase();
        (1..=call.len().min(self.longest))
            .rev()
            .find_map(|n| call.get(..n).and_then(|p| self.table.get(p)))
            .map(String::as_str)
    }
}

/// The package: one observer that annotates new QSOs.
#[derive(Default)]
pub struct PrefixExtension;

impl Extension for PrefixExtension {
    fn name(&self) -> &str {
        "Prefix"
    }

    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        vec![Box::new(Prefix::default())]
    }
}

impl Observer for Prefix {
    fn name(&self) -> &str {
        "Prefix"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        // The update this extension sends comes back as an event; ignore it.
        if cx.caused_by_me() {
            return;
        }
        let LogEvent::Inserted { index } = event else {
            return;
        };
        let qso = &cx.log().qsos[*index];
        let Some(country) = self.lookup(&qso.callsign) else {
            return;
        };
        let message = format!("{}: {country}", qso.callsign);
        if qso.entity.is_empty() {
            let mut updated = qso.clone();
            updated.entity = country.chars().take(10).collect();
            let id = QsoId::of(qso);
            cx.send(Command::Update(id, updated));
        }
        cx.say(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_wins() {
        let p = Prefix::default();
        assert_eq!(p.lookup("3DA0XX"), Some("Eswatini"));
        assert_eq!(p.lookup("3D2XX"), p.table.get("3D2").map(String::as_str));
        assert_eq!(p.lookup("JA1ZLO"), None);
        assert_eq!(p.lookup("k1abc"), Some("US"));
    }
}
