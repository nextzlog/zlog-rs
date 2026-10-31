pub mod audio;
mod messages;
mod options;
mod playback;
mod settings;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::Ui;
use zlog_core::Mode;
use zlog_ui::device::{AudioDrv, DeviceDriver, DriverChoice, PortHub};
use zlog_ui::{EntryAction, EntryKey, Panel, Section, View};

pub use audio::AudioPlayer;
pub use playback::{Player, Voice};
pub use settings::Settings;

zlog_core::texts!();

pub fn plays_voice(mode: Mode) -> bool {
    matches!(mode, Mode::Ssb | Mode::Fm | Mode::Am | Mode::Dv)
}

pub struct VoicePanel {
    settings: Settings,
    voice: Voice<AudioPlayer>,
    recording: Option<(usize, PathBuf, Vec<f32>)>,
    status: String,
    drivers: Vec<Box<dyn AudioDrv>>,
    ports: PortHub,
    device: DriverChoice,
    queue: zlog_ui::queue::Shared,
    playing: Option<u64>,
    hold: bool,
    phone: bool,
}

impl Default for VoicePanel {
    fn default() -> Self {
        VoicePanel {
            settings: Settings::default(),
            voice: Voice::new(AudioPlayer::default()),
            recording: None,
            status: String::new(),
            drivers: Vec::new(),
            ports: PortHub::new(),
            device: DriverChoice::new(SOUND),
            queue: zlog_ui::queue::shared(),
            playing: None,
            hold: false,
            phone: false,
        }
    }
}

const SOUND: &str = "sound";

impl VoicePanel {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>, ports: PortHub) -> Self {
        VoicePanel {
            drivers,
            ports,
            ..VoicePanel::default()
        }
    }

    pub(crate) fn audio(&mut self) -> Result<&mut dyn zlog_ui::device::AudioIo, String> {
        if self.voice.player.io.is_none() {
            let drv = self
                .device
                .find(&self.drivers)
                .ok_or_else(|| texts().get("en", "no_device").to_string())?;
            self.voice.player.io = Some(drv.open(&self.device.section(), &self.ports)?);
        }
        Ok(self.voice.player.io.as_deref_mut().expect("opened"))
    }

    fn report(&mut self, r: Result<(), String>) {
        match r {
            Ok(()) => self.status.clear(),
            Err(e) => self.status = e,
        }
    }

    fn play(&mut self, n: usize) -> bool {
        let Some(path) = self.settings.file(n).map(Path::to_path_buf) else {
            return false;
        };
        let r = self.audio().map(drop).and_then(|()| {
            self.voice
                .play(n, &path, self.settings.timing(), Instant::now())
        });
        self.report(r);
        true
    }

    fn toggle_cq(&mut self) {
        if self.voice.cq_running() {
            self.voice.abort();
            return;
        }
        let Some(path) = self.settings.file(0).map(Path::to_path_buf) else {
            return;
        };
        let r = self.audio().map(drop).and_then(|()| {
            self.voice
                .start_cq(0, &path, self.settings.timing(), Instant::now())
        });
        self.report(r);
    }

    fn update(&mut self) {
        if let (Some((_, _, got)), Some(io)) =
            (self.recording.as_mut(), self.voice.player.io.as_mut())
        {
            io.record(got);
        }
        let settings = &self.settings;
        let r = self
            .voice
            .tick(Instant::now(), settings.timing(), |n| settings.file(n));
        if r.is_err() {
            self.report(r);
        }
    }

    fn cq_ui(&mut self, ui: &mut Ui, lang: &str) {
        use zlog_ui::{icon, layout};
        let t = |k| texts().get(lang, k);
        let running = self.voice.cq_running();
        if layout::toggle(ui, icon::MEGAPHONE, t("cq_repeat"), running).clicked() {
            self.repeat_cq(true);
        }
        if layout::tool(ui, icon::STOP, t("stop")).clicked() {
            self.voice.abort();
        }
    }

    fn toolbar_actions(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = |k| texts().get(language, k);
        let running = self.voice.cq_running();
        let state = t(if running {
            "toolbar.cq.running"
        } else {
            "toolbar.cq.stopped"
        });
        let mut b = vec![
            in_menu(
                zlog_ui::Command::action("cq", t("toolbar.cq"), t("toolbar.cq.tip"))
                    .icon(zlog_ui::icon::MEGAPHONE)
                    .state(state)
                    .selected(running),
                t("menu.cq"),
                3,
            ),
            in_menu(
                zlog_ui::Command::action("stop", t("toolbar.stop"), t("toolbar.stop.tip"))
                    .icon(zlog_ui::icon::STOP)
                    .state(state),
                t("menu.stop"),
                4,
            ),
        ];
        for (i, (id, m)) in [
            ("cq_repeat", egui::Modifiers::CTRL),
            ("cq_hold", egui::Modifiers::SHIFT),
        ]
        .into_iter()
        .enumerate()
        {
            let c = zlog_ui::Command::new(
                id,
                texts().get(language, &format!("key.{id}")).to_string(),
                texts().get(language, &format!("key.{id}.tip")).to_string(),
            )
            .key_hint(egui::KeyboardShortcut::new(m, egui::Key::Z))
            .role(id)
            .section(zlog_ui::MenuSection::Phone, i as i32 + 1)
            .state(state)
            .selected(running);
            b.push(if self.phone {
                c
            } else {
                c.disabled_because(t("key.phone_only"))
            });
        }
        if self.settings.file(0).is_none() {
            b = b
                .into_iter()
                .map(|b| b.disabled_because(t("toolbar.no_memory")))
                .collect();
        }
        let mut record = in_menu(
            zlog_ui::Command::action("record", t("toolbar.record"), t("toolbar.record.tip"))
                .icon(zlog_ui::icon::RECORD)
                .state(t("idle"))
                .on_toolbar(false),
            t("menu.record"),
            5,
        );
        record.checkable = true;
        if let Some((n, _, _)) = &self.recording {
            record = record
                .state(texts().format(language, "toolbar.recording", &[&(n + 1)]))
                .selected(true);
        }
        if self.device.is_none() {
            record = record.disabled_because(t("no_device"));
        }
        b.push(record);
        b
    }
}

