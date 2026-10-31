use crate::book::Command;
use crate::contest::{Contest, ContestDef};
use crate::log::Log;
use crate::qso::Qso;
use crate::step::Step;

#[derive(Debug, Clone, PartialEq)]
pub enum LogEvent {
    Opened { contest: String },
    Inserted { index: usize },
    Updated { index: usize, before: Qso },
    Deleted { index: usize, qso: Qso },
    Sorted,
    Resynced,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    User,
    Member(usize),
}

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

    pub fn origin(&self) -> Origin {
        self.origin
    }

    pub fn root_origin(&self) -> Origin {
        self.root
    }

    pub(crate) fn with_root(mut self, root: Origin) -> Self {
        self.root = root;
        self
    }

    pub fn caused_by_me(&self) -> bool {
        self.origin == Origin::Member(self.me)
    }

    pub fn send(&mut self, command: Command) {
        self.commands.push((Origin::Member(self.me), command));
    }

    pub fn resync(&mut self) {
        self.resynced = Some(Origin::Member(self.me));
    }

    pub fn say(&mut self, message: impl Into<String>) {
        self.messages.push(message.into());
    }
}

pub trait LogFormat: Send + Sync {
    fn name(&self) -> &str;

    fn extensions(&self) -> &[&str];

    fn can_import(&self) -> bool {
        false
    }

    fn can_export(&self) -> bool {
        false
    }

    fn import(&self, bytes: &[u8]) -> Result<Vec<Qso>, String> {
        let _ = bytes;
        Err(format!("{} cannot import", self.name()))
    }

    fn export(&self, log: &Log, contest: &dyn Contest) -> Result<Vec<u8>, String> {
        let _ = (log, contest);
        Err(format!("{} cannot export", self.name()))
    }
}

pub trait ContestFile: Send + Sync {
    fn name(&self, language: &str) -> String;

    fn extensions(&self) -> &[&str];

    fn accepts(&self, path: &std::path::Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| self.extensions().iter().any(|x| x.eq_ignore_ascii_case(e)))
    }

    fn title(&self, path: &std::path::Path) -> Option<String> {
        self.open(path, "en").ok().map(|c| c.def.caption("en"))
    }

    fn open(&self, path: &std::path::Path, language: &str) -> Result<OpenedContest, String>;
}

pub struct OpenedContest {
    pub def: Box<dyn ContestDef>,
    pub warnings: Vec<String>,
}

pub trait Observer {
    fn name(&self) -> &str;

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent);

    fn on_step(&mut self, log: &Log, step: &Step) {
        let _ = (log, step);
    }
}

pub trait Link: Observer {
    fn poll(&mut self, cx: &mut Context<'_>);
}

pub trait Extension {
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

impl<E: Extension + ?Sized> Extension for Box<E> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        (**self).contests()
    }
    fn formats(&self) -> Vec<Box<dyn LogFormat>> {
        (**self).formats()
    }
    fn contest_files(&self) -> Vec<Box<dyn ContestFile>> {
        (**self).contest_files()
    }
    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        (**self).observers()
    }
    fn links(&mut self) -> Vec<Box<dyn Link>> {
        (**self).links()
    }
}

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

    pub fn has_links(&self) -> bool {
        self.members.iter().any(|m| matches!(m, Member::Link(_)))
    }

    pub fn has_periods(&self) -> bool {
        !self.contest_files.is_empty()
            || self
                .contests
                .iter()
                .any(|c| c.modes().iter().any(|&m| c.build(m).uses_period()))
    }

    pub fn contest_files(&self) -> &[Box<dyn ContestFile>] {
        &self.contest_files
    }

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
    fn periods_are_possible_with_a_contest_file() {
        assert!(!Registry::new().has_periods());
        assert!(Registry::new().with(Files).has_periods());
        assert!(!Registry::new().with(Files).has_links());
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
