use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::device::{AudioDrv, AudioIo, DeviceDriver, PortHub, SettingField, SettingKind};
use crate::Section;

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Loop,
    Tone,
    Silence,
    Samples(Arc<[f32]>),
}

enum Opening {
    Ready(Source),
    Loading(Wav),
}

pub type Generator = Arc<dyn Fn(&str, u32) -> Vec<f32> + Send + Sync>;

#[derive(Clone)]
pub struct TextSource {
    pub id: &'static str,
    pub label: fn(&str) -> String,
    pub generate: Generator,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Heard {
    pub rate: u32,
    pub source: String,
    pub played: usize,
    pub peak: f32,
    pub loading: bool,
}

#[derive(Clone, Default)]
pub struct LoopbackDrv {
    pub played: Arc<Mutex<Vec<f32>>>,
    pub feed: Arc<Mutex<VecDeque<f32>>>,
    pub texts: Vec<TextSource>,
    heard: Arc<Mutex<Option<Heard>>>,
    opened: Arc<AtomicU64>,
}

type Wav = hound::WavReader<std::io::BufReader<std::fs::File>>;

fn open_wav(path: &std::path::Path) -> Result<Wav, String> {
    hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read_wav(path: &std::path::Path, rate: u32) -> Result<Vec<f32>, String> {
    Ok(decode_wav(open_wav(path)?, rate))
}

fn decode_wav(mut r: Wav, rate: u32) -> Vec<f32> {
    let spec = r.spec();
    let channels = usize::from(spec.channels.max(1));
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / scale)
                .collect()
        }
    };
    let mono: Vec<f32> = samples
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect();
    resample(&mono, spec.sample_rate.max(1), rate.max(1))
}

fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    let ratio = f64::from(from) / f64::from(to);
    let scale = ratio.max(1.0);
    let half = 8.0 * scale;
    let n = (input.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let centre = i as f64 * ratio;
            let first = (centre - half).ceil().max(0.0) as usize;
            let last = ((centre + half).floor() as usize).min(input.len() - 1);
            let (mut sum, mut weights) = (0.0, 0.0);
            for (k, s) in input.iter().enumerate().take(last + 1).skip(first) {
                let x = (k as f64 - centre) / scale;
                let sinc = match x {
                    0.0 => 1.0,
                    _ => (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x),
                };
                let window = 0.5 + 0.5 * (std::f64::consts::PI * (k as f64 - centre) / half).cos();
                let w = sinc * window;
                sum += w * f64::from(*s);
                weights += w;
            }
            match weights {
                0.0 => 0.0,
                _ => (sum / weights) as f32,
            }
        })
        .collect()
}

const KEPT_SECONDS: usize = 10;

impl LoopbackDrv {
    pub fn with_texts(texts: Vec<TextSource>) -> Self {
        LoopbackDrv {
            texts,
            ..LoopbackDrv::default()
        }
    }

    pub fn heard(&self) -> Option<Heard> {
        self.lock_heard().clone()
    }

    fn lock_heard(&self) -> std::sync::MutexGuard<'_, Option<Heard>> {
        self.heard.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn load(&self, wav: Wav, rate: u32) -> Receiver<Arc<[f32]>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let opened = self.opened.load(Ordering::SeqCst);
        let (drv, heard) = (self.opened.clone(), self.heard.clone());
        std::thread::spawn(move || {
            let samples: Arc<[f32]> = decode_wav(wav, rate).into();
            let _ = tx.send(samples);
            if drv.load(Ordering::SeqCst) == opened {
                if let Some(h) = heard.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    h.loading = false;
                }
            }
        });
        rx
    }

    fn source(&self, settings: &Section, rate: u32) -> Result<Opening, String> {
        let key = settings.get("source").map_or("", |s| s.as_str());
        let text = settings.get("text").map_or("", |s| s.as_str());
        Ok(match key {
            "tone" => Opening::Ready(Source::Tone),
            "silence" => Opening::Ready(Source::Silence),
            "file" => {
                let path = settings.get("file").map_or("", |s| s.as_str());
                Opening::Loading(open_wav(std::path::Path::new(path))?)
            }
            k => Opening::Ready(match self.texts.iter().find(|t| t.id == k) {
                Some(t) => Source::Samples((t.generate)(text, rate).into()),
                None => Source::Loop,
            }),
        })
    }
}

