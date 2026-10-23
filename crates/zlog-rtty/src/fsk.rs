//! The FSK demodulator (MMTTY does this for zLog): each tone is mixed down and
//! averaged over one bit, a matched filter; the larger envelope gives the bit, and
//! an asynchronous receiver finds the start bit and samples the middle of each bit.

use std::collections::VecDeque;

use crate::baudot::Decoder;
use crate::modem::{Demodulator, Params};

/// The envelope of one tone: the signal mixed down by the tone and averaged over
/// the last bit.
struct Tone {
    step: f64,
    phase: f64,
    window: VecDeque<(f64, f64)>,
    sum: (f64, f64),
    len: usize,
}

impl Tone {
    fn new(hz: f64, rate: u32, len: usize) -> Self {
        Tone {
            step: std::f64::consts::TAU * hz / rate as f64,
            phase: 0.0,
            window: VecDeque::with_capacity(len + 1),
            sum: (0.0, 0.0),
            len: len.max(1),
        }
    }

    fn push(&mut self, x: f32) -> f64 {
        let (s, c) = self.phase.sin_cos();
        self.phase = (self.phase + self.step) % std::f64::consts::TAU;
        let v = (x as f64 * c, -(x as f64) * s);
        self.window.push_back(v);
        self.sum.0 += v.0;
        self.sum.1 += v.1;
        if self.window.len() > self.len {
            let old = self.window.pop_front().unwrap_or_default();
            self.sum.0 -= old.0;
            self.sum.1 -= old.1;
        }
        self.sum.0.hypot(self.sum.1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// Waiting for mark lasting half a bit (idle or a stop bit).
    Mark(usize),
    /// Mark seen; waiting for the fall to space of a start bit.
    Ready,
    /// In a character whose start bit fell at sample `start`; `bit` is the next
    /// bit to sample (0 start, 1 to 5 data, 6 stop).
    Char { start: f64, bit: u32, code: u8 },
}

/// The RTTY demodulator of one signal.
pub struct Fsk {
    mark: Tone,
    space: Tone,
    bit_len: f64,
    /// Samples taken so far.
    clock: usize,
    state: State,
    /// Sum of the clarity of the bits of the character being read.
    clarity: f64,
    /// Characters whose bits are less clear, on average, are taken for noise.
    pub squelch: f64,
    /// Clarity of the last character, 0 for noise to 1 for a clean signal.
    pub level: f64,
    decoder: Decoder,
}

/// The default squelch: noise alone averages about 0.4.
pub const SQUELCH: f64 = 0.55;

impl Fsk {
    pub fn new(params: Params, rate: u32) -> Self {
        let bit_len = params.bit_len(rate);
        let len = bit_len.round() as usize;
        Fsk {
            mark: Tone::new(params.mark, rate, len),
            space: Tone::new(params.space, rate, len),
            bit_len,
            clock: 0,
            state: State::Mark(0),
            clarity: 0.0,
            squelch: SQUELCH,
            level: 0.0,
            decoder: Decoder::new(params.uos),
        }
    }

    /// A sample in; the code of a character when one has just been read.
    fn push(&mut self, x: f32) -> Option<u8> {
        let m = self.mark.push(x);
        let s = self.space.push(x);
        let mark = m >= s;
        let clarity = (m - s).abs() / (m + s).max(1e-9);
        let now = self.clock as f64;
        self.clock += 1;
        match self.state {
            State::Mark(n) => {
                let n = if mark { n + 1 } else { 0 };
                self.state = if n as f64 >= self.bit_len / 2.0 {
                    State::Ready
                } else {
                    State::Mark(n)
                };
            }
            // The average of a bit crosses over half a bit into it.
            State::Ready if !mark => {
                self.state = State::Char {
                    start: now - self.bit_len / 2.0,
                    bit: 0,
                    code: 0,
                };
                self.clarity = 0.0;
            }
            State::Ready => {}
            // The average is that of bit `bit` alone at its end.
            State::Char { start, bit, code } if now >= start + (bit + 1) as f64 * self.bit_len => {
                self.clarity += clarity;
                match bit {
                    // A false start: mark where the start bit should be.
                    0 if mark => self.state = State::Ready,
                    0..=5 => {
                        let code = match bit {
                            0 => code,
                            _ => code | (mark as u8) << (bit - 1),
                        };
                        self.state = State::Char {
                            start,
                            bit: bit + 1,
                            code,
                        };
                    }
                    // The stop bit: mark, with clear enough bits, ends a character.
                    _ => {
                        let level = self.clarity / 7.0;
                        if mark && level >= self.squelch {
                            self.level = level;
                            self.state = State::Ready;
                            return Some(code);
                        }
                        self.state = State::Mark(0);
                    }
                }
            }
            State::Char { .. } => {}
        }
        None
    }
}

impl Demodulator for Fsk {
    fn demodulate(&mut self, samples: &[f32]) -> String {
        let mut out = String::new();
        for &x in samples {
            if let Some(code) = self.push(x) {
                out.extend(self.decoder.decode(code));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::afsk::Afsk;
    use crate::modem::Modulator;

    /// Gaussian noise from a fixed seed (xorshift and Box-Muller).
    struct Noise(u64);

    impl Noise {
        fn uniform(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }

        fn gauss(&mut self) -> f64 {
            let (a, b) = (self.uniform(), self.uniform());
            (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
        }
    }

    /// `text` sent, with white noise at `snr_db` in a 3 kHz band, decoded in blocks.
    fn through_noise(text: &str, p: Params, rate: u32, snr_db: f64, seed: u64) -> String {
        let mut signal = Afsk::new(p, rate).modulate(text);
        let mut noise = Noise(seed);
        // The tones have power A^2 / 2; the noise in 3 kHz of the band up to rate / 2.
        let power = 0.25 / 2.0;
        let sigma = (power / 10f64.powf(snr_db / 10.0) * rate as f64 / 2.0 / 3000.0).sqrt();
        for x in signal.iter_mut() {
            *x += (sigma * noise.gauss()) as f32;
        }
        let mut fsk = Fsk::new(p, rate);
        signal.chunks(512).map(|b| fsk.demodulate(b)).collect()
    }

    /// Characters of `want` not found in place in `got` (an edit distance).
    fn errors(want: &str, got: &str) -> usize {
        let (a, b): (Vec<char>, Vec<char>) = (want.chars().collect(), got.chars().collect());
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for i in 1..=a.len() {
            let mut prev = row[0];
            row[0] = i;
            for j in 1..=b.len() {
                let keep = prev + (a[i - 1] != b[j - 1]) as usize;
                prev = row[j];
                row[j] = keep.min(row[j] + 1).min(row[j - 1] + 1);
            }
        }
        row[b.len()]
    }

    const TEXT: &str = "CQ TEST JA1ZLO JA1ZLO TEST 599-13M 599 100110M RYRYRY $1.5 (A/B)?\r\n";

    #[test]
    fn clean_audio_round_trips_at_common_rates_and_speeds() {
        for rate in [8000, 11025, 44100, 48000] {
            for baud in [45.45, 50.0, 75.0] {
                let p = Params {
                    baud,
                    ..Params::default()
                };
                assert_eq!(through_noise(TEXT, p, rate, 60.0, 1), TEXT, "{rate} {baud}");
            }
        }
    }

    #[test]
    fn reversed_and_other_tones() {
        let reversed = Params {
            mark: 2295.0,
            space: 2125.0,
            ..Params::default()
        };
        assert_eq!(through_noise(TEXT, reversed, 48000, 60.0, 2), TEXT);
        let low = Params {
            mark: 1275.0,
            space: 1445.0,
            ..Params::default()
        };
        assert_eq!(through_noise(TEXT, low, 48000, 60.0, 3), TEXT);
    }

    #[test]
    fn a_wrong_shift_or_reversal_does_not_decode() {
        let sent = Params::default();
        let mut signal = Afsk::new(sent, 48000).modulate(TEXT);
        signal.extend(Afsk::new(sent, 48000).modulate(TEXT));
        let reversed = Params {
            mark: sent.space,
            space: sent.mark,
            ..sent
        };
        let got = Fsk::new(reversed, 48000).demodulate(&signal);
        assert!(errors(TEXT, &got) > TEXT.len() / 2, "{got:?}");
    }

    #[test]
    fn noise_costs_few_characters_at_moderate_snr() {
        let text = TEXT.repeat(3);
        let mut report = Vec::new();
        // The most errors in each of three runs; below 0 dB only reported.
        let limits = [(20.0, 0), (10.0, 0), (6.0, 0), (3.0, 2), (0.0, 10)];
        let report_only = [(-3.0, usize::MAX), (-6.0, usize::MAX)];
        for (snr, most) in limits.into_iter().chain(report_only) {
            let bad: usize = (0..3)
                .map(|seed| {
                    let got = through_noise(&text, Params::default(), 48000, snr, 7 + seed);
                    errors(&text, &got)
                })
                .sum();
            report.push((snr, bad));
            assert!(bad <= most.saturating_mul(3), "{report:?}");
        }
        eprintln!(
            "character errors in 3 x {} at SNR (dB, 3 kHz): {report:?}",
            text.len()
        );
    }

    #[test]
    fn noise_alone_prints_little() {
        let mut noise = Noise(99);
        let samples: Vec<f32> = (0..48000 * 10)
            .map(|_| (0.1 * noise.gauss()) as f32)
            .collect();
        let got = Fsk::new(Params::default(), 48000).demodulate(&samples);
        // Ten seconds hold over 60 characters.
        assert!(got.len() <= 3, "{got:?}");
    }

    #[test]
    fn decoding_starts_in_the_middle_of_a_transmission() {
        let signal = Afsk::new(Params::default(), 48000).modulate(TEXT);
        let got = Fsk::new(Params::default(), 48000).demodulate(&signal[signal.len() / 3..]);
        let tail = &TEXT[TEXT.len() / 2..];
        assert!(got.ends_with(tail), "{got:?}");
    }
}
