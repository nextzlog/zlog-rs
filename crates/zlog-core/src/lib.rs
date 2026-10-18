//! Core of zLog, independent of any contest and of the user interface.
//!
//! - [`types`]: bands, modes, powers.
//! - [`qso`]: one contact.
//! - [`zlo`]: ZLO/ZLOX record encoding.
//! - [`log`]: the log with its header and dupe check.
//! - [`book`]: the only place that changes QSOs; runs commands and delivers events.
//! - [`contest`]: the interface every contest implements (the core has none of its own).
//! - [`text`]: user-interface text by key, from each crate's `lang/` files.
//! - [`extension`]: what extension crates contribute, and the registry that holds them.

pub mod book;
pub mod contest;
pub mod extension;
pub mod log;
pub mod qso;
pub mod text;
pub mod types;
pub mod zlo;

pub use book::{Book, BookError, Command, QsoId};
pub use contest::{
    rescore, BandScore, Contest, ContestDef, MultiCheck, QsoScore, ScoreSheet, Scoring, Station,
};
pub use extension::{Context, Extension, Link, LogEvent, LogFormat, Observer, Origin, Registry};
pub use log::{DupeRule, Log};
pub use qso::{DelphiTime, Qso};
pub use types::{Band, ContestMode, Mode, Power};
pub use zlo::Format;
