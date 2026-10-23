//! The user-defined contest window (zLog's `TSelectUserDefinedContest`, made a
//! small manager): the contest files of the contest folder, and the contests of
//! the lists linked in ([`zlog_ui::ContestList`]), searched together.
//!
//! Nothing is downloaded here. A contest missing from the folder, or older than
//! its list says, is fetched by opening its files in the browser, so the
//! operator visits the site that publishes them; the files are then imported
//! into the folder from wherever the browser saved them (Import, or dropped on
//! the window).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{Color32, RichText, Ui};
use zlog_core::ContestFile;
use zlog_core::Registry;
use zlog_ui::{
    ChosenContest, ContestEntry, ContestList, Date, EntryView, ListedContest, ListedFile,
};

use crate::texts;

/// The text of `key` in `language`.
fn tr(language: &str, key: &'static str) -> &'static str {
    texts().get(language, key)
}

/// [`tr`] with `{0}`, `{1}`, ... replaced by `args`.
fn trf(language: &str, key: &str, args: &[&dyn std::fmt::Display]) -> String {
    texts().format(language, key, args)
}

/// Extensions of the files that may always be imported; other files only when a
/// list names them (a contest may need more than CFG and DAT files).
const IMPORTED: [&str; 2] = ["cfg", "dat"];

/// How a file of a listed contest stands in the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileState {
    Missing,
    /// Older than the list says the file was last changed.
    Outdated,
    Present,
}

/// How a listed contest stands: its worst file.
pub fn contest_state(states: &[FileState]) -> FileState {
    states.iter().copied().min().unwrap_or(FileState::Present)
}

/// Copy `files` (CFG and DAT files, from anywhere) into `folder`. A file equal to
/// the one there is left alone; a different one replaces it, the old one kept as
/// `name.yymmdd` (`today`). Returns the files written and the names skipped
/// (neither CFG nor DAT nor `listed`, or unreadable).
pub fn import(
    files: &[PathBuf],
    folder: &Path,
    today: &str,
    listed: &dyn Fn(&str) -> bool,
) -> std::io::Result<(Vec<PathBuf>, Vec<String>)> {
    std::fs::create_dir_all(folder)?;
    let (mut written, mut skipped) = (Vec::new(), Vec::new());
    for src in files {
        let name = file_name(src);
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let Ok(bytes) = std::fs::read(src).map_err(|_| ()).and_then(|b| {
            if IMPORTED.contains(&ext.as_str()) || listed(&name) {
                Ok(b)
            } else {
                Err(())
            }
        }) else {
            skipped.push(name);
            continue;
        };
        let dest = folder.join(&name);
        if std::fs::read(&dest).is_ok_and(|old| old == bytes) {
            // Imported again: it counts as fresh.
            let _ = std::fs::File::options()
                .append(true)
                .open(&dest)
                .and_then(|f| f.set_modified(SystemTime::now()));
            written.push(dest);
            continue;
        }
        if dest.exists() {
            let backup = folder.join(format!("{name}.{today}"));
            let _ = std::fs::remove_file(&backup);
            std::fs::rename(&dest, &backup)?;
        }
        std::fs::write(&dest, &bytes)?;
        written.push(dest);
    }
    Ok((written, skipped))
}

/// The local date a file was last changed.
fn changed(path: &Path) -> Option<Date> {
    use chrono::Datelike;
    let t: chrono::DateTime<chrono::Local> = std::fs::metadata(path).ok()?.modified().ok()?.into();
    Some((t.year(), t.month(), t.day()))
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The default contest folder: `zlog/cfg` ([`zlog_ui::APP_DIR`]) in the
/// documents folder.
pub fn default_folder() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("cfg")
}

/// A row of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    /// A contest file of the folder that no list offers.
    Local(PathBuf),
    /// Contest `contest` of list `list`.
    Listed { list: usize, contest: usize },
}

/// Which rows are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Show {
    #[default]
    All,
    Installed,
    Missing,
    Updates,
}

/// One list's contests as last seen, or its state.
enum Listing {
    Loading,
    Failed(String),
    Ready(Vec<ListedContest>),
}

/// A downloaded file: its name and its bytes, or why it could not be had.
type Downloaded = (String, Result<Vec<u8>, String>);

