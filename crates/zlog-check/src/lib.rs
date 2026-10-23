//! zLog's check windows: partial check (`UPartials.pas`), super check
//! (`USuperCheck.pas`, `USuperCheck2.pas`, `UzLogSpc.pas`), check call
//! (`UCheckCall2.pas`) and check multi (`UCheckMulti.pas`).
//!
//! Matching lives in [`matching`]; the panels only cache its results and redo them
//! when the callsign in the entry row (or what they depend on) changes.

pub mod matching;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use egui::{RichText, Ui};
use zlog_core::{Band, Contest, Log, Qso};
use zlog_ui::{EntryAction, EntryField, Panel, Section, View};

use matching::{Database, PartialOptions, SortBy};

zlog_core::texts!();

/// The four check windows, in the order of zLog's Windows menu.
pub fn panels() -> Vec<Box<dyn Panel>> {
    vec![
        Box::new(PartialCheck::default()),
        Box::new(SuperCheck::default()),
        Box::new(CheckCall::default()),
        Box::new(CheckMulti),
    ]
}

/// zLog's `MaxSuperHit` default.
const MAX_SUPER_HIT: usize = 100;

/// One line of a list (`TQSO.PartialSummary`): time, call, number, band, mode.
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

/// A clickable monospace line; returns whether it was clicked.
fn line(ui: &mut Ui, text: String, highlight: bool) -> bool {
    let mut t = RichText::new(text).monospace();
    if highlight {
        t = t.strong().color(ui.visuals().warn_fg_color);
    }
    ui.selectable_label(false, t).clicked()
}

/// Bands of the open contest, in order (the rows of `TCheckWin`).
fn contest_bands(view: &View<'_>) -> Vec<Band> {
    let (low, high) = view.contest.band_range();
    let warc = view.contest.uses_warc();
    Band::range(low, high)
        .filter(|b| warc || !b.is_warc())
        .collect()
}

/// The partial-check window (`TPartialCheck`).
#[derive(Default)]
pub struct PartialCheck {
    options: PartialOptions,
    /// Check received numbers instead of callsigns (zLog does so while the number
    /// field has the focus); also on whenever a number is typed in the entry row.
    by_number: bool,
    /// What the cached hits were computed from: fragment, number mode, band,
    /// options, log size.
    key: Option<(String, bool, Band, PartialOptions, usize)>,
    hits: Vec<usize>,
    actions: Vec<EntryAction>,
}

impl Panel for PartialCheck {
    fn id(&self) -> &str {
        "partial-check"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Check, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "partial").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar.partial"),
            texts().get(language, "toolbar.partial.tip"),
        )
        .icon(zlog_ui::icon::MAGNIFYING_GLASS)
        .menu_label(texts().get(language, "menu.partial"))]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let o = &mut self.options;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut o.all_bands, texts().get(lang, "all_bands"));
            ui.checkbox(&mut o.this_band_first, texts().get(lang, "this_band_first"));
            ui.label(texts().get(lang, "sort"));
            ui.radio_value(&mut o.sort, SortBy::Time, texts().get(lang, "sort_time"));
            ui.radio_value(&mut o.sort, SortBy::Band, texts().get(lang, "sort_band"));
            ui.radio_value(&mut o.sort, SortBy::Call, texts().get(lang, "sort_call"));
            ui.label(texts().get(lang, "show_max"));
            ui.add(egui::DragValue::new(&mut o.max).range(1..=10000));
            ui.checkbox(&mut self.by_number, texts().get(lang, "by_number"));
        });
        ui.separator();
        // Edits that keep the number of QSOs are not seen until the next keystroke.
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
            view.log.qsos.len(),
        );
        if self.key.as_ref() != Some(&key) {
            self.hits = if by_number {
                matching::partial_check_number(view.log, fragment, view.band, &self.options)
            } else {
                matching::partial_check(view.log, fragment, view.band, &self.options)
            };
            self.key = Some(key);
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for &i in &self.hits {
                let q = &view.log.qsos[i];
                if line(ui, summary(q), q.band == view.band) {
                    self.actions.push(EntryAction::Callsign(q.callsign.clone()));
                }
            }
        });
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }
}

/// Whether the partial check looks at received numbers (`CheckPartialNumber`, run
/// by zLog from the number field, main.pas `NumberEditChange`): when switched on,
/// or while the number field has the keyboard.
fn number_mode(by_number: bool, focus: Option<EntryField>) -> bool {
    by_number || focus == Some(EntryField::Number)
}

