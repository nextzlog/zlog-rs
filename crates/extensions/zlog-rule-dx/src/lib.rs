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
