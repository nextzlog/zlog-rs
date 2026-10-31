use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::resample::Resampler;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use zlog_ui::device::{AudioDrv, AudioIo, DeviceDriver, PortHub, SettingField, SettingKind};
use zlog_ui::Section;

use crate::texts;

static NO_DEVICES: AtomicBool = AtomicBool::new(false);

pub fn use_no_devices(off: bool) {
    NO_DEVICES.store(off, Ordering::SeqCst);
}

struct Listed {
    inputs: Vec<String>,
    outputs: Vec<String>,
    at: Option<std::time::Instant>,
    busy: bool,
}

static LISTED: Mutex<Listed> = Mutex::new(Listed {
    inputs: Vec::new(),
    outputs: Vec::new(),
    at: None,
    busy: false,
});

pub fn devices(input: bool) -> Vec<String> {
    if NO_DEVICES.load(Ordering::SeqCst) {
        return Vec::new();
    }
    cached(&LISTED, || (list(true), list(false)), input)
}

fn cached(
    listed: &'static Mutex<Listed>,
    scan: fn() -> (Vec<String>, Vec<String>),
    input: bool,
) -> Vec<String> {
    let mut l = listed.lock().unwrap_or_else(|e| e.into_inner());
    let fresh =
        l.at.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5));
    if !fresh && !l.busy {
        l.busy = true;
        thread::spawn(move || {
            let (inputs, outputs) = scan();
            let mut l = listed.lock().unwrap_or_else(|e| e.into_inner());
            (l.inputs, l.outputs) = (inputs, outputs);
            l.at = Some(std::time::Instant::now());
            l.busy = false;
        });
    }
    if input { &l.inputs } else { &l.outputs }.clone()
}

fn list(input: bool) -> Vec<String> {
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

fn device(input: bool, name: &str) -> Result<cpal::Device, String> {
    if NO_DEVICES.load(Ordering::SeqCst) {
        return Err("no sound device".into());
    }
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

#[derive(Debug, Clone)]
enum Cmd {
    Input,
}

struct Sound {
    rate: u32,
    out: Arc<Mutex<VecDeque<f32>>>,
    failed: Arc<AtomicBool>,
    input: Arc<Mutex<Vec<f32>>>,
    in_rate: Arc<AtomicU32>,
    resampler: Option<Resampler>,
    commands: Sender<Cmd>,
    input_started: bool,
}

fn output_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    queue: Arc<Mutex<VecDeque<f32>>>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels.max(1) as usize;
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &_| {
            let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
            for frame in data.chunks_mut(channels) {
                frame.fill(T::from_sample(q.pop_front().unwrap_or(0.0)));
            }
        },
        move |e| {
            eprintln!("audio output: {e}");
            failed.store(true, Ordering::SeqCst);
        },
        None,
    )
}

fn input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    input: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    device.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            let mut i = input.lock().unwrap_or_else(|e| e.into_inner());
            i.extend(data.chunks(channels).map(|f| f32::from_sample(f[0])));
        },
        |e| eprintln!("audio input: {e}"),
        None,
    )
}

