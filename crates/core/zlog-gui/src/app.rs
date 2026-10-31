pub(crate) mod chrome;
pub mod files;
pub(crate) mod frame;
pub mod keys;
pub mod panels;

use crate::commands::{self, CoreCommands, CoreState, Source};
use crate::dialog::dialog;
use crate::i18n::{self, tr};
use crate::main_window::{self, View};
use crate::options::Options;
use crate::registry::{apply_style, entry_style, panels, registry, settings_pages, start_screen};
use crate::session::{Session, Status};
use crate::settings::Settings;
use zlog_ui::{Commands, EntryField, EntryStyle};

use chrome::{command_of_role, edit_sections, options_commands};
use files::*;
use keys::*;
use panels::*;

pub struct ZLogApp {
    session: Option<Session>,
    toolbar_rows: zlog_ui::toolbar::Rows,
    shortcuts: zlog_ui::keys::CommandOverrides,
    panels: Vec<PanelSlot>,
    status_items: Vec<zlog_ui::StatusItem>,
    menu: Option<Box<dyn zlog_ui::StartScreen>>,
    view: View,
    entry: Option<Box<dyn EntryStyle>>,
    show_about: bool,
    menu_bar: Option<Box<dyn zlog_ui::chrome::MenuBar>>,
    tool_bar: Option<Box<dyn zlog_ui::chrome::ToolBar>>,
    status_bar: Option<Box<dyn zlog_ui::chrome::StatusBar>>,
    log_table: Option<Box<dyn zlog_ui::chrome::LogTable>>,
    window_host: Option<Box<dyn zlog_ui::chrome::WindowHost>>,
    places: Places,
    files: Vec<Box<dyn zlog_ui::SettingsFile>>,
    settings: Settings,
    options: Options,
    rig_number: u8,
    spots: Vec<zlog_ui::Spot>,
    panels_checked: std::time::Instant,
    started: std::time::Instant,
    listed: Vec<String>,
    asking_old_other: Option<crate::session::Setup>,
    quitting: Option<u8>,
    layout_kept: bool,
    pub save_failed: Option<SaveFailed>,
    leave_unsaved: bool,
    pub overwrite: Option<Overwrite>,
    run_role: Option<String>,
    restyle: bool,
    exited: bool,
    entry_focus: EntryFocus,
    window_title: String,
    reopen: Option<crate::session::Setup>,
    connection_actions: Vec<zlog_ui::EntryAction>,
    rig_test: Option<std::time::Instant>,
}

const PANEL_SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

const SPOT_LIFETIME: i64 = 30 * 60;

const MANUAL_URL: &str = "https://use.zlog.org";

impl Default for ZLogApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ZLogApp {
    fn quit_after_windows(&mut self, ctx: &egui::Context) {
        let asked = ctx.input(|i| i.viewport().close_requested());
        if asked && !self.saved_before(After::Quit) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.quitting = None;
            self.layout_kept = false;
            return;
        }
        if asked && self.quitting.is_none() {
            let open = !open_dialogs(&self.options, &self.panels, self.show_about).is_empty()
                || self.menu.as_ref().is_some_and(|m| m.has_window());
            if open {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.save_panels();
                self.layout_kept = true;
                self.options.open = false;
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
            None if asked => self.finish(),
            None => {}
        }
    }

    pub(super) fn quit_cancelled(&mut self) {
        self.layout_kept = false;
        for slot in &mut self.panels {
            if slot.panel.quit_cancelled() {
                slot.open = true;
            }
        }
    }

    fn finish(&mut self) {
        self.close();
        self.exit_panels();
    }

    fn exit_panels(&mut self) {
        if self.exited {
            return;
        }
        self.save_panels();
        self.exited = true;
        zlog_process::shut_down();
        for slot in &mut self.panels {
            slot.panel.on_exit();
        }
    }