struct ContestManager {
    open: bool,
    /// The contest folder (zLog's `CFGDATPath`).
    folder: String,
    lists: Vec<Box<dyn ContestList>>,
    listings: Vec<Listing>,
    requested: bool,
    /// Contest files of the folder: path and title.
    local: Vec<(PathBuf, String)>,
    /// Every file of the folder by lower-case name, with the date it changed.
    files: HashMap<String, (PathBuf, Option<Date>)>,
    search: String,
    show: Show,
    selected: Option<Key>,
    /// The contest file OK opens: one of the selected row's.
    cfg: Option<PathBuf>,
    message: Option<(String, bool)>,
    /// Language of the window, as of the last frame.
    language: String,
    /// Downloads in flight: each file's name and its bytes, or an error.
    downloads: Vec<zlog_net::Job<Vec<Downloaded>>>,
}

impl ContestManager {
    fn new(folder: &str, lists: Vec<Box<dyn ContestList>>) -> Self {
        ContestManager {
            open: false,
            folder: folder.into(),
            listings: lists.iter().map(|_| Listing::Loading).collect(),
            lists,
            requested: false,
            local: Vec::new(),
            files: HashMap::new(),
            search: String::new(),
            show: Show::default(),
            selected: None,
            cfg: None,
            message: None,
            language: "en".into(),
            downloads: Vec::new(),
        }
    }

    /// Open the window on `folder` (the default folder when empty).
    fn show_folder(&mut self, folder: &str, kinds: &[Box<dyn ContestFile>]) {
        let folder = if folder.is_empty() {
            default_folder().display().to_string()
        } else {
            folder.into()
        };
        self.open = true;
        self.change_folder(folder, kinds);
    }

    /// Show the files of `folder`; the selection of another folder is dropped, so
    /// OK never opens a file of the folder left.
    fn change_folder(&mut self, folder: String, kinds: &[Box<dyn ContestFile>]) {
        if folder != self.folder {
            self.selected = None;
            self.cfg = None;
            self.folder = folder;
        }
        self.rescan(kinds);
    }

    fn rescan(&mut self, kinds: &[Box<dyn ContestFile>]) {
        let dir = Path::new(&self.folder);
        self.files.clear();
        self.local.clear();
        for p in std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
        {
            self.files
                .insert(file_name(&p).to_lowercase(), (p.clone(), changed(&p)));
            if let Some(kind) = kinds.iter().find(|k| k.accepts(&p)) {
                let title = kind.title(&p).unwrap_or_default();
                self.local.push((p, title));
            }
        }
        self.local.sort_by_key(|(p, _)| file_name(p).to_lowercase());
    }

    fn poll_lists(&mut self) {
        if !self.requested {
            self.requested = true;
            for l in &mut self.lists {
                l.refresh();
            }
        }
        for (l, listing) in self.lists.iter_mut().zip(&mut self.listings) {
            *listing = match l.contests() {
                None => Listing::Loading,
                Some(Err(e)) => Listing::Failed(e),
                Some(Ok(c)) => Listing::Ready(c.to_vec()),
            };
        }
    }

    /// How `file` of `contest` of list `list` stands in the folder; whether it is
    /// outdated is the list's to say.
    fn state_of(&self, list: usize, contest: &ListedContest, file: &ListedFile) -> FileState {
        match self.files.get(&file.name.to_lowercase()) {
            None => FileState::Missing,
            Some((_, Some(changed))) if self.lists[list].outdated(contest, file, *changed) => {
                FileState::Outdated
            }
            Some(_) => FileState::Present,
        }
    }

    fn listed(&self, list: usize, contest: usize) -> Option<&ListedContest> {
        match self.listings.get(list)? {
            Listing::Ready(c) => c.get(contest),
            _ => None,
        }
    }

