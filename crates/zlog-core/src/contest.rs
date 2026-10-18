//! The interface every contest implements. The core has no contest of its own.
//!
//! A rule extension exports [`ContestDef`]s; each builds a [`Contest`] when chosen
//! in the zLog Menu. How QSOs are scored is entirely up to the contest.
//!
//! Scoring is one function of the whole log, [`Contest::score`], which reads the log
//! and returns a result; it cannot change the log nor send commands, so it can
//! neither corrupt a QSO nor start a chain of changes. [`rescore`] writes the result
//! into the scoring fields of the QSOs. The book calls it when a file is opened
//! (scores stored in the file are never trusted) and after every change.
//! Common zLog scoring (points and multipliers per band) is offered as a helper by
//! the `zlog-scoring` crate, which rule extensions may use or ignore.

use std::collections::BTreeMap;

use crate::types::Power;

use crate::log::Log;
use crate::qso::Qso;
use crate::types::{Band, ContestMode};

/// Result of checking a received number against the multiplier list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiCheck {
    /// Not a known multiplier.
    Invalid,
    /// Known; `name` is its display name and `worked_on` lists bands already worked.
    Known {
        code: String,
        name: String,
        needed: bool,
        worked_on: Vec<Band>,
    },
}

/// The operator's own station, as entered in the contest menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Station {
    pub callsign: String,
    pub operator: String,
    /// Prefecture number (`Prov`), for example `10`.
    pub prov: String,
    /// City number (`City`), for example `100101`.
    pub city: String,
    /// Power per band, where it differs from [`Station::default_power`].
    pub power: BTreeMap<Band, Power>,
}

impl Station {
    /// zLog's default power per band (`DefIniPower`): H up to 50 MHz, M on 144 and
    /// 430 MHz, L on 1200 MHz, P from 2400 MHz.
    pub fn default_power(band: Band) -> Power {
        match band {
            b if b <= Band::B50 => Power::P010,
            Band::B144 | Band::B430 => Power::P005,
            Band::B1200 => Power::P002,
            _ => Power::P001,
        }
    }

    pub fn power_for(&self, band: Band) -> Power {
        self.power
            .get(&band)
            .copied()
            .unwrap_or_else(|| Self::default_power(band))
    }

    /// Use one power on every band.
    pub fn set_power_everywhere(&mut self, power: Power) {
        self.power = Band::ALL.iter().map(|b| (*b, power)).collect();
    }
}

/// A contest that can be chosen in the zLog Menu; it builds a fresh [`Contest`].
pub trait ContestDef: Send + Sync {
    /// Stable identifier, used on the command line (`allja`).
    fn id(&self) -> &str;

    /// Caption in the zLog Menu (`ALL JA`) in `language`.
    fn caption(&self, language: &str) -> String;

    /// Group the contest is listed under in the zLog Menu (`JARL`, `World`, ...), in
    /// the given language. Contests of the same group are listed together.
    fn group(&self, language: &str) -> String {
        let _ = language;
        "Other".into()
    }

    /// Mode categories offered for this contest.
    fn modes(&self) -> &[ContestMode] {
        &[ContestMode::Mix, ContestMode::Cw, ContestMode::Phone]
    }

    /// Whether the zLog Menu asks for a score coefficient (Field Day).
    fn uses_coeff(&self) -> bool {
        false
    }

    /// Whether a new log of this contest records UTC rather than local time
    /// (`UseUTC`: international contests). The choice is stored in the log's header.
    fn uses_utc(&self) -> bool {
        false
    }

    fn build(&self, mode: ContestMode) -> Box<dyn Contest>;
}

/// Rules of one contest while it is being operated.
///
/// [`Contest::score`] is called after every change with the whole log. A contest may
/// keep state derived from it (the worked multipliers for [`Contest::multi_table`]),
/// rebuilt on each call. The other methods only read and may be called at any time.
pub trait Contest {
    fn name(&self) -> &str;

    /// Bands of the contest, inclusive; the entry row cycles through them.
    fn band_range(&self) -> (Band, Band);

    /// Whether 10, 18 and 24 MHz belong to the contest.
    fn uses_warc(&self) -> bool {
        false
    }

    fn contest_mode(&self) -> ContestMode;

    /// Number sent on a band with a power (`SetNrSent`); the power is that of the QSO.
    fn sent_number(&self, band: Band, power: Power, station: &Station) -> String;

    /// The number to fill in on Space for a station worked before as `past`
    /// (`SpaceBarProc`, `DispExchangeOnOtherBands`), or none.
    ///
    /// Contests whose exchange changes from QSO to QSO (serial numbers; zLog's
    /// `SameExchange` off, as in CQ WPX or NYP) return `None`.
    fn exchange_from(&self, band: Band, past: &Qso) -> Option<String> {
        let _ = band;
        Some(past.nr_rcvd.clone())
    }

    /// Whether a received number is well formed (`ValidMulti`); checked before logging.
    fn valid_number(&self, q: &Qso) -> bool;

    /// Score the whole log: one [`QsoScore`] per QSO, in log order, and the sheet.
    fn score(&mut self, log: &Log) -> Scoring;

    /// Status-line information for a number being typed (`CheckMulti`).
    fn check_multi(&self, q: &Qso) -> MultiCheck {
        let _ = q;
        MultiCheck::Invalid
    }

    /// All multipliers in display order with the bands they were worked on.
    fn multi_table(&self) -> Vec<(String, String, Vec<Band>)> {
        Vec::new()
    }
}

/// What scoring decided about one QSO; these are the only fields a contest sets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QsoScore {
    pub dupe: bool,
    pub points: u8,
    pub multi1: String,
    pub new_multi1: bool,
    pub multi2: String,
    pub new_multi2: bool,
    pub multi3: String,
    pub new_multi3: bool,
}

/// The result of [`Contest::score`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scoring {
    pub qsos: Vec<QsoScore>,
    pub sheet: ScoreSheet,
}

/// Score the log and write the result into the scoring fields of its QSOs.
///
/// A result that does not have one entry per QSO is rejected and the log is left
/// as it was.
pub fn rescore(log: &mut Log, contest: &mut dyn Contest) -> Result<ScoreSheet, String> {
    let scoring = contest.score(log);
    if scoring.qsos.len() != log.qsos.len() {
        return Err(format!(
            "{} scored {} QSOs of {}",
            contest.name(),
            scoring.qsos.len(),
            log.qsos.len()
        ));
    }
    for (q, s) in log.qsos.iter_mut().zip(scoring.qsos) {
        q.dupe = s.dupe;
        q.points = s.points;
        q.multi1 = s.multi1;
        q.new_multi1 = s.new_multi1;
        q.multi2 = s.multi2;
        q.new_multi2 = s.new_multi2;
        q.multi3 = s.multi3;
        q.new_multi3 = s.new_multi3;
    }
    Ok(scoring.sheet)
}

/// One row of the score window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BandScore {
    pub qsos: u32,
    pub cw_qsos: u32,
    pub points: u32,
    pub multi: u32,
}

/// What the score window shows; filled in by the contest.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScoreSheet {
    pub bands: BTreeMap<Band, BandScore>,
    pub total: BandScore,
    pub score: u64,
    /// Coefficient and the score multiplied by it, for contests that use one.
    pub with_coeff: Option<(f64, f64)>,
}