fn start_output(
    name: &str,
    queue: Arc<Mutex<VecDeque<f32>>>,
    failed: Arc<AtomicBool>,
) -> Result<(cpal::Stream, u32), String> {
    let device = device(false, name)?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate();
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let stream = match format {
        cpal::SampleFormat::F32 => output_stream::<f32>(&device, config, queue, failed),
        cpal::SampleFormat::I16 => output_stream::<i16>(&device, config, queue, failed),
        cpal::SampleFormat::I32 => output_stream::<i32>(&device, config, queue, failed),
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}

fn start_input(name: &str, input: Arc<Mutex<Vec<f32>>>) -> Result<(cpal::Stream, u32), String> {
    let device = device(true, name)?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate();
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let stream = match format {
        cpal::SampleFormat::F32 => input_stream::<f32>(&device, config, input),
        cpal::SampleFormat::I16 => input_stream::<i16>(&device, config, input),
        cpal::SampleFormat::I32 => input_stream::<i32>(&device, config, input),
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}

impl Sound {
    fn open(output: String, input: String) -> Result<Self, String> {
        let out = Arc::new(Mutex::new(VecDeque::new()));
        let failed = Arc::new(AtomicBool::new(false));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let in_rate = Arc::new(AtomicU32::new(0));
        let (commands, rx): (Sender<Cmd>, Receiver<Cmd>) = channel();
        let (opened, wait) = channel();
        {
            let (out, failed, recorded, in_rate) = (
                out.clone(),
                failed.clone(),
                recorded.clone(),
                in_rate.clone(),
            );
            thread::Builder::new()
                .name("zlog-audio".into())
                .spawn(move || {
                    let _output = match start_output(&output, out, failed.clone()) {
                        Ok((s, rate)) => {
                            let _ = opened.send(Ok(rate));
                            s
                        }
                        Err(e) => {
                            let _ = opened.send(Err(e));
                            return;
                        }
                    };
                    let mut _input = None;
                    while let Ok(Cmd::Input) = rx.recv() {
                        match start_input(&input, recorded.clone()) {
                            Ok((s, rate)) => {
                                in_rate.store(rate, Ordering::SeqCst);
                                _input = Some(s);
                            }
                            Err(e) => eprintln!("audio input: {e}"),
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        let rate = wait.recv().map_err(|e| e.to_string())??;
        Ok(Sound {
            rate,
            out,
            failed,
            input: recorded,
            in_rate,
            resampler: None,
            commands,
            input_started: false,
        })
    }
}

impl AudioIo for Sound {
    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn play(&mut self, samples: &[f32]) {
        if !self.failed.load(Ordering::SeqCst) {
            self.out
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend(samples);
        }
    }

    fn stop(&mut self) {
        self.out.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn fade_out(&mut self, samples: usize) {
        fade(
            &mut self.out.lock().unwrap_or_else(|e| e.into_inner()),
            samples,
        );
    }

    fn playing(&self) -> bool {
        !self.failed.load(Ordering::SeqCst)
            && !self
                .out
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
    }

    fn record(&mut self, out: &mut Vec<f32>) {
        if !self.input_started {
            self.input_started = true;
            let _ = self.commands.send(Cmd::Input);
            return;
        }
        let from = self.in_rate.load(Ordering::SeqCst);
        if from == 0 {
            return;
        }
        let block = std::mem::take(&mut *self.input.lock().unwrap_or_else(|e| e.into_inner()));
        let rate = self.rate;
        self.resampler
            .get_or_insert_with(|| Resampler::new(from, rate))
            .push(&block, out);
    }
}

pub fn fade(queue: &mut VecDeque<f32>, samples: usize) {
    queue.truncate(samples);
    let n = queue.len() as f32;
    for (i, v) in queue.iter_mut().enumerate() {
        *v *= 0.5 + 0.5 * (std::f32::consts::PI * (i as f32 + 1.0) / n).cos();
    }
}

#[derive(Default)]
pub struct SoundDrv;

fn device_choice(language: &str, names: Vec<String>) -> SettingKind {
    let mut list = vec![(
        String::new(),
        texts().get(language, "default_device").into(),
    )];
    list.extend(names.into_iter().map(|n| (n.clone(), n)));
    SettingKind::Choice(list)
}

impl DeviceDriver for SoundDrv {
    fn id(&self) -> &str {
        "sound"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "sound").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        let t = |k| texts().get(language, k).to_string();
        vec![
            SettingField::new(
                "output",
                t("output"),
                device_choice(language, devices(false)),
            ),
            SettingField::new("input", t("input"), device_choice(language, devices(true))),
        ]
    }
}

impl AudioDrv for SoundDrv {
    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn AudioIo>, String> {
        let name = |k: &str| settings.get(k).cloned().unwrap_or_default();
        Ok(Box::new(Sound::open(name("output"), name("input"))?))
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn devices_are_listed_off_the_calling_thread() {
        static SCANNED: Mutex<Vec<thread::ThreadId>> = Mutex::new(Vec::new());
        static FAKE: Mutex<Listed> = Mutex::new(Listed {
            inputs: Vec::new(),
            outputs: Vec::new(),
            at: None,
            busy: false,
        });
        fn scan() -> (Vec<String>, Vec<String>) {
            SCANNED.lock().unwrap().push(thread::current().id());
            (vec!["mic".into()], vec!["speaker".into()])
        }
        assert!(cached(&FAKE, scan, true).is_empty(), "returns at once");
        let start = std::time::Instant::now();
        while cached(&FAKE, scan, true).is_empty() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(cached(&FAKE, scan, false), ["speaker"]);
        let scanned = SCANNED.lock().unwrap();
        assert_eq!(scanned.len(), 1);
        assert_ne!(scanned[0], thread::current().id());
    }
    use super::*;

    #[test]
    fn device_fields_keep_the_old_keys() {
        let keys: Vec<_> = SoundDrv.settings("en").iter().map(|f| f.key).collect();
        assert_eq!(keys, ["output", "input"]);
        assert!(!SoundDrv.simulated());
    }
}
