//! What an extension may add to the zLog window.
//!
//! A panel is a window of its own, listed in the menu of its [`MenuGroup`]. The application
//! draws it every frame with a read-only [`View`] of the open contest; a panel keeps
//! whatever state it likes but never sees the application's internals.
//!
//! Panels that only show the log (a map, a rate graph) should derive what they show
//! from [`View::log`] each time, rather than from events, so they are right from the
//! moment a file is opened and after every edit or deletion.
//!
//! [`dialog`] draws every window the same way; [`start`] is the screen shown until
//! a log is open, and the entries of its contest menu; [`entry`] is where the
//! operator types the QSO being made.

pub mod command;
pub mod dialog;
pub mod entry;
pub mod icon;
pub mod keys;
pub mod palette;
pub mod queue;
pub mod section;
pub mod start;
pub mod time;

pub use command::{panel_commands, Command, Commands, PanelCommands};
pub use entry::{EntryState, EntryStyle, NotLogged};
pub use section::MenuSection;
pub use start::{
    contest_groups, read_station, write_station, ChosenContest, ContestEntry, EntryView, Sections,
    Setup, StartScreen, StartView,
};
pub use time::parse_time;

zlog_core::texts!();

use zlog_core::{Band, Batch, Contest, Log, Mode, Power, Qso, QsoId, ScoreSheet, Station};

/// The menu of the main window a panel's window is listed in, and the group of its
/// toolbar buttons and right-click items. The application names the groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum MenuGroup {
    /// Searching: partial and super check, check call, QSO search.
    Search,
    /// The station's equipment and running it: the rig (drawn by the
    /// application), SO2R, keyer, voice, CW and RTTY decoders, serial ports.
    Device,
    /// Talking to the outside: cluster, Z-Link, licence look-up, news.
    Network,
    /// Results: score, multipliers, summary sheet, map, band scope, rate.
    Report,
    /// Everything else (serial ports, say).
    #[default]
    Tools,
    /// The File menu: the application's own commands (open, save, import), drawn
    /// first on the toolbar. Not for panels, and not in [`MenuGroup::ALL`].
    File,
    /// The Edit menu: the application's QSO and entry-row commands, then the
    /// panels' items (changing many QSOs at once), which open their window and have
    /// no toolbar buttons. Not in [`MenuGroup::ALL`].
    Edit,
    /// The Help menu, below the application's own items (manual, updates). Not a
    /// menu of its own and without toolbar buttons, so it is not in [`MenuGroup::ALL`].
    Help,
    /// The Settings menu, below the application's own items (the operators, the
    /// ZyLO plugins).
    /// Like [`MenuGroup::Help`]: not a menu of its own, no toolbar buttons, not in
    /// [`MenuGroup::ALL`].
    Settings,
}

impl MenuGroup {
    /// Whether the group's items go below the application's own items of one of
    /// its menus (Edit, Help, Settings): they open their window rather than tick
    /// it and have no toolbar buttons.
    pub fn is_app_menu(self) -> bool {
        matches!(
            self,
            MenuGroup::Edit | MenuGroup::Help | MenuGroup::Settings
        )
    }
}

impl MenuGroup {
    /// Every group with a menu of its own, in the order of the menu bar.
    pub const ALL: [MenuGroup; 5] = [
        MenuGroup::Search,
        MenuGroup::Report,
        MenuGroup::Device,
        MenuGroup::Network,
        MenuGroup::Tools,
    ];
}

/// An item a panel adds to the right-click menu of a QSO in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QsoMenuItem {
    /// Passed back to [`Panel::on_qso_menu`].
    pub id: String,
    /// Text of the item, in the language asked for.
    pub label: String,
    pub enabled: bool,
}

impl QsoMenuItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        QsoMenuItem {
            id: id.into(),
            label: label.into(),
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// A piece of text a panel shows in the status bar ([`Panel::status`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusItem {
    /// Text shown in the bar, in the language of [`View::language`].
    pub text: String,
    /// Longer text shown when the pointer rests on the item.
    pub tooltip: Option<String>,
    /// Drawn in the warning colour.
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

/// A station reported on the air (a cluster or skimmer spot, a Z-Server notice).
///
/// Panels hand spots out with [`Panel::take_spots`]; the application keeps the
/// recent ones and shows them to every panel in [`View::spots`].
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub callsign: String,
    /// Frequency in hertz.
    pub hz: u64,
    pub mode: Option<Mode>,
    /// Who reported it.
    pub spotter: String,
    pub comment: String,
    /// When it was received, in seconds since the Unix epoch.
    pub time: i64,
}

