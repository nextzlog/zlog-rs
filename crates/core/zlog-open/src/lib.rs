use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zlog_core::{ContestDef, ContestFile, ContestMode, Power, Registry, Station};
use zlog_ui::{
    ChosenContest, ContestEntry, EntryView, Section, Sections, Setup, StartScreen, StartView,
};

zlog_core::texts!();

mod form;

fn tr(language: &str, key: &'static str) -> &'static str {
    texts().get(language, key)
}

#[doc(hidden)]
pub fn text(language: &str, key: &'static str) -> &'static str {
    tr(language, key)
}

const MENU: &str = "menu";

const CONTEST_FILES: &str = "contest_files";

const STATION: &str = "station";

struct ChosenFile {
    path: PathBuf,
    def: Box<dyn ContestDef>,
    warnings: Vec<String>,
}

pub struct MenuForm {
    contest: usize,
    search: String,
    entry: Option<usize>,
    entries: Vec<Box<dyn ContestEntry>>,
    recent: Vec<RecentContest>,
    pinned: Vec<RecentContest>,
    pin_toggled: Option<RecentContest>,
    folder_files: Option<(String, Vec<PathBuf>)>,
    file: Option<ChosenFile>,
    titles: HashMap<PathBuf, String>,
    cfg_folder: String,
    log_files: Vec<(PathBuf, PathBuf)>,
    checked_path: String,
    locate: Option<PathBuf>,
    mode: ContestMode,
    station: Station,
    power: Option<Power>,
    coeff: String,
    path: String,
    replace: Option<PathBuf>,
    error: Option<String>,
}

impl MenuForm {
    pub fn new(entries: Vec<Box<dyn ContestEntry>>) -> Self {
        MenuForm {
            contest: 0,
            search: String::new(),
            entry: None,
            entries,
            recent: Vec::new(),
            pinned: Vec::new(),
            pin_toggled: None,
            folder_files: None,
            file: None,
            titles: HashMap::new(),
            cfg_folder: String::new(),
            log_files: Vec::new(),
            checked_path: String::new(),
            locate: None,
            mode: ContestMode::Mix,
            station: Station::default(),
            power: None,
            coeff: "1".into(),
            path: String::new(),
            replace: None,
            error: None,
        }
    }

    fn current<'a>(&'a self, contests: &'a [Box<dyn ContestDef>]) -> Option<&'a dyn ContestDef> {
        match &self.file {
            Some(f) => Some(&*f.def),
            None => contests.get(self.contest).map(|c| &**c),
        }
    }

    fn choose_file(&mut self, path: &Path, registry: &Registry, lang: &str) {
        match registry.open_contest_file(path, lang) {
            Ok(opened) => {
                let title = opened.def.caption(lang);
                self.titles.insert(path.to_path_buf(), title);
                self.file = Some(ChosenFile {
                    path: path.to_path_buf(),
                    def: opened.def,
                    warnings: opened.warnings,
                });
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn choose(&mut self, chosen: ChosenContest, registry: &Registry, lang: &str) {
        match chosen {
            ChosenContest::File(p) => self.choose_file(&p, registry, lang),
            ChosenContest::Linked(id) => {
                match registry.contests().iter().position(|c| c.id() == id) {
                    Some(i) => {
                        self.contest = i;
                        self.file = None;
                    }
                    None => self.error = Some(retired(lang, &[id])),
                }
            }
        }
    }

    fn check_log_path(&mut self, registry: &Registry, lang: &str) {
        if self.path == self.checked_path {
            return;
        }
        self.checked_path = self.path.clone();
        let log = PathBuf::from(self.path.trim());
        match contest_file_of_log(&log, &self.log_files, &self.cfg_folder) {
            Some(LogContestFile::Found(p)) => self.choose_file(&p, registry, lang),
            Some(LogContestFile::Missing(p)) => self.locate = Some(p),
            None => {}
        }
    }

    fn remember(&mut self, setup: &Setup, registry: &Registry, settings: &mut Sections) {
        zlog_ui::write_station(section(settings, STATION), &setup.station);
        let recent = match &setup.contest_file {
            Some(file) => Some(RecentContest::File(file.clone())),
            None => registry
                .contests()
                .get(setup.contest)
                .map(|c| RecentContest::Id(c.id().to_string())),
        };
        if let Some(r) = recent {
            add_recent_contest(section(settings, MENU), r);
            self.recent = recent_contests(section(settings, MENU));
        }
        let files = section(settings, CONTEST_FILES);
        set_log_contest_file(files, &setup.path, setup.contest_file.as_deref());
        self.log_files = log_contest_files(files);
    }
}

fn section<'a>(settings: &'a mut Sections, name: &str) -> &'a mut Section {
    settings.entry(name.to_string()).or_default()
}

impl StartScreen for MenuForm {
    fn sections(&self) -> &[&'static str] {
        &[MENU, CONTEST_FILES, STATION]
    }

    fn load(&mut self, settings: &Sections) {
        let empty = Section::new();
        let get = |name: &str| settings.get(name).unwrap_or(&empty);
        self.station = zlog_ui::read_station(get(STATION));
        self.recent = recent_contests(get(MENU));
        self.pinned = pinned_contests(get(MENU));
        self.cfg_folder = get(MENU).get("cfg_folder").cloned().unwrap_or_default();
        self.log_files = log_contest_files(get(CONTEST_FILES));
    }

    fn show(&mut self, ui: &mut egui::Ui, view: &mut StartView<'_>) -> Option<Setup> {
        let (registry, lang) = (view.registry, view.language);
        self.check_log_path(registry, lang);
        let folder = self.cfg_folder.clone();
        self.dialogs(ui.ctx(), registry, lang);
        if self.cfg_folder != folder {
            section(view.settings, MENU).insert("cfg_folder".into(), self.cfg_folder.clone());
            view.save = true;
        }
        let base = registry
            .contests()
            .iter()
            .position(|c| c.id() == QUICK_CONTEST);
        let ((_, pressed), _) = zlog_ui::dialog::window_parts(
            ui,
            "menu_form",
            None::<fn(&mut egui::Ui)>,
            Some(|ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "button.ok")).clicked() {
                        Some(Pressed::Ok)
                    } else if base.is_some()
                        && ui
                            .button(tr(lang, "menu_form.quick"))
                            .on_hover_text(
                                tr(lang, "menu_form.quick.tip")
                                    .replace("{0}", &quick_log_path().display().to_string()),
                            )
                            .clicked()
                    {
                        Some(Pressed::Quick)
                    } else if view.back && ui.button(tr(lang, "button.cancel")).clicked() {
                        Some(Pressed::Cancel)
                    } else if !view.back && ui.button(tr(lang, "menu.file.exit")).clicked() {
                        Some(Pressed::Exit)
                    } else {
                        None
                    }
                })
                .inner
            }),
            |ui, _| self.form(ui, registry, lang),
        );
        if let Some(c) = self.pin_toggled.take() {
            let menu = section(view.settings, MENU);
            toggle_pin(menu, c);
            self.pinned = pinned_contests(menu);
            view.save = true;
        }
        let setup = match pressed.flatten() {
            Some(Pressed::Ok) => match self
                .validate(lang)
                .and_then(|setup| self.set_aside(&setup.path, lang).map(|_| setup))
            {
                Ok(setup) => Some(setup),
                Err(e) => {
                    self.error = Some(e);
                    None
                }
            },
            Some(Pressed::Quick) => base.map(|contest| self.quick_setup(contest)),
            Some(Pressed::Cancel) => {
                view.cancel = true;
                None
            }
            Some(Pressed::Exit) => {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                None
            }
            None => None,
        };
        if let Some(s) = &setup {
            self.remember(s, registry, view.settings);
            view.save = true;
        }
        setup
    }

    fn show_error(&mut self, error: String) {
        self.error = Some(error);
    }

    fn has_window(&self) -> bool {
        self.entries.iter().any(|e| e.is_open())
    }

    fn close_windows(&mut self) {
        for e in &mut self.entries {
            e.close();
        }
        self.locate = None;
    }
}

