use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    calculate_cutoff, Async, FixedAsync, Resampler as _, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};

pub fn chunk(from: u32) -> usize {
    (from as usize / 400).max(16)
}

const SINC_LEN: usize = 64;

fn sinc(from: u32, to: u32, chunk: usize) -> Option<Async<f32>> {
    let window = WindowFunction::BlackmanHarris2;
    let params = SincInterpolationParameters {
        sinc_len: SINC_LEN,
        f_cutoff: Some(calculate_cutoff::<f32>(SINC_LEN, window)),
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window,
    };
    let ratio = f64::from(to) / f64::from(from);
    Async::new_sinc(ratio, 1.0, &params, chunk, 1, FixedAsync::Input).ok()
}

pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || to == 0 || samples.is_empty() {
        return samples.to_vec();
    }
    let Some(mut r) = sinc(from, to, 1024) else {
        return samples.to_vec();
    };
    let input = InterleavedSlice::new(samples, 1, samples.len());
    let mut out = vec![0.0; r.process_all_needed_output_len(samples.len())];
    let n = out.len();
    let mut output = InterleavedSlice::new_mut(&mut out, 1, n);
    match (input, output.as_mut()) {
        (Ok(input), Ok(output)) => {
            match r.process_all_into_buffer(&input, output, samples.len(), None) {
                Ok((_, written)) => {
                    out.truncate(written);
                    out
                }
                Err(_) => samples.to_vec(),
            }
        }
        _ => samples.to_vec(),
    }
}

