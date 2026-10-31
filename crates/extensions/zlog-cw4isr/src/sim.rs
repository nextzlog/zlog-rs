use std::path::Path;

use zlog_keyer::morse::{keying, Timing};

pub const RATE: u32 = 8000;
pub const FIRST_HZ: f64 = 600.0;
pub const STEP_HZ: f64 = 350.0;
pub const WPM: u32 = 25;
pub const DEFAULT_TEXT: &str = "CQ TEST JA1ZLO JA1ZLO TEST\nJH1ABC 5NN 13M TU";

pub fn station(text: &str, hz: f64, wpm: u32, rate: u32, seed: u64) -> Vec<f32> {
    let timing = Timing {
        wpm,
        ..Timing::default()
    };
    let mut steps = keying(&text.to_uppercase(), &timing);
    let lead = std::time::Duration::from_millis(500);
    steps.insert(
        0,
        zlog_keyer::morse::Step {
            key: false,
            len: lead,
        },
    );
    steps.push(zlog_keyer::morse::Step {
        key: false,
        len: lead,
    });
    let edge = 0.005 * f64::from(rate);
    let mut out = Vec::new();
    let mut seed = seed.max(1);
    for s in steps {
        let n = (s.len.as_secs_f64() * f64::from(rate)) as usize;
        for i in 0..n {
            let ramp = ((i.min(n - i) as f64) / edge).min(1.0);
            let amp = if s.key { 0.4 * ramp } else { 0.0 };
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let noise = ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 0.02;
            let t = out.len() as f64 / f64::from(rate);
            out.push((amp * (2.0 * std::f64::consts::PI * hz * t).sin() + noise) as f32);
        }
    }
    out
}

pub fn signal(text: &str, rate: u32) -> Vec<f32> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let parts: Vec<Vec<f32>> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let hz = FIRST_HZ + STEP_HZ * i as f64;
            station(l.trim(), hz, WPM + 3 * i as u32, rate, i as u64 + 1)
        })
        .collect();
    let n = parts.iter().map(Vec::len).max().unwrap_or(0);
    (0..n)
        .map(|i| parts.iter().filter_map(|p| p.get(i)).sum::<f32>())
        .map(|x| x.clamp(-1.0, 1.0))
        .collect()
}

pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for s in samples {
        w.write_sample((s * 32767.0) as i16)?;
    }
    w.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulated_stations_are_decoded_apart() {
        let audio = signal(DEFAULT_TEXT, RATE);
        assert_eq!(audio, signal(DEFAULT_TEXT, RATE), "deterministic");
        let stations = cw4i_ui::decode(&audio, RATE);
        let list = stations.sorted();
        let texts: Vec<String> = list.iter().map(|s| s.text()).collect();
        let a = list.iter().find(|s| s.text().contains("JA1ZLO"));
        let b = list.iter().find(|s| s.text().contains("JH1ABC"));
        let (a, b) = (
            a.unwrap_or_else(|| panic!("{texts:?}")),
            b.unwrap_or_else(|| panic!("{texts:?}")),
        );
        assert!((a.freq - FIRST_HZ).abs() < 20.0, "{}", a.freq);
        assert!((b.freq - (FIRST_HZ + STEP_HZ)).abs() < 20.0, "{}", b.freq);
    }

    #[test]
    fn the_wav_reads_back() {
        let path = std::env::temp_dir().join(format!("zlog-cw4isr-sim-{}.wav", std::process::id()));
        let audio = signal("E", RATE);
        write_wav(&path, &audio, RATE).unwrap();
        let (back, rate) = cw4i_ui::audio::read_wav(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!((back.len(), rate), (audio.len(), RATE));
        assert!(back.iter().zip(&audio).all(|(x, y)| (x - y).abs() < 1e-3));
        assert!(signal("\n \n", RATE).is_empty());
    }
}
