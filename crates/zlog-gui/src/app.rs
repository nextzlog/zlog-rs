//! Top level: the zLog Menu until a contest is open, then the main window.

use crate::commands::{self, CoreCommands, CoreState, Source};
use crate::dialog::dialog;
use crate::i18n::{self, tr};
use crate::main_window::{self, View};
use crate::options::{rig_window, Options};
use crate::registry::{apply_style, entry_style, panels, registry, settings_pages, start_screen};
use crate::rig::RigConfig;
use crate::rigs::Rigs;
use crate::session::{Session, Status};
use crate::settings::Settings;
use crate::windows;
use zlog_ui::{Commands, EntryField, EntryStyle};

/// A window added by an extension, and whether it is open.
struct PanelSlot {
    panel: Box<dyn zlog_ui::Panel>,
    open: bool,
    /// Whether the window was open when last drawn, to tell [`Panel::opened`].
    ///
    /// [`Panel::opened`]: zlog_ui::Panel::opened
    was_open: bool,
}

pub struct ZLogApp {
    session: Option<Session>,
    /// The toolbar's buttons, for the toolbar tab of Options while it is open.
    toolbar_rows: crate::toolbar::Rows,
    /// The shortcuts chosen in Options, put into the commands every frame.
    shortcuts: zlog_ui::keys::Overrides,
    panels: Vec<PanelSlot>,
    /// What the panels showed in the status bar at the last frame.
    status_items: Vec<zlog_ui::StatusItem>,
    /// The zLog Menu, when an extension draws one ([`zlog_ui::StartScreen`]).
    menu: Option<Box<dyn zlog_ui::StartScreen>>,
    view: View,
    /// The entry row, in the style chosen in Options ([`zlog_ui::EntryStyle`]).
    entry: Option<Box<dyn EntryStyle>>,
    show_about: bool,
    #[cfg(feature = "ini")]
    ini: crate::ini::IniPort,
    settings: Settings,
    options: Options,
    /// Rig 1 and rig 2; used as the selected one.
    rig: Rigs,
    show_rig: bool,
    /// Spots from the panels, kept for [`SPOT_LIFETIME`] seconds.
    spots: Vec<zlog_ui::Spot>,
    /// When the panels' settings were last checked for changes ([`PANEL_SAVE_EVERY`]).
    panels_checked: std::time::Instant,
    /// A setup waiting for the answer whether to convert old Other QSOs.
    asking_old_other: Option<crate::session::Setup>,
    /// Frames left before quitting, once the windows were closed for it.
    quitting: Option<u8>,
    /// Saving failed before closing the log or quitting: the question shown.
    save_failed: Option<SaveFailed>,
    /// The operator chose to close or quit without saving.
    leave_unsaved: bool,
    /// A file a save dialog picked that exists and was not asked about: the
    /// question whether to replace it is shown.
    overwrite: Option<Overwrite>,
    /// The look of the settings is to be given to every window ([`apply_style`]).
    restyle: bool,
}

/// How often the panels' settings are saved when one of them changed.
const PANEL_SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// How long a spot stays in [`zlog_ui::View::spots`], in seconds.
const SPOT_LIFETIME: i64 = 30 * 60;

/// The manual of zLog, opened from the Help menu.
const MANUAL_URL: &str = "https://use.zlog.org";

