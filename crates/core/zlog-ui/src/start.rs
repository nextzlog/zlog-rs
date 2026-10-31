use std::collections::BTreeMap;
use std::path::PathBuf;

use zlog_core::{Band, ContestDef, ContestMode, Power, Registry, Station};

use crate::Section;

#[derive(Debug, Clone)]
pub struct Setup {
    pub contest: usize,
    pub contest_file: Option<PathBuf>,
    pub mode: ContestMode,
    pub station: Station,
    pub coeff: f64,
    pub path: PathBuf,
    pub use_contest_period: bool,
    pub convert_old_other: bool,
}

pub type Sections = BTreeMap<String, Section>;

pub struct StartView<'a> {
    pub registry: &'a Registry,
    pub language: &'a str,
    pub settings: &'a mut Sections,
    pub save: bool,
    pub back: bool,
    pub cancel: bool,
}

pub trait StartScreen {
    fn sections(&self) -> &[&'static str];

    fn load(&mut self, settings: &Sections);

    fn show(&mut self, ui: &mut egui::Ui, view: &mut StartView<'_>) -> Option<Setup>;

    fn show_error(&mut self, error: String);

    fn has_window(&self) -> bool;

    fn close_windows(&mut self);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChosenContest {
    Linked(String),
    File(PathBuf),
}

pub struct EntryView<'a> {
    pub registry: &'a Registry,
    pub language: &'a str,
    pub contest_folder: &'a mut String,
}

pub trait ContestEntry {
    fn id(&self) -> &str;

    fn title(&self, registry: &Registry, language: &str) -> String;

    fn shown(&self, _registry: &Registry) -> bool {
        true
    }

    fn contests(&self, _registry: &Registry, _language: &str) -> Option<Vec<usize>> {
        None
    }

    fn open(&mut self, _view: &mut EntryView<'_>) {}

    fn ui(&mut self, _ctx: &egui::Context, _view: &mut EntryView<'_>) -> Option<ChosenContest> {
        None
    }

    fn is_open(&self) -> bool {
        false
    }

    fn close(&mut self) {}

    fn updates(&mut self, _view: &mut EntryView<'_>) -> Vec<PathBuf> {
        Vec::new()
    }
}

struct ContestGroup {
    first: String,
}

impl ContestGroup {
    fn first<'a>(&self, registry: &'a Registry) -> Option<&'a dyn ContestDef> {
        registry
            .contests()
            .iter()
            .find(|c| c.id() == self.first)
            .map(|c| &**c)
    }
}

impl ContestEntry for ContestGroup {
    fn id(&self) -> &str {
        &self.first
    }

    fn title(&self, registry: &Registry, language: &str) -> String {
        self.first(registry)
            .map(|c| c.group(language))
            .unwrap_or_default()
    }

    fn contests(&self, registry: &Registry, language: &str) -> Option<Vec<usize>> {
        let group = self.title(registry, language);
        let all = registry.contests();
        Some(
            (0..all.len())
                .filter(|i| all[*i].group(language) == group)
                .collect(),
        )
    }
}

pub fn contest_groups(registry: &Registry) -> Vec<Box<dyn ContestEntry>> {
    let mut seen: Vec<String> = Vec::new();
    let mut entries: Vec<Box<dyn ContestEntry>> = Vec::new();
    for c in registry.contests() {
        let g = c.group("en");
        if !seen.contains(&g) {
            seen.push(g);
            entries.push(Box::new(ContestGroup {
                first: c.id().to_string(),
            }));
        }
    }
    entries
}

pub fn read_station(s: &Section) -> Station {
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
        age: text("age"),
        cq_zone: text("cq_zone"),
        iaru_zone: text("iaru_zone"),
        itu_region: text("itu_region"),
        iota: text("iota"),
        handle_cw: text("handle_cw"),
        handle_ph: text("handle_ph"),
        tx_nr: s
            .get("tx_nr")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0u8)
            .min(15),
    }
}

pub fn write_station(s: &mut Section, st: &Station) {
    s.insert("callsign".into(), st.callsign.clone());
    s.insert("operator".into(), st.operator.clone());
    s.insert("prov".into(), st.prov.clone());
    s.insert("city".into(), st.city.clone());
    s.insert("age".into(), st.age.clone());
    s.insert("cq_zone".into(), st.cq_zone.clone());
    s.insert("iaru_zone".into(), st.iaru_zone.clone());
    s.insert("itu_region".into(), st.itu_region.clone());
    s.insert("iota".into(), st.iota.clone());
    s.insert("tx_nr".into(), st.tx_nr.to_string());
    s.insert("handle_cw".into(), st.handle_cw.clone());
    s.insert("handle_ph".into(), st.handle_ph.clone());
    s.retain(|k, _| !k.starts_with("power."));
    for (b, p) in &st.power {
        if let Some(c) = p.code() {
            s.insert(format!("power.{}", b.label()), c.to_string());
        }
    }
}
