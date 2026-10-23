//! Voice memories (`UVoiceForm`, `UzLogSound`, the Voice tab of `UOptions` and
//! `TMainForm.PlayMessagePH`, `VoiceControl`, main.pas): WAV files on F1 to F12,
//! played by the function keys in phone modes, CQ repeat of F1, Esc to stop, PTT
//! through the keyer before and after each memory, and recording a memory from the
//! microphone; a message manager editing every memory in one dialog. With an
//! SO2R box in use, the memories of the function keys go through the play queue
//! of SO2R ([`zlog_ui::queue`]), each shown there until it is played.
//!
//! Not implemented: the extra CQ memories CQ2 and CQ3 (`FAdditionalVoiceConfig`),
//! voice files per operator (`TOperatorInfo.VoiceFiles`), a sound device per rig
//! (`FUseRigSoundDevice`), the rig commands before and after playback
//! (`UPrePostPlaybackDlg`, `UAudioInputDlg`: Icom audio input select), and swapping
//! the PTT line in phone (`PhoneChgPTT`).

pub mod audio;
mod manager;
mod messages;
mod playback;
mod settings;

use std::path::Path;
use std::time::{Duration, Instant};

use egui::Ui;
use zlog_core::Mode;
use zlog_ui::{EntryAction, EntryKey, Panel, Section, View};

pub use messages::new_memory_path;
pub use playback::{Player, Timing, Voice};
pub use settings::Settings;

zlog_core::texts!();

/// Whether the function keys play voice in `mode` (`PlayMessage`: `mSSB`, `mFM`,
/// `mAM`, `mDV`); the keyer takes them on CW.
pub fn plays_voice(mode: Mode) -> bool {
    matches!(mode, Mode::Ssb | Mode::Fm | Mode::Am | Mode::Dv)
}

/// The voice memory window.
pub struct VoicePanel {
    settings: Settings,
    voice: Voice<audio::DevicePlayer>,
    recording: Option<(usize, audio::Recording)>,
    outputs: Vec<String>,
    inputs: Vec<String>,
    status: String,
    manager: manager::Manager,
    /// The play queue of SO2R.
    queue: zlog_ui::queue::Shared,
    /// The queue's generation when the memory being played was taken from it, to
    /// stop it when another panel clears the queue (SO2R turned off).
    playing: Option<u64>,
}

impl Default for VoicePanel {
    fn default() -> Self {
        VoicePanel {
            settings: Settings::default(),
            voice: Voice::new(audio::DevicePlayer::default()),
            recording: None,
            outputs: Vec::new(),
            inputs: Vec::new(),
            status: String::new(),
            manager: manager::Manager::default(),
            queue: zlog_ui::queue::shared(),
            playing: None,
        }
    }
}