    /// The rows: listed contests, then contest files no list offers.
    fn rows(&self) -> Vec<(Key, String, String, FileState)> {
        let mut rows = Vec::new();
        let mut offered = std::collections::HashSet::new();
        for (i, listing) in self.listings.iter().enumerate() {
            let Listing::Ready(contests) = listing else {
                continue;
            };
            for (j, c) in contests.iter().enumerate() {
                let states: Vec<FileState> =
                    c.files.iter().map(|f| self.state_of(i, c, f)).collect();
                offered.extend(c.files.iter().map(|f| f.name.to_lowercase()));
                let hay = [c.name.as_str(), c.date.as_str(), c.note.as_str()]
                    .into_iter()
                    .chain(c.files.iter().map(|f| f.name.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ");
                rows.push((
                    Key::Listed {
                        list: i,
                        contest: j,
                    },
                    c.name.clone(),
                    hay,
                    contest_state(&states),
                ));
            }
        }
        for (p, title) in &self.local {
            if offered.contains(&file_name(p).to_lowercase()) {
                continue;
            }
            let name = if title.is_empty() {
                file_name(p)
            } else {
                title.clone()
            };
            let hay = format!("{name} {}", file_name(p));
            rows.push((Key::Local(p.clone()), name, hay, FileState::Present));
        }
        rows
    }

    /// Draw the window; returns the contest file chosen.
    fn ui(
        &mut self,
        ctx: &egui::Context,
        kinds: &[Box<dyn ContestFile>],
        language: &str,
    ) -> Option<PathBuf> {
        if !self.open {
            return None;
        }
        self.language = language.to_string();
        let lang = language;
        self.poll_lists();
        self.poll_downloads(kinds);
        if !self.downloads.is_empty() || self.listings.iter().any(|l| matches!(l, Listing::Loading))
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.import(&dropped, kinds);
        }
        let mut chosen = None;
        let mut import = false;
        let (mut rescan, mut folder, mut reload, mut close) = (false, false, false, false);
        let mut ok = false;
        let ok_enabled = self.cfg.is_some();
        let mut open = self.open;
        zlog_ui::dialog::dialog_with_buttons(
            ctx,
            "cfg_picker",
            tr(lang, "cfg_picker.title"),
            [860.0, 560.0],
            &mut open,
            |ui| self.body(ui, kinds, &mut chosen),
            |ui| {
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "cfg_picker.import")).clicked() {
                        import = true;
                    }
                    if ui.button(tr(lang, "cfg_picker.choose_folder")).clicked() {
                        folder = true;
                    }
                    if ui.button(tr(lang, "cfg_picker.reload")).clicked() {
                        reload = true;
                    }
                    ui.separator();
                    match zlog_ui::dialog::ok_cancel(ui, lang, ok_enabled, false) {
                        Some(true) => ok = true,
                        Some(false) => close = true,
                        None => {}
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            },
        );
        if close {
            open = false;
        }
        if ok {
            chosen = self.cfg.clone();
        }
        if folder {
            if let Some(d) = zlog_ui::dialog::pick_folder(&self.folder) {
                self.change_folder(d.display().to_string(), kinds);
            }
        }
        if reload {
            // The lists may come back in another order.
            self.requested = false;
            self.selected = None;
            self.cfg = None;
            rescan = true;
        }
        if !open {
            self.open = false;
        }
        if rescan {
            self.rescan(kinds);
        }
        if import {
            let files = zlog_ui::dialog::import_files();
            if !files.is_empty() {
                self.import(&files, kinds);
            }
        }
        if chosen.is_some() {
            self.open = false;
        }
        chosen
    }

    /// Fetch `files` (name and URL) off the UI thread; [`Self::poll_downloads`]
    /// imports them.
    fn download(&mut self, files: Vec<(String, String)>) {
        self.downloads.push(zlog_net::Job::spawn(move || {
            let got = files
                .into_iter()
                .map(|(name, url)| {
                    let mut bytes = Vec::new();
                    let r = zlog_net::download(&url, &mut bytes, |_, _| {}).and_then(|()| {
                        if looks_like_html(&bytes) {
                            Err("HTML".into())
                        } else {
                            Ok(bytes)
                        }
                    });
                    (name, r)
                })
                .collect();
            Ok(got)
        }));
        self.message = Some((tr(&self.language, "cfg_picker.downloading").into(), false));
    }

