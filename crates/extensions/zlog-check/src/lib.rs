pub mod matching;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use egui::{RichText, Ui};
use zlog_core::{Band, Contest, Log, Qso};
use zlog_ui::{EntryAction, EntryField, Panel, Section, View};

use matching::{Database, PartialOptions, SortBy};

zlog_core::texts!();

pub fn panels() -> Vec<Box<dyn Panel>> {
    vec![
        Box::new(Candidates::default()),
        Box::new(ThisStation::default()),
    ]
}

pub const MERGED_PANELS: [(&str, &str); 4] = [
    ("partial-check", CANDIDATES),
    ("super-check", CANDIDATES),
    ("check-call", THIS_STATION),
    ("check-multi", THIS_STATION),
];

fn merged_into(new: &str) -> Vec<zlog_ui::migration::Migration> {
    MERGED_PANELS
        .iter()
        .filter(|(_, n)| *n == new)
        .map(|&(old, n)| zlog_ui::migration::Migration::MergedPanel(old, n))
        .collect()
}

pub const CANDIDATES: &str = "call-candidates";

pub const THIS_STATION: &str = "this-station";

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().strong());
}

fn flag(section: &Section, key: &str, default: bool) -> bool {
    section.get(key).map_or(default, |v| v == "1")
}

fn put_flag(section: &mut Section, key: &str, on: bool) {
    section.insert(key.into(), if on { "1" } else { "0" }.into());
}

const MAX_SUPER_HIT: usize = 100;

fn summary(q: &Qso) -> String {
    let (_, _, _, h, m, _) = q.time.to_ymd_hms();
    format!(
        "{h:02}:{m:02} {:<12} {:<8} {:>5} {}",
        q.callsign,
        q.nr_rcvd,
        q.band.label(),
        q.mode.label()
    )
}

fn line(ui: &mut Ui, text: String, highlight: bool) -> bool {
    let mut t = RichText::new(text).monospace();
    if highlight {
        t = t.strong().color(ui.visuals().warn_fg_color);
    }
    ui.selectable_label(false, t).clicked()
}

fn contest_bands(view: &View<'_>) -> Vec<Band> {
    let (low, high) = view.contest.band_range();
    let warc = view.contest.uses_warc();
    Band::range(low, high)
        .filter(|b| warc || !b.is_warc())
        .collect()
}

type PartialKey = (String, bool, Band, PartialOptions, u64, usize, String);

#[derive(Default)]
struct InLog {
    options: PartialOptions,
    by_number: bool,
    key: Option<PartialKey>,
    hits: Vec<usize>,
}

impl InLog {
    fn options(&mut self, ui: &mut Ui, lang: &str) {
        let o = &mut self.options;
        ui.checkbox(&mut o.all_bands, texts().get(lang, "all_bands"));
        ui.checkbox(&mut o.this_band_first, texts().get(lang, "this_band_first"));
        ui.checkbox(&mut self.by_number, texts().get(lang, "by_number"));
        ui.separator();
        ui.label(texts().get(lang, "sort"));
        ui.radio_value(&mut o.sort, SortBy::Time, texts().get(lang, "sort_time"));
        ui.radio_value(&mut o.sort, SortBy::Band, texts().get(lang, "sort_band"));
        ui.radio_value(&mut o.sort, SortBy::Call, texts().get(lang, "sort_call"));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(texts().get(lang, "show_max"));
            ui.add(egui::DragValue::new(&mut o.max).range(1..=10000));
        });
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>, actions: &mut Vec<EntryAction>) {
        heading(ui, texts().get(view.language, "partial.heading"));
        let by_number = number_mode(self.by_number, view.focus);
        let fragment = if by_number {
            view.number
        } else {
            view.callsign
        };
        let key = (
            fragment.to_string(),
            by_number,
            view.band,
            self.options,
            view.changes,
            view.log.qsos.len(),
            view.log_path.to_string(),
        );
        self.refresh(view.log, key);
        egui::ScrollArea::vertical()
            .id_salt("partial")
            .auto_shrink(false)
            .show(ui, |ui| {
                for &i in &self.hits {
                    let q = &view.log.qsos[i];
                    if line(ui, summary(q), q.band == view.band) {
                        actions.push(EntryAction::Callsign(q.callsign.clone()));
                    }
                }
            });
    }

    fn first(&self, log: &Log) -> Option<String> {
        self.hits.first().map(|&i| log.qsos[i].callsign.clone())
    }

    fn refresh(&mut self, log: &Log, key: PartialKey) {
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let (fragment, by_number, band) = (&key.0, key.1, key.2);
        self.hits = if by_number {
            matching::partial_check_number(log, fragment, band, &self.options)
        } else {
            matching::partial_check(log, fragment, band, &self.options)
        };
        self.key = Some(key);
    }

    fn load(&mut self, s: &Section) {
        let o = &mut self.options;
        o.all_bands = flag(s, "all_bands", o.all_bands);
        o.this_band_first = flag(s, "this_band_first", o.this_band_first);
        o.sort = match s.get("sort").map(String::as_str) {
            Some("band") => SortBy::Band,
            Some("call") => SortBy::Call,
            Some("time") => SortBy::Time,
            _ => o.sort,
        };
        if let Some(n) = s.get("max").and_then(|v| v.parse().ok()) {
            o.max = n;
        }
        self.by_number = flag(s, "by_number", self.by_number);
    }

    fn save(&self, s: &mut Section) {
        let o = &self.options;
        put_flag(s, "all_bands", o.all_bands);
        put_flag(s, "this_band_first", o.this_band_first);
        let sort = match o.sort {
            SortBy::Time => "time",
            SortBy::Band => "band",
            SortBy::Call => "call",
        };
        s.insert("sort".into(), sort.into());
        s.insert("max".into(), o.max.to_string());
        put_flag(s, "by_number", self.by_number);
    }
}

