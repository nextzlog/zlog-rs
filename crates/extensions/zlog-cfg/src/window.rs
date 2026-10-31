use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{Color32, RichText, Ui};
use zlog_core::ContestFile;
use zlog_core::Registry;
use zlog_ui::{
    days_between, ChosenContest, ContestEntry, ContestList, Date, EntryView, ListedContest,
    ListedFile,
};

use crate::texts;

mod draw;

fn tr(language: &str, key: &'static str) -> &'static str {
    texts().get(language, key)
}

fn trf(language: &str, key: &str, args: &[&dyn std::fmt::Display]) -> String {
    texts().format(language, key, args)
}

const IMPORTED: [&str; 2] = ["cfg", "dat"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileState {
    Missing,
    Outdated,
    Present,
}

pub fn contest_state(states: &[FileState]) -> FileState {
    states.iter().copied().min().unwrap_or(FileState::Present)
}

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
        let dest = existing(folder, &name).unwrap_or_else(|| folder.join(&name));
        if std::fs::read(&dest).is_ok_and(|old| old == bytes) {
            let _ = std::fs::File::options()
                .append(true)
                .open(&dest)
                .and_then(|f| f.set_modified(SystemTime::now()));
            written.push(dest);
            continue;
        }
        let staged = zlog_core::file::stage(&dest, |f| f.write_all(&bytes))?;
        let backup = dest.exists().then(|| free_backup(&dest, today));
        if let Some(b) = &backup {
            std::fs::rename(&dest, b)?;
        }
        if let Err(e) = staged.persist(&dest) {
            if let Some(b) = &backup {
                let _ = std::fs::rename(b, &dest);
            }
            return Err(e.error);
        }
        written.push(dest);
    }
    Ok((written, skipped))
}

fn existing(folder: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .find(|p| file_name(p).eq_ignore_ascii_case(name))
}

fn free_backup(dest: &Path, today: &str) -> PathBuf {
    let base = format!("{}.{today}", file_name(dest));
    let dir = dest.parent().unwrap_or(Path::new("."));
    std::iter::once(base.clone())
        .chain((1..).map(|n| format!("{base}.{n}")))
        .map(|n| dir.join(n))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(base))
}

pub const RECENT_DAYS: i64 = 31;

pub fn last_update(contest: &ListedContest) -> Option<Date> {
    let newest = contest.changes.iter().filter_map(|c| c.date).max();
    contest.updated.max(newest)
}

pub fn updated_days_ago(today: Date, contest: &ListedContest) -> Option<i64> {
    last_update(contest)
        .map(|d| days_between(d, today))
        .filter(|n| (0..=RECENT_DAYS).contains(n))
}

fn today() -> Date {
    use chrono::Datelike;
    let t = chrono::Local::now();
    (t.year(), t.month(), t.day())
}

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