/// The open contest as a panel sees it.
pub struct View<'a> {
    pub log: &'a Log,
    pub contest: &'a dyn Contest,
    /// What the contest computed at the last change.
    pub sheet: &'a ScoreSheet,
    /// Callsign being typed in the entry row, upper case.
    pub callsign: &'a str,
    /// The operator's own station (callsign, location, power).
    pub station: &'a Station,
    /// Received number being typed in the entry row.
    pub number: &'a str,
    /// RST to send, and the power, of the entry row.
    pub rst_sent: &'a str,
    pub power: Power,
    /// Band and mode of the entry row.
    pub band: Band,
    pub mode: Mode,
    /// Language code of the user interface (`en`, `ja`).
    pub language: &'a str,
    /// Spots received recently, oldest first.
    pub spots: &'a [Spot],
    /// Frequency of the rig in hertz, when one is connected.
    pub hz: Option<u64>,
    /// The rig the entry row works with, 1 or 2 (zLog `FCurrentRigSet`): the one
    /// keyed and listened to; changed with Shift+X or [`EntryAction::SelectRig`].
    pub rig: u8,
    /// Whether the entry row is in CQ (run) rather than S&P mode (`CurrentQSO.CQ`,
    /// toggled with Shift+O, `actionToggleCqSp`); stored in `Qso::cq` when logged.
    pub cq: bool,
    /// Operator of the entry row (`CurrentQSO.Operator`); written into `Qso::operator`.
    pub operator: &'a str,
    /// Length of the contest period in hours from [`Log::start_time`], or 0 when
    /// QSOs are not checked against a period (zLog `UseContestPeriod` off, or a
    /// contest without one); see [`Log::out_of_period`].
    pub period_hours: u32,
    /// Name of this PC on the network (zLog `Settings._pcname`, zlog.ini `[Z-Link]
    /// PCName`), stored in logged QSOs and sent to Z-Server.
    pub pc_name: &'a str,
    /// Entry field holding the keyboard focus, if any (zLog runs the partial check
    /// by number only from the number field, main.pas `NumberEditChange`).
    pub focus: Option<EntryField>,
    /// The QSO open in the edit (or insert) dialog, as it is in the log (zLog
    /// locks it on Z-Link, `TEditDialog.Init`); see [`Panel::editing`].
    pub editing: Option<&'a Qso>,
    /// The QSO selected in the log grid, while the grid has the keyboard.
    pub selected: Option<&'a Qso>,
    /// Which QSOs lie outside the contest period, as logging judges them.
    pub period: &'a dyn PeriodCheck,
}

impl View<'_> {
    /// Whether `q` lies outside the contest period (zLog `TLog.IsOutOfPeriod`);
    /// always false while QSOs are not checked against one.
    pub fn out_of_period(&self, q: &Qso) -> bool {
        self.period.outside(q)
    }
}

/// Which QSOs lie outside the contest period ([`View::period`]); `()` checks none.
pub trait PeriodCheck {
    fn outside(&self, q: &Qso) -> bool;
}

impl PeriodCheck for () {
    fn outside(&self, _q: &Qso) -> bool {
        false
    }
}

/// A key of the main window handed to the panels ([`Panel::on_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKey {
    /// F1 to F12 (`n`), with Shift when `shift` (zLog plays the other message bank
    /// then). Panels are asked in order until one takes it.
    Function { n: u8, shift: bool },
    /// Esc (zLog `actionCQAbort`: stop sending CW and the CQ repeat). Every panel is
    /// told; the entry row still handles Esc itself.
    Escape,
    /// Typing in the callsign or number field of the entry row (zLog
    /// `OtherKeyPressed`, which stops the CQ repeat). Every panel is told.
    Typed,
}

/// A field of the entry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryField {
    Call,
    RstSent,
    RstRcvd,
    Number,
    Memo,
}

pub trait Panel {
    /// Stable identifier, used to remember whether the panel is open.
    fn id(&self) -> &str;

    /// Title of the window and of its item in the menu of its group.
    fn title(&self, language: &str) -> String;

