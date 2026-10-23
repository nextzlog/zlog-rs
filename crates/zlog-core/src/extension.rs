//! Extensions linked in at build time.
//!
//! An extension crate exports one [`Extension`], a package that bundles components
//! of these kinds, each with its own trait:
//!
//! | Kind      | Trait          | Does                                                    |
//! |-----------|----------------|---------------------------------------------------------|
//! | Rules     | [`ContestDef`] | a contest: sent number, number check, scoring            |
//! | Formats   | [`LogFormat`]  | import and export of a file format                      |
//! | Observers | [`Observer`]   | reacts to changes of the log and to the operator's steps |
//! | Links     | [`Link`]       | an observer that also receives from outside (network)   |
//! | Files     | [`ContestFile`]| a contest defined by a file the operator chooses (CFG)   |
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
use crate::contest::{Contest, ContestDef};
use crate::log::Log;
use crate::qso::Qso;
use crate::step::Step;

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
    /// The log was reordered ([`Command::SortByTime`]); every index known from earlier
    /// events is void. Rebuild index-based state from [`Context::log`].
    Sorted,
    /// A link replaced the log or merged it with another station's
    /// ([`Context::resync`]), after the commands it sent; state kept about the log
    /// as it was (the operator's steps to undo, say) no longer holds.
    Resynced,
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
    root: Origin,
    me: usize,
    pub(crate) commands: Vec<(Origin, Command)>,
    pub(crate) messages: Vec<String>,
    pub(crate) resynced: Option<Origin>,
}

impl<'a> Context<'a> {
    pub(crate) fn new(log: &'a Log, origin: Origin) -> Self {
        Context {
            log,
            origin,
            root: origin,
            me: 0,
            commands: Vec::new(),
            messages: Vec::new(),
            resynced: None,
        }
    }

    pub fn log(&self) -> &Log {
        self.log
    }

    /// Who made the change being reported (`Origin::User` inside [`Link::poll`]).
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Who started the chain of changes the reported one belongs to: `Origin::User`
    /// for the operator's command and every reaction to it, `Origin::Member(link)` for
    /// a command a link sent from [`Link::poll`] (data received from outside) and every
    /// reaction to it. A link that mirrors QSOs elsewhere forwards changes whose root
    /// is the operator, whoever made them, and never those rooted in received data.
    pub fn root_origin(&self) -> Origin {
        self.root
    }

    pub(crate) fn with_root(mut self, root: Origin) -> Self {
        self.root = root;
        self
    }

    /// Whether the change being reported was requested by this observer or link.
    pub fn caused_by_me(&self) -> bool {
        self.origin == Origin::Member(self.me)
    }

    /// Ask the book to change the log once the current call has returned.
    pub fn send(&mut self, command: Command) {
        self.commands.push((Origin::Member(self.me), command));
    }

    /// Tell every extension that the log was replaced or merged with another (inside
    /// [`Link::poll`]): [`LogEvent::Resynced`] follows the commands of this poll.
    pub fn resync(&mut self) {
        self.resynced = Some(Origin::Member(self.me));
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

    /// File contents for the log of `contest` (formats may write contest fields,
    /// such as ADIF's [`Contest::adif_fields`]). Called only when
    /// [`LogFormat::can_export`] is true.
    fn export(&self, log: &Log, contest: &dyn Contest) -> Result<Vec<u8>, String> {
        let _ = (log, contest);
        Err(format!("{} cannot export", self.name()))
    }
}

/// A kind of file that defines a contest, chosen by the operator at run time (zLog's
/// user-defined contests, `TUserDefinedContest`, read from a CFG file).
pub trait ContestFile: Send + Sync {
    /// Name in file dialogs and menus (`User-defined contest (CFG)`).
    fn name(&self, language: &str) -> String;

    /// File extensions without the dot, lower case (`["cfg"]`).
    fn extensions(&self) -> &[&str];

    /// Whether `path` has one of [`ContestFile::extensions`] (any case).
    fn accepts(&self, path: &std::path::Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| self.extensions().iter().any(|x| x.eq_ignore_ascii_case(e)))
    }

    /// The title of the contest in the file, read cheaply for a list of files;
    /// `None` when the file cannot be read.
    fn title(&self, path: &std::path::Path) -> Option<String> {
        self.open(path, "en").ok().map(|c| c.def.caption("en"))
    }

    /// The contest the file defines. Files it refers to (a DAT list) are looked up
    /// next to it; a missing one is an error only when the contest cannot run
    /// without it, else a warning. Messages are in `language`.
    fn open(&self, path: &std::path::Path, language: &str) -> Result<OpenedContest, String>;
}

/// A contest read from a file, with what the operator should be told about it.
pub struct OpenedContest {
    pub def: Box<dyn ContestDef>,
    /// Problems that do not stop the contest (a DAT list not found).
    pub warnings: Vec<String>,
}

/// Reacts to changes of the log.
pub trait Observer {
    /// Name used in error messages, for example when a chain of commands runs away.
    fn name(&self) -> &str;

    /// Called once per change, after rescoring, in the order of the changes.
    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent);

    /// Called when one action of the operator has finished, with what it changed
    /// ([`crate::Book::begin_step`]), after its events; only reads.
    fn on_step(&mut self, log: &Log, step: &Step) {
        let _ = (log, step);
    }
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

    fn contest_files(&self) -> Vec<Box<dyn ContestFile>> {
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
    contest_files: Vec<Box<dyn ContestFile>>,
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
        self.contest_files.extend(extension.contest_files());
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

    pub fn contest_files(&self) -> &[Box<dyn ContestFile>] {
        &self.contest_files
    }

    /// The contest defined by the file at `path`, read by the first contest file kind
    /// that accepts its extension.
    pub fn open_contest_file(
        &self,
        path: &std::path::Path,
        language: &str,
    ) -> Result<OpenedContest, String> {
        self.contest_files
            .iter()
            .find(|f| f.accepts(path))
            .ok_or_else(|| format!("{}: not a contest file", path.display()))?
            .open(path, language)
    }

    /// Add a contest opened from a file; returns its index in [`Registry::contests`].
    pub fn add_contest(&mut self, contest: Box<dyn ContestDef>) -> usize {
        self.contests.push(contest);
        self.contests.len() - 1
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

    pub(crate) fn step(&mut self, log: &Log, step: &Step) {
        for m in &mut self.members {
            match m {
                Member::Observer(o) => o.on_step(log, step),
                Member::Link(l) => l.on_step(log, step),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct Cfg;

    impl ContestFile for Cfg {
        fn name(&self, _: &str) -> String {
            "CFG".into()
        }
        fn extensions(&self) -> &[&str] {
            &["cfg"]
        }
        fn open(&self, path: &Path, _: &str) -> Result<OpenedContest, String> {
            Err(format!("opened {}", path.display()))
        }
    }

    struct Files;

    impl Extension for Files {
        fn name(&self) -> &str {
            "files"
        }
        fn contest_files(&self) -> Vec<Box<dyn ContestFile>> {
            vec![Box::new(Cfg)]
        }
    }

    #[test]
    fn registry_collects_contest_files_and_picks_by_extension() {
        let r = Registry::new().with(Files).with(Files);
        assert_eq!(r.contest_files().len(), 2);
        assert!(r.contest_files()[0].accepts(Path::new("a/TEST.CFG")));
        assert!(!r.contest_files()[0].accepts(Path::new("a/test.dat")));
        let err = |p: &str| r.open_contest_file(Path::new(p), "en").err().unwrap();
        assert_eq!(err("x.Cfg"), "opened x.Cfg");
        assert!(err("x.txt").contains("not a contest file"));
    }
}
