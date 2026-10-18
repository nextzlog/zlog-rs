//! Japanese contests for zLog other than JARL's four majors:
//!
//! - [`user`]: user-defined contests from a zLog CFG file and its DAT list,
//!   chosen by the operator at run time ([`from_cfg`]).
//! - [`ja0`]: ALL JA0, in its two sections (JA0 stations and others).
//! - [`nyp`]: NEW YEAR PARTY.
//! - [`zylo`]: local contests from ZyLO rule plugins (MIT, JA1ZLO), built in.
//!
//! The zLog_cfgdat collection at github.com/jr8ppg/zLog_cfgdat
//! carries no licence, so none of its samples is embedded.

pub mod cfg;
pub mod common;
pub mod ja0;
pub mod nyp;
pub mod user;
pub mod zylo;

pub use cfg::Config;
pub use user::{from_cfg, parse_dat, DatEntry, UserContest, UserDefinedDef};

use zlog_core::{Contest, ContestDef, ContestMode, Extension};

zlog_core::texts!();

/// Built-in Japanese contests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JapanContest {
    /// ALL JA0, section for JA0 stations (`TJA0ContestZero`).
    Ja0Zero,
    /// ALL JA0, section for other stations (`TJA0Contest`).
    Ja0Others,
    NewYearParty,
}

impl JapanContest {
    pub const ALL: [JapanContest; 3] = [
        JapanContest::Ja0Zero,
        JapanContest::Ja0Others,
        JapanContest::NewYearParty,
    ];

    pub fn id(self) -> &'static str {
        match self {
            JapanContest::Ja0Zero => "ja0-in",
            JapanContest::Ja0Others => "ja0-out",
            JapanContest::NewYearParty => "nyp",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.id().eq_ignore_ascii_case(id))
    }
}

impl ContestDef for JapanContest {
    fn id(&self) -> &str {
        JapanContest::id(*self)
    }

    fn caption(&self, language: &str) -> String {
        let key = match self {
            JapanContest::Ja0Zero => "caption.ja0in",
            JapanContest::Ja0Others => "caption.ja0out",
            JapanContest::NewYearParty => "caption.nyp",
        };
        texts().get(language, key).into()
    }

    fn group(&self, language: &str) -> String {
        texts().get(language, "group.japan").into()
    }

    /// The zLog Menu's mode buttons (UMenu.pas, cases 4-6).
    fn modes(&self) -> &[ContestMode] {
        match self {
            JapanContest::NewYearParty => &[
                ContestMode::Mix,
                ContestMode::Cw,
                ContestMode::Phone,
                ContestMode::Rtty,
                ContestMode::All,
            ],
            _ => &[ContestMode::Mix, ContestMode::Cw, ContestMode::All],
        }
    }

    fn build(&self, mode: ContestMode) -> Box<dyn Contest> {
        match self {
            JapanContest::Ja0Zero => Box::new(ja0::Ja0Contest::new(true, mode)),
            JapanContest::Ja0Others => Box::new(ja0::Ja0Contest::new(false, mode)),
            JapanContest::NewYearParty => Box::new(nyp::NypContest::new(mode)),
        }
    }
}

/// The extension that adds the built-in Japanese contests. User-defined contests are
/// built from the operator's CFG file with [`from_cfg`].
pub struct Japan;

impl Extension for Japan {
    fn name(&self) -> &str {
        "Japanese contests"
    }

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        let mut all: Vec<Box<dyn ContestDef>> = JapanContest::ALL
            .into_iter()
            .map(|c| Box::new(c) as Box<dyn ContestDef>)
            .collect();
        all.extend(zylo::ZyloContest::defs());
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
        assert_eq!(Japan.contests().len(), 7);
    }
}