impl MenuForm {
    fn title_of(&mut self, p: &Path, registry: &Registry) {
        if !self.titles.contains_key(p) {
            let title = registry
                .contest_files()
                .iter()
                .find(|f| f.accepts(p))
                .and_then(|f| f.title(p))
                .unwrap_or_else(|| file_name(p));
            self.titles.insert(p.to_path_buf(), title);
        }
    }

    fn folder_contest_files(&mut self, registry: &Registry) -> &[PathBuf] {
        let stale = self
            .folder_files
            .as_ref()
            .is_none_or(|(f, _)| *f != self.cfg_folder);
        if stale {
            let kinds = registry.contest_files();
            let mut files: Vec<PathBuf> = std::fs::read_dir(&self.cfg_folder)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && kinds.iter().any(|k| k.accepts(p)))
                .collect();
            files.sort();
            self.folder_files = Some((self.cfg_folder.clone(), files));
        }
        self.folder_files.as_ref().map_or(&[], |(_, f)| f)
    }

    fn updates(&mut self, registry: &Registry, lang: &str) -> Vec<(usize, PathBuf)> {
        let mut view = EntryView {
            registry,
            language: lang,
            contest_folder: &mut self.cfg_folder,
        };
        let mut out = Vec::new();
        for (i, e) in self.entries.iter_mut().enumerate() {
            if e.shown(registry) {
                out.extend(e.updates(&mut view).into_iter().map(|p| (i, p)));
            }
        }
        out
    }

    fn pinned_first(&self) -> Vec<RecentContest> {
        let mut all = self.pinned.clone();
        all.extend(
            self.recent
                .iter()
                .filter(|r| !self.pinned.contains(r))
                .cloned(),
        );
        all
    }

    fn quick_setup(&self, contest: usize) -> Setup {
        let path = quick_log_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Setup {
            contest,
            contest_file: None,
            mode: ContestMode::All,
            station: self.station.clone(),
            coeff: 1.0,
            path,
            use_contest_period: true,
            convert_old_other: false,
        }
    }

    fn set_aside(&mut self, path: &Path, lang: &str) -> Result<(), String> {
        if self.replace.take().as_deref() != Some(path) || !path.exists() {
            return Ok(());
        }
        let mut aside = path.as_os_str().to_owned();
        aside.push(".bak");
        let aside = PathBuf::from(aside);
        let _ = std::fs::remove_file(&aside);
        std::fs::rename(path, &aside).map_err(|e| {
            texts().format(
                lang,
                "menu_form.replace_failed",
                &[&path.display().to_string(), &e.to_string()],
            )
        })
    }

    fn validate(&self, lang: &str) -> Result<Setup, String> {
        if self.path.trim().is_empty() {
            return Err(tr(lang, "menu_form.need_file").into());
        }
        let coeff: f64 = self
            .coeff
            .trim()
            .parse()
            .map_err(|_| tr(lang, "menu_form.bad_coeff"))?;
        Ok(Setup {
            contest: self.contest,
            contest_file: self.file.as_ref().map(|f| f.path.clone()),
            mode: self.mode,
            station: {
                let mut st = self.station.clone();
                if let Some(p) = self.power {
                    st.set_power_everywhere(p);
                }
                st
            },
            coeff,
            path: PathBuf::from(self.path.trim()),
            use_contest_period: true,
            convert_old_other: false,
        })
    }
}

