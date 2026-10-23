//! The zLog Menu shown at start (`TMenuForm`): contest, mode, station, file.
//!
//! The contest menu on the left lists the recent contests, then every
//! [`ContestEntry`] linked in, in order: the groups of the linked contests and
//! windows such as the user-defined contests. OK, or Start everyday logging,
//! returns the [`Setup`] of the log to open.
//!
//! Settings kept (same sections and keys as before the menu was an extension):
//! `[menu]` `recent.N` and `cfg_folder`, `[contest_files]` `log.N` and `file.N`,
//! and `[station]`, written when a log is opened.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zlog_core::{ContestDef, ContestFile, ContestMode, Power, Registry, Station};
use zlog_ui::{
    ChosenContest, ContestEntry, EntryView, Section, Sections, Setup, StartScreen, StartView,
};

zlog_core::texts!();

/// The text of `key` in `language`.
fn tr(language: &str, key: &'static str) -> &'static str {
    texts().get(language, key)
}

/// The text of `key` in `language`, for tests that click through the menu.
#[doc(hidden)]
pub fn text(language: &str, key: &'static str) -> &'static str {
    tr(language, key)
}

/// The section of the menu's own keys.
const MENU: &str = "menu";

/// The section of the logs' contest files.
const CONTEST_FILES: &str = "contest_files";

/// The section of the station, shared with the Options dialog.
const STATION: &str = "station";

/// A contest opened from a file (a CFG) and chosen in the form, as zLog's
/// "user-defined contest" button (`TMenuForm.SelectButtonClick`).
struct ChosenFile {
    path: PathBuf,
    def: Box<dyn ContestDef>,
    warnings: Vec<String>,
}

pub struct MenuForm {
    /// Index into the registry's contest list.
    contest: usize,
    /// Text typed in the search box; filters across all groups.
    search: String,
    /// The entry of the contest menu shown in the list; `None` is the recently
    /// used contests.
    entry: Option<usize>,
    /// The entries of the contest menu, in the order linked.
    entries: Vec<Box<dyn ContestEntry>>,
    /// Recently used contests, newest first.
    recent: Vec<RecentContest>,
    /// The chosen contest file; when set, it is the contest instead of `contest`.
    file: Option<ChosenFile>,
    /// Titles of contest files shown in the recent list, read once each.
    titles: HashMap<PathBuf, String>,
    /// Folder of the contest files (zLog's `CFGDATPath`).
    cfg_folder: String,
    /// Logs with the contest file they were made with ([`log_contest_files`]).
    log_files: Vec<(PathBuf, PathBuf)>,
    /// The file name last looked up in `log_files`.
    checked_path: String,
    /// The contest file of the log being opened, not found: ask to locate it.
    locate: Option<PathBuf>,
    mode: ContestMode,
    station: Station,
    /// One power on every band, or `None` for zLog's default per band.
    power: Option<Power>,
    coeff: String,
    path: String,
    error: Option<String>,
}

impl MenuForm {
    /// The form listing `entries` in its contest menu.
    pub fn new(entries: Vec<Box<dyn ContestEntry>>) -> Self {
        MenuForm {
            contest: 0,
            search: String::new(),
            entry: None,
            entries,
            recent: Vec::new(),
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
            error: None,
        }
    }

    /// The chosen contest: the contest file, else the built-in one.
    fn current<'a>(&'a self, contests: &'a [Box<dyn ContestDef>]) -> Option<&'a dyn ContestDef> {
        match &self.file {
            Some(f) => Some(&*f.def),
            None => contests.get(self.contest).map(|c| &**c),
        }
    }

    /// Choose the contest defined by the file at `path`; a file that cannot be read
    /// leaves the choice as it was and shows why.
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

    /// Choose what an entry's window returned.
    fn choose(&mut self, chosen: ChosenContest, registry: &Registry, lang: &str) {
        match chosen {
            ChosenContest::File(p) => self.choose_file(&p, registry, lang),
            ChosenContest::Linked(id) => {
                if let Some(i) = registry.contests().iter().position(|c| c.id() == id) {
                    self.contest = i;
                    self.file = None;
                }
            }
        }
    }

    /// When the file name changed, choose the contest file its log was made with
    /// (zLog's `LastCFGFileName`), or ask for it when it is gone.
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

    /// Remember what `setup` opens: the station, the contest among the recent
    /// ones, and the contest file of its log.
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

/// The section `name` of `settings`, made when missing.
fn section<'a>(settings: &'a mut Sections, name: &str) -> &'a mut Section {
    settings.entry(name.to_string()).or_default()
}

impl StartScreen for MenuForm {
    fn sections(&self) -> &[&'static str] {
        &[MENU, CONTEST_FILES, STATION]
    }

    /// The form filled from the settings: station, recent contests, contest folder.
    fn load(&mut self, settings: &Sections) {
        let empty = Section::new();
        let get = |name: &str| settings.get(name).unwrap_or(&empty);
        self.station = zlog_ui::read_station(get(STATION));
        self.recent = recent_contests(get(MENU));
        self.cfg_folder = get(MENU).get("cfg_folder").cloned().unwrap_or_default();
        self.log_files = log_contest_files(get(CONTEST_FILES));
    }

    /// Draw the form filling `ui` (the main window below its menu bar): OK and Exit
    /// pinned to the bottom, the rest scrolling. Returns the setup when OK is
    /// pressed with valid input; Exit closes the program.
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
                    } else if ui.button(tr(lang, "menu.file.exit")).clicked() {
                        Some(Pressed::Exit)
                    } else {
                        None
                    }
                })
                .inner
            }),
            |ui, _| self.form(ui, registry, lang),
        );
        let setup = match pressed.flatten() {
            Some(Pressed::Ok) => match self.validate(lang) {
                Ok(setup) => Some(setup),
                Err(e) => {
                    self.error = Some(e);
                    None
                }
            },
            Some(Pressed::Quick) => base.map(|contest| self.quick_setup(contest)),
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

    /// Whether a window of the form is open (an entry's, such as the user-defined
    /// contests).
    fn has_window(&self) -> bool {
        self.entries.iter().any(|e| e.is_open())
    }

    /// Close the entries' windows and the question for a missing contest file.
    fn close_windows(&mut self) {
        for e in &mut self.entries {
            e.close();
        }
        self.locate = None;
    }
}