fn in_menu(mut c: zlog_ui::Command, label: &str, weight: i32) -> zlog_ui::Command {
    c.menu = true;
    c.menu_label(label)
        .section(zlog_ui::MenuSection::Phone, weight)
}

impl VoicePanel {
    fn function_key(&mut self, n: u8, view: &View<'_>) -> bool {
        if !(1..=12).contains(&n) || !plays_voice(view.mode) {
            return false;
        }
        let n = n as usize - 1;
        if !self.queue.lock().unwrap().so2r {
            return self.play(n);
        }
        if self.settings.file(n).is_none() {
            return false;
        }
        self.voice.abort();
        {
            let mut q = self.queue.lock().unwrap();
            q.clear();
            q.add(zlog_ui::queue::Queued {
                tx: view.rig,
                mode: view.mode,
                play: zlog_ui::queue::Play::Voice(n + 1),
            });
        }
        self.play_next();
        true
    }

    fn play_next(&mut self) {
        let (next, generation) = {
            let mut q = self.queue.lock().unwrap();
            match self.playing {
                Some(g) if g != q.generation() => {
                    self.voice.abort();
                    self.playing = None;
                }
                Some(_) if !self.voice.busy() => self.playing = None,
                _ => {}
            }
            (q.next(true, !self.voice.busy()), q.generation())
        };
        if let Some(zlog_ui::queue::Play::Voice(n)) = next.map(|m| m.play) {
            self.play(n - 1);
            self.playing = Some(generation);
        }
    }

    fn escape(&mut self) -> bool {
        self.queue.lock().unwrap().clear();
        self.voice.abort()
    }

    fn entry_typed(&mut self) {
        if self.voice.cq_running() && !self.hold {
            self.voice.abort();
        }
    }

    fn repeat_cq(&mut self, hold: bool) {
        if !self.voice.cq_running() {
            self.toggle_cq();
            self.hold = hold;
        }
    }
}