fn number_mode(by_number: bool, focus: Option<EntryField>) -> bool {
    by_number || focus == Some(EntryField::Number)
}

fn super_pick(
    contest: &dyn Contest,
    band: Band,
    call: &str,
    number: Option<&str>,
) -> Vec<EntryAction> {
    let mut out = vec![EntryAction::Callsign(call.to_string())];
    let word = number.and_then(|n| n.split_whitespace().next());
    if let Some(word) = word {
        let past = Qso {
            callsign: call.to_string(),
            band,
            nr_rcvd: word.to_string(),
            ..Qso::default()
        };
        if let Some(n) = contest.exchange_from(band, &past).filter(|n| !n.is_empty()) {
            out.push(EntryAction::Number(n));
        }
    }
    out
}

enum Source {
    None,
    Loading(Receiver<Result<Database, String>>),
    Failed(String),
    Ready(Database),
}

pub fn load_database(path: &Path) -> Result<Database, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let records = if ext == "ZLO" || ext == "ZLOX" {
        let (log, _) = Log::load(path).map_err(|e| format!("{e:?}"))?;
        matching::log_records(&log)
    } else {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        matching::parse_spc(&String::from_utf8_lossy(&bytes))
    };
    Ok(Database::new(records))
}

struct Known {
    path: Option<PathBuf>,
    source: Source,
    generation: u64,
    key: Option<(String, u64)>,
    partial: Vec<(usize, Option<usize>)>,
    near: Vec<matching::NearCall>,
}

impl Default for Known {
    fn default() -> Self {
        Known {
            path: None,
            source: Source::None,
            generation: 0,
            key: None,
            partial: Vec::new(),
            near: Vec::new(),
        }
    }
}

