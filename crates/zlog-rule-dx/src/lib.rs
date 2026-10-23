//! International contests of zLog (`UzLogContest.pas` and the `U*Multi.pas`,
//! `U*Score.pas` units): CQ WW, CQ WPX, IARU HF, ARRL DX (both sides), WAEDC, IOTA,
//! ARRL 10 m, All Asian DX, JIDX (both sides), AP Sprint, BARTG HF RTTY,
//! JARL World Wide RTTY and the DXpedition mode.
//!
//! All share one engine, [`WorldRules`] in [`world`]; each contest's rules are in a
//! file of their own, named after the contest's id ([`WorldContest`]).
//! Countries come from the user's `CTY.DAT` ([`cty`]); fixed lists from zLog's
//! `ARDX.DAT`, `ARRL10.DAT` and `IOTA.DAT` ([`lists`]), from zLog's `zlog/resource`
//! at commit 6e309daf (IOTA.DAT converted to UTF-8).
//! CTY.DAT is not embedded: its terms do not state that it may be redistributed.
//!
//! The own station (country, continent, zones) is taken from the log header's
//! callsign, as zLog's `AnalyzeMyCountry` takes it from the settings; the ITU zone
//! and IOTA reference the operator sends are read back from the sent numbers.

pub mod cty;
pub mod lists;
pub mod world;

mod allasian;
mod apsprint;
mod arrl10;
mod arrldx;
mod bartg;
mod cqwpx;
mod cqww;
mod iaru;
mod iota;
mod jarlrtty;
mod jidx;
mod pedi;
mod waedc;

pub use world::{WorldContest, WorldRules};

use zlog_core::{ContestDef, Extension};

zlog_core::texts!();

/// The extension that adds the international contests.
pub struct World;

impl Extension for World {
    fn name(&self) -> &str {
        "World contests"
    }

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        WorldContest::ALL
            .into_iter()
            .map(|c| Box::new(c) as Box<dyn ContestDef>)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::texts;
    use crate::world::tests::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
        assert_eq!(Cqww.caption("en"), "CQ WW");
        assert_eq!(Cqww.group("en"), "World");
        assert!(WorldContest::ALL
            .iter()
            .all(|c| c.uses_utc() == (*c != Pedi)));
    }
}
