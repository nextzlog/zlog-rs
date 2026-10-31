pub mod builder;
pub mod chrome;
pub mod command;
pub mod connection;
pub mod device;
pub mod dialog;
pub mod enter;
pub mod entry;
pub mod file;
pub mod fkeys;
pub mod form;
pub mod icon;
pub mod keys;
pub mod layout;
pub mod loopback;
pub mod migration;
pub mod pairing;
pub mod palette;
pub mod product;
pub mod queue;
pub mod rig;
pub mod section;
pub mod sequence;
pub mod spot;
pub mod start;
pub mod suggest;
pub mod time;
pub mod toolbar;
pub mod tx;

pub use command::{panel_commands, role_commands, Command, Commands, PanelCommands, RoleCommand};
pub use entry::{
    EntryCaps, EntryEnv, EntryFields, EntryOps, EntryState, EntryStyle, LogOptions, NotLogged,
    Period,
};
pub use file::{SettingsEnv, SettingsFile};
pub use form::{form, Form};
pub use section::MenuSection;
pub use sequence::{Cursor, EnterFlow, EntryStatus};
pub use start::{
    contest_groups, read_station, write_station, ChosenContest, ContestEntry, EntryView, Sections,
    Setup, StartScreen, StartView,
};
pub use suggest::{Agreement, Suggestion};
pub use time::{entry_time, parse_time};

pub use builder::{Builder, HostCtx};

zlog_core::texts!();

use zlog_core::{Band, Batch, Contest, Log, Mode, Power, Qso, QsoId, ScoreSheet, Station};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum MenuGroup {
    Search,
    Device,
    Network,
    Report,
    #[default]
    Tools,
    File,
    Edit,
    Help,
    Settings,
    Window,
}

impl MenuGroup {
    pub fn is_app_menu(self) -> bool {
        matches!(
            self,
            MenuGroup::Edit | MenuGroup::Help | MenuGroup::Settings | MenuGroup::Window
        )
    }
}

impl MenuGroup {
    pub const ALL: [MenuGroup; 5] = [
        MenuGroup::Search,
        MenuGroup::Report,
        MenuGroup::Device,
        MenuGroup::Network,
        MenuGroup::Tools,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QsoMenuItem {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub dialog: bool,
}

impl QsoMenuItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        QsoMenuItem {
            id: id.into(),
            label: label.into(),
            enabled: true,
            dialog: false,
        }
    }

