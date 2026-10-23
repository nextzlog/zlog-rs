//! Sound devices through `cpal`: the receiver's audio in, and the AFSK audio out to
//! the transmitter.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};

use crate::sender::Sink;

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

/// Audio coming in from an input device, the first channel of each frame.
pub struct Input {
    _stream: cpal::Stream,
    rx: Receiver<Vec<f32>>,
    pub rate: u32,
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

impl Input {
    /// Start taking audio from the input device named `name` (the default one when
    /// empty).
    pub fn start(name: &str) -> Result<Self, String> {
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
        Ok(Input {
            _stream: stream,
            rx,
            rate,
        })
    }

    /// The samples that came in since the last call.
    pub fn take(&self) -> Vec<f32> {
        self.rx.try_iter().flatten().collect()
    }
}

/// Plays the AFSK audio on an output device; the stream lives while it plays.
#[derive(Default)]
pub struct Output {
    /// Output device by name, empty for the default one.
    pub device: String,
    stream: Option<cpal::Stream>,
    done: Arc<AtomicBool>,
    /// When the audio has surely ended, whatever the device does.
    ends: Option<Instant>,
}

/// How long past its length the audio may play before it is taken for ended.
const END_MARGIN: Duration = Duration::from_secs(2);

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
        // A device that fails (unplugged) never asks for more: the text ends, so
        // PTT is not left on.
        move |e| {
            eprintln!("audio output: {e}");
            failed.store(true, Ordering::SeqCst);
        },
        None,
    )
}

impl Sink for Output {
    fn rate(&mut self) -> Result<u32, String> {
        let device = device(false, &self.device)?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        Ok(config.sample_rate())
    }

    fn play(&mut self, samples: Vec<f32>) -> Result<(), String> {
        self.stop();
        let device = device(false, &self.device)?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let rate = config.sample_rate().max(1);
        self.ends = Some(
            Instant::now()
                + Duration::from_secs_f64(samples.len() as f64 / rate as f64)
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
    /// that stops calling back) the audio's length has long passed.
    fn playing(&self) -> bool {
        self.stream.is_some()
            && !self.done.load(Ordering::SeqCst)
            && self.ends.is_none_or(|t| Instant::now() < t)
    }
}