    pub fn new() -> Self {
        let mut settings = Settings::load();
        if let Some(code) = settings.language() {
            i18n::set_language(&code);
        }
        let mut menu = start_screen();
        load_start_screen(&mut menu, &mut settings);
        let toolbar_rows = zlog_ui::toolbar::Rows::default();
        let options = Options::new(settings_pages(toolbar_rows.clone()));
        let mut panels: Vec<PanelSlot> = panels()
            .into_iter()
            .map(|mut panel| {
                load_panel(panel.as_mut(), &mut settings);
                PanelSlot {
                    panel,
                    open: false,
                    was_open: false,
                    title: String::new(),
                }
            })
            .collect();
        load_rig_sections(&mut panels, &mut settings, &[]);
        let mut window_host = crate::registry::window_host();
        if let Some(h) = window_host.as_mut() {
            h.load(&settings.section(h.section()).clone());
        }
        ZLogApp {
            session: None,
            panels,
            menu,
            view: View::default(),
            entry: loaded_style(&mut settings),
            show_about: false,
            files: crate::registry::settings_files(),
            menu_bar: crate::registry::menu_bar(),
            tool_bar: crate::registry::tool_bar(),
            status_bar: crate::registry::status_bar(),
            log_table: crate::registry::log_table(),
            window_host,
            places: Places::default(),
            shortcuts: options.shortcuts(&mut settings),
            options,
            toolbar_rows,
            settings,
            rig_number: 1,
            spots: Vec::new(),
            panels_checked: std::time::Instant::now(),
            started: std::time::Instant::now(),
            listed: Vec::new(),
            status_items: Vec::new(),
            asking_old_other: None,
            quitting: None,
            layout_kept: false,
            save_failed: None,
            leave_unsaved: false,
            overwrite: None,
            run_role: None,
            restyle: true,
            exited: false,
            entry_focus: EntryFocus::default(),
            window_title: zlog_ui::product::NAME.to_string(),
            reopen: None,
            connection_actions: Vec::new(),
            rig_test: None,
        }
    }

    fn open(&mut self, setup: crate::session::Setup) {
        match Session::open(setup, registry()) {
            Ok(mut s) => {
                s.apply_preferences(&self.settings.preferences());
                if s.asks_start_time {
                    self.view.start_time =
                        Some(zlog_ui::time::StartTimeForm::new(s.start_time(), true));
                }
                self.session = Some(s);
                self.reopen = None;
                restore_windows(
                    &mut self.panels,
                    self.window_host.as_deref(),
                    &mut self.places,
                    &mut self.settings,
                );
                if let Some(e) = &mut self.entry {
                    e.focus(EntryField::Call);
                }
                self.entry_focus.want(EntryField::Call);
            }
            Err(e) => {
                if let Some(m) = &mut self.menu {
                    m.show_error(e);
                }
            }
        }
    }

    fn dialog_shown(&self) -> bool {
        self.view.start_time.is_some()
            || self.overwrite.is_some()
            || self.save_failed.is_some()
            || self.quitting.is_some()
            || self.files.iter().any(|f| f.modal())
            || modal(&self.panels)
            || self
                .panels
                .iter()
                .any(|s| !s.panel.dialog_windows("en").is_empty())
            || self.options.open
            || self.show_about
            || self.menu.as_ref().is_some_and(|m| m.has_window())
    }

    fn save_panels(&mut self) {
        let style = style_section(self.entry.as_deref(), &mut self.settings);
        let hosted = host_section(self.window_host.as_deref(), &mut self.settings);
        let windows = self.session.is_some()
            && !self.layout_kept
            && windows_section(
                &self.panels,
                self.window_host.as_deref(),
                &self.places,
                &mut self.settings,
            );
        if panel_sections(&self.panels, &mut self.settings) | style | hosted | windows {
            let _ = self.settings.save();
        }
        self.panels_checked = std::time::Instant::now();
    }

    pub(super) fn close_log(&mut self) {
        self.reopen = self.session.as_ref().map(Session::reopen_setup);
        self.close();
    }