    /// Default size of the window in points.
    fn default_size(&self) -> [f32; 2] {
        [400.0, 300.0]
    }

    /// The menu its window is listed in, and the group of its toolbar buttons.
    fn menu(&self) -> MenuGroup {
        MenuGroup::Tools
    }

    /// The panel's commands in `language`: usually its window toggle
    /// ([`Command::window`], with the toolbar label and icon) and actions
    /// ([`Command::action`]). Without a window toggle the application makes one
    /// from the title, listed in the menu only. The operator may hide the toolbar
    /// buttons from the Settings menu.
    fn commands(&self, _language: &str) -> Vec<Command> {
        Vec::new()
    }

    /// Whether the toolbar buttons are shown until the operator chooses otherwise
    /// in the Settings menu (the most used ones are; the rest start hidden).
    fn toolbar_default_shown(&self) -> bool {
        true
    }

    /// The section of its menu its items are listed in, and their weight there,
    /// for the commands that name none ([`Command::section`]).
    fn section(&self) -> (MenuSection, i32) {
        (MenuSection::Other, 0)
    }

    /// A command of [`Panel::commands`] other than the window toggle was run
    /// (only while it is enabled).
    fn run_command(&mut self, _id: &str) {}

    /// Items the panel adds to the right-click menu of `qso` in the log, in
    /// `language`; shown after the application's own items.
    fn qso_menu(&self, _language: &str, _qso: &Qso) -> Vec<QsoMenuItem> {
        Vec::new()
    }

    /// An item from [`Panel::qso_menu`] was chosen for `qso`. The panel may answer
    /// through [`Panel::take_actions`] or [`Panel::take_spots`], or open its window
    /// by returning true.
    fn on_qso_menu(&mut self, _id: &str, _qso: &Qso) -> bool {
        false
    }

    /// Whether the panel has a window. A panel without one (only adding to the
    /// status bar, say) gets no window toggle, and [`Panel::ui`] is never called.
    fn has_window(&self) -> bool {
        true
    }

    /// Items the panel shows in the status bar of the main window, every frame,
    /// whether its window is open or not; drawn after the application's own
    /// items, in panel order. Must be cheap and must not block.
    fn status(&self, _view: &View<'_>) -> Vec<StatusItem> {
        Vec::new()
    }