impl Panel for VoicePanel {
    fn migrations(&self) -> Vec<zlog_ui::migration::Migration> {
        ["ptt_before", "ptt_after"]
            .map(|k| zlog_ui::migration::Migration::CopiedKey {
                from: ("panel.voice", k),
                to: ("panel.keyer", k),
            })
            .to_vec()
    }

    fn id(&self) -> &str {
        "voice"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Phone, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        let mut b = vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::MICROPHONE)];
        b.extend(self.toolbar_actions(language));
        b
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [360.0, 440.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let state = if self.recording.is_some() {
            "recording"
        } else if self.voice.busy() {
            "playing"
        } else {
            "idle"
        };
        let state = texts().get(lang, state);
        let status = self.status.clone();
        zlog_ui::layout::toolbar(ui, &[(state, false), (&status, true)], |ui| {
            self.cq_ui(ui, lang)
        });
        zlog_ui::layout::rest(ui, "voice_memories", |ui| self.memories_ui(ui, lang));
        if self.voice.busy() || self.voice.cq_running() || self.recording.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        (self.voice.busy() || self.voice.cq_running() || self.recording.is_some())
            .then_some(std::time::Duration::from_millis(50))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.phone = plays_voice(view.mode);
        if !plays_voice(view.mode) && (self.voice.busy() || self.voice.cq_running()) {
            self.voice.abort();
        }
        self.update();
        self.play_next();
    }

    fn on_key(&mut self, key: EntryKey, view: &View<'_>) -> bool {
        match key {
            EntryKey::Function { n, .. } | EntryKey::Reserved { n } => self.function_key(n, view),
            EntryKey::Escape => self.escape(),
            EntryKey::Typed => {
                self.entry_typed();
                false
            }
        }
    }

    fn run_command(&mut self, id: &str) {
        match id {
            "cq" | "cq_hold" => self.repeat_cq(true),
            "cq_repeat" => self.repeat_cq(false),
            "stop" => {
                self.voice.abort();
            }
            "record" => {
                let n = self.recording.as_ref().map_or(0, |(n, _, _)| *n);
                self.toggle_record(n);
            }
            _ => {}
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        self.voice
            .take_requests()
            .into_iter()
            .map(EntryAction::Transmit)
            .collect()
    }

    fn load(&mut self, section: &Section) {
        self.load_with(section, &zlog_ui::Sections::new());
    }

    fn reads(&self) -> Vec<String> {
        vec![zlog_ui::device::SOUND_SECTION.into()]
    }

    fn load_with(&mut self, section: &Section, others: &zlog_ui::Sections) {
        self.settings = Settings::load(section);
        let list: Vec<&dyn DeviceDriver> = self.drivers.iter().map(|d| &**d as _).collect();
        let sound = zlog_ui::device::sound_section(section, others);
        let device = DriverChoice::load(&sound, SOUND, &list);
        if !zlog_ui::device::same_device(&self.device, &device) {
            self.voice.abort();
            self.recording = None;
            self.voice.player.io = None;
        }
        self.device = device;
    }

    fn save(&self, section: &mut Section) {
        self.settings.save(section);
        self.device.save(section);
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.page(230, |ctx| {
        options::VoiceOptions::new(ctx.new_drivers().take())
    });
    b.panel(420, |ctx| {
        VoicePanel::new(ctx.drivers.take(), ctx.ports.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sound;

    impl zlog_ui::device::DeviceDriver for Sound {
        fn id(&self) -> &str {
            "sound"
        }
        fn title(&self, _language: &str) -> String {
            "Sound".into()
        }
        fn settings(&self, _language: &str) -> Vec<zlog_ui::device::SettingField> {
            let text = || zlog_ui::device::SettingKind::Text;
            vec![
                zlog_ui::device::SettingField::new("output", "Output", text()),
                zlog_ui::device::SettingField::new("input", "Input", text()),
            ]
        }
    }

    impl AudioDrv for Sound {
        fn open(
            &self,
            _settings: &Section,
            _ports: &PortHub,
        ) -> Result<Box<dyn zlog_ui::device::AudioIo>, String> {
            Err("no sound device in tests".into())
        }
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn cq_stop_and_record_are_also_in_the_phone_menu() {
        let p = VoicePanel::new(Vec::new(), PortHub::new());
        for lang in ["en", "ja"] {
            let list = p.commands(lang);
            for id in ["cq", "stop", "record"] {
                let c = list.iter().find(|c| c.id == id).unwrap();
                assert!(c.menu && c.toolbar, "{id}");
                assert_eq!(c.section, zlog_ui::MenuSection::Phone);
                assert_ne!(c.menu_text(), c.label);
                assert_eq!(c.checkable, id == "record");
            }
        }
    }

    fn loopback_panel(dir: &Path) -> (zlog_ui::loopback::LoopbackDrv, VoicePanel) {
        let drv = zlog_ui::loopback::LoopbackDrv::default();
        let mine = drv.clone();
        let mut p = VoicePanel::new(vec![Box::new(mine)], PortHub::new());
        p.queue = zlog_ui::queue::Shared::default();
        let mut s = Section::new();
        let wav = dir.join("cq.wav");
        audio::write_wav(&wav, &[0.5; 800], 8000).unwrap();
        for (k, v) in [
            ("device", "loopback".to_string()),
            ("loopback.rate", "16000".into()),
            ("file1", wav.display().to_string()),
            ("ptt", "true".into()),
            ("ptt_before", "0".into()),
        ] {
            s.insert(k.into(), v);
        }
        p.load(&s);
        (drv, p)
    }

    #[test]
    fn a_memory_plays_through_the_loopback() {
        let dir = std::env::temp_dir().join(format!("zlog-voice-loop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (drv, mut p) = loopback_panel(&dir);
        assert!(p.play(0));
        let start = Instant::now();
        while p.voice.busy() {
            assert!(start.elapsed() < Duration::from_secs(3));
            p.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(drv.played.lock().unwrap().len(), 1600 + 2400);
        assert_eq!(
            p.take_actions(),
            [EntryAction::Transmit(true), EntryAction::Transmit(false)]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn voice_cq_repeat_stops_on_typing_only_for_ctrl_z() {
        let dir = std::env::temp_dir().join(format!("zlog-voice-cq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (drv, mut p) = loopback_panel(&dir);
        p.settings.cq_repeat = 0.0;
        let c = p.commands("en");
        assert!(!c.iter().find(|c| c.id == "cq_repeat").unwrap().enabled);
        p.phone = true;
        let c = p.commands("en");
        for id in ["cq_repeat", "cq_hold"] {
            let c = c.iter().find(|c| c.id == id).unwrap();
            assert!(c.enabled && c.role == Some(id) && c.key_hint.is_some());
        }
        let one = 1600 + 2400;
        for id in ["cq_repeat", "cq_hold", "cq"] {
            drv.played.lock().unwrap().clear();
            p.run_command(id);
            assert!(p.voice.cq_running(), "{id}: {}", p.status);
            let start = Instant::now();
            while drv.played.lock().unwrap().len() <= one {
                assert!(start.elapsed() < Duration::from_secs(5), "{id} repeats");
                p.update();
                std::thread::sleep(Duration::from_millis(5));
            }
            p.run_command(id);
            assert!(p.voice.cq_running(), "{id}: a second press keeps it");
            p.entry_typed();
            assert_eq!(p.voice.cq_running(), id != "cq_repeat", "{id}: typing");
            p.escape();
            assert!(!p.voice.cq_running() && !p.voice.busy(), "{id}: Esc");
        }
    }

    #[test]
    fn a_memory_records_what_the_input_gives() {
        let dir = std::env::temp_dir().join(format!("zlog-voice-rec2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (drv, mut p) = loopback_panel(&dir);
        p.settings.files[1] = dir.join("F2.wav").display().to_string();
        p.toggle_record(1);
        assert!(p.recording.is_some(), "{}", p.status);
        drv.feed.lock().unwrap().extend([0.25f32; 1600]);
        std::thread::sleep(Duration::from_millis(150));
        p.update();
        p.toggle_record(1);
        assert!(p.status.is_empty(), "{}", p.status);
        let (got, rate) = audio::read_wav(&dir.join("F2.wav")).unwrap();
        assert_eq!(rate, 16000);
        assert!(got.len() >= 1600);
        assert!(got[..1600].iter().all(|s| (s - 0.25).abs() < 1e-3));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_memory_records_a_file_fed_to_the_input() {
        let dir = std::env::temp_dir().join(format!("zlog-voice-rec3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.wav");
        let tone: Vec<f32> = (0..1600).map(|i| (i % 16) as f32 / 32.0).collect();
        audio::write_wav(&source, &tone, 16000).unwrap();
        let drv = zlog_ui::loopback::LoopbackDrv::default();
        let mut p = VoicePanel::new(vec![Box::new(drv.clone())], PortHub::new());
        p.queue = zlog_ui::queue::Shared::default();
        let mut s = Section::new();
        for (k, v) in [
            ("device", "loopback".to_string()),
            ("loopback.rate", "16000".into()),
            ("loopback.source", "file".into()),
            ("loopback.file", source.display().to_string()),
        ] {
            s.insert(k.into(), v);
        }
        p.load(&s);
        p.settings.files[2] = dir.join("F3.wav").display().to_string();
        p.toggle_record(2);
        assert!(p.recording.is_some(), "{}", p.status);
        let start = Instant::now();
        while p.recording.as_ref().is_none_or(|(_, _, r)| r.len() < 3200) {
            assert!(start.elapsed() < Duration::from_secs(5), "file never heard");
            std::thread::sleep(Duration::from_millis(20));
            p.update();
        }
        p.toggle_record(2);
        assert!(p.status.is_empty(), "{}", p.status);
        let (got, rate) = audio::read_wav(&dir.join("F3.wav")).unwrap();
        assert_eq!(rate, 16000);
        let at = got
            .iter()
            .position(|s| *s > 0.0)
            .expect("file samples recorded");
        let first = (got[at] * 32.0).round() as usize;
        for (i, s) in got[at..][..1024].iter().enumerate() {
            assert!((s - tone[(first + i) % 16]).abs() < 1e-3, "sample {i}: {s}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_record_button_records_f1_and_shows_it() {
        let dir = std::env::temp_dir().join(format!("zlog-voice-rec1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (drv, mut p) = loopback_panel(&dir);
        let record = |p: &VoicePanel| {
            let c = p
                .commands("en")
                .into_iter()
                .find(|c| c.id == "record")
                .unwrap();
            assert!(c.enabled && c.toolbar && !c.toolbar_default);
            (c.state, c.selected)
        };
        assert_eq!(record(&p), (Some("Idle".into()), false));
        p.run_command("record");
        assert_eq!(record(&p), (Some("Recording F1".into()), true));
        drv.feed.lock().unwrap().extend([0.5f32; 800]);
        std::thread::sleep(Duration::from_millis(100));
        p.update();
        p.run_command("record");
        assert_eq!(record(&p), (Some("Idle".into()), false));
        assert!(p.status.is_empty(), "{}", p.status);
        let (got, rate) = audio::read_wav(&dir.join("cq.wav")).unwrap();
        assert_eq!(rate, 16000);
        assert!(got.len() >= 800 && got[..800].iter().all(|s| (s - 0.5).abs() < 1e-3));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_device_names_reach_the_sound_driver() {
        let mut old = Section::new();
        old.insert("output".into(), "USB Audio".into());
        let mut p = VoicePanel::new(vec![Box::new(Sound)], PortHub::new());
        p.load(&old);
        assert_eq!(p.device.id, "sound");
        assert_eq!(p.device.section()["output"], "USB Audio");
        let mut s = Section::new();
        p.save(&mut s);
        assert_eq!(s["sound.output"], "USB Audio");
        let mut q = VoicePanel::default();
        q.settings.files[0] = "x.wav".into();
        assert!(q.play(0));
        assert!(!q.status.is_empty());
    }

    #[test]
    fn voice_takes_the_keys_in_phone_modes_only() {
        for m in [Mode::Ssb, Mode::Fm, Mode::Am, Mode::Dv] {
            assert!(plays_voice(m), "{m}");
        }
        for m in [Mode::Cw, Mode::Rtty, Mode::Ft8, Mode::Ft4, Mode::Other] {
            assert!(!plays_voice(m), "{m}");
        }
    }
}