pub struct Resampler {
    sinc: Option<Async<f32>>,
    ratio: f64,
    pending: Vec<f32>,
    block: Vec<f32>,
    fed: usize,
    made: usize,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let sinc = (from != to && from > 0 && to > 0)
            .then(|| sinc(from, to, chunk(from)))
            .flatten();
        let block = vec![0.0; sinc.as_ref().map_or(0, |s| s.output_frames_max())];
        Resampler {
            sinc,
            ratio: f64::from(to) / f64::from(from.max(1)),
            pending: Vec::new(),
            block,
            fed: 0,
            made: 0,
        }
    }

    pub fn delay(&self) -> usize {
        self.sinc.as_ref().map_or(0, |s| s.output_delay())
    }

    pub fn push(&mut self, x: &[f32], out: &mut Vec<f32>) {
        self.fed += x.len();
        let before = out.len();
        self.run(x, out, usize::MAX);
        self.made += out.len() - before;
    }

    pub fn finish(&mut self, out: &mut Vec<f32>) {
        if self.sinc.is_some() {
            let want = self.delay() + (self.fed as f64 * self.ratio).round() as usize;
            let zeros = vec![0.0; self.block.len().max(16)];
            while self.made < want {
                let before = out.len();
                self.run(&zeros, out, want - self.made);
                self.made += out.len() - before;
            }
            if let Some(s) = self.sinc.as_mut() {
                s.reset();
            }
        }
        self.pending.clear();
        self.fed = 0;
        self.made = 0;
    }

    fn run(&mut self, x: &[f32], out: &mut Vec<f32>, most: usize) {
        let Some(sinc) = self.sinc.as_mut() else {
            out.extend_from_slice(x);
            return;
        };
        let start = out.len();
        self.pending.extend_from_slice(x);
        let mut used = 0;
        while self.pending.len() - used >= sinc.input_frames_next() {
            let n = sinc.input_frames_next();
            let frames = self.block.len();
            let input = InterleavedSlice::new(&self.pending[used..used + n], 1, n);
            let output = InterleavedSlice::new_mut(&mut self.block, 1, frames);
            let (Ok(input), Ok(mut output)) = (input, output) else {
                break;
            };
            match sinc.process_into_buffer(&input, &mut output, None) {
                Ok((read, written)) => {
                    used += read;
                    let room = most.saturating_sub(out.len() - start);
                    out.extend_from_slice(&self.block[..written.min(room)]);
                }
                Err(_) => break,
            }
        }
        self.pending.drain(..used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin() as f32 * 0.5)
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn a_whole_buffer_keeps_its_length_and_a_tone_in_band() {
        for (from, to) in [(8000, 16000), (48000, 8000), (44100, 48000)] {
            let x = tone(1000.0, from, from as usize / 2);
            let y = resample(&x, from, to);
            let want = x.len() * to as usize / from as usize;
            assert!(y.len().abs_diff(want) <= 2, "{from} {to} {}", y.len());
            let mid = &y[y.len() / 4..y.len() * 3 / 4];
            assert!((rms(mid) - 0.354).abs() < 0.02, "{from} {to} {}", rms(mid));
        }
        assert_eq!(resample(&[0.1, 0.2], 0, 48000), [0.1, 0.2]);
        assert_eq!(resample(&[0.1, 0.2], 8000, 8000), [0.1, 0.2]);
    }

    #[test]
    fn downsampling_removes_what_the_new_rate_cannot_hold() {
        let x = tone(6000.0, 48000, 24000);
        let y = resample(&x, 48000, 8000);
        let mid = &y[y.len() / 4..y.len() * 3 / 4];
        assert!(
            rms(mid) < 0.01,
            "a 6 kHz tone aliases at 8 kHz: {}",
            rms(mid)
        );
    }

    #[test]
    fn streaming_block_by_block_matches_one_push_and_has_low_delay() {
        let x = tone(700.0, 48000, 4800);
        for (from, to) in [(48000, 8000), (44100, 48000), (8000, 48000)] {
            let mut whole = Vec::new();
            Resampler::new(from, to).push(&x, &mut whole);
            let mut parts = Vec::new();
            let mut r = Resampler::new(from, to);
            for c in x.chunks(37) {
                r.push(c, &mut parts);
            }
            assert_eq!(whole, parts, "{from} to {to}");
            let ms = r.delay() as f64 * 1000.0 / f64::from(to)
                + chunk(from) as f64 * 1000.0 / f64::from(from);
            assert!(ms < 10.0, "{from} to {to}: {ms} ms");
        }
        let mut same = Vec::new();
        Resampler::new(8000, 8000).push(&[0.5], &mut same);
        assert_eq!(same, [0.5]);
    }

    #[test]
    fn finishing_a_stream_flushes_the_tail() {
        let x = tone(700.0, 48000, 4801);
        for (from, to) in [(48000, 8000), (44100, 48000), (8000, 48000)] {
            let mut r = Resampler::new(from, to);
            let mut out = Vec::new();
            r.push(&x, &mut out);
            r.finish(&mut out);
            let want =
                r.delay() + (x.len() as f64 * f64::from(to) / f64::from(from)).round() as usize;
            assert_eq!(out.len(), want, "{from} to {to}");
            let mut again = Vec::new();
            r.push(&x, &mut again);
            r.finish(&mut again);
            assert_eq!(again, out, "{from} to {to}");
        }
        let mut same = Resampler::new(8000, 8000);
        let mut out = Vec::new();
        same.push(&[0.5], &mut out);
        same.finish(&mut out);
        assert_eq!(out, [0.5]);
    }

    #[test]
    fn ten_seconds_of_input_cost_little_cpu() {
        for (from, to) in [(48000, 8000), (44100, 48000), (8000, 48000), (48000, 16000)] {
            let x = tone(700.0, from, from as usize * 10);
            let mut r = Resampler::new(from, to);
            let mut out = Vec::new();
            let t = std::time::Instant::now();
            for c in x.chunks(from as usize / 100) {
                r.push(c, &mut out);
            }
            let streamed = t.elapsed();
            let t = std::time::Instant::now();
            let whole = resample(&x, from, to);
            let one_shot = t.elapsed();
            let latency = r.delay() as f64 * 1000.0 / f64::from(to)
                + chunk(from) as f64 * 1000.0 / f64::from(from);
            eprintln!(
                "{from} to {to}: streaming {:.3} % of a core, latency {latency:.1} ms, 10 s one-shot {one_shot:?} ({} samples)",
                streamed.as_secs_f64() * 10.0,
                whole.len()
            );
            if !cfg!(debug_assertions) {
                assert!(
                    streamed < std::time::Duration::from_millis(200),
                    "{streamed:?}"
                );
                assert!(
                    one_shot < std::time::Duration::from_millis(500),
                    "{one_shot:?}"
                );
            }
        }
    }
}