impl ZLogApp {
    /// Quitting with windows open closes them first and quits a few frames later.
    /// On a Mac with a Touch Bar, AppKit crashes at exit when a window it watches
    /// for the Touch Bar goes away with the main one
    /// (`_NSTouchBarFinderObservation invalidate`, macOS 13).
    fn quit_after_windows(&mut self, ctx: &egui::Context) {
        let asked = ctx.input(|i| i.viewport().close_requested());
        if asked && !self.saved_before(After::Quit) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        if asked && self.quitting.is_none() {
            let open = !open_dialogs(&self.options, self.show_rig, &self.panels, self.show_about)
                .is_empty()
                || self.menu.as_ref().is_some_and(|m| m.has_window());
            if open {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.options.open = false;
                self.show_rig = false;
                self.show_about = false;
                for slot in &mut self.panels {
                    slot.open = false;
                    slot.panel.close_dialogs();
                }
                if let Some(m) = &mut self.menu {
                    m.close_windows();
                }
                self.quitting = Some(QUIT_FRAMES);
            }
        }
        match self.quitting {
            Some(0) => {
                self.quitting = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(n) => {
                self.quitting = Some(n - 1);
                ctx.request_repaint();
            }
            None => {}
        }
    }

    pub fn new() -> Self {
        let mut settings = Settings::load();
        // The language chosen in the menu wins over the locale of the system.
        if let Some(code) = settings.language() {
            i18n::set_language(&code);
        }
        let mut menu = start_screen();
        load_start_screen(&mut menu, &mut settings);
        let toolbar_rows = crate::toolbar::Rows::default();
        let options = Options::new(settings_pages(toolbar_rows.clone()));
        ZLogApp {
            session: None,
            panels: panels()
                .into_iter()
                .map(|mut panel| {
                    panel.load(settings.section(&panel_section(panel.id())));
                    PanelSlot {
                        panel,
                        open: false,
                        was_open: false,
                    }
                })
                .collect(),
            menu,
            view: View::default(),
            entry: entry_style(&settings.entry_style()),
            show_about: false,
            #[cfg(feature = "ini")]
            ini: Default::default(),
            shortcuts: options.shortcuts(&mut settings),
            options,
            toolbar_rows,
            settings,
            rig: Rigs::default(),
            show_rig: false,
            spots: Vec::new(),
            panels_checked: std::time::Instant::now(),
            status_items: Vec::new(),
            asking_old_other: None,
            quitting: None,
            save_failed: None,
            leave_unsaved: false,
            overwrite: None,
            restyle: true,
        }
    }

    /// Open the contest of a setup, or show why it failed in the zLog Menu.
    fn open(&mut self, setup: crate::session::Setup) {
        match Session::open(setup, registry()) {
            Ok(mut s) => {
                s.apply_preferences(&self.settings.preferences());
                if s.asks_start_time {
                    self.view.start_time =
                        Some(main_window::StartTimeForm::new(s.start_time(), true));
                }
                self.session = Some(s);
                if let Some(e) = &mut self.entry {
                    e.focus(EntryField::Call);
                }
            }
            Err(e) => {
                if let Some(m) = &mut self.menu {
                    m.show_error(e);
                }
            }
        }
    }

    /// Let every panel write what it keeps, and save the settings file when any
    /// section changed (on exit, when the log is closed and every
    /// [`PANEL_SAVE_EVERY`]).
    fn save_panels(&mut self) {
        if panel_sections(&self.panels, &mut self.settings) {
            let _ = self.settings.save();
        }
        self.panels_checked = std::time::Instant::now();
    }

    /// Save the log, if it has unsaved changes, before `after`; when it cannot be
    /// saved, ask what to do ([`ZLogApp::save_failed_dialog`]). Whether `after`
    /// may go on now.
    fn saved_before(&mut self, after: After) -> bool {
        if self.leave_unsaved || self.save_failed.is_some() {
            return self.leave_unsaved;
        }
        let Some(s) = self.session.as_mut().filter(|s| s.unsaved()) else {
            return true;
        };
        match s.try_save() {
            Ok(()) => true,
            Err(error) => {
                self.save_failed = Some(SaveFailed { error, after });
                false
            }
        }
    }

    /// Carry out what waited for the log to be saved.
    fn go_on(&mut self, ctx: &egui::Context, after: After) {
        match after {
            After::CloseLog => self.close(),
            After::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// The log could not be saved before closing it or quitting: try again, save
    /// it elsewhere, go on without saving, or stay.
    fn save_failed_dialog(&mut self, ctx: &egui::Context) {
        let Some(failed) = &self.save_failed else {
            return;
        };
        // Hidden while its Save As asks whether to overwrite: Esc there must not
        // close this dialog too (Yes would then go on and quit).
        if self.overwrite.is_some() {
            return;
        }
        let (after, error) = (failed.after, failed.error.clone());
        let discard = match after {
            After::CloseLog => tr("unsaved.close"),
            After::Quit => tr("unsaved.quit"),
        };
        let (_, pressed) = crate::dialog::modal_with_buttons(
            ctx,
            "save_failed",
            |ui| {
                ui.label(crate::i18n::trf("unsaved.failed", &[&error]));
            },
            |ui| {
                ui.horizontal(|ui| {
                    let mut pressed = None;
                    if ui.button(tr("unsaved.retry")).clicked() {
                        pressed = Some(0);
                    }
                    if ui.button(tr("menu.file.save_as")).clicked() {
                        pressed = Some(1);
                    }
                    if ui.button(discard).clicked() {
                        pressed = Some(2);
                    }
                    if ui.button(tr("button.cancel")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        pressed = Some(3);
                    }
                    pressed
                })
                .inner
            },
        );
        let Some(pressed) = pressed else {
            return;
        };
        let saved = match (pressed, self.session.as_mut()) {
            (0, Some(s)) => Some(s.try_save()),
            (1, Some(s)) => {
                let name = s
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match crate::dialog::choose_log_file(&name, zlog_core::Format::Zlox) {
                    Some(to) if to.ask => {
                        self.overwrite = Some(Overwrite {
                            path: to.path,
                            then: Then::SaveAs(Some(after)),
                        });
                        None
                    }
                    to => to.map(|to| s.try_save_as(to.path)),
                }
            }
            _ => None,
        };
        match (pressed, saved) {
            (0 | 1, Some(saved)) => self.after_save(ctx, after, saved),
            (2, _) => {
                self.save_failed = None;
                self.leave_unsaved = true;
                self.go_on(ctx, after);
            }
            (3, _) => self.save_failed = None,
            _ => {}
        }
    }

    /// Go on once the log was saved before closing it or quitting, or show why not.
    fn after_save(&mut self, ctx: &egui::Context, after: After, saved: Result<(), String>) {
        match saved {
            Ok(()) => {
                self.save_failed = None;
                self.go_on(ctx, after);
            }
            Err(e) => {
                if let Some(f) = &mut self.save_failed {
                    f.error = e;
                }
            }
        }
    }

    /// Ask whether to replace the file in [`ZLogApp::overwrite`], then do what
    /// waited for the answer.
    fn overwrite_dialog(&mut self, ctx: &egui::Context) {
        let Some(o) = &self.overwrite else {
            return;
        };
        let question = crate::i18n::trf("file.overwrite", &[&o.path.display()]);
        let Some(yes) = crate::dialog::ask_overwrite(ctx, &question, crate::i18n::current()) else {
            return;
        };
        let Some(Overwrite { path, then }) = self.overwrite.take() else {
            return;
        };
        if !yes {
            return;
        }
        match then {
            Then::SaveAs(after) => {
                let Some(s) = self.session.as_mut() else {
                    return;
                };
                match after {
                    Some(after) => {
                        let saved = s.try_save_as(path);
                        self.after_save(ctx, after, saved);
                    }
                    None => s.save_as(path),
                }
            }
            Then::Export(i) => {
                if let Some(s) = self.session.as_mut() {
                    s.export(i, &path);
                }
            }
            #[cfg(feature = "ini")]
            Then::Ini => export_ini(&self.panels, &mut self.settings, &mut self.ini, &path),
        }
    }

    fn close(&mut self) {
        if let Some(mut s) = self.session.take() {
            // Nothing may go on transmitting without a log: stop every panel and
            // carry out the PTT requests that gives at once (the panels are not
            // ticked again until a log is opened).
            let view = panel_view(None, None, None, &s, &self.spots, &self.rig);
            stop_transmitting(&mut self.panels, &view);
            if std::mem::take(&mut self.leave_unsaved) {
                s.close_unsaved();
            } else {
                s.close();
            }
        }
        self.view = View::default();
        self.save_panels();
    }
}

/// Stop every panel (as Esc does) and carry out at once the PTT requests that
/// gives, for when the panels will not be ticked again (the log is closed).
fn stop_transmitting(panels: &mut [PanelSlot], view: &zlog_ui::View<'_>) {
    for slot in panels.iter_mut() {
        slot.panel.on_key(zlog_ui::EntryKey::Escape, view);
    }
    let actions: Vec<zlog_ui::EntryAction> = panels
        .iter_mut()
        .flat_map(|s| s.panel.take_actions())
        .collect();
    for action in device_first(actions) {
        match action {
            zlog_ui::EntryAction::Transmit(on) => {
                panels.iter_mut().any(|s| s.panel.transmit(on));
            }
            // What Esc sends the keyer port (an SO2R box back to its receive rig).
            zlog_ui::EntryAction::KeyerPort(bytes) => {
                panels.iter_mut().any(|s| s.panel.keyer_port(&bytes));
            }
            _ => {}
        }
    }
}

/// Whether nothing is typed in the entry row (the RSTs are filled in by themselves).
fn nothing_typed(e: &crate::entry::Entry) -> bool {
    e.callsign.is_empty() && e.nr_rcvd.is_empty() && e.memo.is_empty()
}

/// The command a row key runs: every row key with a grid row selected (`rows`),
/// one with Ctrl or Alt from an empty entry row; `activate` is a double click on
/// the row (Enter). `Err` says why the command is disabled.
fn row_command(
    ctx: &egui::Context,
    all: &[commands::Listed],
    rows: bool,
    empty_entry: bool,
    activate: bool,
) -> Option<Result<(commands::Source, String), String>> {
    if !rows && !empty_entry {
        return None;
    }
    let enter = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Enter);
    let activated = activate
        .then(|| commands::with_row_key(all, &enter))
        .flatten();
    let (source, c) = activated.or_else(|| commands::row_key_pressed(ctx, all, !rows))?;
    match (&c.disabled_reason, c.enabled) {
        (_, true) => Some(Ok((source, c.id.clone()))),
        (Some(why), false) => Some(Err(why.clone())),
        (None, false) => None,
    }
}

/// `actions` with the rig switches and the keyer port bytes first, each in its
/// order: an SO2R box hears of a rig switch before the keyer sends anything more.
fn device_first(actions: Vec<zlog_ui::EntryAction>) -> Vec<zlog_ui::EntryAction> {
    let (mut first, rest): (Vec<_>, Vec<_>) = actions.into_iter().partition(|a| {
        matches!(
            a,
            zlog_ui::EntryAction::SelectRig(_) | zlog_ui::EntryAction::KeyerPort(_)
        )
    });
    first.extend(rest);
    first
}

/// Write what every panel keeps into its section; whether any section changed.
fn panel_sections(panels: &[PanelSlot], settings: &mut Settings) -> bool {
    let mut changed = false;
    for slot in panels {
        let section = settings.section(&panel_section(slot.panel.id()));
        let mut fresh = section.clone();
        slot.panel.save(&mut fresh);
        if fresh != *section {
            *section = fresh;
            changed = true;
        }
    }
    changed
}

/// Let the start screen read its settings again (at start-up, and after they
/// were changed in Options or imported from zlog.ini).
fn load_start_screen(menu: &mut Option<Box<dyn zlog_ui::StartScreen>>, settings: &mut Settings) {
    if let Some(m) = menu {
        m.load(&settings.sections_named(m.sections()));
    }
}

/// The Language menu; the choice is saved in the settings for the next start.
fn language_menu(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.menu_button(tr("menu.language"), |ui| {
        for l in i18n::LANGUAGES {
            if ui.radio(i18n::current() == l.code, l.name).clicked() {
                i18n::set_language(l.code);
                crate::fonts::install(ui.ctx());
                settings.set_language(l.code);
                let _ = settings.save();
            }
        }
    });
}

impl eframe::App for ZLogApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.restyle) {
            apply_style(&ctx, &mut self.settings);
        }
        self.quit_after_windows(&ctx);
        self.save_failed_dialog(&ctx);
        self.overwrite_dialog(&ctx);
        if self.session.is_some() && self.panels_checked.elapsed() >= PANEL_SAVE_EVERY {
            self.save_panels();
        }
        if self.session.is_none() && self.asking_old_other.is_some() {
            if let Some(answer) = ask_old_other(&ctx) {
                if let Some(mut setup) = self.asking_old_other.take() {
                    setup.convert_old_other = answer;
                    self.open(setup);
                }
            }
        }
        let Some(session) = self.session.as_mut() else {
            egui::Panel::top("menu").show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| language_menu(ui, &mut self.settings))
            });
            let Some(menu) = self.menu.as_mut() else {
                egui::CentralPanel::default().show(ui, |ui| ui.label(tr("start_screen.none")));
                return;
            };
            {
                let linked = registry();
                let mut view = zlog_ui::StartView {
                    registry: &linked,
                    language: i18n::current(),
                    settings: &mut self.settings.sections_named(menu.sections()),
                    save: false,
                };
                let shown = menu.show(ui, &mut view);
                let save = view.save;
                let sections = std::mem::take(view.settings);
                self.settings.set_sections(sections);
                if save {
                    let _ = self.settings.save();
                }
                if let Some(mut setup) = shown.filter(|_| self.asking_old_other.is_none()) {
                    setup.use_contest_period = self.settings.use_contest_period();
                    if crate::session::asks_old_other(&setup.path) {
                        self.asking_old_other = Some(setup);
                    } else {
                        self.open(setup);
                    }
                }
            }
            return;
        };

        session.poll();
        // The main window's keys wait while the overwrite question is up.
        if self.overwrite.is_none() {
            route_keys(
                &ctx,
                session,
                &mut self.view,
                self.entry.as_deref_mut(),
                &mut self.panels,
                &self.options,
                &self.spots,
                &self.rig,
            );
        }
        // A selection past the end (QSOs deleted, here or over Z-Link) is none: the
        // entry row's keys work again.
        let n = session.log().qsos.len();
        if self.view.selected.is_some_and(|i| i >= n) {
            self.view.selected = None;
        }
        // Shift+X in the entry row (`actionToggleRig`).
        if std::mem::take(&mut self.view.toggle_rig) && self.rig.toggle() {
            session.status = rig_selected(self.rig.number());
        }
        self.rig.tick(
            std::time::Instant::now(),
            &mut session.entry,
            &session.station,
        );

        let mut close = false;
        let mut close_all = false;
        let dialogs = open_dialogs(&self.options, self.show_rig, &self.panels, self.show_about);
        crate::dialog::set_keep_on_top(&ctx, self.settings.keep_dialogs_on_top());
        // Every command of the menu bar and the toolbar, from one list.
        let state = CoreState {
            unsaved: session.unsaved(),
            period: session.rules().uses_period() && session.use_contest_period,
            formats: session
                .formats()
                .iter()
                .map(|f| (f.name().to_string(), f.can_import(), f.can_export()))
                .collect(),
            rig_connected: self.rig.connected(),
            show_rig: self.show_rig,
        };
        let core: Vec<CoreCommands> = commands::CORE_GROUPS
            .iter()
            .map(|g| CoreCommands {
                group: *g,
                state: state.clone(),
                chosen: None,
            })
            .collect();
        let lang = i18n::current();
        let mut all = commands::list(
            &core,
            self.panels.iter().map(|s| (s.panel.as_ref(), s.open)),
            lang,
        );
        if self.options.open {
            options_commands(&mut self.options, &self.toolbar_rows, &all, lang);
        }
        commands::apply_shortcuts(&mut all, &self.shortcuts);
        // No shortcut runs under the overwrite question: Import or Export would
        // replace the question waiting for its answer.
        let asking = self.overwrite.is_some();
        let mut chosen = if self.options.wants_keys() || asking {
            None
        } else {
            commands::pressed(&ctx, &all)
        };
        // Keys of the log grid while a row is selected (Ctrl+Z); a disabled
        // command says why on the status line. Those with Ctrl or Alt also run
        // from the entry row while nothing is typed in it: an empty row has no
        // text of its own to undo.
        let v = &mut self.view;
        let empty_entry =
            focus(&ctx, self.entry.as_deref()).is_some() && nothing_typed(&session.entry);
        if !asking && !modal(&self.panels) && v.start_time.is_none() {
            let activate = std::mem::take(&mut v.activate) && v.selected.is_some();
            match row_command(&ctx, &all, v.selected.is_some(), empty_entry, activate) {
                Some(Ok(c)) => chosen = Some(c),
                Some(Err(why)) => session.status = Status::warning(why),
                None => {}
            }
        }
        #[cfg(feature = "ini")]
        let mut ini_to: Option<crate::dialog::SaveTo> = None;
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(tr("menu.file"), |ui| {
                    let sections = commands::menu_sections(&all, zlog_ui::MenuGroup::File);
                    if let Some(c) = commands::draw_sections(ui, &sections, |_, _| None) {
                        chosen = Some(c);
                    }
                });
                ui.menu_button(tr("menu.edit"), |ui| {
                    // The QSO commands of every provider, as in the right-click
                    // menu, with the submenus of the QSO selected.
                    let sections = edit_sections(&all);
                    let picked = commands::draw_sections(ui, &sections, |ui, section| {
                        (section == zlog_ui::MenuSection::Qso)
                            .then(|| qso_submenus(ui, session, &mut self.view, &self.panels))
                            .flatten()
                    });
                    if let Some(c) = picked {
                        chosen = Some(c);
                    }
                });
                // The extensions' menus; the Device menu is always there, for the rig.
                let mut groups: Vec<zlog_ui::MenuGroup> = zlog_ui::MenuGroup::ALL
                    .into_iter()
                    .filter(|g| commands::of_group(&all, *g).next().is_some())
                    .collect();
                groups.sort_by_key(|g| crate::menus::position(*g));
                for group in groups {
                    ui.menu_button(tr(crate::menus::group_key(group)), |ui| {
                        if group == zlog_ui::MenuGroup::Device {
                            let config = RigConfig::load(self.settings.section(self.rig.section()));
                            ui.weak(rig_status(
                                zlog_rig_cat::MODELS[config.model].name,
                                &config.port,
                                self.rig.connected(),
                                self.rig.error().or_else(|| self.rig.message.clone()),
                            ));
                        }
                        let sections = commands::menu_sections(&all, group);
                        if let Some(c) = commands::draw_sections(ui, &sections, |_, _| None) {
                            chosen = Some(c);
                        }
                    });
                }
                ui.menu_button(tr("menu.settings"), |ui| {
                    if ui.button(tr("menu.settings.options")).clicked() {
                        options_commands(&mut self.options, &self.toolbar_rows, &all, lang);
                        self.options.show_with(&mut self.settings);
                        ui.close();
                    }
                    ui.separator();
                    language_menu(ui, &mut self.settings);
                    #[cfg(feature = "ini")]
                    {
                        ui.separator();
                        ini_to = self.ini.menu(ui);
                    }
                    // Extensions' Settings items (the ZyLO plugins) open their windows.
                    let sections = commands::menu_sections(&all, zlog_ui::MenuGroup::Settings);
                    if !sections.is_empty() {
                        ui.separator();
                    }
                    if let Some(c) = commands::draw_sections(ui, &sections, |_, _| None) {
                        chosen = Some(c);
                    }
                });
                ui.menu_button(tr("menu.window"), |ui| {
                    let any = !dialogs.is_empty();
                    if ui
                        .add_enabled(any, egui::Button::new(tr("menu.window.front")))
                        .clicked()
                    {
                        for (id, _) in &dialogs {
                            crate::dialog::focus(&ctx, id);
                        }
                    }
                    let mut on_top = self.settings.keep_dialogs_on_top();
                    if ui.checkbox(&mut on_top, tr("menu.window.on_top")).changed() {
                        self.settings.set_keep_dialogs_on_top(on_top);
                        let _ = self.settings.save();
                        crate::dialog::set_keep_on_top(&ctx, on_top);
                        let level = if on_top {
                            egui::WindowLevel::AlwaysOnTop
                        } else {
                            egui::WindowLevel::Normal
                        };
                        for (id, _) in &dialogs {
                            ctx.send_viewport_cmd_to(
                                crate::dialog::viewport_id(id),
                                egui::ViewportCommand::WindowLevel(level),
                            );
                        }
                    }
                    if ui
                        .add_enabled(any, egui::Button::new(tr("menu.window.close_all")))
                        .clicked()
                    {
                        close_all = true;
                    }
                    if any {
                        ui.separator();
                    }
                    for (id, title) in &dialogs {
                        if ui.button(title).clicked() {
                            crate::dialog::focus(&ctx, id);
                        }
                    }
                });
                ui.menu_button(tr("menu.help"), |ui| {
                    if ui.button(tr("menu.help.about")).clicked() {
                        self.show_about = true;
                    }
                    if ui.button(tr("menu.help.manual")).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(MANUAL_URL));
                    }
                    // Extensions' Help items (the updater, say) open their windows.
                    let sections = commands::menu_sections(&all, zlog_ui::MenuGroup::Help);
                    if !sections.is_empty() {
                        ui.separator();
                    }
                    if let Some(c) = commands::draw_sections(ui, &sections, |_, _| None) {
                        chosen = Some(c);
                    }
                });
            });
            // Wrapped onto more rows when the window is too narrow for every button.
            ui.horizontal_wrapped(|ui| {
                let style = self.settings.toolbar_style();
                for (n, group) in commands::toolbar_buttons(&all, &mut self.settings)
                    .into_iter()
                    .enumerate()
                {
                    if n > 0 {
                        ui.separator();
                    }
                    for (source, c) in group {
                        if let Some(id) = commands::toolbar_button(ui, c, style) {
                            chosen = Some((source, id));
                        }
                    }
                }
            });
        });
        // A command of the right-click menu of a QSO, chosen at the last frame.
        let grid_command = self
            .view
            .grid_action
            .take_if(|(_, a)| matches!(a, main_window::GridAction::Command(..)));
        if let Some((_, main_window::GridAction::Command(source, id))) = grid_command {
            chosen = Some((source, id));
        }
        if let Some((source, id)) = chosen {
            let mut ran = Ran {
                ctx: &ctx,
                session,
                view: &mut self.view,
                entry: self.entry.as_deref_mut(),
                rig: &mut self.rig,
                show_rig: &mut self.show_rig,
                close: &mut close,
                overwrite: &mut self.overwrite,
            };
            match source {
                Source::Core(group) => {
                    let mut provider = CoreCommands {
                        group,
                        state,
                        chosen: None,
                    };
                    provider.run(&id);
                    if let Some(id) = provider.chosen {
                        ran.core(&id);
                    }
                }
                Source::Panel(i) => {
                    if let Some(slot) = self.panels.get_mut(i) {
                        zlog_ui::PanelCommands {
                            panel: slot.panel.as_mut(),
                            open: &mut slot.open,
                        }
                        .run(&id);
                    }
                }
            }
        }
        if close_all {
            // The panels' dialogs and Options close as by Cancel.
            self.options.open = false;
            self.show_rig = false;
            self.show_about = false;
            for slot in &mut self.panels {
                slot.open = false;
                slot.panel.close_dialogs();
            }
        }
        #[cfg(feature = "ini")]
        {
            match ini_to {
                Some(to) if to.ask => {
                    self.overwrite = Some(Overwrite {
                        path: to.path,
                        then: Then::Ini,
                    });
                }
                Some(to) => export_ini(&self.panels, &mut self.settings, &mut self.ini, &to.path),
                None => {}
            }
            if self.ini.dialogs(&ctx, &mut self.settings) == crate::ini::Done::Imported {
                for slot in &mut self.panels {
                    slot.panel
                        .load(self.settings.section(&panel_section(slot.panel.id())));
                }
                // As after OK in Options; an open Options would write its old copy back.
                self.options.open = false;
                session.station = self.settings.station();
                session.use_contest_period = self.settings.use_contest_period();
                session.apply_preferences(&self.settings.preferences());
                load_start_screen(&mut self.menu, &mut self.settings);
                self.restyle = true;
            }
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            main_window::status_bar(ui, session, &self.status_items)
        });
        let hz = self.rig.state().and_then(|s| s.freq());
        let typed = match self.entry.as_deref_mut() {
            Some(entry) => {
                egui::Panel::bottom("entry")
                    .show(ui, |ui| {
                        main_window::entry_row(ui, session, &mut self.view, entry, hz)
                    })
                    .inner
            }
            None => false,
        };
        if typed {
            let view = panel_view(
                focus(&ctx, self.entry.as_deref()),
                editing(&self.panels),
                self.view.selected,
                session,
                &self.spots,
                &self.rig,
            );
            for slot in &mut self.panels {
                slot.panel.on_key(zlog_ui::EntryKey::Typed, &view);
            }
        }
        let panels = &self.panels;
        let extra = |q: &zlog_core::Qso| qso_items(panels, q);
        let context = context_commands(&all);
        egui::CentralPanel::default().show(ui, |ui| {
            main_window::log_grid(ui, session, &mut self.view, &context, &extra)
        });

        match self.view.grid_action.take() {
            Some((id, main_window::GridAction::Extension(i, item))) => {
                if let Some(q) = session.find(id).map(|n| session.log().qsos[n].clone()) {
                    if let Some(slot) = self.panels.get_mut(i) {
                        if slot.panel.on_qso_menu(&item, &q) {
                            slot.open = true;
                        }
                    }
                }
            }
            // Run at the next frame, with the commands.
            Some(action @ (_, main_window::GridAction::Command(..))) => {
                self.view.grid_action = Some(action)
            }
            Some((id, action)) => main_window::apply_grid_action(session, id, action),
            None => {}
        }
        windows::start_time_dialog(&ctx, session, &mut self.view);
        if let Some(changed) = self.options.ui(&ctx, &mut self.settings) {
            self.shortcuts = self.options.shortcuts(&mut self.settings);
            session.station = self.settings.station();
            session.use_contest_period = self.settings.use_contest_period();
            session.apply_preferences(&self.settings.preferences());
            load_start_screen(&mut self.menu, &mut self.settings);
            self.restyle = true;
            let style = self.settings.entry_style();
            if self.entry.as_ref().is_some_and(|e| e.id() != style) {
                self.entry = entry_style(&style);
            }
            // A tab may edit a panel's section (the Discord tab, `panel.dc`); only
            // the panels whose section changed reload, as the others keep newer
            // state than their section.
            for slot in &mut self.panels {
                let section = panel_section(slot.panel.id());
                if changed.contains(&section) {
                    slot.panel.load(self.settings.section(&section));
                }
            }
            for (n, section) in crate::rigs::SECTIONS.iter().enumerate() {
                let rig = self.rig.slot_mut(n as u8 + 1);
                // Only a rig whose settings changed: reconnecting drops its link.
                if rig.connected() && changed.iter().any(|c| c == section) {
                    rig.connect(RigConfig::load(self.settings.section(section)));
                }
            }
        }
        let rig_config = RigConfig::load(self.settings.section(self.rig.section()));
        if let Some(n) = rig_window(&ctx, &mut self.rig, &mut self.show_rig, &rig_config) {
            if self.rig.select(n) {
                session.status = rig_selected(n);
            }
        }
        let view = panel_view(
            focus(&ctx, self.entry.as_deref()),
            editing(&self.panels),
            self.view.selected,
            session,
            &self.spots,
            &self.rig,
        );
        for slot in &mut self.panels {
            slot.panel.tick(&view);
        }
        // Drawn at the next frame, which is asked for at once when they changed
        // (the entry row is drawn after the status bar).
        let items: Vec<zlog_ui::StatusItem> = self
            .panels
            .iter()
            .flat_map(|s| s.panel.status(&view))
            .collect();
        if items != self.status_items {
            self.status_items = items;
            ctx.request_repaint();
        }
        for slot in &mut self.panels {
            if slot.open && !slot.was_open {
                slot.panel.opened(&view);
            }
            slot.was_open = slot.open;
        }
        for slot in self.panels.iter_mut().filter(|s| s.panel.has_window()) {
            let (id, title, size) = (
                slot.panel.id().to_string(),
                slot.panel.title(view.language),
                slot.panel.default_size(),
            );
            if slot.panel.has_buttons() {
                // Both parts draw the panel, one after the other.
                let panel = std::cell::RefCell::new(&mut slot.panel);
                let mut close = false;
                crate::dialog::dialog_with_buttons(
                    &ctx,
                    &id,
                    &title,
                    size,
                    &mut slot.open,
                    |ui| panel.borrow_mut().ui(ui, &view),
                    |ui| close |= panel.borrow_mut().buttons(ui, &view),
                );
                if close {
                    slot.open = false;
                }
            } else {
                dialog(&ctx, &id, &title, size, &mut slot.open, |ui| {
                    slot.panel.ui(ui, &view)
                });
            }
        }
        for slot in &mut self.panels {
            slot.panel.dialogs(&ctx, &view);
        }
        let now = chrono::Utc::now().timestamp();
        for slot in &mut self.panels {
            self.spots.extend(slot.panel.take_spots());
        }
        self.spots.retain(|s| now - s.time < SPOT_LIFETIME);
        // Requests from panels to the entry row (a decoded callsign clicked).
        let actions: Vec<zlog_ui::EntryAction> = self
            .panels
            .iter_mut()
            .flat_map(|s| s.panel.take_actions())
            .collect();
        for action in device_first(actions) {
            match action {
                zlog_ui::EntryAction::Callsign(c) => {
                    pick_callsign(&mut session.entry, c);
                    session.check_call();
                    if let Some(e) = &mut self.entry {
                        e.focus(EntryField::Call);
                    }
                }
                zlog_ui::EntryAction::Number(n) => {
                    session.entry.nr_rcvd = n;
                    session.check_number();
                    if let Some(e) = &mut self.entry {
                        e.focus(EntryField::Number);
                    }
                }
                zlog_ui::EntryAction::Qsy(hz, mode) => {
                    self.rig.qsy(hz, mode, &mut session.entry, &session.station);
                }
                zlog_ui::EntryAction::Cq(cq) => session.entry.cq = cq,
                zlog_ui::EntryAction::SelectRig(n) => {
                    if self.rig.select(n) {
                        session.status = rig_selected(n);
                    }
                }
                zlog_ui::EntryAction::Transmit(on) => {
                    let keyed = self.panels.iter_mut().any(|s| s.panel.transmit(on));
                    if on && !keyed {
                        session.status = Status::warning(tr("status.no_ptt"));
                    }
                }
                zlog_ui::EntryAction::KeyerPort(bytes) => {
                    if !self.panels.iter_mut().any(|s| s.panel.keyer_port(&bytes)) {
                        session.status = Status::warning(tr("status.no_keyer_port"));
                    }
                }
                zlog_ui::EntryAction::Edit { batch, done } => session.edit(batch, done),
                zlog_ui::EntryAction::Say(item) => {
                    session.status = Status {
                        text: item.text,
                        warning: item.warning,
                    }
                }
                // Into the settings too, so that Options and the next start keep it.
                zlog_ui::EntryAction::Operator(op) => {
                    self.settings
                        .section("station")
                        .insert("operator".into(), op.clone());
                    let _ = self.settings.save();
                    session.station.operator = op;
                }
                zlog_ui::EntryAction::Power(p) => session.entry.power = p,
                zlog_ui::EntryAction::SelectQso(id) => {
                    if let Some(i) = session.find(id) {
                        self.view.selected = Some(i);
                    }
                }
            }
        }
        let shown = crate::dialog::dialog_with_buttons(
            &ctx,
            "about",
            tr("menu.help.about"),
            [520.0, 360.0],
            &mut self.show_about,
            |ui| {
                ui.heading("zLog RS");
                ui.label(env!("CARGO_PKG_VERSION"));
                ui.separator();
                ui.label(tr("about.contributors"));
                for call in CONTRIBUTORS {
                    ui.label(format!("  {call}"));
                }
            },
            |ui| {
                ui.button(tr("button.close")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
            },
        );
        if shown.is_some_and(|(_, close)| close) {
            self.show_about = false;
        }

        let title = format!(
            "zLog RS - {} - {}",
            session.rules().name(),
            session.path.display()
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        // The clock in the entry row, and extensions waiting on the outside.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));

        if close && self.saved_before(After::CloseLog) {
            self.close();
        }
    }

    fn on_exit(&mut self) {
        self.save_panels();
        self.close();
    }
}