    /// Draw the window, every frame while it is open; nothing for a panel without one.
    fn ui(&mut self, _ui: &mut egui::Ui, _view: &View<'_>) {}

    /// The window was opened (it was closed the frame before); called before its
    /// first [`Panel::ui`], for windows that start afresh each time (a check for
    /// updates, a form filled in from the log).
    fn opened(&mut self, _view: &View<'_>) {}

    /// Whether the window has a row of buttons pinned to its bottom
    /// ([`Panel::buttons`]), as the application's dialogs have OK and Cancel.
    fn has_buttons(&self) -> bool {
        false
    }

    /// Draw the buttons of the bottom row (the action of the window first, then
    /// Cancel or Close); return true to close the window.
    fn buttons(&mut self, _ui: &mut egui::Ui, _view: &View<'_>) -> bool {
        false
    }

    /// Requests to the entry row made during the last [`Panel::ui`] (a decoded
    /// callsign clicked, say); the application takes them after each frame.
    fn take_actions(&mut self) -> Vec<EntryAction> {
        Vec::new()
    }

    /// Spots received since the last call, for every panel to see.
    fn take_spots(&mut self) -> Vec<Spot> {
        Vec::new()
    }

    /// Called by the application every frame for every panel, whether its window is
    /// open or not, before [`Panel::ui`]; for work that must go on while closed (a
    /// band scope taking in spots). Must not block.
    fn tick(&mut self, _view: &View<'_>) {}

    /// The operator pressed `key` in the main window ([`EntryKey`] says which panels
    /// are asked); return true when the panel used it. Called even while the panel's
    /// window is closed, but not while a dialog or another text field has the
    /// keyboard.
    fn on_key(&mut self, _key: EntryKey, _view: &View<'_>) -> bool {
        false
    }

    /// Key (`on`) or unkey the transmitter for another panel (an
    /// [`EntryAction::Transmit`]); return true when the panel drives a PTT line and
    /// did it. Panels are asked in order until one takes it.
    fn transmit(&mut self, _on: bool) -> bool {
        false
    }

    /// Write `bytes` on the keyer's port for another panel (an
    /// [`EntryAction::KeyerPort`]); return true when the panel has the port open
    /// and wrote them. Panels are asked in order until one takes them.
    fn keyer_port(&mut self, _bytes: &[u8]) -> bool {
        false
    }

    /// Draw the panel's dialogs and questions other than its window (a dialog on
    /// one QSO, a question before deleting it), every frame, with
    /// [`crate::dialog`]; they open and close as the panel decides.
    fn dialogs(&mut self, _ctx: &egui::Context, _view: &View<'_>) {}

    /// Whether a dialog or question of [`Panel::dialogs`] is open that keeps the
    /// keys of the main window from the entry row, the log grid and the panels.
    fn modal(&self) -> bool {
        false
    }

    /// The dialogs of [`Panel::dialogs`] open now that the Window menu lists, as
    /// (identifier given to [`crate::dialog`], title in `language`).
    fn dialog_windows(&self, _language: &str) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Close the dialogs and questions of [`Panel::dialogs`], as by Cancel (the
    /// Window menu's Close all, quitting).
    fn close_dialogs(&mut self) {}

    /// The QSO a dialog of the panel is open on, shown to every panel as
    /// [`View::editing`].
    fn editing(&self) -> Option<QsoId> {
        None
    }

    /// Restore what the panel saved (a host name, a file path) at start-up, and
    /// whenever its section changed elsewhere (a tab of Options editing it, an
    /// imported zlog.ini).
    fn load(&mut self, _section: &Section) {}

    /// Save what the panel wants back at the next start-up.
    fn save(&self, _section: &mut Section) {}
}

/// The width of the fields of a form laid out as a grid of label and field: what
/// the window leaves after the widest of `labels`, never below [`FIELD_MIN`]. Use
/// it with [`sized_field`], so every field of the form lines up and grows with the
/// window.
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
    (ui.available_width() - label - 3.0 * ui.spacing().item_spacing.x).max(FIELD_MIN)
}

/// The narrowest a field of [`form_field_width`] gets.
pub const FIELD_MIN: f32 = 160.0;

/// A one-line field exactly `width` wide (inside a grid a field gets no more
/// than its column had in the frame before, so it is sized outright).
pub fn sized_field(ui: &mut egui::Ui, edit: egui::TextEdit<'_>, width: f32) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    ui.add_sized([width, height], edit)
}

/// The name of the folder zLog RS keeps its files in, in the settings, data and
/// documents folders of the system: `zlog`, the name zLog itself will share.
pub const APP_DIR: &str = "zlog";

/// A row of a window's toolbar whose text field (a search box, a command line)
/// takes whatever width the other widgets leave, so it grows with the window.
/// `leading` is drawn first, left to right; `trailing` at the right end, right
/// to left (add its widgets last one first); `field` gets the rest and should
/// use `desired_width(f32::INFINITY)`.
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

/// A contest offered by a [`ContestList`]: what the user-defined contest window
/// shows to find it, and the files it needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListedContest {
    pub name: String,
    /// When the contest is held, as the list writes it (empty when unknown).
    pub date: String,
    /// The rules of the contest.
    pub rules: Option<String>,
    /// The CFG and DAT files, by the names the browser saves them under.
    pub files: Vec<ListedFile>,
    /// Remarks of the list.
    pub note: String,
    /// When the files last changed, when the list says (shown to the operator).
    pub updated: Option<Date>,
    /// What changed in the files, newest first, when the list says.
    pub changes: Vec<Change>,
}

/// One entry of a list's change history.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Change {
    pub date: Option<Date>,
    pub text: String,
}

/// A local date, `(year, month, day)`.
pub type Date = (i32, u32, u32);

/// One file of a [`ListedContest`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListedFile {
    /// File name, as saved by the browser.
    pub name: String,
    /// Where the browser downloads it.
    pub url: String,
    /// What it is for (`県内局`), empty when the list says nothing.
    pub note: String,
}

/// A list of contest files published somewhere, searched in the user-defined
/// contest window. The window never downloads the files itself: they are opened
/// in the browser, so the operator visits the site that publishes them, and then
/// imported into the contest folder from wherever the browser saved them.
pub trait ContestList {
    /// Stable identifier.
    fn id(&self) -> &str;

