use zlog_core::{Contest, ContestDef, ContestMode};

use crate::{ja0, nyp, texts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JapanContest {
    Ja0Zero,
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