impl MenuForm {
    /// The Base contest in all modes, logged into [`quick_log_path`] (added to
    /// when it exists), with the station as typed in the form.
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

    /// The fields of the form: contest and mode, then the station in two columns.
    fn form(&mut self, ui: &mut egui::Ui, registry: &Registry, lang: &str) {
        let contests = registry.contests();
        // The contest list takes the width the mode buttons leave.
        let width = (ui.available_width() - MODE_WIDTH).max(340.0);
        ui.horizontal_top(|ui| {
            self.contest_selector(ui, registry, lang, width);
            ui.vertical(|ui| {
                ui.group(|ui| {
                    ui.set_width(SIDE_WIDTH);
                    ui.vertical(|ui| {
                        ui.label(tr(lang, "menu_form.mode"));
                        let modes = self.current(contests).map_or(&[][..], |c| c.modes());
                        let mut mode = self.mode;
                        for &m in modes {
                            ui.radio_value(&mut mode, m, mode_caption(m));
                        }
                        self.mode = mode;
                    });
                });
                ui.group(|ui| {
                    ui.set_width(SIDE_WIDTH);
                    ui.vertical(|ui| {
                        ui.label(tr(lang, "menu_form.power"));
                        ui.radio_value(&mut self.power, None, tr(lang, "menu_form.power_by_band"));
                        for p in ['H', 'M', 'L', 'P'] {
                            ui.radio_value(&mut self.power, Power::from_code(p), p.to_string());
                        }
                    });
                });
            });
        });
        if let Some(f) = &self.file {
            ui.label(format!(
                "{}: {}",
                tr(lang, "menu_form.cfg_file"),
                f.path.display()
            ));
            for w in &f.warnings {
                ui.colored_label(zlog_ui::palette::warning(ui), w);
            }
        }
        ui.add_space(6.0);
        // Two label-field pairs a row, so the common case fits without scrolling.
        let st = &mut self.station;
        let pairs: [(&'static str, &mut String); 9] = [
            ("menu_form.callsign", &mut st.callsign),
            ("menu_form.operator", &mut st.operator),
            ("menu_form.prov", &mut st.prov),
            ("menu_form.city", &mut st.city),
            ("menu_form.age", &mut st.age),
            ("menu_form.iota", &mut st.iota),
            ("menu_form.cq_zone", &mut st.cq_zone),
            ("menu_form.iaru_zone", &mut st.iaru_zone),
            ("menu_form.itu_region", &mut st.itu_region),
        ];
        // As wide as the contest panel above: the labels of each column as wide as
        // the widest, the fields sharing the rest equally.
        let text_width = |ui: &egui::Ui, key: &'static str| {
            egui::WidgetText::from(tr(lang, key))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x
        };
        let label = |col: usize| {
            pairs
                .iter()
                .skip(col)
                .step_by(2)
                .map(|(k, _)| *k)
                .chain((col == 1).then_some("menu_form.tx_nr"))
                .map(|k| text_width(ui, k))
                .fold(0.0, f32::max)
        };
        let labels = [label(0), label(1)];
        let gap = ui.spacing().item_spacing.x;
        let outer = width + GROUP_FRAME;
        let field = ((outer - labels[0] - labels[1] - 3.0 * gap) / 2.0).max(FIELD_WIDTH);
        egui::Grid::new("menu_station")
            .num_columns(4)
            .show(ui, |ui| {
                for (i, (key, value)) in pairs.into_iter().enumerate() {
                    let height = ui.spacing().interact_size.y;
                    ui.allocate_ui_with_layout(
                        egui::vec2(labels[i % 2], height),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(labels[i % 2]);
                            ui.label(tr(lang, key));
                        },
                    );
                    upper_edit(ui, value, field);
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
                ui.label(tr(lang, "menu_form.tx_nr"));
                ui.add(egui::DragValue::new(&mut st.tx_nr).range(0..=15));
                ui.end_row();
            });
        egui::Grid::new("menu_fields")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(tr(lang, "menu_form.coeff"));
                ui.add_enabled(
                    self.current(contests).is_some_and(|c| c.uses_coeff()),
                    egui::TextEdit::singleline(&mut self.coeff).desired_width(60.0),
                );
                ui.end_row();
                ui.end_row();
            });
        let (mut open, mut new) = (false, false);
        ui.scope(|ui| {
            ui.set_max_width(width + GROUP_FRAME);
            zlog_ui::field_row(
                ui,
                |ui| {
                    ui.label(tr(lang, "menu_form.file"));
                },
                // A save panel cannot pick an existing file on macOS, so opening and
                // creating are separate buttons.
                |ui| {
                    new = ui.button(tr(lang, "menu_form.new")).clicked();
                    open = ui.button(tr(lang, "menu_form.open")).clicked();
                },
                |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(f32::INFINITY))
                },
            );
        });
        if open {
            if let Some(p) = zlog_ui::dialog::open_log_file() {
                self.path = p.display().to_string();
            }
        }
        if new {
            if let Some(p) =
                zlog_ui::dialog::choose_log_file("contest.zlox", zlog_core::Format::Zlox)
                    .map(|to| to.path)
            {
                self.path = p.display().to_string();
            }
        }
        if let Some(e) = &self.error {
            ui.colored_label(zlog_ui::palette::error(ui), e);
        }
    }

    /// The contest menu on the left (recent contests, then every entry), the
    /// contests of the chosen entry (or the search results) on the right; the
    /// layout grows with the number of contests linked in. An entry with a window
    /// of its own (the user-defined contests) opens it.
    fn contest_selector(&mut self, ui: &mut egui::Ui, registry: &Registry, lang: &str, width: f32) {
        let contests = registry.contests();
        let listing: Vec<usize> = (0..self.entries.len())
            .filter(|i| self.entries[*i].shown(registry))
            .collect();
        if self.entry.is_none() && self.recent.is_empty() {
            self.entry = listing
                .iter()
                .copied()
                .find(|i| self.entries[*i].contests(registry, lang).is_some());
        }
        let needle = self.search.trim().to_lowercase();
        let shown: Vec<Row> = if !needle.is_empty() {
            (0..contests.len())
                .filter(|i| {
                    contests[*i].caption(lang).to_lowercase().contains(&needle)
                        || contests[*i].id().contains(&needle)
                })
                .map(Row::Builtin)
                .collect()
        } else if let Some(e) = self.entry {
            self.entries
                .get(e)
                .and_then(|e| e.contests(registry, lang))
                .unwrap_or_default()
                .into_iter()
                .map(Row::Builtin)
                .collect()
        } else {
            recent_rows(&self.recent, contests)
        };
        // Titles of the contest files listed, read once each.
        for row in &shown {
            if let Row::File(p) = row {
                if !self.titles.contains_key(p) {
                    let title = registry
                        .contest_files()
                        .iter()
                        .find(|f| f.accepts(p))
                        .and_then(|f| f.title(p))
                        .unwrap_or_else(|| file_name(p));
                    self.titles.insert(p.clone(), title);
                }
            }
        }
        let mut pick: Option<Row> = None;
        let mut opened: Option<usize> = None;
        ui.group(|ui| {
            ui.set_max_width(width);
            ui.vertical(|ui| {
                ui.label(tr(lang, "menu_form.contest"));
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "menu_form.search"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search).desired_width(f32::INFINITY),
                    );
                });
                // Two columns as wide and as high as the form allows: the menu,
                // and the contests of the entry.
                // The rest of the form (station, file, buttons) keeps its room.
                let height = (ui.ctx().content_rect().height() - RESERVED_HEIGHT).max(230.0);
                let size = egui::vec2(width - 24.0, height);
                let groups_width = (size.x * 0.3).clamp(110.0, 220.0);
                let layout = egui::Layout::left_to_right(egui::Align::Min);
                ui.allocate_ui_with_layout(size, layout, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("groups")
                        .max_height(size.y)
                        .show(ui, |ui| {
                            ui.set_width(groups_width);
                            ui.vertical(|ui| {
                                if !self.recent.is_empty() {
                                    ui.selectable_value(
                                        &mut self.entry,
                                        None,
                                        tr(lang, "menu_form.recent"),
                                    );
                                }
                                for &i in &listing {
                                    let e = &self.entries[i];
                                    let title = e.title(registry, lang);
                                    if e.contests(registry, lang).is_some() {
                                        ui.selectable_value(&mut self.entry, Some(i), title);
                                    } else if ui.selectable_label(false, title).clicked() {
                                        opened = Some(i);
                                    }
                                }
                            });
                        });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("contests")
                        .max_height(size.y)
                        .show(ui, |ui| {
                            ui.set_width((size.x - groups_width - 20.0).max(210.0));
                            ui.vertical(|ui| {
                                for row in shown {
                                    let (on, caption) = match &row {
                                        Row::Builtin(i) => (
                                            self.file.is_none() && self.contest == *i,
                                            contests[*i].caption(lang),
                                        ),
                                        Row::File(p) => (
                                            self.file.as_ref().is_some_and(|f| f.path == *p),
                                            self.titles.get(p).cloned().unwrap_or_default(),
                                        ),
                                    };
                                    if ui.radio(on, caption).clicked() {
                                        pick = Some(row);
                                    }
                                }
                            });
                        });
                });
            });
        });
        match pick {
            Some(Row::Builtin(i)) => {
                self.contest = i;
                self.file = None;
            }
            Some(Row::File(p)) => self.choose_file(&p, registry, lang),
            None => {}
        }
        if let Some(i) = opened {
            let mut view = EntryView {
                registry,
                language: lang,
                contest_folder: &mut self.cfg_folder,
            };
            self.entries[i].open(&mut view);
        }
        // Keep the mode one the chosen contest offers.
        if let Some(c) = self.current(contests) {
            if !c.modes().contains(&self.mode) {
                self.mode = c.modes().first().copied().unwrap_or(ContestMode::Mix);
            }
        }
    }

    /// The entries' windows and the question for a log's missing contest file,
    /// each over the zLog Menu while open.
    fn dialogs(&mut self, ctx: &egui::Context, registry: &Registry, lang: &str) {
        let mut chosen = Vec::new();
        for e in &mut self.entries {
            let mut view = EntryView {
                registry,
                language: lang,
                contest_folder: &mut self.cfg_folder,
            };
            chosen.extend(e.ui(ctx, &mut view));
        }
        for c in chosen {
            self.choose(c, registry, lang);
        }
        if let Some(missing) = self.locate.clone() {
            let (_, answer) = zlog_ui::dialog::modal_with_buttons(
                ctx,
                "locate_contest_file",
                |ui| {
                    ui.label(
                        tr(lang, "menu_form.cfg_missing")
                            .replace("{0}", &missing.display().to_string()),
                    );
                },
                |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(tr(lang, "menu_form.locate")).clicked() {
                            Some(true)
                        } else if ui.button(tr(lang, "button.cancel")).clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            Some(false)
                        } else {
                            None
                        }
                    })
                    .inner
                },
            );
            match answer {
                Some(true) => {
                    self.locate = None;
                    if let Some(p) = pick_contest_file(registry.contest_files(), lang) {
                        self.choose_file(&p, registry, lang);
                    }
                }
                Some(false) => self.locate = None,
                None => {}
            }
        }
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
            // Filled in by the caller from the settings and the operator's answer.
            use_contest_period: true,
            convert_old_other: false,
        })
    }
}