pub fn default_folder() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("cfg")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Builtin(String),
    Local(PathBuf),
    Listed { list: usize, contest: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Show {
    #[default]
    All,
    Installed,
    Missing,
    Updates,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    key: Key,
    name: String,
    hay: String,
    state: FileState,
    source: String,
    days: Option<i64>,
}

enum Listing {
    Loading,
    Failed(String),
    Ready(Vec<ListedContest>),
}

type Downloaded = (ListedFile, Result<Vec<u8>, String>);

struct ContestManager {
    open: bool,
    folder: String,
    lists: Vec<Box<dyn ContestList>>,
    listings: Vec<Listing>,
    requested: bool,
    force: bool,
    builtin: Vec<(String, String, String)>,
    today: Date,
    local: Vec<(PathBuf, String)>,
    files: HashMap<String, (PathBuf, Option<Date>)>,
    search: String,
    show: Show,
    selected: Option<Key>,
    cfg: Option<PathBuf>,
    linked: Option<String>,
    message: Option<(String, bool)>,
    language: String,
    downloads: Vec<zlog_net::Job<(usize, Vec<Downloaded>)>>,
    docs: HashMap<String, zlog_net::Fetch>,
}

impl ContestManager {
    fn new(folder: &str, lists: Vec<Box<dyn ContestList>>) -> Self {
        ContestManager {
            open: false,
            folder: folder.into(),
            listings: lists.iter().map(|_| Listing::Loading).collect(),
            lists,
            requested: false,
            force: false,
            builtin: Vec::new(),
            today: today(),
            local: Vec::new(),
            files: HashMap::new(),
            search: String::new(),
            show: Show::default(),
            selected: None,
            cfg: None,
            linked: None,
            message: None,
            language: "en".into(),
            downloads: Vec::new(),
            docs: HashMap::new(),
        }
    }

    fn show_folder(&mut self, folder: &str, kinds: &[Box<dyn ContestFile>]) {
        let folder = if folder.is_empty() {
            default_folder().display().to_string()
        } else {
            folder.into()
        };
        self.open = true;
        self.change_folder(folder, kinds);
    }

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
                if self.force || l.stale(self.today) {
                    l.refresh();
                }
            }
            self.force = false;
        }
        for (l, listing) in self.lists.iter_mut().zip(&mut self.listings) {
            *listing = match l.contests() {
                None => Listing::Loading,
                Some(Err(e)) => Listing::Failed(e),
                Some(Ok(c)) => Listing::Ready(c.to_vec()),
            };
        }
    }

    fn state_of(&self, list: usize, contest: &ListedContest, file: &ListedFile) -> FileState {
        match self.files.get(&file.name.to_lowercase()) {
            None => FileState::Missing,
            Some((_, Some(changed)))
                if self.lists[list].outdated(Path::new(&self.folder), contest, file, *changed) =>
            {
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

    fn rows(&self) -> Vec<Row> {
        let lang = self.language.as_str();
        let mut rows = Vec::new();
        let mut offered = std::collections::HashSet::new();
        for (i, listing) in self.listings.iter().enumerate() {
            let Listing::Ready(contests) = listing else {
                continue;
            };
            let source = self.lists[i].title(lang);
            for (j, c) in contests.iter().enumerate() {
                let states: Vec<FileState> =
                    c.files.iter().map(|f| self.state_of(i, c, f)).collect();
                offered.extend(c.files.iter().map(|f| f.name.to_lowercase()));
                let hay = [c.name.as_str(), c.date.as_str(), c.note.as_str(), &source]
                    .into_iter()
                    .chain(c.files.iter().map(|f| f.name.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ");
                let name = match c.experimental {
                    true => format!("{} {}", c.name, tr(lang, "cfg_picker.experimental")),
                    false => c.name.clone(),
                };
                rows.push(Row {
                    key: Key::Listed {
                        list: i,
                        contest: j,
                    },
                    name,
                    hay,
                    state: contest_state(&states),
                    source: source.clone(),
                    days: updated_days_ago(self.today, c),
                });
            }
        }
        let folder = tr(lang, "cfg_picker.badge_local");
        for (p, title) in &self.local {
            if offered.contains(&file_name(p).to_lowercase()) {
                continue;
            }
            let name = if title.is_empty() {
                file_name(p)
            } else {
                title.clone()
            };
            rows.push(Row {
                key: Key::Local(p.clone()),
                hay: format!("{name} {} {folder}", file_name(p)),
                name,
                state: FileState::Present,
                source: folder.into(),
                days: None,
            });
        }
        for (id, caption, group) in &self.builtin {
            rows.push(Row {
                key: Key::Builtin(id.clone()),
                name: caption.clone(),
                hay: format!("{caption} {id} {group}"),
                state: FileState::Present,
                source: group.clone(),
                days: None,
            });
        }
        rows
    }

    fn shown_rows(&self) -> Vec<Row> {
        let needle = self.search.to_lowercase();
        self.rows()
            .into_iter()
            .filter(|r| {
                let shown = match self.show {
                    Show::All => !matches!(r.key, Key::Builtin(_)) || !needle.trim().is_empty(),
                    _ if matches!(r.key, Key::Builtin(_)) => false,
                    Show::Installed => r.state != FileState::Missing,
                    Show::Missing => r.state == FileState::Missing,
                    Show::Updates => r.state == FileState::Outdated,
                };
                let hay = r.hay.to_lowercase();
                shown && needle.split_whitespace().all(|w| hay.contains(w))
            })
            .collect()
    }

    fn select(&mut self, key: &Key) {
        self.selected = Some(key.clone());
        self.linked = match key {
            Key::Builtin(id) => Some(id.clone()),
            _ => None,
        };
        self.cfg = self.cfgs_of(key).into_iter().next();
    }

    fn choice(&self) -> Option<ChosenContest> {
        match &self.linked {
            Some(id) => Some(ChosenContest::Linked(id.clone())),
            None => self.cfg.clone().map(ChosenContest::File),
        }
    }

    fn waiting(&mut self) -> bool {
        !self.downloads.is_empty()
            || self.listings.iter().any(|l| matches!(l, Listing::Loading))
            || self.lists.iter().any(|l| l.pending())
            || self.docs.values_mut().any(|d| d.poll().is_none())
    }

    fn doc_text(&mut self, doc: &str) -> String {
        if !doc.starts_with("https://") {
            return doc.to_string();
        }
        let job = self
            .docs
            .entry(doc.to_string())
            .or_insert_with(|| zlog_net::Fetch::get(doc));
        match job.poll() {
            Some(Ok(text)) => text.clone(),
            Some(Err(e)) => format!("{doc} ({e})"),
            None => tr(&self.language, "cfg_picker.doc_loading").into(),
        }
    }

    fn download(&mut self, list: usize, files: Vec<ListedFile>) {
        self.downloads.push(zlog_net::Job::spawn(move || {
            let got = files
                .into_iter()
                .map(|file| {
                    let mut bytes = Vec::new();
                    let r = zlog_net::download(&file.url, &mut bytes, |_, _| {}).and_then(|()| {
                        if looks_like_html(&bytes) {
                            Err("HTML".into())
                        } else {
                            Ok(bytes)
                        }
                    });
                    (file, r)
                })
                .collect();
            Ok((list, got))
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
        for (list, got) in done {
            let temp = tempfile::tempdir();
            let mut paths = Vec::new();
            let mut failed = Vec::new();
            for (file, r) in got {
                let name = file_name(Path::new(&file.name));
                let p = temp.as_ref().map(|d| d.path().join(&name));
                let verified = r.and_then(|b| {
                    let list = self.lists.get(list).ok_or_else(String::new)?;
                    list.verify(&file, &b, &self.language).map(|()| b)
                });
                match verified.and_then(|b| {
                    let p = p.map_err(|e| e.to_string())?;
                    std::fs::write(&p, b).map_err(|e| e.to_string())?;
                    Ok(p)
                }) {
                    Ok(p) => paths.push(p),
                    Err(e) => failed.push(format!("{name} ({e})")),
                }
            }
            if !paths.is_empty() {
                self.import(&paths, kinds);
            }
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
                    if let Some(p) = written.iter().find(|p| kinds.iter().any(|k| k.accepts(p))) {
                        self.selected = self
                            .rows()
                            .into_iter()
                            .map(|r| r.key)
                            .find(|k| self.cfgs_of(k).contains(p));
                        self.cfg = Some(p.clone());
                        self.linked = None;
                    }
                    (text, !skipped.is_empty())
                }
                Err(e) => (trf(lang, "cfg_picker.import_failed", &[&e]), true),
            },
        );
    }

    fn cfgs_of(&self, key: &Key) -> Vec<PathBuf> {
        match key {
            Key::Builtin(_) => Vec::new(),
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
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let head = bytes.iter().position(|b| !b.is_ascii_whitespace());
    head.is_none_or(|i| bytes[i] == b'<')
}

#[derive(Default)]
struct Tools {
    import: bool,
    folder: bool,
    reload: bool,
}

pub struct ContestWindow {
    manager: ContestManager,
    updates: Option<(String, std::time::Instant, Vec<PathBuf>)>,
    listing: bool,
}

const UPDATES_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

impl ContestWindow {
    pub fn new(lists: Vec<Box<dyn ContestList>>) -> Self {
        ContestWindow {
            manager: ContestManager::new("", lists),
            updates: None,
            listing: false,
        }
    }
}

impl ContestManager {
    fn outdated_files(
        &mut self,
        folder: &str,
        kinds: &[Box<dyn ContestFile>],
        rescan: bool,
    ) -> (Vec<PathBuf>, bool) {
        if rescan && !self.open && !folder.is_empty() {
            self.change_folder(folder.into(), kinds);
        }
        let mut out = Vec::new();
        let mut listing = false;
        for (i, l) in self.lists.iter_mut().enumerate() {
            match l.contests() {
                Some(Ok(c)) => self.listings[i] = Listing::Ready(c.to_vec()),
                Some(Err(_)) => {}
                None => listing = true,
            }
            listing |= l.pending();
        }
        for (i, listing) in self.listings.iter().enumerate() {
            let Listing::Ready(contests) = listing else {
                continue;
            };
            for c in contests {
                for f in &c.files {
                    if self.state_of(i, c, f) == FileState::Outdated {
                        if let Some((p, _)) = self.files.get(&f.name.to_lowercase()) {
                            out.push(p.clone());
                        }
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        (out, listing)
    }
}

impl ContestEntry for ContestWindow {
    fn id(&self) -> &str {
        "contests"
    }

    fn title(&self, _registry: &Registry, language: &str) -> String {
        tr(language, "cfg_picker.entry").into()
    }

    fn shown(&self, registry: &Registry) -> bool {
        !registry.contest_files().is_empty()
    }

    fn open(&mut self, view: &mut EntryView<'_>) {
        self.manager
            .show_folder(view.contest_folder, view.registry.contest_files());
    }

    fn ui(&mut self, ctx: &egui::Context, view: &mut EntryView<'_>) -> Option<ChosenContest> {
        let chosen = self.manager.ui(ctx, view.registry, view.language);
        let folder = &self.manager.folder;
        if folder != view.contest_folder && !folder.is_empty() {
            *view.contest_folder = folder.clone();
        }
        chosen
    }

    fn is_open(&self) -> bool {
        self.manager.open
    }

    fn close(&mut self) {
        self.manager.open = false;
    }

    fn updates(&mut self, view: &mut EntryView<'_>) -> Vec<PathBuf> {
        let folder = view.contest_folder.clone();
        let fresh = self.updates.as_ref().is_some_and(|(f, at, _)| {
            *f == folder && at.elapsed() < UPDATES_EVERY && !self.manager.open
        });
        if !fresh || self.listing {
            let (files, listing) =
                self.manager
                    .outdated_files(&folder, view.registry.contest_files(), !fresh);
            if self.updates.as_ref().is_some_and(|(_, _, f)| *f != files) {
                zlog_core::wake::wake();
            }
            self.listing = listing;
            let at = match &self.updates {
                Some((_, at, _)) if fresh => *at,
                _ => std::time::Instant::now(),
            };
            self.updates = Some((folder, at, files));
        }
        self.updates
            .as_ref()
            .map_or(Vec::new(), |(_, _, f)| f.clone())
    }
}

#[doc(hidden)]
pub fn text(language: &str, key: &'static str) -> &'static str {
    tr(language, key)
}

#[cfg(test)]
mod tests {
    use super::draw::{badge_text, days_text};
    use super::*;

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
        let (written, _) = import(&files[2..3], &to, "261022", &|n| n == "notes.txt").unwrap();
        assert_eq!(written, [to.join("notes.txt")]);
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

    #[test]
    fn same_day_updates_keep_every_backup_and_the_case_on_disk() {
        let from = scratch("case-from");
        let to = scratch("case-to");
        std::fs::write(to.join("TEST.CFG"), "v1").unwrap();
        let file = [from.join("test.cfg")];
        for v in ["v2", "v3"] {
            std::fs::write(&file[0], v).unwrap();
            let (written, _) = import(&file, &to, "261024", &|_| false).unwrap();
            assert_eq!(written, [to.join("TEST.CFG")]);
        }
        let read = |n: &str| std::fs::read_to_string(to.join(n)).unwrap();
        assert_eq!(read("TEST.CFG"), "v3");
        assert_eq!(read("TEST.CFG.261024"), "v1");
        assert_eq!(read("TEST.CFG.261024.1"), "v2");
        let mut names: Vec<_> = std::fs::read_dir(&to)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["TEST.CFG", "TEST.CFG.261024", "TEST.CFG.261024.1"]);
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
        fn outdated(
            &self,
            _folder: &Path,
            contest: &ListedContest,
            _file: &ListedFile,
            changed: Date,
        ) -> bool {
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
        let rows: Vec<(String, FileState)> =
            m.rows().into_iter().map(|r| (r.name, r.state)).collect();
        assert_eq!(
            rows,
            [
                ("Akita".to_string(), FileState::Present),
                ("Kumamoto".to_string(), FileState::Missing),
                ("Future".to_string(), FileState::Outdated),
                ("Mine".to_string(), FileState::Present),
            ]
        );
        let akita = m.rows().remove(0).key;
        assert_eq!(m.cfgs_of(&akita), [dir.join("akita.cfg")]);
    }

    #[test]
    fn the_menu_is_told_of_files_with_an_update() {
        let dir = scratch("updates");
        std::fs::write(dir.join("akita.cfg"), "#Akita\n").unwrap();
        std::fs::write(dir.join("mine.cfg"), "#Mine\n").unwrap();
        let file = |n: &str| zlog_ui::ListedFile {
            name: n.into(),
            ..Default::default()
        };
        let listed = vec![ListedContest {
            name: "Future".into(),
            files: vec![file("AKITA.CFG"), file("missing.dat")],
            updated: Some((2999, 1, 1)),
            ..Default::default()
        }];
        let registry = zlog_core::Registry::new().with(crate::Cfg);
        let mut w = ContestWindow::new(vec![Box::new(Fixed(listed))]);
        let mut folder = dir.display().to_string();
        let mut view = EntryView {
            registry: &registry,
            language: "ja",
            contest_folder: &mut folder,
        };
        assert_eq!(w.updates(&mut view), [dir.join("akita.cfg")]);
        assert!(!w.is_open(), "the window stays closed");
        let mut none = String::new();
        let mut view = EntryView {
            registry: &registry,
            language: "ja",
            contest_folder: &mut none,
        };
        assert!(ContestWindow::new(Vec::new()).updates(&mut view).is_empty());
    }

    struct Late(Vec<ListedContest>, std::rc::Rc<std::cell::Cell<bool>>);

    impl ContestList for Late {
        fn id(&self) -> &str {
            "late"
        }
        fn title(&self, _language: &str) -> String {
            "Late".into()
        }
        fn page(&self) -> &str {
            "https://example.org/"
        }
        fn refresh(&mut self) {}
        fn contests(&mut self) -> Option<Result<&[ListedContest], String>> {
            self.1.get().then_some(Ok(&self.0))
        }
        fn outdated(
            &self,
            _folder: &Path,
            contest: &ListedContest,
            _file: &ListedFile,
            changed: Date,
        ) -> bool {
            contest.updated.is_some_and(|u| changed < u)
        }
    }

    #[test]
    fn an_update_found_when_the_list_arrives_is_told_at_once() {
        let dir = scratch("late-updates");
        std::fs::write(dir.join("akita.cfg"), "#Akita\n").unwrap();
        let listed = vec![ListedContest {
            name: "Future".into(),
            files: vec![zlog_ui::ListedFile {
                name: "akita.cfg".into(),
                ..Default::default()
            }],
            updated: Some((2999, 1, 1)),
            ..Default::default()
        }];
        let arrived = std::rc::Rc::new(std::cell::Cell::new(false));
        let registry = zlog_core::Registry::new().with(crate::Cfg);
        let mut w = ContestWindow::new(vec![Box::new(Late(listed, arrived.clone()))]);
        let mut folder = dir.display().to_string();
        let mut view = EntryView {
            registry: &registry,
            language: "ja",
            contest_folder: &mut folder,
        };
        assert!(w.updates(&mut view).is_empty());
        arrived.set(true);
        assert_eq!(w.updates(&mut view), [dir.join("akita.cfg")]);
        assert_eq!(w.updates(&mut view), [dir.join("akita.cfg")]);
    }

    #[test]
    fn a_pending_document_keeps_the_manager_repainting() {
        let mut m = ContestManager::new("", Vec::new());
        assert!(!m.waiting());
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let url = "https://example.org/rules";
        m.docs.insert(
            url.into(),
            zlog_net::Job::spawn(move || {
                let _ = rx.recv();
                Ok("rules".into())
            }),
        );
        assert!(m.waiting());
        drop(tx);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while m.waiting() && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!m.waiting());
        assert_eq!(m.doc_text(url), "rules");
    }

    struct Counted {
        checked: Option<Date>,
        fetched: std::rc::Rc<std::cell::Cell<usize>>,
        contests: Vec<ListedContest>,
    }

    impl ContestList for Counted {
        fn id(&self) -> &str {
            "counted"
        }
        fn title(&self, _language: &str) -> String {
            "Counted".into()
        }
        fn page(&self) -> &str {
            "https://example.org/"
        }
        fn last_checked(&self) -> Option<Date> {
            self.checked
        }
        fn refresh(&mut self) {
            self.fetched.set(self.fetched.get() + 1);
        }
        fn contests(&mut self) -> Option<Result<&[ListedContest], String>> {
            Some(Ok(&self.contests))
        }
    }

    fn counted(checked: Option<Date>) -> (ContestManager, std::rc::Rc<std::cell::Cell<usize>>) {
        let fetched = std::rc::Rc::new(std::cell::Cell::new(0));
        let list = Counted {
            checked,
            fetched: fetched.clone(),
            contests: Vec::new(),
        };
        let mut m = ContestManager::new("", vec![Box::new(list)]);
        m.today = (2026, 10, 24);
        (m, fetched)
    }

    #[test]
    fn a_list_is_fetched_by_itself_only_when_stale() {
        for (checked, want) in [
            (None, 1),
            (Some((2026, 10, 1)), 0),
            (Some((2026, 9, 24)), 0),
            (Some((2026, 9, 23)), 1),
        ] {
            let (mut m, fetched) = counted(checked);
            m.poll_lists();
            m.poll_lists();
            assert_eq!(fetched.get(), want, "{checked:?}");
        }
        let (mut m, fetched) = counted(Some((2026, 10, 23)));
        m.poll_lists();
        m.requested = false;
        m.force = true;
        m.poll_lists();
        assert_eq!(fetched.get(), 1);
    }

    #[test]
    fn days_between_dates() {
        use chrono::NaiveDate;
        for (a, b) in [
            ((2026, 9, 23), (2026, 10, 24)),
            ((2024, 2, 28), (2024, 3, 1)),
            ((1999, 12, 31), (2026, 1, 1)),
            ((2026, 10, 24), (2026, 10, 1)),
        ] {
            let d = |(y, m, dd): Date| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
            assert_eq!(days_between(a, b), (d(b) - d(a)).num_days(), "{a:?} {b:?}");
        }
    }

    #[test]
    fn rows_say_how_lately_their_list_changed_them() {
        let c = |updated, change: Option<Date>| ListedContest {
            name: "X".into(),
            updated,
            changes: change
                .map(|d| zlog_ui::Change {
                    date: Some(d),
                    text: "X".into(),
                })
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let today = (2026, 10, 24);
        assert_eq!(
            updated_days_ago(today, &c(Some((2026, 10, 20)), None)),
            Some(4)
        );
        assert_eq!(
            updated_days_ago(today, &c(None, Some((2026, 10, 24)))),
            Some(0)
        );
        assert_eq!(
            updated_days_ago(today, &c(Some((2026, 1, 1)), Some((2026, 10, 14)))),
            Some(10)
        );
        assert_eq!(updated_days_ago(today, &c(Some((2026, 8, 1)), None)), None);
        assert_eq!(days_text("ja", 3), "3 日前に更新");
    }

    #[test]
    fn one_search_over_lists_folder_and_built_in_contests() {
        let dir = scratch("search");
        std::fs::write(dir.join("tokyo.cfg"), "#Tokyo UHF\n").unwrap();
        let file = |n: &str| zlog_ui::ListedFile {
            name: n.into(),
            url: format!("https://example.org/{n}"),
            note: String::new(),
        };
        let listed = vec![ListedContest {
            name: "Tokyo VHF".into(),
            files: vec![file("tvhf.cfg")],
            updated: Some((2999, 1, 1)),
            ..Default::default()
        }];
        let kinds = zlog_core::Registry::new().with(crate::Cfg);
        let mut m = ContestManager::new("", vec![Box::new(Fixed(listed))]);
        m.show_folder(&dir.display().to_string(), kinds.contest_files());
        std::fs::write(dir.join("tvhf.cfg"), "#Tokyo VHF\n").unwrap();
        m.rescan(kinds.contest_files());
        m.builtin = vec![
            ("tokyo".into(), "Tokyo Built-in".into(), "Local".into()),
            ("allja1".into(), "ALL JA1".into(), "Local".into()),
        ];
        m.language = "ja".into();
        m.poll_lists();
        let names = |m: &ContestManager| -> Vec<String> {
            m.shown_rows().into_iter().map(|r| r.name).collect()
        };
        assert_eq!(names(&m), ["Tokyo VHF", "Tokyo UHF"]);
        m.search = "tokyo".into();
        assert_eq!(names(&m), ["Tokyo VHF", "Tokyo UHF", "Tokyo Built-in"]);
        let rows = m.shown_rows();
        let lang = "ja";
        assert_eq!(badge_text(lang, &rows[0]), "更新あり");
        assert_eq!(rows[0].source, "Fixed");
        assert_eq!(badge_text(lang, &rows[1]), "フォルダー内");
        assert_eq!(badge_text(lang, &rows[2]), "Local");
        m.search = "fixed".into();
        assert_eq!(names(&m), ["Tokyo VHF"]);
        m.search.clear();
        m.show = Show::Updates;
        assert_eq!(names(&m), ["Tokyo VHF"]);
        m.select(&rows[2].key);
        assert_eq!(m.choice(), Some(ChosenContest::Linked("tokyo".into())));
        m.select(&rows[1].key);
        assert_eq!(m.choice(), Some(ChosenContest::File(dir.join("tokyo.cfg"))));
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
        let key = m.rows().remove(0).key;
        m.selected = Some(key.clone());
        m.downloads.push(zlog_net::Job::ready(Ok((
            0,
            vec![
                (file("kumamoto.cfg"), Ok(b"#Kumamoto\n".to_vec())),
                (file("kumamoto.dat"), Ok(b"01 X\n".to_vec())),
            ],
        ))));
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

    struct Checked(Vec<ListedContest>);

    impl ContestList for Checked {
        fn id(&self) -> &str {
            "checked"
        }
        fn title(&self, _language: &str) -> String {
            "Checked".into()
        }
        fn page(&self) -> &str {
            "https://example.org/"
        }
        fn refresh(&mut self) {}
        fn contests(&mut self) -> Option<Result<&[ListedContest], String>> {
            Some(Ok(&self.0))
        }
        fn outdated(
            &self,
            folder: &Path,
            _contest: &ListedContest,
            file: &ListedFile,
            _changed: Date,
        ) -> bool {
            std::fs::read(folder.join(&file.name)).is_ok_and(|b| b != b"good")
        }
        fn verify(&self, file: &ListedFile, bytes: &[u8], _language: &str) -> Result<(), String> {
            match bytes {
                b"good" => Ok(()),
                _ => Err(format!("{} sum", file.name)),
            }
        }
    }

    #[test]
    fn the_list_verifies_each_download_and_judges_the_shown_folder() {
        let dir = scratch("verify");
        let file = zlog_ui::ListedFile {
            name: "a.cfg".into(),
            url: "https://example.org/a.cfg".into(),
            note: String::new(),
        };
        let listed = vec![ListedContest {
            name: "A".into(),
            files: vec![file.clone()],
            ..Default::default()
        }];
        let kinds = zlog_core::Registry::new().with(crate::Cfg);
        let mut m = ContestManager::new("", vec![Box::new(Checked(listed.clone()))]);
        m.show_folder(&dir.display().to_string(), kinds.contest_files());
        m.poll_lists();
        std::fs::write(dir.join("a.cfg"), "old").unwrap();
        m.rescan(kinds.contest_files());
        assert_eq!(m.state_of(0, &listed[0], &file), FileState::Outdated);
        m.downloads.push(zlog_net::Job::ready(Ok((
            0,
            vec![(file.clone(), Ok(b"bad".to_vec()))],
        ))));
        m.poll_downloads(kinds.contest_files());
        assert!(m.message.as_ref().unwrap().0.contains("a.cfg sum"));
        assert_eq!(std::fs::read(dir.join("a.cfg")).unwrap(), b"old");
        m.downloads.push(zlog_net::Job::ready(Ok((
            0,
            vec![(file.clone(), Ok(b"good".to_vec()))],
        ))));
        m.poll_downloads(kinds.contest_files());
        assert_eq!(std::fs::read(dir.join("a.cfg")).unwrap(), b"good");
        assert_eq!(m.state_of(0, &listed[0], &file), FileState::Present);
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names.len(),
            2,
            "the file and its backup, no temporary: {names:?}"
        );
        assert!(names[1].starts_with("a.cfg."));
    }

    #[test]
    fn packages_show_their_description_and_whether_they_are_experimental() {
        let listed = vec![ListedContest {
            name: "Beta".into(),
            doc: "## Beta\nRules".into(),
            experimental: true,
            files: vec![zlog_ui::ListedFile {
                name: "beta.cfg".into(),
                ..Default::default()
            }],
            ..Default::default()
        }];
        let mut m = ContestManager::new("", vec![Box::new(Fixed(listed))]);
        m.language = "ja".into();
        m.poll_lists();
        assert_eq!(m.rows()[0].name, "Beta (試験版)");
        assert_eq!(m.doc_text("## Beta\nRules"), "## Beta\nRules");
        let url = "https://example.org/beta.md";
        m.docs
            .insert(url.into(), zlog_net::Fetch::ready(Ok("fetched".into())));
        assert_eq!(m.doc_text(url), "fetched");
        m.docs
            .insert(url.into(), zlog_net::Fetch::ready(Err("offline".into())));
        assert!(m.doc_text(url).contains("offline"));
    }
}