    /// Name of the list, for its filter and the details (`JA6YCU`), in `language`.
    fn title(&self, language: &str) -> String;

    /// The page the list comes from, linked from the window.
    fn page(&self) -> &str;

    /// The list's own change history (every contest), newest first; empty until
    /// the list is fetched, or when the list has none.
    fn changes(&self) -> &[Change] {
        &[]
    }

    /// A page with older changes, when there is one.
    fn history_page(&self) -> Option<&str> {
        None
    }

    /// Fetch the list again. Must not block.
    fn refresh(&mut self);

    /// The list: `None` while it is being fetched (or before the first
    /// [`ContestList::refresh`]), else the contests or why there are none.
    fn contests(&mut self) -> Option<Result<&[ListedContest], String>>;

    /// Whether `file` of `contest`, which is in the contest folder and was last
    /// changed on `changed`, should be fetched again ("Update" in the window).
    /// Each list decides how; it must be cheap (no download), and by default no
    /// file is ever outdated.
    fn outdated(&self, _contest: &ListedContest, _file: &ListedFile, _changed: Date) -> bool {
        false
    }
}

/// The settings of one extension: text values by key, kept and saved by the
/// application under the extension's own name. An extension sees only its section.
pub type Section = std::collections::BTreeMap<String, String>;

/// A page of the Options dialog, contributed by an extension.
pub trait SettingsPage {
    /// Name of the section this page edits; also its identifier.
    fn id(&self) -> &str;

    fn title(&self, language: &str) -> String;

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, language: &str);

    /// The commands of the menus now, and the keys the entry row and the log
    /// grid keep for themselves, for a tab that assigns shortcuts; given before
    /// the tab is drawn.
    fn set_commands(
        &mut self,
        _commands: &[keys::CommandInfo],
        _reserved: &[egui::KeyboardShortcut],
    ) {
    }

    /// The shortcuts chosen in `section`, the tab's own, for a tab that assigns
    /// them; the application puts them into the commands.
    fn shortcuts(&self, _section: &Section) -> Option<keys::Overrides> {
        None
    }

    /// Whether the tab is waiting for a key to be pressed, so the application
    /// runs no shortcut meanwhile.
    fn wants_keys(&self) -> bool {
        false
    }
}

/// A request from a panel to the entry row; the application carries it out.
#[derive(Debug, Clone, PartialEq)]
pub enum EntryAction {
    /// Put this callsign into the callsign field.
    Callsign(String),
    /// Put this text into the received-number field.
    Number(String),
    /// Tune the rig to this frequency in hertz (and mode), or, without a rig, move the
    /// entry row to its band.
    Qsy(u64, Option<Mode>),
    /// Put the entry row into CQ (`true`) or S&P mode (zLog `SetCQ`).
    Cq(bool),
    /// Select this QSO in the log and scroll to it (zLog `Grid.Row`).
    SelectQso(QsoId),
    /// Change the log as one edit by the operator: the commands of the batch, when
    /// the log is as it expects ([`zlog_core::Book::execute_if`]); updates that
    /// change nothing are left out. The log is saved once, the links are told of
    /// each change, and the status line says `done`, or by default how many QSOs
    /// changed; a refusal says which QSO differs.
    Edit { batch: Batch, done: Option<String> },
    /// Show this on the status line.
    Say(StatusItem),
    /// Key (`true`) or unkey the transmitter (zLog `ControlPTT`, used around voice
    /// memories); the application hands it to the panels by [`Panel::transmit`].
    Transmit(bool),
    /// Write these bytes on the WinKeyer port of the keyer, for a device that
    /// shares it (SO2R Neo, whose commands zLog sends there); the application
    /// hands them to the panels by [`Panel::keyer_port`].
    KeyerPort(Vec<u8>),
    /// Make rig 1 or 2 the one the entry row works with (zLog `SwitchRig`); the
    /// entry row then follows that rig.
    SelectRig(u8),
    /// Make this the operator of the entry row, empty for none (zLog
    /// `SelectOperator`); kept as the station's operator, so later QSOs carry it.
    Operator(String),
    /// Set the power of the entry row (an operator's power, zLog `SetOpPower`).
    Power(Power),
}

impl EntryAction {
    /// Replace these QSOs, by ID, as one edit by the operator (the edit dialog does
    /// one at a time).
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