    fn poll_downloads(&mut self, kinds: &[Box<dyn ContestFile>]) {
        let mut done = Vec::new();
        self.downloads.retain_mut(|job| match job.take() {
            Some(Ok(got)) => {
                done.push(got);
                false
            }
            None => true,
            Some(Err(_)) => false,
        });
        for got in done {
            let dir = std::env::temp_dir().join(format!("zlog-cfg-{}", std::process::id()));
            let mut paths = Vec::new();
            let mut failed = Vec::new();
            for (name, r) in got {
                // The name comes from the list: keep only its last part.
                let name = file_name(Path::new(&name));
                let p = dir.join(&name);
                match r.map_err(|e| e.to_string()).and_then(|b| {
                    std::fs::create_dir_all(&dir)
                        .and_then(|()| std::fs::write(&p, b))
                        .map_err(|e| e.to_string())
                }) {
                    Ok(()) => paths.push(p),
                    Err(e) => failed.push(format!("{name} ({e})")),
                }
            }
            if !paths.is_empty() {
                self.import(&paths, kinds);
            }
            let _ = std::fs::remove_dir_all(&dir);
            if !failed.is_empty() {
                let text = trf(
                    &self.language,
                    "cfg_picker.download_failed",
                    &[&failed.join(", ")],
                );
                self.message = Some(match self.message.take() {
                    Some((m, _)) if !paths.is_empty() => (format!("{m} {text}"), true),
                    _ => (text, true),
                });
            }
        }
    }

    fn import(&mut self, files: &[PathBuf], kinds: &[Box<dyn ContestFile>]) {
        let language = self.language.clone();
        let lang = language.as_str();
        let today = chrono::Local::now().format("%y%m%d").to_string();
        let names: std::collections::HashSet<String> = self
            .listings
            .iter()
            .filter_map(|l| match l {
                Listing::Ready(c) => Some(c),
                _ => None,
            })
            .flatten()
            .flat_map(|c| c.files.iter().map(|f| f.name.to_lowercase()))
            .collect();
        let listed = |n: &str| names.contains(&n.to_lowercase());
        self.message = Some(
            match import(files, Path::new(&self.folder), &today, &listed) {
                Ok((written, skipped)) => {
                    let names: Vec<String> = written.iter().map(|p| file_name(p)).collect();
                    let mut text = trf(lang, "cfg_picker.imported", &[&names.join(", ")]);
                    if !skipped.is_empty() {
                        text.push(' ');
                        text.push_str(&trf(lang, "cfg_picker.skipped", &[&skipped.join(", ")]));
                    }
                    self.rescan(kinds);
                    // Show the contest just imported.
                    if let Some(p) = written.iter().find(|p| kinds.iter().any(|k| k.accepts(p))) {
                        self.selected = self
                            .rows()
                            .into_iter()
                            .map(|r| r.0)
                            .find(|k| self.cfgs_of(k).contains(p));
                        self.cfg = Some(p.clone());
                    }
                    (text, !skipped.is_empty())
                }
                Err(e) => (trf(lang, "cfg_picker.import_failed", &[&e]), true),
            },
        );
    }

