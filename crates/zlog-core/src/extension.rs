//! Extensions linked in at build time.
//!
//! An extension crate exports one [`Extension`], a package that bundles components
//! of these kinds, each with its own trait:
//!
//! | Kind      | Trait          | Does                                                    |
//! |-----------|----------------|---------------------------------------------------------|
//! | Rules     | [`ContestDef`] | a contest: sent number, number check, scoring            |
//! | Formats   | [`LogFormat`]  | import and export of a file format                      |
//! | Observers | [`Observer`]   | reacts to changes of the log                            |
//! | Links     | [`Link`]       | an observer that also receives from outside (network)   |
//!
//! Windows (panels) and devices (rigs, keyers) have their traits in `zlog-ui` and
//! `zlog-rig`, since the core knows neither a screen nor hardware.
//!
//! This replaces ZyLO's DLL entry points:
//!
//! | ZyLO                                                       | here                    |
//! |------------------------------------------------------------|-------------------------|
//! | `AttachEvent`, `VerifyEvent`, `PointsEvent`, `QueryCities` | [`ContestDef`]          |
//! | `ImportEvent`, `ExportEvent`, `QueryFormat`                | [`LogFormat`]           |
//! | `InsertEvent`, `DeleteEvent`, update                       | [`Observer::on_event`]  |
//! | `LaunchEvent`, `FinishEvent`                               | [`LogEvent::Opened`], [`LogEvent::Closed`] |
//! | `CommitCallBack`, `InsertCallBack`, ...                    | [`Context::send`]       |
//!
//! Rules every observer and link can rely on:
//!
//! - It sees the log only through [`Context::log`], and only during its own call.
//! - It changes the log only by sending [`Command`]s. The book runs them after the
//!   current call has returned, so handlers are never re-entered.
//! - Every event says where the change came from ([`Origin`]); a link that mirrors
//!   QSOs elsewhere (zLink) skips events it caused itself.
//! - Events arrive in the order the changes were made, after rescoring.

use crate::book::Command;
use crate::contest::ContestDef;
use crate::log::Log;
use crate::qso::Qso;

/// Something that happened to the log.
#[derive(Debug, Clone, PartialEq)]
pub enum LogEvent {
    /// A contest was opened (or a new one started) with this contest id.
    ///
    /// When an existing file is opened, [`Context::log`] already holds all its QSOs,
    /// scored; an observer that keeps its own state builds it from them here.
    Opened { contest: String },
    /// The QSO now at `index` was added.
    Inserted { index: usize },
    /// The QSO at `index` was edited; `before` is the old record.
    Updated { index: usize, before: Qso },
    /// The QSO that was at `index` was removed.
    Deleted { index: usize, qso: Qso },
    /// The contest is being closed.
    Closed,
}

/// Who caused a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The operator, through the user interface.
    User,
    /// The observer or link at this position in the registry.
    Member(usize),
}

/// Everything an observer or link may do during a call, and nothing more.
///
/// The log is read-only; changes are requested with [`Context::send`] and run by the
/// book after the call returns.
pub struct Context<'a> {
    log: &'a Log,
    origin: Origin,
    me: usize,
    pub(crate) commands: Vec<(Origin, Command)>,
    pub(crate) messages: Vec<String>,
}

impl<'a> Context<'a> {
    pub(crate) fn new(log: &'a Log, origin: Origin) -> Self {
        Context {
            log,
            origin,
            me: 0,
            commands: Vec::new(),
            messages: Vec::new(),
        }
    }

    pub fn log(&self) -> &Log {
        self.log
    }

    /// Who made the change being reported (`Origin::User` inside [`Link::poll`]).
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Whether the change being reported was requested by this observer or link.
    pub fn caused_by_me(&self) -> bool {
        self.origin == Origin::Member(self.me)
    }

    /// Ask the book to change the log once the current call has returned.
    pub fn send(&mut self, command: Command) {
        self.commands.push((Origin::Member(self.me), command));
    }