    pub fn dialog(mut self) -> Self {
        self.dialog = true;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusItem {
    pub text: String,
    pub tooltip: Option<String>,
    pub warning: bool,
}

impl StatusItem {
    pub fn new(text: impl Into<String>) -> Self {
        StatusItem {
            text: text.into(),
            ..Default::default()
        }
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn warning(mut self, warning: bool) -> Self {
        self.warning = warning;
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spot {
    pub callsign: String,
    pub hz: u64,
    pub mode: Option<Mode>,
    pub spotter: String,
    pub comment: String,
    pub time: i64,
    pub number: String,
    pub source: String,
}

pub struct View<'a> {
    pub log: &'a Log,
    pub contest: &'a dyn Contest,
    pub sheet: &'a ScoreSheet,
    pub callsign: &'a str,
    pub station: &'a Station,
    pub number: &'a str,
    pub memo: &'a str,
    pub rst_sent: &'a str,
    pub power: Power,
    pub band: Band,
    pub mode: Mode,
    pub language: &'a str,
    pub spots: &'a [Spot],
    pub hz: Option<u64>,
    pub rig: u8,
    pub cq: bool,
    pub operator: &'a str,
    pub period_hours: u32,
    pub pc_name: &'a str,
    pub log_path: &'a str,
    pub focus: Option<EntryField>,
    pub editing: Option<&'a Qso>,
    pub selected: Option<&'a Qso>,
    pub period: &'a dyn PeriodCheck,
    pub tab: TabState,
    pub changes: u64,
}

impl View<'_> {
    pub fn out_of_period(&self, q: &Qso) -> bool {
        self.period.outside(q)
    }
}

pub trait PeriodCheck {
    fn outside(&self, q: &Qso) -> bool;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TabState {
    #[default]
    Normal,
    Sent(String),
}

impl TabState {
    pub fn edited(&self, callsign: &str) -> bool {
        matches!(self, TabState::Sent(c) if c != callsign.trim())
    }
}

impl PeriodCheck for () {
    fn outside(&self, _q: &Qso) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKey {
    Function { n: u8, shift: bool },
    Reserved { n: u8 },
    Escape,
    Typed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryField {
    Call,
    RstSent,
    RstRcvd,
    Number,
    Memo,
    Time,
    Date,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowKind {
    #[default]
    Tool,
    Dialog,
    None,
}

impl WindowKind {
    pub fn has_window(self) -> bool {
        self != WindowKind::None
    }
}

pub trait ButtonRow {
    fn buttons(&mut self, ui: &mut egui::Ui, view: &View<'_>) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want<'l> {
    Rig,
    MenuNote(&'l str),
    Connections(&'l str),
}

pub enum Capability<'a> {
    Rig(&'a dyn rig::RigControl),
    MenuNote(String),
    Connections(Vec<connection::Connection>),
}

pub fn connections_of(panel: &dyn Panel, language: &str) -> Vec<connection::Connection> {
    match panel.capability(Want::Connections(language)) {
        Some(Capability::Connections(c)) => c,
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WantMut {
    Rig,
    Buttons,
}

pub enum CapabilityMut<'a> {
    Rig(&'a mut dyn rig::RigControl),
    Buttons(&'a mut dyn ButtonRow),
}

pub fn rig_of(panel: &dyn Panel) -> Option<&dyn rig::RigControl> {
    match panel.capability(Want::Rig) {
        Some(Capability::Rig(r)) => Some(r),
        _ => None,
    }
}

pub fn rig_of_mut(panel: &mut dyn Panel) -> Option<&mut dyn rig::RigControl> {
    match panel.capability_mut(WantMut::Rig) {
        Some(CapabilityMut::Rig(r)) => Some(r),
        _ => None,
    }
}

pub fn menu_note_of(panel: &dyn Panel, language: &str) -> Option<String> {
    match panel.capability(Want::MenuNote(language)) {
        Some(Capability::MenuNote(n)) => Some(n),
        _ => None,
    }
}

pub fn buttons_of(panel: &mut dyn Panel) -> Option<&mut dyn ButtonRow> {
    match panel.capability_mut(WantMut::Buttons) {
        Some(CapabilityMut::Buttons(b)) => Some(b),
        _ => None,
    }
}

pub trait Panel {
    fn id(&self) -> &str;

    fn title(&self, language: &str) -> String;

    fn default_size(&self) -> [f32; 2] {
        [400.0, 300.0]
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Tools
    }

    fn commands(&self, _language: &str) -> Vec<Command> {
        Vec::new()
    }

    fn toolbar_default_shown(&self) -> bool {
        true
    }

    fn section(&self) -> (MenuSection, i32) {
        (MenuSection::Other, 0)
    }

    fn run_command(&mut self, _id: &str) {}

    fn qso_menu(&self, _language: &str, _qso: &Qso) -> Vec<QsoMenuItem> {
        Vec::new()
    }

    fn on_qso_menu(&mut self, _id: &str, _qso: &Qso) -> bool {
        false
    }

    fn window_kind(&self) -> WindowKind {
        WindowKind::Tool
    }

    fn status(&self, _view: &View<'_>) -> Vec<StatusItem> {
        Vec::new()
    }

    fn suggestions(&self, _view: &View<'_>) -> Vec<Suggestion> {
        Vec::new()
    }

    fn ui(&mut self, _ui: &mut egui::Ui, _view: &View<'_>) {}

    fn opened(&mut self, _view: &View<'_>) {}

    fn take_actions(&mut self) -> Vec<EntryAction> {
        Vec::new()
    }

    fn tick(&mut self, _view: &View<'_>) {}

    fn tick_after(&self) -> Option<std::time::Duration> {
        None
    }

    fn on_key(&mut self, _key: EntryKey, _view: &View<'_>) -> bool {
        false
    }

    fn key_messages(&self, _view: &View<'_>) -> Option<Vec<String>> {
        None
    }

    fn transmit(&mut self, _on: bool) -> bool {
        false
    }

    fn role_commands(&mut self, _commands: &[RoleCommand]) {}

    fn dialogs(&mut self, _ctx: &egui::Context, _view: &View<'_>) {}

    fn modal(&self) -> bool {
        false
    }

    fn dialog_windows(&self, _language: &str) -> Vec<(String, String)> {
        Vec::new()
    }

    fn close_dialogs(&mut self) {}

    fn quit_cancelled(&mut self) -> bool {
        false
    }

    fn on_exit(&mut self) {}

    fn editing(&self) -> Option<QsoId> {
        None
    }

    fn load(&mut self, _section: &Section) {}

    fn reads(&self) -> Vec<String> {
        Vec::new()
    }

    fn load_with(&mut self, section: &Section, _others: &Sections) {
        self.load(section);
    }

    fn save(&self, _section: &mut Section) {}

    fn section_kind(&self) -> SectionKind {
        SectionKind::Personal
    }

    fn migrations(&self) -> Vec<migration::Migration> {
        Vec::new()
    }

    fn capability(&self, _want: Want<'_>) -> Option<Capability<'_>> {
        None
    }

    fn capability_mut(&mut self, _want: WantMut) -> Option<CapabilityMut<'_>> {
        None
    }
}

pub fn form_field_width(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let label = labels
        .iter()
        .map(|t| {
            egui::WidgetText::from(*t)
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max);
    let label = form::shared_label(ui, label);
    let room = form::shared_room(ui).unwrap_or_else(|| ui.available_width());
    (room - label - 3.0 * ui.spacing().item_spacing.x).max(FIELD_MIN.min(room * 0.5))
}

pub const FIELD_MIN: f32 = 160.0;

pub fn sized_field(ui: &mut egui::Ui, edit: egui::TextEdit<'_>, width: f32) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    ui.add_sized([width, height], edit)
}

pub const APP_DIR: &str = "zlog";

pub fn field_row<R>(
    ui: &mut egui::Ui,
    leading: impl FnOnce(&mut egui::Ui),
    trailing: impl FnOnce(&mut egui::Ui),
    field: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        leading(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            trailing(ui);
            field(ui)
        })
        .inner
    })
    .inner
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListedContest {
    pub name: String,
    pub date: String,
    pub rules: Option<String>,
    pub files: Vec<ListedFile>,
    pub note: String,
    pub updated: Option<Date>,
    pub changes: Vec<Change>,
    pub doc: String,
    pub experimental: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Change {
    pub date: Option<Date>,
    pub text: String,
}

pub type Date = (i32, u32, u32);

pub const LIST_STALE_DAYS: i64 = 30;

pub fn days_between(from: Date, to: Date) -> i64 {
    let days = |(y, m, d): Date| zlog_core::qso::days_from_civil(y, m, d);
    days(to) - days(from)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListedFile {
    pub name: String,
    pub url: String,
    pub note: String,
}

pub trait ContestList {
    fn id(&self) -> &str;

    fn title(&self, language: &str) -> String;

    fn page(&self) -> &str;

    fn changes(&self) -> &[Change] {
        &[]
    }

    fn history_page(&self) -> Option<&str> {
        None
    }

    fn refresh(&mut self);

    fn pending(&self) -> bool {
        false
    }

    fn contests(&mut self) -> Option<Result<&[ListedContest], String>>;

    fn last_checked(&self) -> Option<Date> {
        None
    }

    fn stale(&self, today: Date) -> bool {
        self.last_checked()
            .is_none_or(|d| days_between(d, today) > LIST_STALE_DAYS)
    }

    fn outdated(
        &self,
        _folder: &std::path::Path,
        _contest: &ListedContest,
        _file: &ListedFile,
        _changed: Date,
    ) -> bool {
        false
    }

    fn verify(&self, _file: &ListedFile, _bytes: &[u8], _language: &str) -> Result<(), String> {
        Ok(())
    }
}

pub type Section = std::collections::BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum SectionKind {
    #[default]
    Custom,
    Personal,
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionInfo {
    pub name: String,
    pub title: String,
    pub kind: SectionKind,
}

pub trait SettingsPage {
    fn id(&self) -> &str;

    fn title(&self, language: &str) -> String;

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, language: &str);

    fn ui_with(
        &mut self,
        ui: &mut egui::Ui,
        section: &mut Section,
        _others: &Sections,
        language: &str,
    ) {
        self.ui(ui, section, language);
    }

    fn shortcuts(&self) -> Option<&dyn ShortcutPage> {
        None
    }

    fn shortcuts_mut(&mut self) -> Option<&mut dyn ShortcutPage> {
        None
    }

    fn section_kind(&self) -> SectionKind {
        SectionKind::Custom
    }
}

pub trait ShortcutPage {
    fn set_commands(&mut self, commands: &[keys::CommandInfo], reserved: &[egui::KeyboardShortcut]);

    fn overrides(&self, section: &Section) -> keys::CommandOverrides;

    fn wants_keys(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryAction {
    Callsign(String),
    TakeCallsign(String),
    Number(String),
    RstRcvd(u16),
    Qsy(u64, Option<Mode>),
    Cq(bool),
    SelectQso(QsoId),
    Edit { batch: Batch, done: Option<String> },
    Say(StatusItem),
    Transmit(bool),
    SelectRig(u8),
    Operator(String),
    Power(Power),
    Log(Box<Qso>),
    LogEntry,
    Run(String),
    Spot(Spot),
    Focus(EntryField),
    StopSending,
    Options(String),
    UseRig(u8, String),
}

impl EntryAction {
    pub fn update_qsos(list: Vec<(QsoId, Qso)>) -> Self {
        let commands = list
            .into_iter()
            .map(|(id, q)| zlog_core::Command::Update(id, q))
            .collect();
        EntryAction::Edit {
            batch: Batch {
                commands,
                ..Batch::default()
            },
            done: None,
        }
    }
}