    /// The contest files in the folder for a row.
    fn cfgs_of(&self, key: &Key) -> Vec<PathBuf> {
        match key {
            Key::Local(p) => vec![p.clone()],
            Key::Listed { list, contest } => self
                .listed(*list, *contest)
                .map(|c| {
                    c.files
                        .iter()
                        .filter_map(|f| self.files.get(&f.name.to_lowercase()))
                        .map(|(p, _)| p.clone())
                        .filter(|p| self.local.iter().any(|(l, _)| l == p))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    fn body(&mut self, ui: &mut Ui, kinds: &[Box<dyn ContestFile>], chosen: &mut Option<PathBuf>) {
        let language = self.language.clone();
        let lang = language.as_str();
        let show = &mut self.show;
        zlog_ui::field_row(
            ui,
            |_| {},
            |ui| {
                for (s, key) in [
                    (Show::Updates, "cfg_picker.updates"),
                    (Show::Missing, "cfg_picker.missing"),
                    (Show::Installed, "cfg_picker.installed"),
                    (Show::All, "cfg_picker.all"),
                ] {
                    ui.selectable_value(show, s, tr(lang, key));
                }
            },
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text(tr(lang, "cfg_picker.search"))
                        .desired_width(f32::INFINITY),
                )
            },
        );
        ui.horizontal_wrapped(|ui| {
            ui.weak(trf(lang, "cfg_picker.folder_is", &[&self.folder]));
            for (l, listing) in self.lists.iter().zip(&self.listings) {
                ui.separator();
                ui.hyperlink_to(l.title(lang), l.page());
                match listing {
                    Listing::Loading => {
                        ui.spinner();
                    }
                    Listing::Failed(e) => {
                        ui.colored_label(
                            zlog_ui::palette::error(ui),
                            trf(lang, "cfg_picker.list_failed", &[e]),
                        );
                    }
                    Listing::Ready(c) => {
                        ui.weak(trf(lang, "cfg_picker.list_count", &[&c.len()]));
                    }
                }
            }
        });
        for l in &self.lists {
            if l.changes().is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(trf(
                lang,
                "cfg_picker.recent",
                &[&l.title(lang), &l.changes().len()],
            ))
            .id_salt(("cfg_changes", l.id()))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("cfg_changes_scroll", l.id()))
                    .max_height(140.0)
                    .show(ui, |ui| changes(ui, l.changes()));
                if let Some(url) = l.history_page() {
                    ui.hyperlink_to(tr(lang, "cfg_picker.older"), url);
                }
            });
        }
        if let Some((m, warning)) = &self.message {
            if *warning {
                ui.colored_label(zlog_ui::palette::warning(ui), m);
            } else {
                ui.label(m);
            }
        }
        ui.separator();
        let needle = self.search.to_lowercase();
        let rows: Vec<_> = self
            .rows()
            .into_iter()
            .filter(|(_, _, hay, state)| {
                let shown = match self.show {
                    Show::All => true,
                    Show::Installed => *state != FileState::Missing,
                    Show::Missing => *state == FileState::Missing,
                    Show::Updates => *state == FileState::Outdated,
                };
                let hay = hay.to_lowercase();
                shown && needle.split_whitespace().all(|w| hay.contains(w))
            })
            .collect();
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() * 0.4).clamp(240.0, 480.0));
                if rows.is_empty() {
                    ui.weak(tr(lang, "cfg_picker.none"));
                }
                egui::ScrollArea::vertical()
                    .id_salt("cfg_rows")
                    .max_height(height)
                    .show(ui, |ui| {
                        for (key, name, _, state) in &rows {
                            let on = self.selected.as_ref() == Some(key);
                            let r = ui
                                .horizontal(|ui| {
                                    let r = ui.selectable_label(on, name);
                                    badge(ui, lang, *state, key);
                                    r
                                })
                                .inner;
                            if r.clicked() {
                                self.selected = Some(key.clone());
                                self.cfg = self.cfgs_of(key).into_iter().next();
                            }
                            if r.double_clicked() {
                                *chosen = self.cfgs_of(key).into_iter().next();
                            }
                        }
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("cfg_details")
                    .max_height(height)
                    .show(ui, |ui| match self.selected.clone() {
                        None => {
                            ui.weak(tr(lang, "cfg_picker.choose"));
                        }
                        Some(key) => self.details(ui, &key, kinds, chosen),
                    });
            });
        });
    }

    fn details(
        &mut self,
        ui: &mut Ui,
        key: &Key,
        kinds: &[Box<dyn ContestFile>],
        chosen: &mut Option<PathBuf>,
    ) {
        let language = self.language.clone();
        let lang = language.as_str();
        let cfgs = self.cfgs_of(key);
        match key {
            Key::Local(p) => {
                let title = self
                    .local
                    .iter()
                    .find(|(l, _)| l == p)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                ui.heading(if title.is_empty() {
                    file_name(p)
                } else {
                    title
                });
                ui.weak(p.display().to_string());
                ui.label(tr(lang, "cfg_picker.local"));
            }
            Key::Listed { list, contest } => {
                let Some(c) = self.listed(*list, *contest).cloned() else {
                    return;
                };
                ui.heading(&c.name);
                if let Some(url) = &c.rules {
                    ui.hyperlink_to(tr(lang, "cfg_picker.rules"), url)
                        .on_hover_text(url);
                }
                ui.horizontal(|ui| {
                    if !c.date.is_empty() {
                        ui.label(trf(lang, "cfg_picker.date", &[&c.date]));
                    }
                    if let Some((y, m, d)) = c.updated {
                        ui.label(trf(
                            lang,
                            "cfg_picker.updated",
                            &[&format!("{y}-{m:02}-{d:02}")],
                        ));
                    }
                    ui.weak(self.lists[*list].title(lang));
                });
                if !c.note.is_empty() {
                    ui.add(egui::Label::new(&c.note).wrap());
                }
                if !c.changes.is_empty() {
                    ui.add_space(4.0);
                    ui.strong(tr(lang, "cfg_picker.changes"));
                    changes(ui, &c.changes);
                }
                ui.add_space(4.0);
                // A list may give only a DAT for a contest with built-in rules
                // (such as ACAG): nothing here opens it.
                let opens = c
                    .files
                    .iter()
                    .any(|f| kinds.iter().any(|k| k.accepts(Path::new(&f.name))));
                if !opens {
                    ui.colored_label(zlog_ui::palette::warning(ui), tr(lang, "cfg_picker.no_cfg"));
                }
                let busy = !self.downloads.is_empty();
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(tr(lang, "cfg_picker.download_all")),
                    )
                    .clicked()
                {
                    self.download(
                        c.files
                            .iter()
                            .map(|f| (f.name.clone(), f.url.clone()))
                            .collect(),
                    );
                }
                egui::Grid::new("cfg_listed_files")
                    .striped(true)
                    .show(ui, |ui| {
                        for f in &c.files {
                            let state = self.state_of(*list, &c, f);
                            let path = self
                                .files
                                .get(&f.name.to_lowercase())
                                .map(|(p, _)| p.clone())
                                .filter(|p| cfgs.contains(p));
                            match path {
                                // A contest file in the folder: chosen for OK.
                                Some(p) => {
                                    let on = self.cfg.as_ref() == Some(&p);
                                    let r = ui.selectable_label(on, &f.name);
                                    if r.clicked() {
                                        self.cfg = Some(p.clone());
                                    }
                                    if r.double_clicked() {
                                        *chosen = Some(p);
                                    }
                                }
                                None => {
                                    ui.label(&f.name);
                                }
                            }
                            ui.weak(&f.note);
                            ui.label(state_text(ui, lang, state));
                            if ui
                                .add_enabled(
                                    !busy,
                                    egui::Button::new(tr(lang, "cfg_picker.download")),
                                )
                                .on_hover_text(&f.url)
                                .clicked()
                            {
                                self.download(vec![(f.name.clone(), f.url.clone())]);
                            }
                            ui.end_row();
                        }
                    });
            }
        }
    }
}