impl VoicePanel {
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
        self.voice.player.device = self.settings.output.clone();
        let r = self
            .voice
            .play(n, &path, self.settings.timing(), Instant::now());
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
        self.voice.player.device = self.settings.output.clone();
        let r = self
            .voice
            .start_cq(0, &path, self.settings.timing(), Instant::now());
        self.report(r);
    }

    fn update(&mut self) {
        let settings = &self.settings;
        let r = self
            .voice
            .tick(Instant::now(), settings.timing(), |n| settings.file(n));
        if r.is_err() {
            self.report(r);
        }
    }

    fn device_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        let default = t("default_device");
        for (label, id, value, list) in [
            (
                "output",
                "voice-output",
                &mut self.settings.output,
                &self.outputs,
            ),
            (
                "input",
                "voice-input",
                &mut self.settings.input,
                &self.inputs,
            ),
        ] {
            ui.horizontal(|ui| {
                ui.label(t(label));
                let shown = if value.is_empty() {
                    default.to_string()
                } else {
                    value.clone()
                };
                egui::ComboBox::from_id_salt(id)
                    .selected_text(shown)
                    .width(ui.available_width().max(zlog_ui::FIELD_MIN))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(value, String::new(), default);
                        for d in list {
                            ui.selectable_value(value, d.clone(), d);
                        }
                    });
            });
        }
        ui.horizontal(|ui| {
            if ui.button(t("find_devices")).clicked() {
                self.outputs = audio::devices(false);
                self.inputs = audio::devices(true);
            }
            ui.checkbox(&mut self.settings.ptt, t("ptt"));
            ui.label(t("ptt_delay"));
            ui.add(egui::DragValue::new(&mut self.settings.ptt_before_ms).range(0..=1000));
            ui.add(egui::DragValue::new(&mut self.settings.ptt_after_ms).range(0..=1000));
            ui.label("ms");
        });
    }

    fn cq_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.horizontal(|ui| {
            let running = self.voice.cq_running();
            if ui.selectable_label(running, t("cq_repeat")).clicked() {
                self.toggle_cq();
            }
            ui.add(
                egui::DragValue::new(&mut self.settings.cq_repeat)
                    .range(0.0..=60.0)
                    .speed(0.1)
                    .suffix(" s"),
            );
            ui.label(t("cq_max"));
            ui.add(egui::DragValue::new(&mut self.settings.cq_max).range(1..=999));
            if ui.button(t("stop")).clicked() {
                self.voice.abort();
            }
            if ui.button(t("manager")).clicked() && !self.manager.is_open() {
                self.manager.open(&self.settings);
            }
        });
    }

    /// The toolbar's CQ and Stop buttons, usable with an F1 memory.
    fn toolbar_actions(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = |k| texts().get(language, k);
        let running = self.voice.cq_running();
        let state = t(if running {
            "toolbar.cq.running"
        } else {
            "toolbar.cq.stopped"
        });
        let b = vec![
            zlog_ui::Command::action("cq", t("toolbar.cq"), t("toolbar.cq.tip"))
                .icon(zlog_ui::icon::MEGAPHONE)
                .state(state)
                .selected(running),
            zlog_ui::Command::action("stop", t("toolbar.stop"), t("toolbar.stop.tip"))
                .icon(zlog_ui::icon::STOP)
                .state(state),
        ];
        if self.settings.file(0).is_none() {
            return b
                .into_iter()
                .map(|b| b.disabled_because(t("toolbar.no_memory")))
                .collect();
        }
        b
    }
}

impl VoicePanel {
    /// In phone modes F1 to F12 play their memory, Shift too (zLog has one voice
    /// bank); a key without a file is left to other panels.
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
        // SO2R: the queue is cleared, the memory added and started
        // (`PlayMessagePH`).
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

    /// Play the next memory of the SO2R queue when idle, dropping the one played
    /// (`ContinueQue`).
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

    /// Typing in the entry row stops the CQ repeat (`OtherKeyPressed`).
    fn entry_typed(&mut self) {
        self.voice.stop_cq();
    }
}

impl Panel for VoicePanel {
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

    /// Hidden until chosen: most stations run CW with the keyer's buttons.
    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [720.0, 560.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        self.device_ui(ui, lang);
        self.cq_ui(ui, lang);
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 2.0 * ui.spacing().interact_size.y)
            .show(ui, |ui| self.memories_ui(ui, lang));
        ui.separator();
        let state = if self.recording.is_some() {
            "recording"
        } else if self.voice.busy() {
            "playing"
        } else {
            "idle"
        };
        ui.horizontal(|ui| {
            ui.label(texts().get(lang, state));
            if !self.status.is_empty() {
                ui.colored_label(ui.visuals().warn_fg_color, &self.status);
            }
        });
        let ctx = ui.ctx().clone();
        self.manager_ui(&ctx, lang);
        if self.voice.busy() || self.voice.cq_running() || self.recording.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }

    fn tick(&mut self, view: &View<'_>) {
        // Out of phone (to CW, say), a running memory or CQ repeat stops.
        if !plays_voice(view.mode) && (self.voice.busy() || self.voice.cq_running()) {
            self.voice.abort();
        }
        self.update();
        self.play_next();
    }

    fn on_key(&mut self, key: EntryKey, view: &View<'_>) -> bool {
        match key {
            EntryKey::Function { n, .. } => self.function_key(n, view),
            EntryKey::Escape => self.escape(),
            EntryKey::Typed => {
                self.entry_typed();
                false
            }
        }
    }

    fn run_command(&mut self, id: &str) {
        match id {
            "cq" => self.toggle_cq(),
            "stop" => {
                self.voice.abort();
            }
            _ => {}
        }
    }

    /// PTT on and off around each memory, carried out by the keyer.
    fn take_actions(&mut self) -> Vec<EntryAction> {
        self.voice
            .take_requests()
            .into_iter()
            .map(EntryAction::Transmit)
            .collect()
    }

    fn load(&mut self, section: &Section) {
        self.settings = Settings::load(section);
    }

    fn save(&self, section: &mut Section) {
        self.settings.save(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
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