    fn close(&mut self) {
        self.save_panels();
        if let Some(mut s) = self.session.take() {
            let rig = rig_now(&self.panels, self.rig_number);
            let view = panel_view(None, None, None, &s, &self.spots, rig);
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

fn style_section(style: Option<&dyn EntryStyle>, settings: &mut Settings) -> bool {
    let Some(style) = style else {
        return false;
    };
    let section = settings.section(zlog_ui::enter::SECTION);
    let mut fresh = section.clone();
    style.save(&mut fresh);
    let changed = fresh != *section;
    *section = fresh;
    changed
}

fn host_section(host: Option<&dyn zlog_ui::chrome::WindowHost>, settings: &mut Settings) -> bool {
    let Some(host) = host else {
        return false;
    };
    let section = settings.section(host.section());
    let mut fresh = section.clone();
    host.save(&mut fresh);
    let changed = fresh != *section;
    *section = fresh;
    changed
}

fn load_start_screen(menu: &mut Option<Box<dyn zlog_ui::StartScreen>>, settings: &mut Settings) {
    if let Some(m) = menu {
        m.load(&settings.sections_named(m.sections()));
    }
}

fn choose_language(ctx: &egui::Context, settings: &mut Settings, code: &str) {
    i18n::set_language(code);
    crate::fonts::install(ctx);
    settings.set_language(code);
    let _ = settings.save();
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
        let Some(mut session) = self.session.take() else {
            self.start_frame(&ctx, ui);
            return;
        };
        let close = self.log_frame(&ctx, ui, &mut session);
        self.session = Some(session);
        let blocked = self.dialog_shown();
        let selected = self.view.selected.is_some();
        self.entry_focus
            .after_frame(&ctx, self.entry.as_deref_mut(), blocked, selected);
        if close && self.saved_before(After::CloseLog) {
            self.close_log();
        }
    }

    fn on_exit(&mut self) {
        self.save_panels();
        self.finish();
    }
}

const CONTRIBUTORS: &[&str] = &["JR8PPG", "JS2GGD", "JG1VPP", "JS2FVO"];

fn loaded_style(settings: &mut Settings) -> Option<Box<dyn EntryStyle>> {
    let mut style = entry_style(&settings.entry_style())?;
    style.load(settings.section(zlog_ui::enter::SECTION));
    Some(style)
}

#[cfg(any(test, feature = "testing"))]
impl ZLogApp {
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn push_panel(&mut self, panel: Box<dyn zlog_ui::Panel>) {
        self.panels.push(PanelSlot {
            panel,
            open: false,
            was_open: false,
            title: String::new(),
        });
    }

    pub fn open_panel(&mut self, id: &str) {
        for slot in self.panels.iter_mut().filter(|s| s.panel.id() == id) {
            slot.open = true;
        }
    }

    pub fn push_settings_file(&mut self, file: Box<dyn zlog_ui::SettingsFile>) {
        self.files.push(file);
    }

    pub fn run_settings_file(&mut self, ctx: &egui::Context, command: Option<&str>) {
        let Some(mut s) = self.session.take() else {
            return;
        };
        let pick = command.map(|c| (self.files.len() - 1, c.to_string()));
        self.settings_files(ctx, &mut s, "en", pick);
        self.session = Some(s);
    }

    pub fn reload_host(&mut self) {
        if let Some(h) = self.window_host.as_mut() {
            h.load(&self.settings.section(h.section()).clone());
        }
    }

    pub fn save_host(&mut self) -> bool {
        host_section(self.window_host.as_deref(), &mut self.settings)
    }

    pub fn selected_qso(&self) -> Option<usize> {
        self.view.selected
    }

    pub fn select_qso(&mut self, row: Option<usize>) {
        self.view.selected = row;
    }

    pub fn entry_band(&self) -> Option<String> {
        self.session.as_ref().map(|s| format!("{:?}", s.entry.band))
    }

    pub fn exit(&mut self) {
        eframe::App::on_exit(self);
    }

    pub fn make_panel_save_due(&mut self) {
        if let Some(t) = self.panels_checked.checked_sub(PANEL_SAVE_EVERY) {
            self.panels_checked = t;
        }
    }

    pub fn fail_save(&mut self, fail: bool) {
        self.save_failed = fail.then(|| SaveFailed {
            error: "test".into(),
            after: After::Quit,
        });
    }

    pub fn close_dialogs(&mut self) {
        self.view.start_time = None;
        self.options.open = false;
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

    pub fn open_setup(&mut self, setup: crate::session::Setup) {
        self.open(setup);
    }

    pub fn open_setup_with(
        &mut self,
        setup: crate::session::Setup,
        extension: impl zlog_core::Extension,
    ) {
        if let Ok(s) = Session::open(setup, registry().with(extension)) {
            self.session = Some(s);
        }
    }

    pub fn log_path(&self) -> Option<&std::path::Path> {
        self.session.as_ref().map(|s| s.path.as_path())
    }

    pub fn log_state(&self) -> Option<(String, usize)> {
        self.session
            .as_ref()
            .map(|s| (s.rules().name().to_string(), s.log().qsos.len()))
    }

    pub fn entry_state(&self) -> Option<(String, String, String)> {
        self.session.as_ref().map(|s| {
            (
                s.status.text.clone(),
                s.entry.callsign.clone(),
                s.entry.nr_rcvd.clone(),
            )
        })
    }

    pub fn asking_old_other(&self) -> bool {
        self.asking_old_other.is_some()
    }

    pub fn listed_commands(&self) -> &[String] {
        &self.listed
    }

    pub fn rig_connected(&self) -> bool {
        rig_of(&self.panels).is_some_and(|r| r.connected())
    }

    pub fn entry_field(&self, ctx: &egui::Context) -> Option<EntryField> {
        focus(ctx, self.entry.as_deref())
    }

    pub fn focus_entry(&mut self, field: EntryField) {
        if let Some(e) = self.entry.as_deref_mut() {
            e.focus(field);
        }
    }

    pub fn entry(&self) -> Option<crate::entry::Entry> {
        self.session.as_ref().map(|s| s.entry.clone())
    }

    pub fn set_entry(&mut self, entry: crate::entry::Entry) {
        if let Some(s) = &mut self.session {
            s.entry = entry;
            s.status = Default::default();
        }
    }

    pub fn status_text(&self) -> Option<String> {
        self.session.as_ref().map(|s| s.status.text.clone())
    }
}