pub struct Loopback {
    rate: u32,
    source: Source,
    pending: Option<Receiver<Arc<[f32]>>>,
    played: Arc<Mutex<Vec<f32>>>,
    feed: Arc<Mutex<VecDeque<f32>>>,
    heard: Arc<Mutex<Option<Heard>>>,
    echo: VecDeque<f32>,
    until: Option<Instant>,
    read: Instant,
    phase: f64,
    at: usize,
}

impl Loopback {
    pub fn new(rate: u32, source: Source) -> Self {
        Loopback {
            rate: rate.max(1),
            source,
            pending: None,
            played: Arc::default(),
            feed: Arc::default(),
            heard: Arc::default(),
            echo: VecDeque::new(),
            until: None,
            read: Instant::now(),
            phase: 0.0,
            at: 0,
        }
    }

    fn input(&mut self, n: usize, out: &mut Vec<f32>) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(samples) => {
                    self.source = Source::Samples(samples);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => self.pending = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut feed = self.feed.lock().unwrap_or_else(|e| e.into_inner());
        for _ in 0..n {
            let s = match feed.pop_front() {
                Some(s) => s,
                None => match self.source {
                    Source::Loop => self.echo.pop_front().unwrap_or(0.0),
                    Source::Tone => {
                        self.phase = (self.phase + 1000.0 / f64::from(self.rate)).fract();
                        0.5 * (2.0 * std::f64::consts::PI * self.phase).sin() as f32
                    }
                    Source::Silence => 0.0,
                    Source::Samples(ref all) if all.is_empty() => 0.0,
                    Source::Samples(ref all) => {
                        let s = all[self.at % all.len()];
                        self.at = (self.at + 1) % all.len();
                        s
                    }
                },
            };
            out.push(s);
        }
    }
}

impl AudioIo for Loopback {
    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn play(&mut self, samples: &[f32]) {
        let mut played = self.played.lock().unwrap_or_else(|e| e.into_inner());
        played.extend_from_slice(samples);
        let keep = KEPT_SECONDS * self.rate as usize;
        if played.len() > keep {
            let cut = played.len() - keep;
            played.drain(..cut);
        }
        drop(played);
        if let Some(h) = self
            .heard
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            h.played += samples.len();
            h.peak = samples.iter().fold(h.peak, |m, s| m.max(s.abs()));
        }
        if matches!(self.source, Source::Loop) {
            self.echo.extend(samples);
            let keep = KEPT_SECONDS * self.rate as usize;
            if self.echo.len() > keep {
                let cut = self.echo.len() - keep;
                self.echo.drain(..cut);
            }
        }
        let len = Duration::from_secs_f64(samples.len() as f64 / self.rate as f64);
        let now = Instant::now();
        let from = self.until.filter(|t| *t > now).unwrap_or(now);
        self.until = Some(from + len);
    }

    fn stop(&mut self) {
        self.until = None;
        self.echo.clear();
    }

    fn playing(&self) -> bool {
        self.until.is_some_and(|t| t > Instant::now())
    }

    fn record(&mut self, out: &mut Vec<f32>) {
        let now = Instant::now();
        let n = (now.duration_since(self.read).as_secs_f64() * self.rate as f64) as usize;
        if n == 0 {
            return;
        }
        self.read += Duration::from_secs_f64(n as f64 / self.rate as f64);
        self.input(n.min(self.rate as usize), out);
    }
}

impl DeviceDriver for LoopbackDrv {
    fn id(&self) -> &str {
        "loopback"
    }

    fn title(&self, language: &str) -> String {
        crate::texts().get(language, "loopback").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        let t = |k: &str| {
            crate::texts()
                .get(language, &format!("loopback.{k}"))
                .to_string()
        };
        let label = |k: &str| t(&format!("source.{k}"));
        let named = |ks: &[&str]| -> Vec<(String, String)> {
            ks.iter().map(|k| (k.to_string(), label(k))).collect()
        };
        let texts: Vec<(String, String)> = self
            .texts
            .iter()
            .map(|t| (t.id.to_string(), (t.label)(language)))
            .collect();
        let mut common = named(&["loop", "file"]);
        common.extend(texts.iter().cloned());
        let mut all = named(&["loop", "tone", "silence", "file"]);
        all.extend(texts);
        let wav = vec![(t("wav"), vec!["wav".to_string()])];
        vec![
            SettingField::new(
                "rate",
                t("rate"),
                SettingKind::Int {
                    min: 8000,
                    max: 96000,
                },
            )
            .default_value("48000")
            .advanced(true),
            SettingField::new("source", t("source"), SettingKind::Choice(common))
                .default_value("loop"),
            SettingField::new("file", t("file"), SettingKind::File { filters: wav }),
            SettingField::new("text", t("text"), SettingKind::Text).default_value("CQ TEST JA1ZLO"),
            SettingField::new("source", t("source.all"), SettingKind::Choice(all))
                .default_value("loop")
                .advanced(true),
        ]
    }