    /// Show a message in the status bar.
    pub fn say(&mut self, message: impl Into<String>) {
        self.messages.push(message.into());
    }
}

/// A file format for importing or exporting a log.
pub trait LogFormat: Send + Sync {
    /// Name in file dialogs (`Cabrillo`).
    fn name(&self) -> &str;

    /// File extensions without the dot (`["cbr", "log"]`).
    fn extensions(&self) -> &[&str];

    fn can_import(&self) -> bool {
        false
    }

    fn can_export(&self) -> bool {
        false
    }

    /// Contacts read from a file. Called only when [`LogFormat::can_import`] is true.
    fn import(&self, bytes: &[u8]) -> Result<Vec<Qso>, String> {
        let _ = bytes;
        Err(format!("{} cannot import", self.name()))
    }

    /// File contents for the log. Called only when [`LogFormat::can_export`] is true.
    fn export(&self, log: &Log) -> Result<Vec<u8>, String> {
        let _ = log;
        Err(format!("{} cannot export", self.name()))
    }
}

/// Reacts to changes of the log.
pub trait Observer {
    /// Name used in error messages, for example when a chain of commands runs away.
    fn name(&self) -> &str;

    /// Called once per change, after rescoring, in the order of the changes.
    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent);
}

/// An observer that also receives from outside, such as a network or a timer.
pub trait Link: Observer {
    /// Called regularly by the application (several times a second). Must not block.
    fn poll(&mut self, cx: &mut Context<'_>);
}

/// A package of components, the unit an extension crate exports.
///
/// The registry takes the observers and links once, when the package is added,
/// and keeps them for the life of the program.
pub trait Extension {
    /// Name shown in the About box.
    fn name(&self) -> &str;

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        Vec::new()
    }

    fn formats(&self) -> Vec<Box<dyn LogFormat>> {
        Vec::new()
    }

    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        Vec::new()
    }

    fn links(&mut self) -> Vec<Box<dyn Link>> {
        Vec::new()
    }
}

/// An observer or a link held by the registry.
enum Member {
    Observer(Box<dyn Observer>),
    Link(Box<dyn Link>),
}

impl Member {
    fn name(&self) -> &str {
        match self {
            Member::Observer(o) => o.name(),
            Member::Link(l) => l.name(),
        }
    }
}

/// All extensions linked into one binary.
#[derive(Default)]
pub struct Registry {
    packages: Vec<String>,
    contests: Vec<Box<dyn ContestDef>>,
    formats: Vec<Box<dyn LogFormat>>,
    members: Vec<Member>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, mut extension: impl Extension) -> Self {
        self.packages.push(extension.name().to_string());
        self.contests.extend(extension.contests());
        self.formats.extend(extension.formats());
        self.members
            .extend(extension.observers().into_iter().map(Member::Observer));
        self.members
            .extend(extension.links().into_iter().map(Member::Link));
        self
    }

    /// Names of the packages, in the order they were added.
    pub fn names(&self) -> Vec<&str> {
        self.packages.iter().map(String::as_str).collect()
    }

    pub fn name_of(&self, origin: Origin) -> &str {
        match origin {
            Origin::User => "the operator",
            Origin::Member(i) => self.members.get(i).map_or("?", Member::name),
        }
    }

    pub fn contests(&self) -> &[Box<dyn ContestDef>] {
        &self.contests
    }

    pub fn formats(&self) -> &[Box<dyn LogFormat>] {
        &self.formats
    }

    pub(crate) fn dispatch(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        for (i, m) in self.members.iter_mut().enumerate() {
            cx.me = i;
            match m {
                Member::Observer(o) => o.on_event(cx, event),
                Member::Link(l) => l.on_event(cx, event),
            }
        }
    }

    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) {
        for (i, m) in self.members.iter_mut().enumerate() {
            if let Member::Link(l) = m {
                cx.me = i;
                l.poll(cx);
            }
        }
    }
}
