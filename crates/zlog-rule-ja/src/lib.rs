//! Japanese contests for zLog:
//!
//! - [`jarl`]: JARL's four majors, each in its own file: [`allja`], [`six_down`],
//!   [`field_day`], [`acag`].
//! - [`ja0`]: ALL JA0, in its two sections (JA0 stations and others).
//! - [`nyp`]: NEW YEAR PARTY.
//! - [`japan`]: [`JapanContest`], the contest list entries of ALL JA0 and NYP.
//! - [`asahikawa`], [`high_school`], [`tokai_marathon`], [`yl`]: local contests,
//!   each a CFG and DAT file with its own rules, on the CFG contests of
//!   zlog-cfg.
//!
//! The zLog_cfgdat collection at github.com/jr8ppg/zLog_cfgdat
//! carries no licence, so none of its samples is embedded.

pub mod acag;
pub mod allja;
pub mod asahikawa;
pub mod field_day;
pub mod high_school;
pub mod ja0;
pub mod japan;
pub mod jarl;
pub mod nyp;
pub mod six_down;
pub mod tokai_marathon;
pub mod yl;

pub use japan::JapanContest;
pub use jarl::{JarlContest, JarlRules};

use zlog_core::{ContestDef, Extension};

zlog_core::texts!();

/// The extension that adds the built-in Japanese contests (JARL's four and the
/// others). The operator's own CFG files are zlog-cfg's.
pub struct Japan;

impl Extension for Japan {
    fn name(&self) -> &str {
        "Japanese contests"
    }

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        let mut all: Vec<Box<dyn ContestDef>> = JarlContest::ALL
            .into_iter()
            .map(|c| Box::new(c) as Box<dyn ContestDef>)
            .collect();
        all.extend(
            JapanContest::ALL
                .into_iter()
                .map(|c| Box::new(c) as Box<dyn ContestDef>),
        );
        all.extend([
            asahikawa::boxed(),
            high_school::boxed(),
            tokai_marathon::boxed(),
            yl::boxed(),
        ]);
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn extension_lists_all_contests() {
        assert_eq!(Japan.contests().len(), 11);
    }
}
