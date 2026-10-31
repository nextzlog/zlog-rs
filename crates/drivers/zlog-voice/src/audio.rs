use std::path::Path;
use std::time::{Duration, Instant};

use zlog_ui::device::AudioIo;

use crate::Player;

pub fn read_wav(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let all: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    if spec.sample_rate == 0 {
        return Err("the WAV file gives no sample rate".into());
    }
    let mono = all
        .chunks(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}

pub use zlog_audio::resample;

#[derive(Default)]
pub struct AudioPlayer {
    pub io: Option<Box<dyn AudioIo>>,
    ends: Option<Instant>,
}

const TAIL_MS: usize = 150;

const END_MARGIN: Duration = Duration::from_secs(2);

impl Player for AudioPlayer {
    fn play(&mut self, path: &Path) -> Result<(), String> {
        self.stop();
        let (samples, rate) = read_wav(path)?;
        let io = self.io.as_mut().ok_or("no sound device")?;
        let out_rate = io.sample_rate();
        let mut samples = resample(&samples, rate, out_rate);
        samples.extend(std::iter::repeat_n(0.0, out_rate as usize * TAIL_MS / 1000));
        self.ends = Some(
            Instant::now()
                + Duration::from_secs_f64(samples.len() as f64 / out_rate.max(1) as f64)
                + END_MARGIN,
        );
        io.play(&samples);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(io) = self.io.as_mut() {
            io.stop();
        }
        self.ends = None;
    }

    fn playing(&self) -> bool {
        self.ends.is_some_and(|t| Instant::now() < t)
            && self.io.as_ref().is_some_and(|io| io.playing())
    }
}

pub fn save_recording(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    if samples.is_empty() {
        return Err(format!("{}: nothing was recorded", path.display()));
    }
    let staged = zlog_core::file::stage(path, |f| {
        wav_into(std::io::BufWriter::new(f), samples, rate).map_err(std::io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    if path.exists() {
        let _ = std::fs::rename(path, path.with_extension("wav.bak"));
    }
    staged
        .persist(path)
        .map(drop)
        .map_err(|e| e.error.to_string())
}

pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    wav_into(std::io::BufWriter::new(file), samples, rate)
}

fn wav_into<W: std::io::Write + std::io::Seek>(
    out: W,
    samples: &[f32],
    rate: u32,
) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::new(out, spec).map_err(|e| e.to_string())?;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32_768.0) as i16)
            .map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip_and_resampling() {
        let dir = std::env::temp_dir().join(format!("zlog-voice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cq.wav");
        write_wav(&path, &[0.0, 0.5, -0.5, 1.0], 8000).unwrap();
        let (s, rate) = read_wav(&path).unwrap();
        assert_eq!(rate, 8000);
        assert_eq!(s.len(), 4);
        assert!((s[1] - 0.5).abs() < 1e-3 && (s[2] + 0.5).abs() < 1e-3);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(resample(&[0.25; 800], 8000, 16000).len(), 1600);
    }

    #[test]
    fn an_empty_recording_keeps_the_memory_and_a_new_one_keeps_the_old() {
        let dir = std::env::temp_dir().join(format!("zlog-voice-rec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("F1.wav");
        write_wav(&path, &[0.5; 10], 8000).unwrap();
        assert!(save_recording(&path, &[], 8000).is_err());
        assert_eq!(read_wav(&path).unwrap().0.len(), 10);
        save_recording(&path, &[0.1; 20], 8000).unwrap();
        assert_eq!(read_wav(&path).unwrap().0.len(), 20);
        assert_eq!(read_wav(&dir.join("F1.wav.bak")).unwrap().0.len(), 10);
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            2,
            "no temporary is left"
        );
        assert!(save_recording(&dir.join("none").join("F2.wav"), &[0.1], 8000).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_memory_plays_through_the_device() {
        #[derive(Default)]
        struct Dev {
            played: Vec<f32>,
            stopped: bool,
        }
        impl AudioIo for Dev {
            fn sample_rate(&self) -> u32 {
                16000
            }
            fn play(&mut self, samples: &[f32]) {
                self.stopped = false;
                self.played.extend_from_slice(samples);
            }
            fn stop(&mut self) {
                self.stopped = true;
            }
            fn playing(&self) -> bool {
                !self.stopped
            }
            fn record(&mut self, _out: &mut Vec<f32>) {}
        }
        let dir = std::env::temp_dir().join(format!("zlog-voice-play-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("F1.wav");
        write_wav(&path, &[0.5; 80], 8000).unwrap();
        let mut p = AudioPlayer::default();
        assert!(p.play(&path).is_err(), "no device");
        p.io = Some(Box::new(Dev::default()));
        p.play(&path).unwrap();
        assert!(p.playing());
        p.stop();
        assert!(!p.playing());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_rate_of_zero_does_not_divide() {
        assert_eq!(resample(&[0.1, 0.2], 0, 48000), [0.1, 0.2]);
        assert_eq!(resample(&[0.1, 0.2], 8000, 0), [0.1, 0.2]);
    }
}
