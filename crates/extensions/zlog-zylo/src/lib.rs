pub mod format;
pub mod helper;
pub mod host;
pub mod library;
pub mod link;
pub mod manager;
pub mod options;
pub mod panel;
pub mod protocol;
pub mod record;
pub mod rules;

use zlog_core::{ContestFile, Extension, Link, LogFormat};

pub use options::ZyloOptions;
pub use panel::ZyloPanel;

zlog_core::texts!();

pub const SUBCOMMAND: &str = "zylo-host";

pub fn helper_main() -> ! {
    helper::main()
}

pub struct Zylo;

impl Extension for Zylo {
    fn name(&self) -> &str {
        "ZyLO"
    }

    fn formats(&self) -> Vec<Box<dyn LogFormat>> {
        format::formats(&manager::global())
    }

    fn contest_files(&self) -> Vec<Box<dyn ContestFile>> {
        vec![Box::new(rules::ZyloCfgFile {
            manager: manager::global(),
        })]
    }

    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(link::ZyloLink::new(manager::global()))]
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.extension(zlog_ui::builder::order::RULE_PLUGIN, || Zylo);
    b.panel(590, |_| ZyloPanel::default());
    b.page(520, |_| ZyloOptions::default());
    b.subcommand(SUBCOMMAND, || helper_main());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