/// What a group's frame adds to the width of its contents (margins and stroke).
const GROUP_FRAME: f32 = 14.0;

/// Narrowest a station field of the form gets.
const FIELD_WIDTH: f32 = 160.0;

/// Height of the window the contest list leaves to the menu bar, the search,
/// the station fields, the file name and the buttons.
const RESERVED_HEIGHT: f32 = 390.0;

/// Width of the mode and power boxes right of the contest list, one for both.
const SIDE_WIDTH: f32 = 110.0;

/// Room the mode and power boxes take, with their frames and spacing.
const MODE_WIDTH: f32 = SIDE_WIDTH + 40.0;

/// The contest of the quick-start button: the Base rules of zlog-rule.
const QUICK_CONTEST: &str = "base";

/// A button at the bottom of the form.
#[derive(Clone, Copy)]
enum Pressed {
    Ok,
    Quick,
    Exit,
}

/// The log of the quick-start button: `zlog/log.zlox` ([`zlog_ui::APP_DIR`]) in the documents folder
/// (the data folder when there is none), the same file every time.
pub fn quick_log_path() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("log.zlox")
}

/// A station field, `width` wide.
fn upper_edit(ui: &mut egui::Ui, text: &mut String, width: f32) {
    zlog_ui::sized_field(ui, egui::TextEdit::singleline(text), width);
    *text = text.to_uppercase();
}