impl Known {
    fn start_loading(&mut self, path: PathBuf) {
        let (tx, rx) = channel();
        let p = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load_database(&p));
        });
        self.path = Some(path);
        self.source = Source::Loading(rx);
    }

    fn open(&mut self, ui: &mut Ui, lang: &str) {
        if zlog_ui::layout::tool(ui, zlog_ui::icon::FOLDER_OPEN, texts().get(lang, "open"))
            .on_hover_text(texts().get(lang, "super.heading"))
            .clicked()
        {
            let picked =
                zlog_ui::dialog::import_file("SCP/SPC/ZLO", &["scp", "spc", "zlo", "zlox"]);
            if let Some(p) = picked {
                self.start_loading(p);
            }
        }
    }

    fn state(&self, lang: &str) -> (String, bool) {
        match &self.source {
            Source::None => (texts().get(lang, "no_database").into(), false),
            Source::Loading(_) => (texts().get(lang, "loading").into(), false),
            Source::Failed(e) => (format!("{} {e}", texts().get(lang, "load_failed")), true),
            Source::Ready(db) => {
                let name = self
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                (
                    format!("{name} {} {}", db.len(), texts().get(lang, "calls")),
                    false,
                )
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>, actions: &mut Vec<EntryAction>) {
        let lang = view.language;
        if let Source::Loading(rx) = &self.source {
            match rx.try_recv() {
                Ok(Ok(db)) => {
                    self.source = Source::Ready(db);
                    self.generation += 1;
                }
                Ok(Err(e)) => self.source = Source::Failed(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint(),
                Err(_) => self.source = Source::Failed(String::new()),
            }
        }
        heading(ui, texts().get(lang, "super.heading"));
        let Source::Ready(db) = &self.source else {
            return;
        };
        let key = (view.callsign.to_string(), self.generation);
        if self.key.as_ref() != Some(&key) {
            self.partial = db.lines(view.callsign, MAX_SUPER_HIT);
            self.near = db.n_plus_one(view.callsign, MAX_SUPER_HIT);
            self.key = Some(key);
        }
        let mut picked = None;
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical()
                .id_salt("super")
                .auto_shrink(false)
                .show(&mut cols[0], |ui| {
                    for &(i, k) in &self.partial {
                        let e = db.entry(i);
                        let number = k.map(|k| e.numbers[k].as_str());
                        let text = format!("{:<11} {}", e.callsign, number.unwrap_or(""));
                        if line(ui, text, e.callsign == view.callsign) {
                            picked = Some((e.callsign.clone(), number.map(str::to_string)));
                        }
                    }
                });
            egui::ScrollArea::vertical()
                .id_salt("nplus1")
                .auto_shrink(false)
                .show(&mut cols[1], |ui| {
                    for n in &self.near {
                        let e = db.entry(n.index);
                        if line(
                            ui,
                            format!("{} {}", n.marker(), e.callsign),
                            n.distance == 0,
                        ) {
                            picked = Some((e.callsign.clone(), None));
                        }
                    }
                });
        });
        if let Some((c, n)) = picked {
            actions.extend(super_pick(view.contest, view.band, &c, n.as_deref()));
        }
    }

    fn first(&self) -> Option<String> {
        let Source::Ready(db) = &self.source else {
            return None;
        };
        self.partial
            .first()
            .map(|&(i, _)| db.entry(i).callsign.clone())
    }

    fn load(&mut self, section: &Section) {
        if let Some(p) = section.get("super_check_file").filter(|p| !p.is_empty()) {
            self.start_loading(PathBuf::from(p));
        }
    }

    fn save(&self, section: &mut Section) {
        if let Some(p) = &self.path {
            section.insert("super_check_file".into(), p.display().to_string());
        }
    }
}

pub struct Candidates {
    in_log: InLog,
    known: Known,
    show_in_log: bool,
    show_known: bool,
    take: bool,
    actions: Vec<EntryAction>,
    split: f32,
}

const SPLIT: f32 = 0.5;

pub const TAKE_HIT: &str = "take_hit";

impl Candidates {
    fn hit(&self, log: &Log) -> Option<String> {
        let in_log = self.show_in_log.then(|| self.in_log.first(log)).flatten();
        in_log.or_else(|| self.show_known.then(|| self.known.first()).flatten())
    }
}

impl Default for Candidates {
    fn default() -> Self {
        Candidates {
            in_log: InLog::default(),
            known: Known::default(),
            show_in_log: true,
            show_known: true,
            take: false,
            actions: Vec::new(),
            split: SPLIT,
        }
    }
}

impl Panel for Candidates {
    fn migrations(&self) -> Vec<zlog_ui::migration::Migration> {
        merged_into(CANDIDATES)
    }