/// Keys of the main window: F1 to F12 (with or without Shift) and Esc go first to
/// the first panel that takes them (a CW keyer), then the entry row and the log grid
/// get the rest. zLog's `actionCQAbort` (ShortCut 27) runs before the grid's
/// `KeyDown`, so Esc aborts CW even with a grid row selected.
#[allow(clippy::too_many_arguments)]
fn route_keys(
    ctx: &egui::Context,
    session: &mut Session,
    view: &mut View,
    mut entry: Option<&mut (dyn EntryStyle + 'static)>,
    panels: &mut [PanelSlot],
    options: &Options,
    spots: &[zlog_ui::Spot],
    rig: &Rigs,
) {
    if keys_for_panels(ctx, view, entry.as_deref(), panels, options) {
        let pv = panel_view(
            focus(ctx, entry.as_deref()),
            editing(panels),
            view.selected,
            session,
            spots,
            rig,
        );
        for n in 1..=12u8 {
            let Some(key) = egui::Key::from_name(&format!("F{n}")) else {
                continue;
            };
            for (modifiers, shift) in [
                (egui::Modifiers::NONE, false),
                (egui::Modifiers::SHIFT, true),
            ] {
                let key_of_panels = zlog_ui::EntryKey::Function { n, shift };
                if pressed(ctx, modifiers, key)
                    && panels
                        .iter_mut()
                        .any(|s| s.panel.on_key(key_of_panels, &pv))
                {
                    ctx.input_mut(|i| i.consume_key(modifiers, key));
                }
            }
        }
        // Every panel stops (zLog `CQAbort` stops CW and voice alike).
        if pressed(ctx, egui::Modifiers::NONE, egui::Key::Escape)
            && panels.iter_mut().fold(false, |took, s| {
                s.panel.on_key(zlog_ui::EntryKey::Escape, &pv) | took
            })
        {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        }
    }
    let hz = rig.state().and_then(|s| s.freq());
    main_window::handle_keys(ctx, session, view, entry.take(), modal(panels), hz);
}

/// The question whether to convert old Other QSOs (`TMainForm.LoadNewContestFromFile`):
/// `Some(answer)` once answered; Esc, Enter and closing answer No (zLog's default
/// button).
fn ask_old_other(ctx: &egui::Context) -> Option<bool> {
    let (_, answer) = crate::dialog::modal_with_buttons(
        ctx,
        "old_other",
        |ui| {
            ui.label(tr("open.old_other"));
        },
        |ui| {
            ui.horizontal(|ui| {
                if ui.button(tr("button.yes")).clicked() {
                    Some(true)
                } else if ui.button(tr("button.no")).clicked()
                    || ui.input(|i| {
                        i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)
                    })
                {
                    Some(false)
                } else {
                    None
                }
            })
            .inner
        },
    );
    answer
}