/// Whether downloaded bytes are a web page (or nothing) rather than a CFG or DAT
/// file: no CFG or DAT file starts with `<`, whatever markup follows.
fn looks_like_html(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let head = bytes.iter().position(|b| !b.is_ascii_whitespace());
    head.is_none_or(|i| bytes[i] == b'<')
}

fn state_text(ui: &Ui, lang: &str, state: FileState) -> RichText {
    match state {
        FileState::Missing => {
            RichText::new(tr(lang, "cfg_picker.state_missing")).color(zlog_ui::palette::weak(ui))
        }
        FileState::Outdated => RichText::new(tr(lang, "cfg_picker.state_outdated"))
            .color(zlog_ui::palette::warning(ui)),
        FileState::Present => RichText::new(tr(lang, "cfg_picker.state_present")),
    }
}

/// Entries of a change history, one a line with its date.
fn changes(ui: &mut Ui, list: &[zlog_ui::Change]) {
    for c in list {
        ui.horizontal_wrapped(|ui| {
            if let Some((y, m, d)) = c.date {
                ui.weak(format!("{y}-{m:02}-{d:02}"));
            }
            ui.label(&c.text);
        });
    }
}

/// The state of a row as a small coloured badge.
fn badge(ui: &mut Ui, lang: &str, state: FileState, key: &Key) {
    let (text, color) = match (key, state) {
        (Key::Local(_), _) => (
            tr(lang, "cfg_picker.badge_local"),
            Color32::from_rgb(110, 110, 130),
        ),
        (_, FileState::Missing) => (
            tr(lang, "cfg_picker.state_missing"),
            Color32::from_rgb(90, 90, 90),
        ),
        (_, FileState::Outdated) => (
            tr(lang, "cfg_picker.state_outdated"),
            Color32::from_rgb(190, 110, 0),
        ),
        (_, FileState::Present) => (
            tr(lang, "cfg_picker.state_present"),
            Color32::from_rgb(40, 130, 70),
        ),
    };
    egui::Frame::new()
        .fill(color)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(Color32::WHITE));
        });
}

/// The entry "User-defined contest..." of the start screen's contest menu: opens the
/// window on the contest folder, and returns the contest file chosen in it.
pub struct ContestWindow {
    manager: ContestManager,
}

