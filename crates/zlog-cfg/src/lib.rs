//! zLog's user-defined contests, for contests of any country:
//!
//! - [`cfg`]: the CFG file (`TUserDefinedContest`).
//! - [`user`]: the contest a CFG file and its DAT list define ([`from_cfg`]),
//!   with zLog's general multipliers and points.
//! - [`file`]: CFG files chosen by the operator at run time ([`CfgFile`]).
//! - [`common`]: scoring helpers of zLog's general contests, also used by rule
//!   crates for contests of their own.
//! - [`window`] (feature `window`): the user-defined contest window, an entry
//!   of the zLog Menu over the contest folder and the contest lists linked in.

pub mod cfg;
pub mod common;
pub mod file;
pub mod user;
#[cfg(feature = "window")]
pub mod window;

pub use cfg::Config;
pub use file::CfgFile;
pub use user::{from_cfg, parse_dat, DatEntry, UserContest, UserDefinedDef};

use zlog_core::{ContestFile, Extension};

zlog_core::texts!();

/// The extension that opens CFG files as contests.
pub struct Cfg;

impl Extension for Cfg {
    fn name(&self) -> &str {
        "User-defined contests"
    }

    fn contest_files(&self) -> Vec<Box<dyn ContestFile>> {
        vec![Box::new(CfgFile)]
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