/// Whether keys of the main window may go to the panels: no modal dialog is open
/// and no text field other than the entry row's has the keyboard.
fn keys_for_panels(
    ctx: &egui::Context,
    view: &View,
    entry: Option<&dyn EntryStyle>,
    panels: &[PanelSlot],
    options: &Options,
) -> bool {
    let modal = modal(panels) || view.start_time.is_some() || options.open;
    let other_text = ctx.memory(|m| m.focused()).is_some() && focus(ctx, entry).is_none();
    !modal && !other_text
}

/// Whether `key` was pressed this frame with exactly `modifiers`, without consuming it.
fn pressed(ctx: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    ctx.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
                if *k == key && m.matches_exact(modifiers))
        })
    })
}

/// The field of the entry row holding the keyboard focus.
fn focus(ctx: &egui::Context, entry: Option<&dyn EntryStyle>) -> Option<EntryField> {
    entry.and_then(|e| e.focused(ctx))
}

/// The QSO a panel's dialog is open on, by ID.
fn editing(panels: &[PanelSlot]) -> Option<zlog_core::QsoId> {
    panels.iter().find_map(|s| s.panel.editing())
}

/// Whether a panel's dialog keeps the keys of the main window.
fn modal(panels: &[PanelSlot]) -> bool {
    panels.iter().any(|s| s.panel.modal())
}