impl ContestWindow {
    /// The window searching the contest folder and `lists`.
    pub fn new(lists: Vec<Box<dyn ContestList>>) -> Self {
        ContestWindow {
            manager: ContestManager::new("", lists),
        }
    }
}

impl ContestEntry for ContestWindow {
    fn id(&self) -> &str {
        "contests"
    }

    fn title(&self, _registry: &Registry, language: &str) -> String {
        tr(language, "cfg_picker.entry").into()
    }

    /// Only with a kind of contest file linked in.
    fn shown(&self, registry: &Registry) -> bool {
        !registry.contest_files().is_empty()
    }

    fn open(&mut self, view: &mut EntryView<'_>) {
        self.manager
            .show_folder(view.contest_folder, view.registry.contest_files());
    }

    fn ui(&mut self, ctx: &egui::Context, view: &mut EntryView<'_>) -> Option<ChosenContest> {
        let chosen = self
            .manager
            .ui(ctx, view.registry.contest_files(), view.language);
        let folder = &self.manager.folder;
        if folder != view.contest_folder && !folder.is_empty() {
            // zLog records the folder only while none is set; here the last
            // folder listed is kept, which is what the operator expects next time.
            *view.contest_folder = folder.clone();
        }
        chosen.map(ChosenContest::File)
    }

    fn is_open(&self) -> bool {
        self.manager.open
    }

    fn close(&mut self) {
        self.manager.open = false;
    }
}

