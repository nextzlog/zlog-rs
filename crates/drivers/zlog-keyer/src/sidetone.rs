use zlog_ui::Section;

use crate::morse::{keying, Timing};

pub const ON: &str = "sidetone";
pub const PITCH: &str = "sidetone_pitch";
pub const VOLUME: &str = "sidetone_volume";

pub const MIN_PITCH: u32 = 400;
pub const MAX_PITCH: u32 = 1000;
pub const MIN_VOLUME: i32 = -40;
pub const MAX_VOLUME: i32 = -6;
pub const RAMP_MS: f64 = 6.0;
pub const FADE_MS: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sidetone {
    pub on: bool,
    pub pitch: u32,
    pub volume: i32,
}

impl Default for Sidetone {
    fn default() -> Self {
        Sidetone {
            on: true,
            pitch: 600,
            volume: -20,
        }
    }
}

impl Sidetone {
    pub fn load(s: &Section) -> Self {
        let d = Sidetone::default();
        Sidetone {
            on: crate::driver::get(s, ON, d.on),
            pitch: crate::driver::get(s, PITCH, d.pitch).clamp(MIN_PITCH, MAX_PITCH),
            volume: crate::driver::get(s, VOLUME, d.volume).clamp(MIN_VOLUME, MAX_VOLUME),
        }
    }

    pub fn save(&self, s: &mut Section) {
        s.insert(ON.into(), self.on.to_string());
        s.insert(PITCH.into(), self.pitch.to_string());
        s.insert(VOLUME.into(), self.volume.to_string());
    }

    pub fn amplitude(&self) -> f32 {
        10f32.powf(self.volume as f32 / 20.0)
    }

    pub fn samples(&self, text: &str, timing: &Timing, rate: u32) -> Vec<f32> {
        let rate = rate.max(1) as f64;
        let amp = self.amplitude() as f64;
        let step = std::f64::consts::TAU * self.pitch as f64 / rate;
        let ramp = (RAMP_MS / 1000.0 * rate).max(1.0);
        let mut out = Vec::new();
        let mut at = 0.0f64;
        let mut phase = 0.0f64;
        for s in keying(text, timing) {
            at += s.len.as_secs_f64() * rate;
            let n = (at.round() as usize).saturating_sub(out.len());
            if !s.key {
                out.resize(out.len() + n, 0.0);
                continue;
            }
            let r = ramp.min(n as f64 / 2.0).max(1.0);
            for i in 0..n {
                let edge = (i as f64 + 0.5).min(n as f64 - i as f64 - 0.5);
                let g = if edge >= r {
                    1.0
                } else {
                    0.5 - 0.5 * (std::f64::consts::PI * edge / r).cos()
                };
                out.push((amp * g * phase.sin()) as f32);
                phase = (phase + step) % std::f64::consts::TAU;
            }
        }
        out
    }
}

pub const NO_DEVICE: &str = "sidetone.no_device";
pub const FAILED: &str = "sidetone.failed";

pub fn failed(raw: &str) -> String {
    format!("{FAILED}\t{raw}")
}

pub fn explain(language: &str, raw: &str) -> Option<String> {
    let (key, detail) = raw.split_once('\t').unwrap_or((raw, ""));
    [NO_DEVICE, FAILED]
        .contains(&key)
        .then(|| crate::texts().format(language, key, &[&detail]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morse;

    const RATE: u32 = 48_000;

    fn runs(x: &[f32]) -> Vec<(bool, usize)> {
        let ramp = (RAMP_MS / 1000.0 * RATE as f64) as usize;
        let mut runs: Vec<(bool, usize)> = Vec::new();
        let mut silent = 0;
        for &v in x {
            silent = if v == 0.0 { silent + 1 } else { 0 };
            let key = silent < ramp / 4;
            match runs.last_mut() {
                Some((k, n)) if *k == key => *n += 1,
                _ => runs.push((key, 1)),
            }
        }
        runs
    }

    #[test]
    fn the_envelope_has_no_clicks_and_the_peak_is_the_level() {
        for pitch in [MIN_PITCH, 600, MAX_PITCH] {
            for rate in [8000, 44_100, 48_000] {
                let t = Sidetone {
                    pitch,
                    ..Sidetone::default()
                };
                let x = t.samples("CQ TEST E I 5NN", &Timing::default(), rate);
                let a = t.amplitude();
                let sine = a * std::f32::consts::TAU * pitch as f32 / rate as f32;
                let max_step = x
                    .windows(2)
                    .map(|w| (w[1] - w[0]).abs())
                    .fold(0.0, f32::max);
                assert!(
                    max_step <= sine * 1.05,
                    "{pitch} Hz {rate}: {max_step} > {sine}"
                );
                let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(peak <= a && peak > a * 0.99, "{peak} vs {a}");
                assert!(x[0].abs() < a * 0.01 && x.last().unwrap().abs() < a * 0.01);
                let dc = x.iter().map(|&v| v as f64).sum::<f64>() / x.len() as f64;
                assert!(dc.abs() < 1e-3 * a as f64, "dc {dc}");
            }
        }
        let a = Sidetone::default().amplitude();
        assert!((20.0 * a.log10() + 20.0).abs() < 1e-4);
    }

    #[test]
    fn element_durations_follow_the_wpm_and_weight() {
        let t = Sidetone::default();
        for (wpm, weight) in [(20, 50), (25, 50), (40, 50), (25, 70)] {
            let timing = Timing {
                wpm,
                weight,
                ..Timing::default()
            };
            let x = t.samples("TE", &timing, RATE);
            let ms = |d: std::time::Duration| d.as_secs_f64() * RATE as f64;
            let r = runs(&x);
            let total: f64 = morse::total(&keying("TE", &timing)).as_secs_f64() * RATE as f64;
            assert!((x.len() as f64 - total).abs() <= 1.0);
            assert!(r[0].0 && (r[0].1 as f64 - ms(timing.dash())).abs() < RATE as f64 * 0.003);
            assert!(r[2].0 && (r[2].1 as f64 - ms(timing.dot())).abs() < RATE as f64 * 0.003);
            let gap = r[1].1 as f64;
            assert!((gap - ms(timing.char_gap())).abs() < RATE as f64 * 0.003);
        }
    }

    #[test]
    fn messages_are_explained() {
        assert_eq!(explain("en", "port busy"), None);
        let m = explain("en", &failed("no sound device")).unwrap();
        assert!(
            m.contains("(no sound device)") && m.contains("Options > Sound"),
            "{m}"
        );
        assert!(explain("ja", NO_DEVICE)
            .unwrap()
            .contains("オプション > 音声"));
    }

    #[test]
    fn settings_round_trip_and_clamp() {
        let mut s = Section::new();
        assert_eq!(Sidetone::load(&s), Sidetone::default());
        let t = Sidetone {
            on: false,
            pitch: 750,
            volume: -30,
        };
        t.save(&mut s);
        assert_eq!(Sidetone::load(&s), t);
        s.insert(PITCH.into(), "5000".into());
        s.insert(VOLUME.into(), "6".into());
        let l = Sidetone::load(&s);
        assert_eq!((l.pitch, l.volume), (MAX_PITCH, MAX_VOLUME));
    }
}