/// The commands of the right-click menu of a QSO, by provider.
fn context_commands(all: &[commands::Listed]) -> main_window::ContextCommands {
    all.iter()
        .map(|l| {
            let list: Vec<_> = l.commands.iter().filter(|c| c.context).cloned().collect();
            (l.source, list)
        })
        .filter(|(_, list)| !list.is_empty())
        .collect()
}

/// What the panels see of the open contest, with `focus` in the entry row, the
/// edit dialog open on `editing` and the row `selected` of the log grid.
fn panel_view<'a>(
    focus: Option<zlog_ui::EntryField>,
    editing: Option<zlog_core::QsoId>,
    selected: Option<usize>,
    session: &'a Session,
    spots: &'a [zlog_ui::Spot],
    rig: &Rigs,
) -> zlog_ui::View<'a> {
    zlog_ui::View {
        log: session.log(),
        contest: session.rules(),
        sheet: session.sheet(),
        callsign: &session.entry.callsign,
        station: &session.station,
        number: &session.entry.nr_rcvd,
        rst_sent: &session.entry.rst_sent,
        power: session.entry.power,
        band: session.entry.band,
        mode: session.entry.mode,
        language: i18n::current(),
        spots,
        hz: rig.state().and_then(|s| s.freq()),
        rig: rig.number(),
        cq: session.entry.cq,
        operator: &session.station.operator,
        period_hours: session.period_hours(),
        pc_name: &session.pc_name,
        focus,
        editing: editing.and_then(|id| session.find(id).map(|i| &session.log().qsos[i])),
        selected: selected.and_then(|i| session.log().qsos.get(i)),
        period: session,
    }
}