    fn simulated(&self) -> bool {
        true
    }

    fn status(&self, language: &str) -> Option<String> {
        let h = self.heard()?;
        let secs = h.played as f64 / f64::from(h.rate.max(1));
        let source = match h.loading {
            true => crate::texts().format(language, "loopback.loading", &[&h.source]),
            false => h.source,
        };
        Some(crate::texts().format(
            language,
            "loopback.status",
            &[
                &h.rate,
                &format!("{secs:.1}"),
                &format!("{:.2}", h.peak),
                &source,
            ],
        ))
    }
}

impl AudioDrv for LoopbackDrv {
    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn AudioIo>, String> {
        let rate = settings
            .get("rate")
            .and_then(|r| r.parse().ok())
            .unwrap_or(48000u32)
            .clamp(8000, 96000);
        let opening = self.source(settings, rate)?;
        self.opened.fetch_add(1, Ordering::SeqCst);
        let loading = matches!(opening, Opening::Loading(_));
        *self.lock_heard() = Some(Heard {
            rate,
            source: settings
                .get("source")
                .cloned()
                .unwrap_or_else(|| "loop".into()),
            loading,
            ..Heard::default()
        });
        let (source, pending) = match opening {
            Opening::Ready(source) => (source, None),
            Opening::Loading(wav) => (Source::Silence, Some(self.load(wav, rate))),
        };
        let mut l = Loopback::new(rate, source);
        l.pending = pending;
        l.played = self.played.clone();
        l.feed = self.feed.clone();
        l.heard = self.heard.clone();
        Ok(Box::new(l))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(io: &mut dyn AudioIo, want: usize) -> Vec<f32> {
        let mut got = Vec::new();
        let start = Instant::now();
        while got.len() < want {
            assert!(start.elapsed() < Duration::from_secs(3), "input too slow");
            std::thread::sleep(Duration::from_millis(5));
            io.record(&mut got);
        }
        got
    }

    #[test]
    fn what_is_played_is_kept_and_takes_its_time() {
        let drv = LoopbackDrv::default();
        assert!(drv.simulated());
        let mut s = Section::new();
        s.insert("rate".into(), "8000".into());
        s.insert("source".into(), "silence".into());
        let mut io = drv.open(&s, &PortHub::new()).unwrap();
        assert_eq!(io.sample_rate(), 8000);
        io.play(&[0.25; 400]);
        assert!(io.playing());
        io.play(&[0.5; 400]);
        std::thread::sleep(Duration::from_millis(60));
        assert!(io.playing(), "the second block follows the first");
        std::thread::sleep(Duration::from_millis(60));
        assert!(!io.playing());
        assert_eq!(drv.played.lock().unwrap().len(), 800);
        io.play(&[0.1; 8000]);
        io.stop();
        assert!(!io.playing());
        assert!(wait(io.as_mut(), 80).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn the_input_hears_what_was_played_after_what_is_fed() {
        let drv = LoopbackDrv::default();
        let mut s = Section::new();
        s.insert("rate".into(), "8000".into());
        let mut io = drv.open(&s, &PortHub::new()).unwrap();
        drv.feed.lock().unwrap().extend([0.9, -0.9]);
        io.play(&[0.25, 0.5, 0.75]);
        let got = wait(io.as_mut(), 8);
        assert_eq!(&got[..5], [0.9, -0.9, 0.25, 0.5, 0.75]);
        assert!(got[5..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn resampling_filters_out_what_the_new_rate_cannot_carry() {
        let high: Vec<f32> = (0..1600)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let low = resample(&high, 16000, 8000);
        assert_eq!(low.len(), 800);
        assert!(
            low[20..780].iter().all(|v| v.abs() < 0.05),
            "{:?}",
            &low[20..24]
        );
    }

    #[test]
    fn only_the_last_seconds_are_kept_and_stop_clears_the_echo() {
        let drv = LoopbackDrv::default();
        let mut s = Section::new();
        s.insert("rate".into(), "8000".into());
        let mut io = drv.open(&s, &PortHub::new()).unwrap();
        for _ in 0..12 {
            io.play(&[0.3; 8000]);
        }
        io.play(&[0.9]);
        assert_eq!(drv.played.lock().unwrap().len(), KEPT_SECONDS * 8000);
        let h = drv.heard().unwrap();
        assert_eq!(h.played, 12 * 8000 + 1);
        assert!((h.peak - 0.9).abs() < 1e-6);
        io.stop();
        assert!(wait(io.as_mut(), 80).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_long_pause_gives_at_most_one_second_and_the_tone_stays_clean() {
        let mut io = Loopback::new(8000, Source::Tone);
        io.read -= Duration::from_secs(3600);
        io.phase = 1.0 - 1000.0 / 8000.0;
        let mut got = Vec::new();
        io.record(&mut got);
        assert_eq!(got.len(), 8000);
        assert!(got[0].abs() < 1e-6, "{}", got[0]);
    }

    #[test]
    fn a_tone_as_input() {
        let mut io = Loopback::new(8000, Source::Tone);
        let got = wait(&mut io, 80);
        let peak = got.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((0.45..=0.5).contains(&peak), "{peak}");
        let mut keys: Vec<_> = LoopbackDrv::default()
            .settings("ja")
            .iter()
            .map(|f| f.key)
            .collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys, ["file", "rate", "source", "text"]);
    }

    #[test]
    fn a_wav_file_plays_as_the_other_station_and_loops() {
        let dir = std::env::temp_dir().join(format!("zlog-loopback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("other.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for s in [16384i16; 400] {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
        let drv = LoopbackDrv::default();
        let mut s = Section::new();
        s.insert("rate".into(), "8000".into());
        s.insert("source".into(), "file".into());
        s.insert("file".into(), path.display().to_string());
        let mut io = drv.open(&s, &PortHub::new()).unwrap();
        let start = Instant::now();
        while drv.heard().unwrap().loading {
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(1));
        }
        let got = wait(io.as_mut(), 4);
        assert!(
            got[..4].iter().all(|v| (*v - 0.5).abs() < 1e-3),
            "16 kHz read at 8 kHz: {:?}",
            &got[..4]
        );
        io.play(&[0.25; 800]);
        let st = drv.status("en").unwrap();
        assert!(
            st.contains("8000") && st.contains("0.1") && st.contains("0.25") && st.contains("file"),
            "{st}"
        );
        s.insert("file".into(), dir.join("none.wav").display().to_string());
        assert!(drv.open(&s, &PortHub::new()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_text_source_is_generated_by_its_modem() {
        let drv = LoopbackDrv::with_texts(vec![TextSource {
            id: "beep",
            label: |_| "Beep".into(),
            generate: Arc::new(|text, rate| vec![text.len() as f32 / 10.0; rate as usize / 1000]),
        }]);
        let kinds = &drv.settings("en")[1].kind;
        assert!(matches!(kinds, SettingKind::Choice(c) if c.last().unwrap().0 == "beep"));
        let mut s = Section::new();
        s.insert("rate".into(), "8000".into());
        s.insert("source".into(), "beep".into());
        s.insert("text".into(), "CQ".into());
        let mut io = drv.open(&s, &PortHub::new()).unwrap();
        assert!(wait(io.as_mut(), 8).iter().all(|v| (*v - 0.2).abs() < 1e-6));
    }

    #[test]
    fn a_long_wav_file_loads_in_the_background() {
        let dir = std::env::temp_dir().join(format!("zlog-loopback-long-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..44100 * 30 {
            w.write_sample((i % 100) as i16).unwrap();
        }
        w.finalize().unwrap();
        let drv = LoopbackDrv::default();
        let mut s = Section::new();
        s.insert("rate".into(), "48000".into());
        s.insert("source".into(), "file".into());
        s.insert("file".into(), path.display().to_string());
        let start = Instant::now();
        let mut io = drv.open(&s, &PortHub::new()).unwrap();
        assert!(start.elapsed() < Duration::from_millis(200));
        if drv.heard().unwrap().loading {
            let mut got = Vec::new();
            io.record(&mut got);
            assert!(got.iter().all(|v| *v == 0.0));
        }
        let start = Instant::now();
        while drv.heard().unwrap().loading {
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(io);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_echo_never_holds_more_than_the_kept_seconds() {
        let mut io = Loopback::new(8000, Source::Loop);
        for _ in 0..30 {
            io.play(&[0.3; 8000]);
        }
        assert_eq!(io.echo.len(), KEPT_SECONDS * 8000);
    }

    type Dialog = egui_kittest::Harness<'static, crate::device::DriverChoice>;

    fn dialog(section: Section) -> Dialog {
        let mut choice = crate::device::DriverChoice::new("loopback");
        choice.sections.insert("loopback".into(), section);
        let drv = LoopbackDrv::with_texts(vec![TextSource {
            id: "cw",
            label: |_| "相手局の CW".into(),
            generate: Arc::new(|_, _| Vec::new()),
        }]);
        let mut h = egui_kittest::Harness::builder()
            .with_size([420.0, 600.0])
            .with_accessibility_check(false)
            .build_ui_state(
                move |ui, c: &mut crate::device::DriverChoice| {
                    ui.ctx()
                        .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                    c.ui(ui, "ja", "入力", "なし", &[&drv]);
                },
                choice,
            );
        h.run_steps(3);
        h
    }

    fn texts_of(h: &Dialog) -> Vec<String> {
        use egui_kittest::kittest::NodeT;
        h.root()
            .children_recursive()
            .filter_map(|n| {
                let a = n.accesskit_node();
                a.label().or(a.value())
            })
            .collect()
    }

    #[test]
    fn the_dialog_is_plain_with_details_folded() {
        use egui_kittest::kittest::{NodeT, Queryable};
        let mut s = Section::new();
        s.insert("source".into(), "tone".into());
        s.insert("file".into(), "/somewhere/deep/other.wav".into());
        s.insert("rate".into(), "8000".into());
        s.insert("text".into(), "CQ".into());
        let mut h = dialog(s.clone());
        let shown = texts_of(&h);
        assert!(shown.iter().any(|l| l == "シミュレーター"), "{shown:?}");
        assert!(
            shown.iter().any(|l| l == "1 kHz の音"),
            "uncommon value named: {shown:?}"
        );
        assert!(shown.iter().any(|l| l == "other.wav"), "{shown:?}");
        assert!(!shown.iter().any(|l| l.contains("/somewhere")), "{shown:?}");
        assert!(
            !shown.iter().any(|l| l.contains("サンプリング")),
            "{shown:?}"
        );
        let inputs: Vec<String> = h
            .query_all_by_role(egui::accesskit::Role::TextInput)
            .filter_map(|n| n.accesskit_node().value())
            .collect();
        assert_eq!(inputs, ["CQ"], "the file is a button, not a text box");
        h.get_by_label("詳細").click();
        h.run_steps(3);
        let shown = texts_of(&h);
        assert!(
            shown.iter().any(|l| l.contains("サンプリング")),
            "{shown:?}"
        );
        assert!(
            shown.iter().any(|l| l == "入力 (試験用を含む)"),
            "{shown:?}"
        );
        let picked = std::env::temp_dir().join("picked.wav");
        crate::dialog::set_test_pick(Some(picked.clone()));
        h.get_by_label("開く…").click();
        h.run_steps(2);
        crate::dialog::set_test_pick(None);
        let mut want = s.clone();
        want.insert("file".into(), picked.display().to_string());
        assert_eq!(h.state().sections["loopback"], want, "the keys round-trip");
        h.get_by_label("\u{00D7}").click();
        h.run_steps(2);
        assert!(h.state().sections["loopback"]["file"].is_empty());
        assert!(texts_of(&h).iter().any(|l| l == "未選択"));
        let mut saved = Section::new();
        h.state().save(&mut saved);
        let back = crate::device::DriverChoice::load(&saved, "none", &[&LoopbackDrv::default()]);
        assert_eq!(back.sections["loopback"], h.state().sections["loopback"]);
    }
}
