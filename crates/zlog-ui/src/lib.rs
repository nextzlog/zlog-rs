//! What an extension may add to the zLog window.
//!
//! A panel is a window of its own, listed in the Windows menu. The application
//! draws it every frame with a read-only [`View`] of the open contest; a panel keeps
//! whatever state it likes but never sees the application's internals.
//!
//! Panels that only show the log (a map, a rate graph) should derive what they show
//! from [`View::log`] each time, rather than from events, so they are right from the
//! moment a file is opened and after every edit or deletion.

use zlog_core::{Band, Contest, Log, Mode, ScoreSheet};

/// The open contest as a panel sees it.
pub struct View<'a> {
    pub log: &'a Log,
    pub contest: &'a dyn Contest,
    /// What the contest computed at the last change.
    pub sheet: &'a ScoreSheet,
    /// Callsign being typed in the entry row, upper case.
    pub callsign: &'a str,
    /// Band and mode of the entry row.
    pub band: Band,
    pub mode: Mode,
    /// Language code of the user interface (`en`, `ja`).
    pub language: &'a str,
}

pub trait Panel {
    /// Stable identifier, used to remember whether the panel is open.
    fn id(&self) -> &str;

    /// Title of the window and of its item in the Windows menu.
    fn title(&self, language: &str) -> String;

    /// Default size of the window in points.
    fn default_size(&self) -> [f32; 2] {
        [400.0, 300.0]
    }

    /// Whether the panel has a toggle button on the toolbar (score, multipliers).
    fn on_toolbar(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, view: &View<'_>);

    /// Requests to the entry row made during the last [`Panel::ui`] (a decoded
    /// callsign clicked, say); the application takes them after each frame.
    fn take_actions(&mut self) -> Vec<EntryAction> {
        Vec::new()
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
}

/// A request from a panel to the entry row; the application carries it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryAction {
    /// Put this callsign into the callsign field.
    Callsign(String),
    /// Put this text into the received-number field.
    Number(String),
}
