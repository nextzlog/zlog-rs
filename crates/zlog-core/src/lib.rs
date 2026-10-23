//! Core of zLog, independent of any contest and of the user interface.
//!
//! - [`types`]: bands, modes, powers.
//! - [`qso`]: one contact.
//! - [`zlo`]: ZLO/ZLOX record encoding.
//! - [`log`]: the log with its header and dupe check.
//! - [`book`]: the only place that changes QSOs; runs commands and delivers events.
//! - [`contest`]: the interface every contest implements (the core has none of its own).
//! - [`text`]: user-interface text by key, from each crate's `lang/` files.
//! - [`step`]: what one action of the operator changed, and batches run only on a
//!   log as expected.
//! - [`extension`]: what extension crates contribute, and the registry that holds them.

pub mod book;
pub mod contest;
pub mod extension;
pub mod log;
pub mod qso;
pub mod step;
pub mod text;
pub mod types;
pub mod zlo;

pub use book::{Book, BookError, Command, Polled, QsoId};
pub use contest::{
    rescore, BandScore, Contest, ContestDef, MultiCheck, QsoScore, ScoreSheet, Scoring, Station,
};
pub use extension::{
    ContestFile, Context, Extension, Link, LogEvent, LogFormat, Observer, OpenedContest, Origin,
    Registry,
};
pub use log::{DupeRule, Log, QsyRule};
pub use qso::{DelphiTime, Qso};
pub use step::{Batch, BatchError, Change, Mismatch, Step, IMPORT};
pub use types::{Band, ContestMode, Mode, Power};
pub use zlo::Format;