/// A callsign picked in a panel replaces the call and clears the number
/// (`SetYourCallsignEx`, main.pas); a following `Number` action may fill it again.
fn pick_callsign(entry: &mut crate::entry::Entry, callsign: String) {
    entry.callsign = callsign;
    entry.nr_rcvd.clear();
}

/// What the tabs of Options list: the toolbar's buttons and the menus' commands,
/// as they are before the shortcuts chosen.
fn options_commands(
    options: &mut Options,
    toolbar: &crate::toolbar::Rows,
    all: &[commands::Listed],
    lang: &str,
) {
    *toolbar.borrow_mut() = commands::toolbar_rows(all, &[]);
    let (infos, reserved) = commands::command_infos(all, lang);
    options.set_commands(&infos, &reserved);
}

/// The items of the Edit menu by section: the Edit group's, and the QSO commands
/// of every provider (as in the right-click menu of a row) in the QSO section,
/// which is always there for the submenus of the QSO selected.
fn edit_sections(all: &[commands::Listed]) -> commands::Sections {
    let edit = zlog_ui::MenuGroup::Edit;
    let mut items: Vec<(Source, zlog_ui::Command)> = Vec::new();
    for l in all {
        for c in l.commands.iter().filter(|c| c.menu) {
            if c.context {
                let mut c = c.clone();
                c.section = zlog_ui::MenuSection::Qso;
                items.push((l.source, c));
            } else if l.group == edit {
                items.push((l.source, c.clone()));
            }
        }
    }
    let mut sections = commands::sectioned(items);
    if !sections
        .iter()
        .any(|(s, _)| *s == zlog_ui::MenuSection::Qso)
    {
        sections.push((zlog_ui::MenuSection::Qso, Vec::new()));
        sections.sort_by_key(|(s, _)| *s);
    }
    sections
}

/// The end of the Edit menu's QSO section: the Band, Mode and Power submenus of the
/// QSO selected and the extensions' items for it, or a hint to select one.
fn qso_submenus(
    ui: &mut egui::Ui,
    session: &mut Session,
    view: &mut View,
    panels: &[PanelSlot],
) -> Option<(Source, String)> {
    let selected = view
        .selected
        .and_then(|i| session.log().qsos.get(i))
        .map(|q| (zlog_core::QsoId::of(q), q.clone()));
    ui.add_enabled_ui(selected.is_some(), |ui| {
        let extra = selected
            .as_ref()
            .map(|(_, q)| qso_items(panels, q))
            .unwrap_or_default();
        if let Some(a) = main_window::qso_submenus(ui, session, &extra) {
            if let Some((id, _)) = &selected {
                view.grid_action = Some((*id, a));
            }
        }
    });
    if selected.is_none() {
        ui.weak(tr("menu.edit.select_row"));
    }
    None
}

/// What running one of the application's commands may change.
struct Ran<'a> {
    ctx: &'a egui::Context,
    session: &'a mut Session,
    view: &'a mut View,
    entry: Option<&'a mut (dyn EntryStyle + 'static)>,
    rig: &'a mut Rigs,
    show_rig: &'a mut bool,
    close: &'a mut bool,
    overwrite: &'a mut Option<Overwrite>,
}

/// Export the settings as an INI file at `path`.
#[cfg(feature = "ini")]
fn export_ini(
    panels: &[PanelSlot],
    settings: &mut Settings,
    ini: &mut crate::ini::IniPort,
    path: &std::path::Path,
) {
    // The panels' settings as they are now go into the file too.
    if panel_sections(panels, settings) {
        let _ = settings.save();
    }
    ini.export(path, settings);
}

/// A file to be replaced once the operator says so, and what writes it.
struct Overwrite {
    path: std::path::PathBuf,
    then: Then,
}

/// What waits for the answer whether to replace a file.
enum Then {
    /// Save the log there (Save As), then go on with what waited for the save.
    SaveAs(Option<After>),
    /// Export the log in the format of this index.
    Export(usize),
    /// Export the settings as an INI file.
    #[cfg(feature = "ini")]
    Ini,
}

impl Ran<'_> {
    /// Carry out the application's command `id` ([`crate::commands`]).
    fn core(&mut self, id: &str) {
        let session = &mut *self.session;
        let view = &mut *self.view;
        match id {
            "file.open" => *self.close = true,
            "file.save" => session.save(),
            "file.save_as" | "file.save_as_zlo" => {
                let format = if id == "file.save_as" {
                    zlog_core::Format::Zlox
                } else {
                    zlog_core::Format::Zlo
                };
                let name = session
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
                let picked = crate::dialog::choose_log_file(
                    name.as_deref().unwrap_or("contest.zlox"),
                    format,
                );
                match picked {
                    Some(to) if to.ask => {
                        *self.overwrite = Some(Overwrite {
                            path: to.path,
                            then: Then::SaveAs(None),
                        })
                    }
                    Some(to) => session.save_as(to.path),
                    None => {}
                }
            }
            "file.start_time" => {
                view.start_time = Some(main_window::StartTimeForm::new(session.start_time(), false))
            }
            "file.exit" => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "edit.next_band" => session.cycle_band(false),
            "edit.next_mode" => session.cycle_mode(false),
            "edit.next_power" => session.entry.cycle_power(),
            "edit.cq_sp" => session.entry.cq = !session.entry.cq,
            "edit.clear" => {
                session.entry.clear_call_and_number();
                session.status = Default::default();
                if let Some(e) = self.entry.as_deref_mut() {
                    e.focus(EntryField::Call);
                }
            }
            "device.connect" => *self.show_rig = true,
            "device.disconnect" => self.rig.disconnect(),
            "device.rig_window" => *self.show_rig = !*self.show_rig,
            "device.toggle_rig" => {
                if self.rig.toggle() {
                    session.status = rig_selected(self.rig.number());
                }
            }
            _ => {
                if let Some((dir, i)) = format_command(id) {
                    *self.overwrite = file_format(session, dir, i);
                }
            }
        }
    }
}

/// `file.import.<i>` or `file.export.<i>` as (import?, format index).
fn format_command(id: &str) -> Option<(bool, usize)> {
    let (dir, i) = id.strip_prefix("file.")?.split_once('.')?;
    let i = i.parse().ok()?;
    match dir {
        "import" => Some((true, i)),
        "export" => Some((false, i)),
        _ => None,
    }
}

/// Import into the log, or export it, in the format `i` of an extension; an
/// export to an existing file not asked about waits for the answer.
fn file_format(session: &mut Session, import: bool, i: usize) -> Option<Overwrite> {
    let f = session.formats().get(i)?;
    let name = f.name().to_string();
    let exts: Vec<String> = f.extensions().iter().map(|e| e.to_string()).collect();
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    if import {
        if let Some(p) = crate::dialog::import_file(&name, &exts) {
            session.import(i, &p);
        }
    } else if let Some(to) = crate::dialog::export_file(&name, &exts) {
        if to.ask {
            return Some(Overwrite {
                path: to.path,
                then: Then::Export(i),
            });
        }
        session.export(i, &to.path);
    }
    None
}

/// The items the panels add to the right-click menu of `q`, by menu group.
fn qso_items(panels: &[PanelSlot], q: &zlog_core::Qso) -> main_window::ExtensionItems {
    let lang = i18n::current();
    let items = panels.iter().enumerate().flat_map(|(i, s)| {
        let g = s.panel.menu();
        s.panel
            .qso_menu(lang, q)
            .into_iter()
            .map(move |it| (g, i, it))
    });
    crate::menus::grouped(items)
        .into_iter()
        .map(|(_, list)| list)
        .collect()
}

/// The dialogs open now, as (identifier, title), for the Window menu.
fn open_dialogs(
    options: &Options,
    show_rig: bool,
    panels: &[PanelSlot],
    show_about: bool,
) -> Vec<(String, String)> {
    let lang = i18n::current();
    let mut all = Vec::new();
    if options.open {
        all.push(("options".into(), tr("options.title").into()));
    }
    if show_rig {
        all.push(("rig".into(), tr("window.rig").into()));
    }
    for slot in panels {
        all.extend(slot.panel.dialog_windows(lang));
    }
    for slot in panels.iter().filter(|s| s.open) {
        all.push((slot.panel.id().to_string(), slot.panel.title(lang)));
    }
    if show_about {
        all.push(("about".into(), tr("menu.help.about").into()));
    }
    all
}