const GROUP_FRAME: f32 = 14.0;

const FIELD_WIDTH: f32 = 160.0;

const RESERVED_HEIGHT: f32 = 390.0;

const SIDE_WIDTH: f32 = 110.0;

const MODE_WIDTH: f32 = SIDE_WIDTH + 40.0;

const QUICK_CONTEST: &str = "base";

#[derive(Clone, Copy)]
enum Pressed {
    Ok,
    Quick,
    Exit,
    Cancel,
}

pub fn quick_log_path() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("log.zlox")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Builtin(usize),
    File(PathBuf),
}

const RECENT_KEPT: usize = 8;

const LOGS_KEPT: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecentContest {
    Id(String),
    File(PathBuf),
}

impl RecentContest {
    fn parse(v: &str) -> Self {
        match v.strip_prefix("file:") {
            Some(p) => RecentContest::File(PathBuf::from(p)),
            None => RecentContest::Id(v.to_string()),
        }
    }

    fn format(&self) -> String {
        match self {
            RecentContest::Id(id) => id.clone(),
            RecentContest::File(p) => format!("file:{}", p.display()),
        }
    }
}

fn recent_contests(s: &Section) -> Vec<RecentContest> {
    let numbered: Vec<RecentContest> = (0..RECENT_KEPT)
        .map_while(|i| s.get(&format!("recent.{i}")))
        .map(|v| RecentContest::parse(v))
        .collect();
    if !numbered.is_empty() {
        return numbered;
    }
    s.get("recent")
        .map(|r| {
            r.split(',')
                .filter(|x| !x.is_empty())
                .map(|x| RecentContest::Id(x.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn add_recent_contest(s: &mut Section, contest: RecentContest) {
    let mut all = recent_contests(s);
    all.retain(|x| *x != contest);
    all.insert(0, contest);
    all.truncate(RECENT_KEPT);
    s.remove("recent");
    s.retain(|k, _| !k.starts_with("recent."));
    for (i, c) in all.iter().enumerate() {
        s.insert(format!("recent.{i}"), c.format());
    }
}

fn pinned_contests(s: &Section) -> Vec<RecentContest> {
    (0..)
        .map_while(|i| s.get(&format!("pinned.{i}")))
        .map(|v| RecentContest::parse(v))
        .collect()
}

fn toggle_pin(s: &mut Section, contest: RecentContest) {
    let mut all = pinned_contests(s);
    match all.iter().position(|c| *c == contest) {
        Some(i) => {
            all.remove(i);
        }
        None => all.push(contest),
    }
    s.retain(|k, _| !k.starts_with("pinned."));
    for (i, c) in all.iter().enumerate() {
        s.insert(format!("pinned.{i}"), c.format());
    }
}

fn recent_of(row: &Row, contests: &[Box<dyn ContestDef>]) -> RecentContest {
    match row {
        Row::Builtin(i) => RecentContest::Id(contests[*i].id().to_string()),
        Row::File(p) => RecentContest::File(p.clone()),
    }
}

fn search_rows(
    needle: &str,
    contests: &[Box<dyn ContestDef>],
    lang: &str,
    files: &[PathBuf],
    titles: &HashMap<PathBuf, String>,
) -> Vec<Row> {
    let hit = |text: &str| text.to_lowercase().contains(needle);
    let builtin = (0..contests.len())
        .filter(|i| {
            let c = &contests[*i];
            hit(&c.caption(lang)) || hit(c.id()) || hit(&c.group(lang))
        })
        .map(Row::Builtin);
    let local = files
        .iter()
        .filter(|p| hit(titles.get(*p).map_or("", String::as_str)) || hit(&file_name(p)))
        .cloned()
        .map(Row::File);
    builtin.chain(local).collect()
}

fn log_contest_files(s: &Section) -> Vec<(PathBuf, PathBuf)> {
    (0..LOGS_KEPT)
        .map_while(|i| Some((s.get(&format!("log.{i}"))?, s.get(&format!("file.{i}"))?)))
        .map(|(l, f)| (PathBuf::from(l), PathBuf::from(f)))
        .collect()
}

fn set_log_contest_file(s: &mut Section, log: &Path, file: Option<&Path>) {
    let mut all = log_contest_files(s);
    all.retain(|(l, _)| l != log);
    if let Some(f) = file {
        all.insert(0, (log.to_path_buf(), f.to_path_buf()));
    }
    all.truncate(LOGS_KEPT);
    s.clear();
    for (i, (l, f)) in all.iter().enumerate() {
        s.insert(format!("log.{i}"), l.display().to_string());
        s.insert(format!("file.{i}"), f.display().to_string());
    }
}

fn retired(lang: &str, ids: &[String]) -> String {
    let named: Vec<String> = ids.iter().map(|id| format!("{id} ({id}.cfg)")).collect();
    tr(lang, "menu_form.retired").replace("{0}", &named.join(", "))
}

fn recent_rows(recent: &[RecentContest], contests: &[Box<dyn ContestDef>]) -> Vec<Row> {
    recent
        .iter()
        .filter_map(|r| match r {
            RecentContest::Id(id) => contests.iter().position(|c| c.id() == id).map(Row::Builtin),
            RecentContest::File(p) => Some(Row::File(p.clone())),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LogContestFile {
    Found(PathBuf),
    Missing(PathBuf),
}

fn contest_file_of_log(
    log: &Path,
    remembered: &[(PathBuf, PathBuf)],
    folder: &str,
) -> Option<LogContestFile> {
    let (_, file) = remembered.iter().find(|(l, _)| l == log)?;
    if file.is_file() {
        return Some(LogContestFile::Found(file.clone()));
    }
    if !folder.is_empty() {
        let moved = Path::new(folder).join(file.file_name()?);
        if moved.is_file() {
            return Some(LogContestFile::Found(moved));
        }
    }
    Some(LogContestFile::Missing(file.clone()))
}

fn badge(ui: &mut egui::Ui, lang: &str) {
    let text = egui::RichText::new(tr(lang, "menu_form.update"))
        .small()
        .color(zlog_ui::palette::good(ui));
    ui.label(text)
        .on_hover_text(tr(lang, "menu_form.update.tip"));
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn pick_contest_file(files: &[Box<dyn ContestFile>], lang: &str) -> Option<PathBuf> {
    let exts: Vec<&str> = files.iter().flat_map(|f| f.extensions().to_vec()).collect();
    let name = files.first().map(|f| f.name(lang)).unwrap_or_default();
    zlog_ui::dialog::import_file(&name, &exts)
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.start_screen(|ctx| MenuForm::new(ctx.contest_entries()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zlog-open-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    struct Def(&'static str, &'static str, &'static str);

    impl zlog_core::ContestDef for Def {
        fn id(&self) -> &str {
            self.0
        }
        fn caption(&self, _language: &str) -> String {
            self.1.into()
        }
        fn group(&self, _language: &str) -> String {
            self.2.into()
        }
        fn build(&self, mode: zlog_core::ContestMode) -> Box<dyn zlog_core::Contest> {
            let base = Registry::new().with(zlog_rule::Base);
            base.contests()[0].build(mode)
        }
    }

    struct TitleFile;

    impl ContestFile for TitleFile {
        fn name(&self, _language: &str) -> String {
            "Title file".into()
        }
        fn extensions(&self) -> &[&str] {
            &["cfg"]
        }
        fn open(&self, path: &Path, _language: &str) -> Result<zlog_core::OpenedContest, String> {
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let title = text.lines().next().unwrap_or("").trim_start_matches('#');
            let title: &'static str = Box::leak(title.to_string().into_boxed_str());
            Ok(zlog_core::OpenedContest {
                def: Box::new(Def("file", title, "File")),
                warnings: Vec::new(),
            })
        }
    }

    struct Fakes;

    impl zlog_core::Extension for Fakes {
        fn name(&self) -> &str {
            "fakes"
        }
        fn contests(&self) -> Vec<Box<dyn zlog_core::ContestDef>> {
            vec![
                Box::new(Def("alpha", "Alpha Test", "Group A")),
                Box::new(Def("beta", "Beta Test", "Group A")),
                Box::new(Def("gamma", "Gamma Test", "Group B")),
            ]
        }
        fn contest_files(&self) -> Vec<Box<dyn ContestFile>> {
            vec![Box::new(TitleFile)]
        }
    }

    fn fakes() -> Registry {
        Registry::new().with(Fakes)
    }

    fn registry() -> Registry {
        fakes().with(zlog_rule::Base)
    }

    #[test]
    fn every_language_has_every_key() {
        assert!(texts().missing().is_empty(), "{:?}", texts().missing());
    }

    #[test]
    fn quick_start_uses_the_base_rules_and_one_fixed_file() {
        assert!(registry()
            .contests()
            .iter()
            .any(|c| c.id() == QUICK_CONTEST));
        let path = quick_log_path();
        assert!(path.ends_with("zlog/log.zlox"), "{}", path.display());
        assert_eq!(path, quick_log_path());
    }

    #[test]
    fn contests_no_longer_built_in_are_named_with_their_contest_file() {
        let contests = fakes();
        assert!(retired("en", &["retired".into()]).contains("retired (retired.cfg)"));
        let mut form = MenuForm::new(Vec::new());
        form.choose(ChosenContest::Linked("retired".into()), &contests, "en");
        assert!(form.error.unwrap().contains("retired"));
    }

    #[test]
    fn recent_rows_keep_files_and_linked_contests() {
        let contests = fakes();
        let contests = contests.contests();
        let id = contests[1].id().to_string();
        let recent = vec![
            RecentContest::File(PathBuf::from("/x/a.cfg")),
            RecentContest::Id("gone".into()),
            RecentContest::Id(id),
        ];
        assert_eq!(
            recent_rows(&recent, contests),
            vec![Row::File(PathBuf::from("/x/a.cfg")), Row::Builtin(1)]
        );
    }

    #[test]
    fn recent_contests_and_contest_files_round_trip() {
        let mut menu = Section::new();
        menu.insert("recent".into(), "acag,allja".into());
        assert_eq!(
            recent_contests(&menu),
            vec![
                RecentContest::Id("acag".into()),
                RecentContest::Id("allja".into())
            ]
        );
        let cfg = PathBuf::from("/cfg/a=b, c.CFG");
        add_recent_contest(&mut menu, RecentContest::File(cfg.clone()));
        add_recent_contest(&mut menu, RecentContest::Id("allja".into()));
        let mut files = Section::new();
        set_log_contest_file(&mut files, Path::new("/logs/x.zlox"), Some(&cfg));
        set_log_contest_file(&mut files, Path::new("/logs/y.zlox"), Some(&cfg));
        set_log_contest_file(&mut files, Path::new("/logs/y.zlox"), None);
        assert_eq!(
            recent_contests(&menu),
            vec![
                RecentContest::Id("allja".into()),
                RecentContest::File(cfg.clone()),
                RecentContest::Id("acag".into()),
            ]
        );
        assert_eq!(menu["recent.1"], "file:/cfg/a=b, c.CFG");
        assert_eq!(
            log_contest_files(&files),
            vec![(PathBuf::from("/logs/x.zlox"), cfg)]
        );
    }

    #[test]
    fn contest_groups_are_entries() {
        let reg = registry();
        let groups = zlog_ui::contest_groups(&reg);
        let mut titles: Vec<String> = Vec::new();
        for c in reg.contests() {
            if !titles.contains(&c.group("en")) {
                titles.push(c.group("en"));
            }
        }
        let shown: Vec<String> = groups.iter().map(|g| g.title(&reg, "en")).collect();
        assert_eq!(shown, titles);
        let first = groups[0].contests(&reg, "en").unwrap();
        assert!(first
            .iter()
            .all(|i| reg.contests()[*i].group("en") == titles[0]));
    }

    #[test]
    fn a_logs_contest_file_is_found_moved_or_missing() {
        let dir = scratch("log");
        let moved = dir.join("moved");
        std::fs::create_dir_all(&moved).unwrap();
        let cfg = dir.join("a.cfg");
        std::fs::write(&cfg, "#A\n").unwrap();
        let log = PathBuf::from("/logs/a.zlox");
        let gone = PathBuf::from("/nowhere/b.cfg");
        let remembered = vec![
            (log.clone(), cfg.clone()),
            (PathBuf::from("/b.zlox"), gone.clone()),
        ];
        assert_eq!(
            contest_file_of_log(&log, &remembered, ""),
            Some(LogContestFile::Found(cfg))
        );
        assert_eq!(
            contest_file_of_log(Path::new("/c.zlox"), &remembered, ""),
            None
        );
        let b = Path::new("/b.zlox");
        assert_eq!(
            contest_file_of_log(b, &remembered, ""),
            Some(LogContestFile::Missing(gone))
        );
        std::fs::write(moved.join("b.cfg"), "#B\n").unwrap();
        assert_eq!(
            contest_file_of_log(b, &remembered, &moved.display().to_string()),
            Some(LogContestFile::Found(moved.join("b.cfg")))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pins_are_kept_and_listed_first() {
        let mut menu = Section::new();
        let cfg = RecentContest::File(PathBuf::from("/cfg/a.cfg"));
        toggle_pin(&mut menu, RecentContest::Id("allja".into()));
        toggle_pin(&mut menu, cfg.clone());
        assert_eq!(menu["pinned.1"], "file:/cfg/a.cfg");
        for i in 0..RECENT_KEPT + 2 {
            add_recent_contest(&mut menu, RecentContest::Id(format!("c{i}")));
        }
        let mut form = MenuForm::new(Vec::new());
        form.load(&[(MENU.to_string(), menu.clone())].into_iter().collect());
        let first = form.pinned_first();
        assert_eq!(first[..2], [RecentContest::Id("allja".into()), cfg.clone()]);
        assert_eq!(first.len(), 2 + RECENT_KEPT);
        toggle_pin(&mut menu, RecentContest::Id("allja".into()));
        assert_eq!(pinned_contests(&menu), [cfg]);
        assert!(!menu.contains_key("pinned.1"));
    }

    #[test]
    fn search_finds_linked_contests_and_folder_files() {
        let dir = scratch("search");
        std::fs::write(dir.join("kanagawa.cfg"), "#Kanagawa QSO Party\n").unwrap();
        std::fs::write(dir.join("other.cfg"), "#Other\n").unwrap();
        let reg = registry();
        let mut form = MenuForm::new(Vec::new());
        form.cfg_folder = dir.display().to_string();
        let files = form.folder_contest_files(&reg).to_vec();
        assert_eq!(files.len(), 2);
        for p in &files {
            form.title_of(p, &reg);
        }
        let contests = reg.contests();
        let rows = search_rows("kanagawa", contests, "en", &files, &form.titles);
        assert_eq!(rows, [Row::File(dir.join("kanagawa.cfg"))]);
        let c = &contests[0];
        let rows = search_rows(
            &c.caption("en").to_lowercase(),
            contests,
            "en",
            &files,
            &form.titles,
        );
        assert!(rows.contains(&Row::Builtin(0)));
        let group = c.group("en").to_lowercase();
        let rows = search_rows(&group, contests, "en", &files, &form.titles);
        assert!(rows.contains(&Row::Builtin(0)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn choosing_a_contest_file() {
        let dir = scratch("scan");
        std::fs::write(dir.join("b.CFG"), "#Beta Contest\n").unwrap();
        let reg = fakes();
        let mut form = MenuForm {
            path: "x.zlox".into(),
            ..MenuForm::new(Vec::new())
        };
        form.choose_file(&dir.join("b.CFG"), &reg, "en");
        assert_eq!(
            form.current(reg.contests()).unwrap().caption("en"),
            "Beta Contest"
        );
        let setup = form.validate("en").unwrap();
        assert_eq!(setup.contest_file, Some(dir.join("b.CFG")));
        form.choose_file(&dir.join("none.cfg"), &reg, "en");
        assert!(form.error.is_some());
        assert_eq!(
            form.validate("en").unwrap().contest_file,
            Some(dir.join("b.CFG"))
        );
        form.choose(
            ChosenContest::Linked(reg.contests()[1].id().into()),
            &reg,
            "en",
        );
        assert_eq!(form.validate("en").unwrap().contest_file, None);
        assert_eq!(form.contest, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod replace_tests {
    use super::*;

    #[test]
    fn a_replaced_log_is_set_aside_and_an_opened_one_is_kept() {
        let dir = std::env::temp_dir().join(format!("zlog-open-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("old.zlox");
        std::fs::write(&log, b"old").unwrap();
        let mut form = MenuForm::new(Vec::new());
        form.set_aside(&log, "en").unwrap();
        assert!(log.exists());
        form.replace = Some(log.clone());
        form.set_aside(&log, "en").unwrap();
        assert!(!log.exists());
        assert_eq!(std::fs::read(dir.join("old.zlox.bak")).unwrap(), b"old");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