/// What a click on a super-check line does (`GridDblClick`, `SetYourCallsignEx`):
/// the callsign, which clears the number (`N.Text := ''`), then the first word of
/// its number when the contest keeps the exchange (`SameExchange`).
///
/// Not implemented: zLog's `AutoEnterSuper` (fill the estimate on Space); the
/// panel has no hook for the space bar.
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

/// State of the super-check database.
enum Source {
    None,
    Loading(Receiver<Result<Database, String>>),
    Failed(String),
    Ready(Database),
}

/// Read a database file: MASTER.SCP or an SPC file as text, a ZLO/ZLOX log as a log
/// (zLog's SuperCheckMethod 0 and 1, which read every such file of a folder).
///
/// Departure: zLog takes a folder and loads all `*.SPC`, `MASTER.SCP` and `*.ZLO?`
/// files in it; here one file is chosen.
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

/// The super-check window with its N+1 list (`TSuperCheck`, `TSuperCheck2`).
///
/// zLog shows the two lists in two windows; here they are side by side in one.
pub struct SuperCheck {
    path: Option<PathBuf>,
    source: Source,
    /// Database generation, bumped on each load, so caches see a new database.
    generation: u64,
    key: Option<(String, u64)>,
    /// Lines of the super-check list: entry, and which of its numbers.
    partial: Vec<(usize, Option<usize>)>,
    near: Vec<matching::NearCall>,
    actions: Vec<EntryAction>,
}

impl Default for SuperCheck {
    fn default() -> Self {
        SuperCheck {
            path: None,
            source: Source::None,
            generation: 0,
            key: None,
            partial: Vec::new(),
            near: Vec::new(),
            actions: Vec::new(),
        }
    }
}

impl SuperCheck {
    fn start_loading(&mut self, path: PathBuf) {
        let (tx, rx) = channel();
        let p = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load_database(&p));
        });
        self.path = Some(path);
        self.source = Source::Loading(rx);
    }
}

impl Panel for SuperCheck {
    fn id(&self) -> &str {
        "super-check"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Check, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "super").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar.super"),
            texts().get(language, "toolbar.super.tip"),
        )
        .icon(zlog_ui::icon::LIST_MAGNIFYING_GLASS)
        .menu_label(texts().get(language, "menu.super"))]
    }

    fn default_size(&self) -> [f32; 2] {
        [480.0, 300.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
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
        ui.horizontal(|ui| {
            if ui.button(texts().get(lang, "open")).clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter(
                        "SCP/SPC/ZLO",
                        &["scp", "SCP", "spc", "SPC", "zlo", "ZLO", "zlox", "ZLOX"],
                    )
                    .pick_file();
                if let Some(p) = picked {
                    self.start_loading(p);
                }
            }
            match &self.source {
                Source::None => ui.label(texts().get(lang, "no_database")),
                Source::Loading(_) => ui.label(texts().get(lang, "loading")),
                Source::Failed(e) => ui.label(format!("{} {e}", texts().get(lang, "load_failed"))),
                Source::Ready(db) => {
                    ui.label(format!("{} {}", db.len(), texts().get(lang, "calls")))
                }
            };
        });
        if let Some(p) = &self.path {
            ui.small(p.display().to_string());
        }
        ui.separator();
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
            self.actions
                .extend(super_pick(view.contest, view.band, &c, n.as_deref()));
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
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

/// A QSO standing for the entry row, as far as a panel can see it.
fn entry_row(view: &View<'_>) -> Qso {
    Qso {
        callsign: view.callsign.to_string(),
        band: view.band,
        mode: view.mode,
        nr_rcvd: view.number.to_string(),
        ..Qso::default()
    }
}

/// Rows of a check window: label, band, then the QSO found there, if any.
fn band_rows(ui: &mut Ui, view: &View<'_>, rows: &[(String, Band, Option<usize>)]) {
    for (label, b, hit) in rows {
        let text = match hit {
            Some(i) => format!("{label:>8}  {}", summary(&view.log.qsos[*i])),
            None => format!("{label:>8}"),
        };
        let t = RichText::new(text).monospace();
        ui.label(if *b == view.band { t.strong() } else { t });
    }
}

/// The check-call window (`TCheckCall2`): where the call in the entry row was worked.
#[derive(Default)]
pub struct CheckCall {
    /// A callsign chosen from a QSO's right-click menu, and the entry row's callsign
    /// at that moment: shown until the entry row changes (an addition to zLog).
    pinned: Option<(String, String)>,
    /// The entry row's callsign seen at the last frame.
    entry: String,
}

/// The QSO menu item checking the QSO's callsign.
const CHECK_CALL: &str = "check-call";

impl Panel for CheckCall {
    fn id(&self) -> &str {
        "check-call"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Check, 2)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "checkcall").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar.check_call"),
            texts().get(language, "toolbar.check_call.tip"),
        )
        .icon(zlog_ui::icon::USER_CHECK)
        .menu_label(texts().get(language, "menu.check_call"))]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
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
        let call = qso.callsign.trim().to_uppercase();
        self.pinned = Some((call, self.entry.clone()));
        true
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        self.entry = view.callsign.to_string();
        if self.pinned.as_ref().is_some_and(|(_, e)| *e != self.entry) {
            self.pinned = None;
        }
        let call = self
            .pinned
            .as_ref()
            .map_or(view.callsign, |(c, _)| c.as_str());
        ui.label(RichText::new(call).monospace().strong());
        // Cheap (one pass per band), so it is recomputed each frame.
        let rows = matching::check_call(view.log, call, &contest_bands(view), &entry_row(view));
        let rows: Vec<_> = rows
            .into_iter()
            .map(|(b, hit)| (b.label().to_string(), b, hit))
            .collect();
        band_rows(ui, view, &rows);
    }
}

