pub mod cfg;
pub mod check;
pub mod common;
pub mod file;
pub mod script;
mod table;
pub mod user;
#[cfg(feature = "window")]
pub mod window;

pub use cfg::Config;
pub use file::CfgFile;
pub use user::{from_cfg, parse_dat, DatEntry, UserDefinedDef};

use zlog_core::{Command, ContestFile, Context, Extension, Link, LogEvent, Observer};

zlog_core::texts!();

pub struct Cfg;

impl Extension for Cfg {
    fn name(&self) -> &str {
        "User-defined contests"
    }

    fn contest_files(&self) -> Vec<Box<dyn ContestFile>> {
        vec![Box::new(CfgFile)]
    }

    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(ScriptResults {
            seen: script::host::generation(),
        })]
    }
}

struct ScriptResults {
    seen: u64,
}

impl Observer for ScriptResults {
    fn name(&self) -> &str {
        "Scoring scripts"
    }

    fn on_event(&mut self, _cx: &mut Context<'_>, _event: &LogEvent) {}
}

impl Link for ScriptResults {
    fn poll(&mut self, cx: &mut Context<'_>) {
        for notice in script::host::take_notices() {
            cx.say(notice);
        }
        let now = script::host::generation();
        if now != self.seen {
            self.seen = now;
            cx.send(Command::Rescore);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