fn mode_caption(m: ContestMode) -> &'static str {
    match m {
        ContestMode::Mix => "PH/CW(MIX)",
        ContestMode::Cw => "CW",
        ContestMode::Phone => "PH",
        ContestMode::Rtty => "RTTY",
        ContestMode::All => "ALL",
    }
}

/// A row of the contest list: a built-in contest, or a contest file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Builtin(usize),
    File(PathBuf),
}

/// How many recent contests the zLog Menu lists.
const RECENT_KEPT: usize = 8;

/// How many logs are remembered with their contest file.
const LOGS_KEPT: usize = 50;

/// A recently used contest: a built-in one by identifier, or a contest file.
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

/// The contests opened last, newest first (`[menu]` `recent.0`, `recent.1`, ...;
/// a file contest as `file:<path>`). The old comma list `recent` is read when
/// there are none of these.
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

/// Which contest file each log was made with (`log.N` and `file.N` of the
/// `contest_files` section, newest first), so that reopening a log chooses it
/// again (zLog keeps only the last one, `LastCFGFileName`).
fn log_contest_files(s: &Section) -> Vec<(PathBuf, PathBuf)> {
    (0..LOGS_KEPT)
        .map_while(|i| Some((s.get(&format!("log.{i}"))?, s.get(&format!("file.{i}"))?)))
        .map(|(l, f)| (PathBuf::from(l), PathBuf::from(f)))
        .collect()
}

