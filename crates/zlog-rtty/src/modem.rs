//! The modem interfaces of RTTY and its signal: tones, speed and shift handling.

/// Turns audio into text, a block of samples at a time.
pub trait Demodulator {
    /// The text decoded from `samples` (mono, in [-1, 1]) and those before them.
    fn demodulate(&mut self, samples: &[f32]) -> String;
}

/// Turns text into audio.
pub trait Modulator {
    /// The samples (mono, in [-1, 1]) that send `text`.
    fn modulate(&mut self, text: &str) -> Vec<f32>;
}

/// The RTTY signal: zLog's defaults are mark 2125 Hz, space 2295 Hz (170 Hz shift)
/// and 45.45 baud, one start bit, five data bits and 1.5 stop bits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub mark: f64,
    pub space: f64,
    pub baud: f64,
    /// Unshift on space (`use_txuos`), on both receive and transmit.
    pub uos: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            mark: 2125.0,
            space: 2295.0,
            baud: 45.45,
            uos: true,
        }
    }
}

impl Params {
    /// Samples a bit lasts at `rate`.
    pub fn bit_len(&self, rate: u32) -> f64 {
        rate as f64 / self.baud.max(1.0)
    }
}