/// What waits for the log to be saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    /// Back to the zLog Menu (File > Open).
    CloseLog,
    Quit,
}

/// Saving failed before [`After`]: why, and what waits.
struct SaveFailed {
    error: String,
    after: After,
}

/// Frames between closing the windows and quitting (see
/// [`ZLogApp::quit_after_windows`]).
const QUIT_FRAMES: u8 = 3;

/// Contributors listed in the About dialog.
const CONTRIBUTORS: &[&str] = &["JR8PPG", "JS2GGD", "JG1VPP", "JS2FVO"];

/// Settings section of a panel.
fn panel_section(id: &str) -> String {
    format!("panel.{id}")
}

/// The status line after another rig was selected (`SwitchRig` writes
/// `RigName + ' active'`).
fn rig_selected(n: u8) -> Status {
    Status::info(i18n::trf("status.rig_active", &[&n]))
}

/// One line saying which rig is set up and whether it is connected; the
/// simulator is named as such.
fn rig_status(model: &str, port: &str, connected: bool, error: Option<String>) -> String {
    let rig = if port == crate::rig::SIMULATOR {
        tr("rig.simulator").to_string()
    } else {
        i18n::trf("menu.device.rig_on", &[&model, &port])
    };
    let state = match (connected, error) {
        (_, Some(e)) => i18n::trf("menu.device.error", &[&e]),
        (true, None) => tr("menu.device.connected").to_string(),
        (false, None) => tr("rig.not_connected").to_string(),
    };
    i18n::trf("menu.device.status", &[&rig, &state])
}

/// What the headless tests ([`crate::gui_tests`]) need to reach between clicks.
#[cfg(test)]
impl ZLogApp {
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Close every dialog and question, as by Cancel, keeping the log open.
    pub fn close_dialogs(&mut self) {
        self.view.start_time = None;
        self.options.open = false;
        self.show_rig = false;
        self.show_about = false;
        self.asking_old_other = None;
        if let Some(m) = &mut self.menu {
            m.close_windows();
        }
        for slot in &mut self.panels {
            slot.open = false;
            slot.panel.close_dialogs();
        }
    }

    /// Open a contest as OK in the zLog Menu does after its checks.
    pub fn open_setup(&mut self, setup: crate::session::Setup) {
        self.open(setup);
    }

    /// The open log: its contest name and number of QSOs.
    pub fn log_state(&self) -> Option<(String, usize)> {
        self.session
            .as_ref()
            .map(|s| (s.rules().name().to_string(), s.log().qsos.len()))
    }

    /// The status line and the entry row's callsign and number.
    pub fn entry_state(&self) -> Option<(String, String, String)> {
        self.session.as_ref().map(|s| {
            (
                s.status.text.clone(),
                s.entry.callsign.clone(),
                s.entry.nr_rcvd.clone(),
            )
        })
    }

    /// Whether the old-Other question is waiting for an answer.
    pub fn asking_old_other(&self) -> bool {
        self.asking_old_other.is_some()
    }