/// The check-multi window (`TCheckMulti`): where the multiplier of the station in the
/// entry row was worked.
///
/// As in zLog, the multiplier comes only from the entry row (`ExtractMulti`), not
/// from past QSOs of the same call.
#[derive(Default)]
pub struct CheckMulti;

/// The multiplier code of the entry row `q` (`ExtractMulti`), known to the contest
/// or not; `None` when empty (`TCheckMulti.Renew`).
pub fn entry_multi(contest: &dyn Contest, q: &Qso) -> Option<String> {
    Some(contest.extract_multi(q)).filter(|c| !c.is_empty())
}

/// Whether the check-multi window has a CW and a phone row per band (zLog's
/// `ListCWandPh`, set only for the ARRL 10m contest).
fn cw_and_ph(contest: &dyn Contest) -> bool {
    contest.name() == "ARRL 10m Contest"
}

impl Panel for CheckMulti {
    fn id(&self) -> &str {
        "check-multi"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Check, 3)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "checkmulti").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar.check_multi"),
            texts().get(language, "toolbar.check_multi.tip"),
        )
        .icon(zlog_ui::icon::CHECK_SQUARE)
        .menu_label(texts().get(language, "menu.check_multi"))]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let Some(code) = entry_multi(view.contest, &entry_row(view)) else {
            ui.label(texts().get(view.language, "no_multi"));
            return;
        };
        let name = view
            .contest
            .multi_table()
            .into_iter()
            .find(|(c, _, _)| *c == code)
            .map(|(_, n, _)| n)
            .unwrap_or_default();
        ui.label(RichText::new(format!("{code} {name}")).monospace().strong());
        let rows: Vec<_> = matching::check_multi(
            view.log,
            &code,
            &contest_bands(view),
            cw_and_ph(view.contest),
        )
        .into_iter()
        .map(|r| {
            let label = match r.cw {
                None => r.band.label().to_string(),
                Some(true) => format!("{} CW", r.band.label()),
                Some(false) => format!("{} Ph", r.band.label()),
            };
            (label, r.band, r.hit)
        })
        .collect();
        band_rows(ui, view, &rows);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn check_call_from_the_qso_menu_lasts_until_the_entry_changes() {
        let mut c = CheckCall::default();
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

    /// A contest that keeps the exchange (or not) and takes the number as multiplier.
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
        // The click clears the typed number first, so the past one fills it.
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
        // A past QSO with a multiplier no longer stands in for the entry row.
        assert_eq!(entry_multi(&SAME, &q), None);
        let q = Qso {
            nr_rcvd: "13".into(),
            ..q
        };
        assert_eq!(entry_multi(&SAME, &q), Some("13".into()));
    }

    /// A contest that knows no multiplier at all, as zLog's `TBasicMulti`.
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
        // Past QSOs with that multi1 are listed although it is no known multiplier.
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
        // The default keeps the number as typed, spaces included (`NrRcvd`).
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
