//! Settings kept between runs, in one file in the system's configuration folder
//! (`%APPDATA%` on Windows, `~/Library/Application Support` on macOS).
//!
//! The file has sections of `key = value` lines; each part of the application and
//! each extension owns one section and sees no other.

use std::collections::BTreeMap;
use std::path::PathBuf;

use zlog_core::{Band, Power, Station};
use zlog_ui::Section;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    sections: BTreeMap<String, Section>,
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("zlog-rs").join("settings.ini"))
    }

    /// The saved settings, or empty ones when there are none yet.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.format())
    }

    pub fn parse(text: &str) -> Self {
        let mut sections: BTreeMap<String, Section> = BTreeMap::new();
        let mut current = String::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                current = name.to_string();
            } else if let Some((k, v)) = line.split_once('=') {
                sections
                    .entry(current.clone())
                    .or_default()
                    .insert(k.trim().into(), v.trim().into());
            }
        }
        Settings { sections }
    }

    pub fn format(&self) -> String {
        let mut out = String::new();
        for (name, section) in &self.sections {
            out.push_str(&format!("[{name}]\n"));
            for (k, v) in section {
                out.push_str(&format!("{k} = {v}\n"));
            }
            out.push('\n');
        }
        out
    }

    pub fn section(&mut self, name: &str) -> &mut Section {
        self.sections.entry(name.to_string()).or_default()
    }

    /// The station, from the `station` section.
    pub fn station(&mut self) -> Station {
        let s = self.section("station");
        let text = |k: &str| s.get(k).cloned().unwrap_or_default();
        let power = Band::ALL
            .iter()
            .filter_map(|b| {
                let code = s.get(&format!("power.{}", b.label()))?.chars().next()?;
                Some((*b, Power::from_code(code)?))
            })
            .collect();
        Station {
            callsign: text("callsign"),
            operator: text("operator"),
            prov: text("prov"),
            city: text("city"),
            power,
        }
    }

    /// Identifiers of the contests opened last, newest first.
    pub fn recent_contests(&mut self) -> Vec<String> {
        let s = self.section("menu");
        s.get("recent")
            .map(|r| {
                r.split(',')
                    .filter(|x| !x.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn add_recent_contest(&mut self, id: &str) {
        let mut ids = self.recent_contests();
        ids.retain(|x| x != id);
        ids.insert(0, id.to_string());
        ids.truncate(8);
        self.section("menu").insert("recent".into(), ids.join(","));
    }

    pub fn set_station(&mut self, st: &Station) {
        let s = self.section("station");
        s.insert("callsign".into(), st.callsign.clone());
        s.insert("operator".into(), st.operator.clone());
        s.insert("prov".into(), st.prov.clone());
        s.insert("city".into(), st.city.clone());
        s.retain(|k, _| !k.starts_with("power."));
        for (b, p) in &st.power {
            if let Some(c) = p.code() {
                s.insert(format!("power.{}", b.label()), c.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut s = Settings::default();
        let mut st = Station {
            callsign: "JA1ZLO".into(),
            prov: "10".into(),
            ..Station::default()
        };
        st.power.insert(Band::B430, Power::P002);
        s.set_station(&st);
        s.section("rig").insert("model".into(), "IC-7300".into());
        let back = Settings::parse(&s.format());
        assert_eq!(back, s);
        let mut back = back;
        assert_eq!(back.station(), st);
    }
}
