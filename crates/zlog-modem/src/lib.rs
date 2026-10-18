//! The modem family: everything that turns a mode's signals into text and back.
//!
//! A decoder ([`Demodulator`]) takes audio and reports, per signal heard, the text
//! read so far, what it is still unsure of, and the words it has recognised
//! (callsigns, exchanges, procedure words). An encoder ([`Modulator`]) sends text.
//! CW is the first member; RTTY and FT8 fit the same traits.
//!
//! Decoders never touch the log or the entry row. A window that shows a decoder may
//! ask the application to put a recognised word into the entry row.

use zlog_core::Mode;

/// What a word is, as far as the decoder can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordKind {
    Callsign,
    /// Signal report, serial number or contest exchange.
    Exchange,
    /// CQ, DE, TU, 73 and the like.
    Procedure,
    Other,
}

/// A word the decoder has closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub kind: WordKind,
    /// Every character was read with confidence.
    pub sure: bool,
}

/// One signal being decoded (a station on its own frequency).
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub id: u32,
    /// Audio frequency in hertz.
    pub freq: f64,
    /// Text read with confidence, never revised.
    pub text: String,
    /// Text not decided yet, which may still change.
    pub pending: String,
    /// Recognised words, oldest first.
    pub words: Vec<Word>,
    /// Still heard.
    pub alive: bool,
}

/// Decodes a mode from audio.
pub trait Demodulator: Send {
    fn mode(&self) -> Mode;

    /// Sample rate the decoder was made for.
    fn sample_rate(&self) -> u32;

    /// Feed mono samples (-1.0 to 1.0) at [`Demodulator::sample_rate`].
    fn push(&mut self, samples: &[f32]);

    /// The signals heard, alive or recently lost, ordered by frequency.
    fn signals(&self) -> Vec<Signal>;

    /// Forget all signals and start over.
    fn reset(&mut self);
}

/// Sends a mode (a CW keyer, an FSK generator).
pub trait Modulator: Send {
    fn mode(&self) -> Mode;

    /// Queue text to send.
    fn send(&mut self, text: &str);

    /// Stop sending at once and drop what is queued.
    fn abort(&mut self);

    fn busy(&self) -> bool;
}