/// The text of `key` in `language`, for tests that click through the window.
#[doc(hidden)]
pub fn text(language: &str, key: &'static str) -> &'static str {
    tr(language, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression G10: an error page with a BOM, a comment or an XML declaration,
    /// or an empty download, was kept as the CFG file.
    #[test]
    fn web_pages_and_empty_downloads_are_refused() {
        for page in [
            &b"<!DOCTYPE html>"[..],
            b"\xEF\xBB\xBF<html>",
            b"  <!-- error -->",
            b"<?xml version=\"1.0\"?>",
            b"<head>",
            b"",
            b" \r\n",
        ] {
            assert!(looks_like_html(page), "{page:?}");
        }
        assert!(!looks_like_html(b";ALL JA\r\n"));
        assert!(!looks_like_html(b"\xEF\xBB\xBF#NAME"));
    }

    /// Regression G11: after choosing another folder, OK opened the CFG file
    /// selected in the folder left.
    #[test]
    fn another_folder_drops_the_selected_file() {
        let (a, b) = (scratch("folder-a"), scratch("folder-b"));
        let mut m = ContestManager::new(&a.display().to_string(), Vec::new());
        m.cfg = Some(a.join("x.cfg"));
        m.change_folder(a.display().to_string(), &[]);
        assert_eq!(m.cfg, Some(a.join("x.cfg")), "same folder keeps it");
        m.change_folder(b.display().to_string(), &[]);
        assert_eq!(m.cfg, None);
        assert!(m.selected.is_none());
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zlog-cfg-window-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_language_has_every_key() {
        assert!(texts().missing().is_empty(), "{:?}", texts().missing());
    }

    #[test]
    fn states_of_files_and_contests() {
        use FileState::*;
        assert_eq!(contest_state(&[Present, Outdated]), Outdated);
        assert_eq!(contest_state(&[Present, Missing, Outdated]), Missing);
        assert_eq!(contest_state(&[Present]), Present);
    }

    #[test]
    fn import_from_anywhere_keeps_the_old_file() {
        let from = scratch("from");
        let to = scratch("to").join("cfg");
        std::fs::write(from.join("a.cfg"), "v1").unwrap();
        std::fs::write(from.join("a.DAT"), "list").unwrap();
        std::fs::write(from.join("notes.txt"), "x").unwrap();
        let files = ["a.cfg", "a.DAT", "notes.txt", "gone.cfg"].map(|n| from.join(n));
        let (written, skipped) = import(&files, &to, "261022", &|_| false).unwrap();
        assert_eq!(written, [to.join("a.cfg"), to.join("a.DAT")]);
        assert_eq!(skipped, ["notes.txt", "gone.cfg"]);
        // Other files go in when a list names them.
        let (written, _) = import(&files[2..3], &to, "261022", &|n| n == "notes.txt").unwrap();
        assert_eq!(written, [to.join("notes.txt")]);
        // The same file again writes nothing new and keeps no backup.
        import(&files[..1], &to, "261022", &|_| false).unwrap();
        assert!(!to.join("a.cfg.261022").exists());
        std::fs::write(from.join("a.cfg"), "v2").unwrap();
        import(&files[..1], &to, "261022", &|_| false).unwrap();
        assert_eq!(std::fs::read_to_string(to.join("a.cfg")).unwrap(), "v2");
        assert_eq!(
            std::fs::read_to_string(to.join("a.cfg.261022")).unwrap(),
            "v1"
        );
    }

    struct Fixed(Vec<ListedContest>);

    impl ContestList for Fixed {
        fn id(&self) -> &str {
            "fixed"
        }
        fn title(&self, _language: &str) -> String {
            "Fixed".into()
        }
        fn page(&self) -> &str {
            "https://example.org/"
        }
        fn refresh(&mut self) {}
        fn contests(&mut self) -> Option<Result<&[ListedContest], String>> {
            Some(Ok(&self.0))
        }
        /// Outdated when the list's date is later than the file's.
        fn outdated(&self, contest: &ListedContest, _file: &ListedFile, changed: Date) -> bool {
            contest.updated.is_some_and(|u| changed < u)
        }
    }

    #[test]
    fn rows_join_the_folder_and_the_lists() {
        let dir = scratch("rows");
        std::fs::write(dir.join("akita.cfg"), "#Akita\n").unwrap();
        std::fs::write(dir.join("mine.cfg"), "#Mine\n").unwrap();
        let file = |n: &str| zlog_ui::ListedFile {
            name: n.into(),
            url: format!("https://example.org/{n}"),
            note: String::new(),
        };
        let listed = vec![
            ListedContest {
                name: "Akita".into(),
                files: vec![file("AKITA.CFG")],
                updated: Some((2000, 1, 1)),
                ..Default::default()
            },
            ListedContest {
                name: "Kumamoto".into(),
                files: vec![file("kumamoto.cfg"), file("kumamoto.dat")],
                ..Default::default()
            },
            ListedContest {
                name: "Future".into(),
                files: vec![file("akita.cfg")],
                updated: Some((2999, 1, 1)),
                ..Default::default()
            },
        ];
        let kinds = zlog_core::Registry::new().with(crate::Cfg);
        let mut m = ContestManager::new("", vec![Box::new(Fixed(listed))]);
        m.show_folder(&dir.display().to_string(), kinds.contest_files());
        m.poll_lists();
        let rows: Vec<(String, FileState)> = m.rows().into_iter().map(|r| (r.1, r.3)).collect();
        assert_eq!(
            rows,
            [
                ("Akita".to_string(), FileState::Present),
                ("Kumamoto".to_string(), FileState::Missing),
                ("Future".to_string(), FileState::Outdated),
                ("Mine".to_string(), FileState::Present),
            ]
        );
        let akita = m.rows().remove(0).0;
        assert_eq!(m.cfgs_of(&akita), [dir.join("akita.cfg")]);
    }

    #[test]
    fn a_download_is_chosen_for_ok() {
        let dir = scratch("download");
        let file = |n: &str| zlog_ui::ListedFile {
            name: n.into(),
            url: format!("https://example.org/{n}"),
            note: String::new(),
        };
        let listed = vec![ListedContest {
            name: "Kumamoto".into(),
            files: vec![file("kumamoto.cfg"), file("kumamoto.dat")],
            ..Default::default()
        }];
        let kinds = zlog_core::Registry::new().with(crate::Cfg);
        let mut m = ContestManager::new("", vec![Box::new(Fixed(listed))]);
        m.show_folder(&dir.display().to_string(), kinds.contest_files());
        m.poll_lists();
        let key = m.rows().remove(0).0;
        m.selected = Some(key.clone());
        m.downloads.push(zlog_net::Job::ready(Ok(vec![
            ("kumamoto.cfg".into(), Ok(b"#Kumamoto\n".to_vec())),
            ("kumamoto.dat".into(), Ok(b"01 X\n".to_vec())),
        ])));
        m.poll_downloads(kinds.contest_files());
        assert_eq!(
            m.message.as_ref().map(|m| m.1),
            Some(false),
            "{:?}",
            m.message
        );
        assert_eq!(m.selected, Some(key));
        assert_eq!(m.cfg, Some(dir.join("kumamoto.cfg")));
    }
}
