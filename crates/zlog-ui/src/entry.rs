//! The entry, where the operator types the QSO being made: what every style of it
//! shares ([`EntryState`]) and what a style adds ([`EntryStyle`]).
//!
//! The application keeps the contents of the entry (callsign, numbers, RSTs, band,
//! mode, power, memo, CQ or S&P) and the rules every style follows: logging with its
//! checks, the dupe check and the number taken from earlier QSOs, the cycles of band,
//! mode and power. A style draws the entry and turns the operator's keys into these
//! operations: zLog's row under the log, the row of another logger, a window of its
//! own. The requests of the panels ([`crate::EntryAction`]) change the same state,
//! whichever style is chosen.

use zlog_core::{Band, DelphiTime, Mode, Power};

use crate::EntryField;

/// Why logging the entry did nothing; the style moves the cursor accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotLogged {
    NoCallsign,
    InvalidNumber,
    /// The book refused the change (the status line says why).
    Refused,
}

/// The entry as a style sees it: its fields and the operations every style shares,
/// carried out by the application. The status line says what they found.
pub trait EntryState {
    /// The text of `field`.
    fn text(&self, field: EntryField) -> &str;

    /// Set `field` as the operator typed it: the callsign and the number are kept
    /// in upper case, and a changed number is looked up as a multiplier (zLog
    /// `CheckMulti`).
    fn set_text(&mut self, field: EntryField, text: String);

    fn band(&self) -> Band;

    fn mode(&self) -> Mode;

    fn power(&self) -> Power;

    /// Whether the entry is in CQ (run) rather than S&P mode.
    fn cq(&self) -> bool;

    /// The operator a QSO logged now is written with.
    fn operator(&self) -> &str;

    /// The number sent on the band of the entry.
    fn sent(&self) -> String;

    /// The time a QSO logged now gets, in the zone of the log.
    fn now(&self) -> DelphiTime;

    /// How many QSOs the log holds.
    fn qso_count(&self) -> usize;

    /// The widths in points of the columns of the log grid, for a style that lines
    /// up with it: status, date, time, callsign, RST sent, number sent, RST
    /// received, number received, multiplier, band, mode, power, operator, memo
    /// and points.
    fn columns(&self) -> &[f32];

    /// The next band of the contest, or the one before (zLog Shift+B); the power
    /// follows the station's for the band.
    fn cycle_band(&mut self, back: bool);

    /// The next mode the band carries, or the one before (zLog Shift+M).
    fn cycle_mode(&mut self, back: bool);

    /// The next power code: H, P, L, M (zLog Shift+P).
    fn cycle_power(&mut self);

    /// Switch between CQ and S&P (zLog Shift+O, `actionToggleCqSp`).
    fn toggle_cq(&mut self);

    /// Work with the other rig (zLog Shift+X, `actionToggleRig`).
    fn toggle_rig(&mut self);

    /// Raise or lower the S figure of the RST in `field` (zLog Shift+Up and
    /// Shift+Down, `IncreaseS`); other fields are left alone.
    fn bump_s(&mut self, field: EntryField, up: bool);

    /// Clear the callsign, the number, the memo and the status line, keeping the
    /// RSTs (zLog Ctrl+K, `actionClearCallAndRpt`).
    fn clear(&mut self);

    /// The callsign is complete (zLog Space, `SpaceBarProc`): it is checked for a
    /// dupe, an empty number is taken from an earlier QSO with the station, and
    /// unless it is a dupe the number is looked up as a multiplier. Whether it is
    /// a dupe.
    fn complete_call(&mut self) -> bool;

    /// Log the entry (zLog Enter), now and on the frequency of the rig, and clear
    /// it for the next QSO.
    fn log(&mut self) -> Result<(), NotLogged>;

    /// Give the keyboard to the log grid, on its last QSO.
    fn enter_log(&mut self);

    /// The entry took the keyboard: the log grid gives up its selection.
    fn leave_log(&mut self);
}

/// A way of drawing the entry and of typing into it, chosen in Options among
/// those linked in.
pub trait EntryStyle {
    /// Stable identifier, saved as the choice.
    fn id(&self) -> &str;

    /// Name of the style in Options, in `language`.
    fn title(&self, language: &str) -> String;

    /// Take the keys of the main window meant for the entry, before anything is
    /// drawn, so its fields never see them; every frame, except while a dialog
    /// keeps the keys or the log grid has them.
    fn keys(&mut self, ctx: &egui::Context, entry: &mut dyn EntryState);

    /// Draw the entry in `ui`, the bottom of the main window (a style with a
    /// window of its own opens it from `ui.ctx()`), every frame.
    fn ui(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState);

    /// The field holding the keyboard, if any.
    fn focused(&self, ctx: &egui::Context) -> Option<EntryField>;

    /// Give the keyboard to `field` when the entry is next drawn.
    fn focus(&mut self, field: EntryField);
}
