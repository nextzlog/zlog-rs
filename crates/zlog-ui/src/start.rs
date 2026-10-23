//! What an extension may add to the start of the program: the screen shown in the
//! main window until a log is open ([`StartScreen`], the zLog Menu), and the entries
//! of its contest menu ([`ContestEntry`]).
//!
//! Every entry of the contest menu goes through [`ContestEntry`]: the groups of the
//! linked contests ([`contest_groups`], made from [`ContestDef::group`], so rule
//! crates need no user-interface code) as well as windows that let the operator
//! choose a contest elsewhere (the user-defined contest window).

use std::collections::BTreeMap;
use std::path::PathBuf;

use zlog_core::{Band, ContestDef, ContestMode, Power, Registry, Station};

use crate::Section;

/// What the operator chose at the start: the contest, the station and the log.
#[derive(Debug, Clone)]
pub struct Setup {
    /// Index into `Registry::contests()`; unused when `contest_file` is set.
    pub contest: usize,
    /// A contest defined by a file (a CFG), opened with `Registry::open_contest_file`.
    pub contest_file: Option<PathBuf>,
    pub mode: ContestMode,
    pub station: Station,
    pub coeff: f64,
    pub path: PathBuf,
    /// zLog's `UseContestPeriod` from the settings (filled in by the application).
    pub use_contest_period: bool,
    /// Convert QSOs of an old `.ZLO` file in mode 5 to Other and continue as
    /// `.ZLOX` (the operator's answer, asked by the application).
    pub convert_old_other: bool,
}

/// Settings sections by name, as the settings file keeps them.
pub type Sections = BTreeMap<String, Section>;

/// What the application hands a [`StartScreen`] every frame.
pub struct StartView<'a> {
    pub registry: &'a Registry,
    /// Language code of the user interface (`en`, `ja`).
    pub language: &'a str,
    /// The sections named by [`StartScreen::sections`], to read and change.
    pub settings: &'a mut Sections,
    /// Set to true to have the settings file written after this frame.
    pub save: bool,
}

/// The screen drawn in the main window until a log is open (the zLog Menu).
pub trait StartScreen {
    /// Names of the settings sections the screen reads and writes.
    fn sections(&self) -> &[&'static str];

    /// Read the settings (at start-up, and after they were changed elsewhere, as by
    /// OK in Options).
    fn load(&mut self, settings: &Sections);

    /// Draw the screen filling `ui`; returns the log to open when the operator
    /// asks for one.
    fn show(&mut self, ui: &mut egui::Ui, view: &mut StartView<'_>) -> Option<Setup>;

    /// Opening the log of the last [`Setup`] failed: show why.
    fn show_error(&mut self, error: String);

    /// Whether a window of the screen is open.
    fn has_window(&self) -> bool;

    /// Close the screen's windows and questions.
    fn close_windows(&mut self);
}

/// A contest chosen by a [`ContestEntry`] window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChosenContest {
    /// A linked contest, by [`ContestDef::id`].
    Linked(String),
    /// A contest defined by a file.
    File(PathBuf),
}

/// What a [`ContestEntry`] window sees.
pub struct EntryView<'a> {
    pub registry: &'a Registry,
    pub language: &'a str,
    /// The folder of contest files (zLog's `CFGDATPath`), shared by the start
    /// screen and every entry; empty when none was chosen yet.
    pub contest_folder: &'a mut String,
}

/// An entry of the contest menu of the start screen (its left column).
///
/// Choosing an entry either lists contests of the registry ([`ContestEntry::contests`]
/// returns them) or opens a window ([`ContestEntry::open`]) that later returns a
/// contest ([`ContestEntry::ui`]).
pub trait ContestEntry {
    /// Stable identifier.
    fn id(&self) -> &str;

    /// Text of the entry, in `language`.
    fn title(&self, registry: &Registry, language: &str) -> String;

    /// Whether the entry is shown with the contests linked in.
    fn shown(&self, _registry: &Registry) -> bool {
        true
    }

    /// The contests listed when the entry is chosen, as indices into
    /// `Registry::contests()`; `None` for an entry that opens a window instead.
    fn contests(&self, _registry: &Registry, _language: &str) -> Option<Vec<usize>> {
        None
    }

    /// The entry was chosen: open its window.
    fn open(&mut self, _view: &mut EntryView<'_>) {}

    /// Draw the entry's window, every frame; returns the contest chosen in it.
    fn ui(&mut self, _ctx: &egui::Context, _view: &mut EntryView<'_>) -> Option<ChosenContest> {
        None
    }

    /// Whether the entry's window is open.
    fn is_open(&self) -> bool {
        false
    }

    /// Close the entry's window.
    fn close(&mut self) {}
}

/// A group of the linked contests ([`ContestDef::group`]), listing its contests.
struct ContestGroup {
    /// Identifier of the first contest of the group, which names it.
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

/// One entry for each group of the contests of `registry`, in the order their
/// first contests are linked.
pub fn contest_groups(registry: &Registry) -> Vec<Box<dyn ContestEntry>> {
    let mut seen: Vec<String> = Vec::new();
    let mut entries: Vec<Box<dyn ContestEntry>> = Vec::new();
    for c in registry.contests() {
        // Grouped in English; the groups are the same in every language.
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

/// The station kept in a settings section (`station`): one key a field, the power
/// of each band as `power.<band>`.
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

/// Write `st` into a settings section, as [`read_station`] reads it.
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