/// Remember the contest file of `log`, or forget it (`None`, a built-in contest).
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

/// The rows of the recent list: built-in contests still linked in, and every
/// contest file (shown by title; one that is gone fails when chosen).
fn recent_rows(recent: &[RecentContest], contests: &[Box<dyn ContestDef>]) -> Vec<Row> {
    recent
        .iter()
        .filter_map(|r| match r {
            RecentContest::Id(id) => contests.iter().position(|c| c.id() == id).map(Row::Builtin),
            RecentContest::File(p) => Some(Row::File(p.clone())),
        })
        .collect()
}

/// Where the contest file of a log is, when the log was made with one.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LogContestFile {
    Found(PathBuf),
    /// Neither where it was nor in the contest folder.
    Missing(PathBuf),
}

/// The contest file remembered for `log`: where it was, else a file of the same
/// name in the contest folder (`TMenuForm.FormShow`: `CfgDatPath + ExtractFileName`).
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

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Pick a contest file of any kind linked in.
fn pick_contest_file(files: &[Box<dyn ContestFile>], lang: &str) -> Option<PathBuf> {
    let exts: Vec<&str> = files.iter().flat_map(|f| f.extensions().to_vec()).collect();
    let name = files.first().map(|f| f.name(lang)).unwrap_or_default();
    zlog_ui::dialog::import_file(&name, &exts)
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

    fn registry() -> Registry {
        Registry::new()
            .with(zlog_cfg::Cfg)
            .with(zlog_rule_ja::Japan)
            .with(zlog_rule::Base)
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
    fn recent_rows_keep_files_and_linked_contests() {
        let contests = Registry::new()
            .with(zlog_cfg::Cfg)
            .with(zlog_rule_ja::Japan);
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

    /// The groups of the registry are entries of the contest menu, in order.
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
    fn choosing_a_contest_file() {
        let dir = scratch("scan");
        std::fs::write(dir.join("b.CFG"), "#Beta Contest\n").unwrap();
        let reg = Registry::new()
            .with(zlog_cfg::Cfg)
            .with(zlog_rule_ja::Japan);
        // Choosing a file makes it the contest, with its own modes and coefficient.
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
        // A linked contest chosen in an entry's window replaces the file.
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
