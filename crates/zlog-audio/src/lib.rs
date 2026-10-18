//! Audio input from the computer's sound devices (Core Audio on macOS, WASAPI on
//! Windows), through the `cpal` crate.
//!
//! An [`Input`] captures one device and hands mono samples to the caller, who takes
//! them each frame without waiting.

use std::sync::mpsc::{channel, Receiver};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Names of the input devices present.
pub fn devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|ds| {
            ds.filter_map(|d| d.description().ok().map(|x| x.name().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// A device being captured; dropping it stops the capture.
pub struct Input {
    _stream: cpal::Stream,
    rx: Receiver<Vec<f32>>,
    pub sample_rate: u32,
    pub device: String,
}

impl Input {
    /// Start capturing the named device, or the default one when `name` is `None`.
    pub fn open(name: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = match name {
            Some(n) => host
                .input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.description().map(|x| x.name() == n).unwrap_or(false))
                .ok_or_else(|| format!("no input device {n}"))?,
            None => host.default_input_device().ok_or("no input device")?,
        };
        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let channels = config.channels() as usize;
        let sample_rate = config.sample_rate();
        let (tx, rx) = channel();
        let err = |e| eprintln!("audio input: {e}");
        let format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config,
                move |data: &[f32], _: &_| {
                    let _ = tx.send(data.chunks(channels).map(|f| f[0]).collect());
                },
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config,
                move |data: &[i16], _: &_| {
                    let _ = tx.send(
                        data.chunks(channels)
                            .map(|f| f[0] as f32 / 32768.0)
                            .collect(),
                    );
                },
                err,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        let device = device
            .description()
            .map(|x| x.name().to_string())
            .unwrap_or_default();
        Ok(Input {
            _stream: stream,
            rx,
            sample_rate,
            device,
        })
    }

    /// Samples captured since the last call.
    pub fn take(&self) -> Vec<f32> {
        self.rx.try_iter().flatten().collect()
    }
}
