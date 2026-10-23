//! Sound devices through `cpal` (`UzLogSound`'s `TWaveSound`): a WAV file played on
//! an output device, and a recording from an input device into a WAV file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};

use crate::Player;

/// Names of the output devices (`false`) or input devices (`true`) present.
pub fn devices(input: bool) -> Vec<String> {
    let host = cpal::default_host();
    let list = if input {
        host.input_devices()
    } else {
        host.output_devices()
    };
    list.map(|ds| {
        ds.filter_map(|d| d.description().ok().map(|x| x.name().to_string()))
            .collect()
    })
    .unwrap_or_default()
}

/// The device named `name`, or the default one when `name` is empty.
fn device(input: bool, name: &str) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if name.is_empty() {
        let d = if input {
            host.default_input_device()
        } else {
            host.default_output_device()
        };
        return d.ok_or_else(|| "no sound device".into());
    }
    let mut list = if input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .map_err(|e| e.to_string())?;
    list.find(|d| d.description().map(|x| x.name() == name).unwrap_or(false))
        .ok_or_else(|| format!("no sound device {name}"))
}

/// Mono samples in [-1, 1] and the sample rate of a WAV file.
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

/// `samples` at rate `from` taken to rate `to` by linear interpolation.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || to == 0 || samples.is_empty() {
        return samples.to_vec();
    }
    let len = (samples.len() as u64 * to as u64 / from as u64) as usize;
    let step = from as f64 / to as f64;
    (0..len)
        .map(|i| {
            let x = i as f64 * step;
            let j = x as usize;
            let a = samples[j.min(samples.len() - 1)];
            let b = samples[(j + 1).min(samples.len() - 1)];
            a + (b - a) * (x - j as f64) as f32
        })
        .collect()
}

/// Plays WAV files on an output device; the stream lives while the file plays.
#[derive(Default)]
pub struct DevicePlayer {
    /// Output device by name, empty for the default one.
    pub device: String,
    stream: Option<cpal::Stream>,
    done: Arc<AtomicBool>,
    /// When the memory has surely ended, whatever the device does.
    ends: Option<std::time::Instant>,
}

/// Silence played after a memory, in milliseconds.
const TAIL_MS: usize = 150;

/// How long past its length a memory may play before it is taken for ended.
const END_MARGIN: std::time::Duration = std::time::Duration::from_secs(2);

fn output_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    samples: Vec<f32>,
    done: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels.max(1) as usize;
    let mut pos = 0;
    let failed = done.clone();
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &_| {
            for frame in data.chunks_mut(channels) {
                let s = samples.get(pos).copied().unwrap_or(0.0);
                pos += 1;
                frame.fill(T::from_sample(s));
            }
            if pos >= samples.len() {
                done.store(true, Ordering::SeqCst);
            }
        },
        // A device that fails (unplugged) never asks for more: the memory ends,
        // so PTT is not left on.
        move |e| {
            eprintln!("audio output: {e}");
            failed.store(true, Ordering::SeqCst);
        },
        None,
    )
}

impl Player for DevicePlayer {
    fn play(&mut self, path: &Path) -> Result<(), String> {
        self.stop();
        let (samples, rate) = read_wav(path)?;
        let device = device(false, &self.device)?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let out_rate = config.sample_rate();
        let mut samples = resample(&samples, rate, out_rate);
        // Silence after the memory, so its end is heard before PTT goes off (the
        // last samples are still in the device's buffer when all are written).
        samples.extend(std::iter::repeat_n(0.0, out_rate as usize * TAIL_MS / 1000));
        self.ends = Some(
            std::time::Instant::now()
                + std::time::Duration::from_secs_f64(samples.len() as f64 / out_rate.max(1) as f64)
                + END_MARGIN,
        );
        let done = Arc::new(AtomicBool::new(false));
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let stream = match format {
            cpal::SampleFormat::F32 => output_stream::<f32>(&device, config, samples, done.clone()),
            cpal::SampleFormat::I16 => output_stream::<i16>(&device, config, samples, done.clone()),
            cpal::SampleFormat::I32 => output_stream::<i32>(&device, config, samples, done.clone()),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        self.done = done;
        Ok(())
    }

    fn stop(&mut self) {
        self.stream = None;
        self.ends = None;
    }

    /// Playing until every sample was written, the device failed, or (a device
    /// that stops calling back) the memory's length has long passed.
    fn playing(&self) -> bool {
        self.stream.is_some()
            && !self.done.load(Ordering::SeqCst)
            && self.ends.is_none_or(|t| std::time::Instant::now() < t)
    }
}

/// A recording from an input device, written to its WAV file when finished.
pub struct Recording {
    _stream: cpal::Stream,
    rx: Receiver<Vec<f32>>,
    rate: u32,
    pub path: PathBuf,
}

fn input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    tx: std::sync::mpsc::Sender<Vec<f32>>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    device.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            let _ = tx.send(
                data.chunks(channels)
                    .map(|f| f32::from_sample(f[0]))
                    .collect(),
            );
        },
        |e| eprintln!("audio input: {e}"),
        None,
    )
}

impl Recording {
    /// Start recording the input device named `name` (the default one when empty)
    /// into `path`.
    pub fn start(name: &str, path: PathBuf) -> Result<Self, String> {
        let device = device(true, name)?;
        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let rate = config.sample_rate();
        let (tx, rx) = channel();
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let stream = match format {
            cpal::SampleFormat::F32 => input_stream::<f32>(&device, config, tx),
            cpal::SampleFormat::I16 => input_stream::<i16>(&device, config, tx),
            cpal::SampleFormat::I32 => input_stream::<i32>(&device, config, tx),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Recording {
            _stream: stream,
            rx,
            rate,
            path,
        })
    }

    /// Stop and write what was recorded as 16-bit mono WAV.
    /// Nothing recorded leaves the memory as it was; a memory recorded over is
    /// kept as `name.wav.bak`.
    pub fn finish(self) -> Result<PathBuf, String> {
        let Recording { rx, rate, path, .. } = self;
        let samples: Vec<f32> = rx.try_iter().flatten().collect();
        save_recording(&path, &samples, rate)?;
        Ok(path)
    }
}

/// Write a recording to `path`: nothing recorded is an error and leaves the file
/// as it was; a file recorded over is kept as `name.wav.bak`.
pub fn save_recording(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    if samples.is_empty() {
        return Err(format!("{}: nothing was recorded", path.display()));
    }
    let part = path.with_extension("wav.part");
    write_wav(&part, samples, rate)?;
    if path.exists() {
        let _ = std::fs::rename(path, path.with_extension("wav.bak"));
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}

/// Write mono samples in [-1, 1] at `rate` as a 16-bit WAV file.
pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for s in samples {
        w.write_sample(i16::from_sample(s.clamp(-1.0, 1.0)))
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
        assert_eq!(resample(&[0.0, 1.0], 8000, 16000), vec![0.0, 0.5, 1.0, 1.0]);
        assert_eq!(resample(&[0.0, 1.0, 2.0, 3.0], 16000, 8000), vec![0.0, 2.0]);
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
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_rate_of_zero_does_not_divide() {
        assert_eq!(resample(&[0.1, 0.2], 0, 48000), [0.1, 0.2]);
        assert_eq!(resample(&[0.1, 0.2], 8000, 0), [0.1, 0.2]);
    }
}