    /// The rig is connected (to the simulator, in tests).
    pub fn rig_connected(&self) -> bool {
        self.rig.connected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Entry;

    /// U8: Esc while the save-failed dialog's Save As asks whether to overwrite
    /// closed that dialog under the question; Yes then went on and quit.
    #[test]
    fn esc_on_the_overwrite_question_keeps_the_save_failed_dialog() {
        let dir = std::env::temp_dir().join(format!("zlog-gui-ow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = Some(dir.join("settings.toml")));
        let mut app = ZLogApp::new();
        app.save_failed = Some(SaveFailed {
            error: "disk full".into(),
            after: After::Quit,
        });
        app.overwrite = Some(Overwrite {
            path: dir.join("x.zlox"),
            then: Then::SaveAs(Some(After::Quit)),
        });
        let ctx = egui::Context::default();
        let esc = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        // The dialogs show first, then Esc is pressed.
        for events in [vec![], vec![esc]] {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.save_failed_dialog(ui.ctx());
                app.overwrite_dialog(ui.ctx());
            });
            out.textures_delta.clear();
        }
        assert!(app.overwrite.is_some(), "the question waits for its answer");
        assert!(
            app.save_failed.is_some(),
            "Esc did not close the dialog under it"
        );
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn picking_a_callsign_clears_the_number() {
        let mut e = Entry::new(
            zlog_core::Band::B7,
            zlog_core::Mode::Cw,
            zlog_core::Power::P010,
        );
        e.callsign = "JA1ZLO".into();
        e.nr_rcvd = "10H".into();
        e.rst_rcvd = "579".into();
        pick_callsign(&mut e, "JA1YAA".into());
        assert_eq!(e.callsign, "JA1YAA");
        assert_eq!(e.nr_rcvd, "");
        // Other fields stay.
        assert_eq!(e.rst_rcvd, "579");
        // An empty pick clears both.
        pick_callsign(&mut e, String::new());
        assert_eq!((e.callsign.as_str(), e.nr_rcvd.as_str()), ("", ""));
    }

    /// A panel that takes Esc (a CW keyer sending).
    struct Keyer(std::sync::Arc<std::sync::atomic::AtomicBool>);

    impl zlog_ui::Panel for Keyer {
        fn id(&self) -> &str {
            "keyer"
        }
        fn title(&self, _language: &str) -> String {
            "keyer".into()
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
        fn on_key(&mut self, key: zlog_ui::EntryKey, _view: &zlog_ui::View<'_>) -> bool {
            let escape = key == zlog_ui::EntryKey::Escape;
            self.0.store(escape, std::sync::atomic::Ordering::SeqCst);
            escape
        }
    }

    /// A voice keyer: Esc stops it, which asks for PTT off.
    #[derive(Default)]
    struct Voice(Vec<zlog_ui::EntryAction>);

    impl zlog_ui::Panel for Voice {
        fn id(&self) -> &str {
            "voice"
        }
        fn title(&self, _language: &str) -> String {
            "voice".into()
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
        fn on_key(&mut self, key: zlog_ui::EntryKey, _view: &zlog_ui::View<'_>) -> bool {
            let escape = key == zlog_ui::EntryKey::Escape;
            if escape {
                self.0.push(zlog_ui::EntryAction::Transmit(false));
            }
            escape
        }
        fn take_actions(&mut self) -> Vec<zlog_ui::EntryAction> {
            std::mem::take(&mut self.0)
        }
    }

    /// What drives PTT, and what it was asked.
    struct Ptt(std::sync::Arc<std::sync::Mutex<Vec<bool>>>);

    impl zlog_ui::Panel for Ptt {
        fn id(&self) -> &str {
            "ptt"
        }
        fn title(&self, _language: &str) -> String {
            "ptt".into()
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
        fn transmit(&mut self, on: bool) -> bool {
            self.0.lock().unwrap().push(on);
            true
        }
    }

    /// Ctrl+Z from the entry row: undoes when nothing is typed, leaves the text's
    /// own undo otherwise, and says why when Undo is disabled.
    #[test]
    fn ctrl_z_from_an_empty_entry_row_undoes() {
        let undo = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
        let delete = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete);
        let listed = |enabled: bool| {
            let mut u = zlog_ui::Command::new("undo", "Undo", "").row_key(undo);
            if !enabled {
                u = u.disabled_because("nothing to undo");
            }
            vec![commands::Listed {
                source: commands::Source::Core(zlog_ui::MenuGroup::Edit),
                id: "edit".into(),
                title: "Edit".into(),
                group: zlog_ui::MenuGroup::Edit,
                commands: vec![
                    u,
                    zlog_ui::Command::new("delete", "Delete", "").row_key(delete),
                ],
            }]
        };
        let ctx = egui::Context::default();
        let press = |k: egui::KeyboardShortcut, all: &[commands::Listed], empty: bool| {
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key: k.logical_key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: k.modifiers,
                }],
                ..Default::default()
            };
            let mut got = None;
            let mut out = ctx.run_ui(input, |ui| {
                got = row_command(ui.ctx(), all, false, empty, false);
            });
            out.textures_delta.clear();
            got.map(|r| r.map(|(_, id)| id))
        };
        assert_eq!(press(undo, &listed(true), true), Some(Ok("undo".into())));
        assert_eq!(press(undo, &listed(true), false), None, "text typed");
        assert_eq!(press(delete, &listed(true), true), None, "Delete types");
        assert_eq!(
            press(undo, &listed(false), true),
            Some(Err("nothing to undo".into()))
        );
        let mut e = Entry::new(
            zlog_core::Band::B7,
            zlog_core::Mode::Cw,
            zlog_core::Power::P010,
        );
        e.wipe();
        assert!(nothing_typed(&e));
        e.callsign = "JA1".into();
        assert!(!nothing_typed(&e));
    }

    #[test]
    fn rig_switches_and_keyer_port_bytes_go_first() {
        use zlog_ui::EntryAction as A;
        let order = device_first(vec![
            A::Transmit(true),
            A::SelectRig(2),
            A::Cq(true),
            A::KeyerPort(vec![1]),
        ]);
        assert!(matches!(
            order.as_slice(),
            [
                A::SelectRig(2),
                A::KeyerPort(_),
                A::Transmit(true),
                A::Cq(true)
            ]
        ));
    }

    /// Esc on close: the keyer port bytes a panel sends are carried out too.
    #[test]
    fn closing_forwards_keyer_port_bytes() {
        struct Box2(Vec<zlog_ui::EntryAction>);
        impl zlog_ui::Panel for Box2 {
            fn id(&self) -> &str {
                "so2r"
            }
            fn title(&self, _language: &str) -> String {
                "so2r".into()
            }
            fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
            fn on_key(&mut self, key: zlog_ui::EntryKey, _view: &zlog_ui::View<'_>) -> bool {
                self.0.push(zlog_ui::EntryAction::KeyerPort(vec![7]));
                key == zlog_ui::EntryKey::Escape
            }
            fn take_actions(&mut self) -> Vec<zlog_ui::EntryAction> {
                std::mem::take(&mut self.0)
            }
        }
        struct Port(std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>);
        impl zlog_ui::Panel for Port {
            fn id(&self) -> &str {
                "port"
            }
            fn title(&self, _language: &str) -> String {
                "port".into()
            }
            fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
            fn keyer_port(&mut self, bytes: &[u8]) -> bool {
                self.0.lock().unwrap().push(bytes.to_vec());
                true
            }
        }
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let slot = |panel: Box<dyn zlog_ui::Panel>| PanelSlot {
            panel,
            open: false,
            was_open: false,
        };
        let mut panels = vec![
            slot(Box::new(Box2(Vec::new()))),
            slot(Box::new(Port(sent.clone()))),
        ];
        let dir = std::env::temp_dir().join(format!("zlog-gui-port-{}", std::process::id()));
        let session = open_in(&dir);
        let view = panel_view(None, None, None, &session, &[], &Rigs::default());
        stop_transmitting(&mut panels, &view);
        assert_eq!(*sent.lock().unwrap(), vec![vec![7u8]]);
        drop(session);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn closing_the_log_turns_ptt_off_at_once() {
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut panels = vec![
            PanelSlot {
                panel: Box::new(Voice::default()),
                open: false,
                was_open: false,
            },
            PanelSlot {
                panel: Box::new(Ptt(asked.clone())),
                open: false,
                was_open: false,
            },
        ];
        let dir = std::env::temp_dir().join(format!("zlog-gui-ptt-{}", std::process::id()));
        let session = open_in(&dir);
        let view = panel_view(None, None, None, &session, &[], &Rigs::default());
        stop_transmitting(&mut panels, &view);
        assert_eq!(*asked.lock().unwrap(), [false]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A session of ALL JA in a new log in `dir`.
    fn open_in(dir: &std::path::Path) -> Session {
        std::fs::create_dir_all(dir).unwrap();
        let reg = zlog_core::Registry::new()
            .with(zlog_cfg::Cfg)
            .with(zlog_rule_ja::Japan);
        let setup = crate::session::Setup {
            contest: 0,
            contest_file: None,
            mode: zlog_core::ContestMode::Mix,
            station: zlog_core::Station::default(),
            coeff: 1.0,
            path: dir.join("log.zlox"),
            use_contest_period: true,
            convert_old_other: false,
        };
        Session::open(setup, reg).unwrap()
    }

    #[test]
    fn escape_aborts_cw_before_the_grid_sees_it() {
        let dir = std::env::temp_dir().join(format!("zlog-gui-esc-{}", std::process::id()));
        let mut session = open_in(&dir);
        let mut view = View {
            selected: Some(0),
            ..View::default()
        };
        let aborted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut panels = vec![PanelSlot {
            panel: Box::new(Keyer(aborted.clone())),
            open: false,
            was_open: false,
        }];
        let options = Options::new(Vec::new());
        let rig = Rigs::default();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            route_keys(
                ui.ctx(),
                &mut session,
                &mut view,
                None,
                &mut panels,
                &options,
                &[],
                &rig,
            );
        });
        out.textures_delta.clear();
        assert!(
            aborted.load(std::sync::atomic::Ordering::SeqCst),
            "CW aborted"
        );
        assert_eq!(view.selected, Some(0), "the grid did not get Esc");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Regression G5: with a grid row selected the entry row's shortcuts were dead;
    /// zLog's are the form's. Alt+C goes back to the entry row.
    #[cfg(feature = "entry")]
    #[test]
    fn entry_shortcuts_work_with_a_grid_row_selected() {
        let dir = std::env::temp_dir().join(format!("zlog-gui-shift-{}", std::process::id()));
        let mut session = open_in(&dir);
        // The grid's keys run with a row selected, whether it holds QSOs or not.
        let mut view = View {
            selected: Some(0),
            ..View::default()
        };
        let mut style = crate::registry::entry_style("zlog").unwrap();
        let options = Options::new(Vec::new());
        let rig = Rigs::default();
        let ctx = egui::Context::default();
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let mut press = |e: egui::Event, session: &mut Session, view: &mut View| {
            let input = egui::RawInput {
                events: vec![e],
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                route_keys(
                    ui.ctx(),
                    session,
                    view,
                    Some(style.as_mut()),
                    &mut [],
                    &options,
                    &[],
                    &rig,
                );
            });
            out.textures_delta.clear();
        };
        let band = session.entry.band;
        press(
            key(egui::Key::B, egui::Modifiers::SHIFT),
            &mut session,
            &mut view,
        );
        assert_ne!(session.entry.band, band, "Shift+B changed the band");
        assert_eq!(view.selected, Some(0));
        press(
            key(egui::Key::C, egui::Modifiers::ALT),
            &mut session,
            &mut view,
        );
        assert_eq!(view.selected, None, "Alt+C left the grid");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A panel adding one QSO menu item in its group.
    struct Adds(zlog_ui::MenuGroup, &'static str);

    impl zlog_ui::Panel for Adds {
        fn id(&self) -> &str {
            self.1
        }
        fn title(&self, _language: &str) -> String {
            self.1.into()
        }
        fn menu(&self) -> zlog_ui::MenuGroup {
            self.0
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {}
        fn qso_menu(&self, _language: &str, qso: &zlog_core::Qso) -> Vec<zlog_ui::QsoMenuItem> {
            vec![zlog_ui::QsoMenuItem::new(self.1, &qso.callsign)]
        }
    }

    #[test]
    fn qso_menu_items_are_grouped_by_menu_group() {
        use zlog_ui::MenuGroup as G;
        let slot = |g, id| PanelSlot {
            panel: Box::new(Adds(g, id)),
            open: false,
            was_open: false,
        };
        let panels = [
            slot(G::Network, "cluster"),
            slot(G::Search, "search"),
            slot(G::Network, "soumu"),
        ];
        let q = zlog_core::Qso {
            callsign: "JA1ZLO".into(),
            ..Default::default()
        };
        let items = qso_items(&panels, &q);
        let ids: Vec<Vec<(usize, String)>> = items
            .iter()
            .map(|g| g.iter().map(|(i, it)| (*i, it.id.clone())).collect())
            .collect();
        assert_eq!(
            ids,
            [
                vec![(1, "search".to_string())],
                vec![(0, "cluster".to_string()), (2, "soumu".to_string())],
            ]
        );
        assert_eq!(items[0][0].1.label, "JA1ZLO");
    }

    #[test]
    fn rig_status_names_the_simulator_and_the_state() {
        crate::i18n::set_language("en");
        let sim = rig_status("IC-7300", crate::rig::SIMULATOR, true, None);
        assert_eq!(sim, "Rig: Simulator — connected");
        let off = rig_status("IC-7300", "/dev/cu.SLAB", false, None);
        assert_eq!(off, "Rig: IC-7300 on /dev/cu.SLAB — Not connected");
        let err = rig_status("IC-7300", "COM3", false, Some("busy".into()));
        assert_eq!(err, "Rig: IC-7300 on COM3 — error: busy");
    }
}