    fn id(&self) -> &str {
        CANDIDATES
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Check, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "candidates").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![
            zlog_ui::Command::window(
                texts().get(language, "toolbar.candidates"),
                texts().get(language, "toolbar.candidates.tip"),
            )
            .icon(zlog_ui::icon::MAGNIFYING_GLASS)
            .menu_label(texts().get(language, "candidates")),
            zlog_ui::Command::new(
                TAKE_HIT,
                texts().get(language, "take_hit"),
                texts().get(language, "take_hit.tip"),
            )
            .shortcut(egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::I,
            ))
            .role(TAKE_HIT),
        ]
    }

    fn run_command(&mut self, id: &str) {
        if id == TAKE_HIT {
            self.take = true;
        }
    }

    fn default_size(&self) -> [f32; 2] {
        [720.0, 320.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let (state, failed) = self.known.state(lang);
        let status = if self.show_known {
            vec![(state.as_str(), failed)]
        } else {
            Vec::new()
        };
        zlog_ui::layout::toolbar(ui, &status, |ui| {
            use zlog_ui::{icon, layout};
            let t = |k| texts().get(lang, k);
            if layout::toggle(ui, icon::LIST_CHECKS, t("show_in_log"), self.show_in_log).clicked() {
                self.show_in_log = !self.show_in_log;
            }
            if layout::toggle(ui, icon::USER_LIST, t("show_known"), self.show_known).clicked() {
                self.show_known = !self.show_known;
            }
            let glyph = egui::RichText::new(icon::GEAR).family(icon::family());
            ui.menu_button((glyph, egui::RichText::new(t("view"))), |ui| {
                self.in_log.options(ui, lang)
            });
            self.known.open(ui, lang);
        });
        let actions = &mut self.actions;
        match (self.show_in_log, self.show_known) {
            (true, true) => {
                let (in_log, known) = (&mut self.in_log, &mut self.known);
                let mut more = Vec::new();
                zlog_ui::layout::split(
                    ui,
                    &mut self.split,
                    |ui| in_log.ui(ui, view, actions),
                    |ui| known.ui(ui, view, &mut more),
                );
                actions.append(&mut more);
            }
            (true, false) => self.in_log.ui(ui, view, actions),
            (false, true) => self.known.ui(ui, view, actions),
            (false, false) => {}
        }
        if std::mem::take(&mut self.take) {
            if let Some(c) = self.hit(view.log) {
                self.actions.push(EntryAction::TakeCallsign(c));
            }
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        self.take = false;
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &Section) {
        self.show_in_log = flag(section, "show_in_log", true);
        self.show_known = flag(section, "show_known", true);
        self.split = zlog_ui::layout::load_ratio(section, "split", SPLIT);
        self.in_log.load(section);
        self.known.load(section);
    }

    fn save(&self, section: &mut Section) {
        put_flag(section, "show_in_log", self.show_in_log);
        put_flag(section, "show_known", self.show_known);
        zlog_ui::layout::save_ratio(section, "split", self.split);
        self.in_log.save(section);
        self.known.save(section);
    }
}

fn entry_row(view: &View<'_>) -> Qso {
    Qso {
        callsign: view.callsign.to_string(),
        band: view.band,
        mode: view.mode,
        nr_rcvd: view.number.to_string(),
        ..Qso::default()
    }
}

#[derive(Default)]
pub struct ThisStation {
    pinned: Option<(String, String)>,
    entry: String,
}

const CHECK_CALL: &str = "check-call";

pub fn entry_multi(contest: &dyn Contest, q: &Qso) -> Option<String> {
    Some(contest.extract_multi(q)).filter(|c| !c.is_empty())
}

fn cw_and_ph(contest: &dyn Contest) -> bool {
    contest.name() == "ARRL 10m Contest"
}

impl Panel for ThisStation {
    fn migrations(&self) -> Vec<zlog_ui::migration::Migration> {
        merged_into(THIS_STATION)
    }

    fn id(&self) -> &str {
        THIS_STATION
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Check, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "station").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar.station"),
            texts().get(language, "toolbar.station.tip"),
        )
        .icon(zlog_ui::icon::USER_CHECK)
        .menu_label(texts().get(language, "station"))
        .role("check")]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 240.0]
    }

    fn qso_menu(&self, language: &str, qso: &zlog_core::Qso) -> Vec<zlog_ui::QsoMenuItem> {
        vec![
            zlog_ui::QsoMenuItem::new(CHECK_CALL, texts().get(language, "check_this_call"))
                .enabled(!qso.callsign.trim().is_empty()),
        ]
    }

    fn on_qso_menu(&mut self, id: &str, qso: &zlog_core::Qso) -> bool {
        if id != CHECK_CALL {
            return false;
        }
        let call = qso.callsign.trim().to_ascii_uppercase();
        self.pinned = Some((call, self.entry.clone()));
        true
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        self.entry = view.callsign.to_string();
        if self.pinned.as_ref().is_some_and(|(_, e)| *e != self.entry) {
            self.pinned = None;
        }
        let pinned = self.pinned.as_ref().map(|(c, _)| c.as_str());
        let call = pinned.unwrap_or(view.callsign);
        let bands = contest_bands(view);
        let worked = matching::check_call(view.log, call, &bands, &entry_row(view));
        let multi = match pinned {
            None => entry_multi(view.contest, &entry_row(view)),
            Some(c) => view
                .log
                .qsos
                .iter()
                .rev()
                .find(|q| q.callsign.eq_ignore_ascii_case(c))
                .and_then(|q| entry_multi(view.contest, q)),
        };
        let name = multi.as_ref().and_then(|code| {
            view.contest
                .multi_table()
                .into_iter()
                .find(|(c, _, _)| c == code)
                .map(|(_, n, _)| n)
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(call).monospace().strong());
            match &multi {
                Some(code) => ui.label(
                    RichText::new(format!("{code} {}", name.unwrap_or_default())).monospace(),
                ),
                None => ui.label(texts().get(lang, "no_multi")),
            };
        });
        let multis = multi
            .as_deref()
            .map(|code| matching::check_multi(view.log, code, &bands, cw_and_ph(view.contest)))
            .unwrap_or_default();
        egui::ScrollArea::horizontal().show(ui, |ui| {
            let gap = ui.spacing().item_spacing.x;
            let n = bands.len() as f32 + 1.0;
            let room = (ui.available_width() - gap * (n - 1.0)) / n;
            egui::Grid::new("this-station")
                .striped(true)
                .min_col_width(room.max(36.0))
                .show(ui, |ui| {
                    ui.label("");
                    for b in &bands {
                        let t = RichText::new(b.label()).monospace();
                        let t = if *b == view.band { t.strong() } else { t };
                        ui.centered_and_justified(|ui| ui.label(t));
                    }
                    ui.end_row();
                    heading(ui, texts().get(lang, "call.heading"));
                    for (b, hit) in &worked {
                        let text = hit.map_or("-".to_string(), |i| {
                            view.log.qsos[i].mode.label().to_string()
                        });
                        let t = RichText::new(text).monospace();
                        let r = ui.label(if *b == view.band { t.strong() } else { t });
                        if let Some(i) = hit {
                            r.on_hover_text(summary(&view.log.qsos[*i]));
                        }
                    }
                    ui.end_row();
                    heading(ui, texts().get(lang, "multi.heading"));
                    for b in &bands {
                        let cells: Vec<_> = multis.iter().filter(|r| r.band == *b).collect();
                        let text = if cells.is_empty() {
                            "-".to_string()
                        } else {
                            cells
                                .iter()
                                .map(|r| match (r.cw, r.hit) {
                                    (_, None) => "-",
                                    (None, Some(_)) => "*",
                                    (Some(true), Some(_)) => "CW",
                                    (Some(false), Some(_)) => "Ph",
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        };
                        ui.label(RichText::new(text).monospace());
                    }
                    ui.end_row();
                });
        });
        for (_, hit) in &worked {
            if let Some(i) = hit {
                ui.label(
                    RichText::new(summary(&view.log.qsos[*i]))
                        .monospace()
                        .small(),
                );
            }
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panels(150, |_| panels());
}

#[cfg(test)]
mod tests {
    #[test]
    fn check_call_from_the_qso_menu_lasts_until_the_entry_changes() {
        let mut c = ThisStation::default();
        let q = zlog_core::Qso {
            callsign: "ja1zlo".into(),
            ..Default::default()
        };
        assert_eq!(c.qso_menu("en", &q)[0].id, CHECK_CALL);
        assert!(c.on_qso_menu(CHECK_CALL, &q));
        assert!(!c.on_qso_menu("other", &q));
        assert_eq!(c.pinned, Some(("JA1ZLO".into(), String::new())));
    }

    use super::*;

    #[test]
    fn partial_cache_follows_the_book_changes() {
        let mut log = Log {
            qsos: vec![Qso {
                callsign: "JA1ZLO".into(),
                band: Band::B7,
                ..Qso::default()
            }],
            ..Log::default()
        };
        let mut p = InLog::default();
        let key = |changes| {
            let o = PartialOptions::default();
            (
                "ZL".to_string(),
                false,
                Band::B7,
                o,
                changes,
                1,
                String::new(),
            )
        };
        p.refresh(&log, key(1));
        assert_eq!(p.hits, [0]);
        log.qsos[0].callsign = "JA1YAA".into();
        p.refresh(&log, key(1));
        assert_eq!(p.hits, [0], "same changes count keeps the cached hits");
        p.refresh(&log, key(2));
        assert!(p.hits.is_empty());
    }

    fn candidates_with(in_log: &[usize], known: &[&str]) -> (Candidates, Log) {
        let call = |c: &str| Qso {
            callsign: c.into(),
            band: Band::B7,
            ..Qso::default()
        };
        let log = Log {
            qsos: vec![call("JA1ZLO"), call("JA1YWX")],
            ..Log::default()
        };
        let mut c = Candidates::default();
        c.in_log.hits = in_log.to_vec();
        let spc: String = known.iter().map(|k| format!("{k}\n")).collect();
        let db = Database::new(matching::parse_spc(&spc));
        c.known.partial = (0..db.len()).map(|i| (i, None)).collect();
        c.known.source = Source::Ready(db);
        (c, log)
    }

    #[test]
    fn take_hit_prefers_the_first_partial_check_line() {
        let (c, log) = candidates_with(&[1, 0], &["JA1ZLO"]);
        assert_eq!(c.hit(&log).as_deref(), Some("JA1YWX"));
    }

    #[test]
    fn take_hit_falls_back_to_the_first_super_check_line() {
        let (mut c, log) = candidates_with(&[], &["JA1YWX", "JA1ZLO"]);
        assert_eq!(c.hit(&log).as_deref(), Some("JA1YWX"));
        c.show_known = false;
        assert_eq!(c.hit(&log), None);
    }

    #[test]
    fn take_hit_skips_a_hidden_partial_check() {
        let (mut c, log) = candidates_with(&[0], &["JA1YWX"]);
        c.show_in_log = false;
        assert_eq!(c.hit(&log).as_deref(), Some("JA1YWX"));
    }

    #[test]
    fn take_hit_without_hits_is_none() {
        let (c, log) = candidates_with(&[], &[]);
        assert_eq!(c.hit(&log), None);
    }

    #[test]
    fn take_hit_is_a_command_on_ctrl_i_and_lasts_one_frame() {
        let mut c = Candidates::default();
        let take = c
            .commands("en")
            .into_iter()
            .find(|k| k.id == TAKE_HIT)
            .unwrap();
        assert_eq!(
            take.shortcut,
            Some(egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::I
            ))
        );
        c.run_command(TAKE_HIT);
        assert!(c.take);
        assert!(c.take_actions().is_empty());
        assert!(!c.take);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn loads_scp_file() {
        let dir = std::env::temp_dir().join(format!("zlog-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("MASTER.SCP");
        std::fs::write(&p, "# test\nJA1ZLO\nW1AW\n").unwrap();
        let db = load_database(&p).unwrap();
        assert_eq!(db.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(load_database(&p).is_err());
    }

    use zlog_core::{ContestMode, MultiCheck, Power, ScoreSheet, Scoring, Station};

    struct Fixed {
        same_exchange: bool,
        name: &'static str,
    }

    impl Contest for Fixed {
        fn name(&self) -> &str {
            self.name
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::B50)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Cw
        }
        fn sent_number(&self, _band: Band, _power: Power, _station: &Station) -> String {
            String::new()
        }
        fn exchange_from(&self, _band: Band, past: &Qso) -> Option<String> {
            self.same_exchange.then(|| past.nr_rcvd.clone())
        }
        fn valid_number(&self, _q: &Qso) -> bool {
            true
        }
        fn score(&mut self, _log: &Log) -> Scoring {
            Scoring {
                qsos: Vec::new(),
                sheet: ScoreSheet::default(),
            }
        }
        fn check_multi(&self, q: &Qso) -> MultiCheck {
            match q.nr_rcvd.as_str() {
                "" => MultiCheck::Invalid,
                n => MultiCheck::Known {
                    code: n.to_string(),
                    name: String::new(),
                    needed: false,
                    worked_on: Vec::new(),
                },
            }
        }
    }

    const SAME: Fixed = Fixed {
        same_exchange: true,
        name: "test",
    };

    #[test]
    fn super_pick_fills_first_word_of_number() {
        assert_eq!(
            super_pick(&SAME, Band::B7, "W1AW", Some("MA extra")),
            vec![
                EntryAction::Callsign("W1AW".into()),
                EntryAction::Number("MA".into())
            ]
        );
    }

    #[test]
    fn super_pick_replaces_typed_number() {
        assert_eq!(
            super_pick(&SAME, Band::B7, "W1AW", Some("MA")),
            vec![
                EntryAction::Callsign("W1AW".into()),
                EntryAction::Number("MA".into())
            ]
        );
    }

    #[test]
    fn super_pick_callsign_comes_first() {
        let a = super_pick(&SAME, Band::B7, "W1AW", Some("\tMA\t"));
        assert_eq!(a[0], EntryAction::Callsign("W1AW".into()));
        assert_eq!(a[1], EntryAction::Number("MA".into()));
        assert_eq!(a.len(), 2);
        assert_eq!(
            super_pick(&SAME, Band::B7, "W1AW", Some("")),
            vec![EntryAction::Callsign("W1AW".into())]
        );
    }

    #[test]
    fn super_pick_without_number_or_same_exchange() {
        let only_call = vec![EntryAction::Callsign("W1AW".into())];
        assert_eq!(super_pick(&SAME, Band::B7, "W1AW", None), only_call);
        assert_eq!(super_pick(&SAME, Band::B7, "W1AW", Some("  ")), only_call);
        let serial = Fixed {
            same_exchange: false,
            name: "test",
        };
        assert_eq!(
            super_pick(&serial, Band::B7, "W1AW", Some("599")),
            only_call
        );
    }

    #[test]
    fn number_mode_toggle_and_focus() {
        assert!(!number_mode(false, None));
        assert!(number_mode(true, None));
        assert!(number_mode(false, Some(EntryField::Number)));
        for f in [
            EntryField::Call,
            EntryField::RstSent,
            EntryField::RstRcvd,
            EntryField::Memo,
        ] {
            assert!(!number_mode(false, Some(f)));
            assert!(number_mode(true, Some(f)));
        }
    }

    #[test]
    fn entry_multi_only_from_entry_row() {
        let q = Qso {
            callsign: "JA1ZLO".into(),
            ..Qso::default()
        };
        assert_eq!(entry_multi(&SAME, &q), None);
        let q = Qso {
            nr_rcvd: "13".into(),
            ..q
        };
        assert_eq!(entry_multi(&SAME, &q), Some("13".into()));
    }

    struct Unknown;

    impl Contest for Unknown {
        fn name(&self) -> &str {
            "unknown"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::B50)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Cw
        }
        fn sent_number(&self, _band: Band, _power: Power, _station: &Station) -> String {
            String::new()
        }
        fn valid_number(&self, _q: &Qso) -> bool {
            true
        }
        fn score(&mut self, _log: &Log) -> Scoring {
            Scoring {
                qsos: Vec::new(),
                sheet: ScoreSheet::default(),
            }
        }
    }

    #[test]
    fn entry_multi_unknown_to_the_contest() {
        let q = Qso {
            nr_rcvd: "ZZ9".into(),
            ..Qso::default()
        };
        assert_eq!(Unknown.check_multi(&q), MultiCheck::Invalid);
        assert_eq!(entry_multi(&Unknown, &q), Some("ZZ9".into()));
        let mut log = Log::default();
        log.qsos.push(Qso {
            callsign: "JA1ZLO".into(),
            band: Band::B7,
            multi1: "ZZ9".into(),
            ..Qso::default()
        });
        let rows = matching::check_multi(&log, "ZZ9", &[Band::B3_5, Band::B7], false);
        assert_eq!(rows[0].hit, None);
        assert_eq!(rows[1].hit, Some(0));
    }

    #[test]
    fn entry_multi_empty_and_blank() {
        assert_eq!(entry_multi(&Unknown, &Qso::default()), None);
        assert_eq!(entry_multi(&SAME, &Qso::default()), None);
        let q = Qso {
            nr_rcvd: " 1".into(),
            ..Qso::default()
        };
        assert_eq!(entry_multi(&Unknown, &q), Some(" 1".into()));
    }

    #[test]
    fn cw_and_ph_only_for_arrl10() {
        assert!(!cw_and_ph(&SAME));
        let arrl = Fixed {
            same_exchange: true,
            name: "ARRL 10m Contest",
        };
        assert!(cw_and_ph(&arrl));
    }
}
