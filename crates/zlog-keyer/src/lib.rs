//! CW keying (`UzLogKeyer`, `WinKeyer`, `UzLogCW`, `UCwMessageEditor`,
//! `UCwMessagePad`, `UCWKeyBoard`): a keyer on WinKeyer, on a serial port's DTR or
//! RTS line, or simulated; banks A and B of F1 to F12 messages with `$`-macros;
//! CQ repeat; a message pad, a CW keyboard and a message manager editing both
//! banks at once.
//!
//! With an SO2R box in use, the messages of the function keys go through the
//! play queue of SO2R ([`zlog_ui::queue`]), each shown there until it is sent.
//!
//! Not implemented: parallel-port and USBIF4CW keying (Windows-only hardware and
//! drivers), SO2R switching, paddle keying by the PC, RTTY messages (bank 3), and
//! keying through the rig's CAT.

pub mod keyer;
pub mod macros;
mod manager;
mod messages;
pub mod morse;
mod playback;
pub mod serial;
mod settings;
pub mod winkeyer;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use egui::Ui;
use zlog_core::Mode;
use zlog_ui::{EntryKey, Panel, Section, View};

pub use keyer::{Keyer, KeyerEvent, SimKeyer};
pub use macros::{expand, Abbrev, Fields};
pub use morse::Timing;
pub use settings::{ptt_delay, Device, Settings, DEFAULT_MESSAGES};

use crate::playback::{cq_loop, open, Opened, Shared};
use crate::settings::now;

zlog_core::texts!();

/// The CW keyer window.
pub struct KeyerPanel {
    settings: Settings,
    shared: Arc<Mutex<Shared>>,
    opening: Option<Receiver<Opened>>,
    /// Bank the function keys play, 0 for A and 1 for B.
    bank: usize,
    cq: Arc<AtomicBool>,
    /// Text of the CW keyboard and the text as it was after the last change.
    keyboard: String,
    keyboard_last: String,
    ports: Vec<String>,
    manager: manager::Manager,
    /// The play queue of SO2R.
    queue: zlog_ui::queue::Shared,
    /// The queue's generation when the message being sent was taken from it, to
    /// stop it when another panel clears the queue.
    playing: Option<u64>,
    /// What SO2R Neo on the keyer's port last asked for, sent again whenever a
    /// keyer opens.
    port: PortState,
}

/// The state SO2R Neo on the keyer's port was told: the TX/RX byte, AF blend on
/// or off and the blend ratio (see `zlog_so2r::neo`). A keyer opened later (or
/// again) gets it, as the panel sends it only when it changes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct PortState {
    rig: Option<u8>,
    blend: Option<u8>,
    ratio: Vec<u8>,
}

impl PortState {
    fn take(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b @ 0x90..=0x97 => self.rig = Some(b),
                b @ (0x86 | 0x87) => self.blend = Some(b),
                0xc0..=0xcf if i + 1 < bytes.len() => {
                    self.ratio = bytes[i..i + 2].to_vec();
                    i += 1;
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut b: Vec<u8> = self.rig.into_iter().chain(self.blend).collect();
        b.extend_from_slice(&self.ratio);
        b
    }
}

impl Default for KeyerPanel {
    fn default() -> Self {
        KeyerPanel {
            settings: Settings::default(),
            shared: Arc::default(),
            opening: None,
            bank: 0,
            cq: Arc::new(AtomicBool::new(true)),
            keyboard: String::new(),
            keyboard_last: String::new(),
            ports: Vec::new(),
            manager: manager::Manager::default(),
            queue: zlog_ui::queue::shared(),
            playing: None,
            port: PortState::default(),
        }
    }
}

impl KeyerPanel {
    fn configured(&self) -> bool {
        self.shared.lock().unwrap().keyer.is_some()
    }

    fn connect(&mut self) {
        self.disconnect();
        let s = self.settings.clone();
        let (tx, rx) = mpsc::channel();
        // An open still under way holds the port (Windows ports are exclusive): wait
        // for it and close what it opened before opening again.
        let pending = self.opening.take();
        thread::spawn(move || {
            if let Some(old) = pending {
                drop(old.recv());
            }
            let _ = tx.send(open(&s));
        });
        self.opening = Some(rx);
    }

    fn disconnect(&mut self) {
        self.stop();
        self.shared.lock().unwrap().keyer = None;
    }

    fn stop(&mut self) {
        self.cq.store(true, Ordering::SeqCst);
        self.queue.lock().unwrap().clear();
        if let Some(k) = self.shared.lock().unwrap().keyer.as_mut() {
            k.abort();
        }
    }

    /// Expand and send message `n` (0..12) of `bank`, stopping what is being sent
    /// first (`PlayMessageCW` calls `WinKeyerClear`, or `ClrBuffer` without the FIFO
    /// option, which is not implemented); false without a keyer.
    fn play(&mut self, bank: usize, n: usize, view: &View<'_>) -> bool {
        self.cq.store(true, Ordering::SeqCst);
        let text = self.expanded(bank, n, view);
        let mut s = self.shared.lock().unwrap();
        let Some(k) = s.keyer.as_mut() else {
            return false;
        };
        if text.trim().is_empty() {
            return true;
        }
        k.abort();
        let mut q = self.queue.lock().unwrap();
        if !q.so2r {
            k.send(&text);
            return true;
        }
        // SO2R: the queue is cleared, the message added and started
        // (`PlayMessageCW`).
        q.clear();
        q.add(zlog_ui::queue::Queued {
            tx: view.rig,
            mode: Mode::Cw,
            play: zlog_ui::queue::Play::Text(text),
        });
        if play_next(&mut q, k.as_mut()) {
            self.playing = Some(q.generation());
        }
        true
    }

    fn expanded(&self, bank: usize, n: usize, view: &View<'_>) -> String {
        let (secs, hour) = now();
        let f = self.settings.fields(view, secs, hour);
        expand(&self.settings.banks[bank][n], &f, &self.settings.abbrev)
    }

    /// Take the keyer when it has opened, read its events and keep the macro values
    /// for the CQ repeat; every frame, open or closed.
    fn update(&mut self, view: &View<'_>) {
        if let Some(rx) = &self.opening {
            if let Ok(r) = rx.try_recv() {
                self.opening = None;
                let mut s = self.shared.lock().unwrap();
                match r {
                    Ok(mut k) => {
                        let port = self.port.bytes();
                        if !port.is_empty() {
                            k.write_port(&port);
                        }
                        s.keyer = Some(k);
                        s.status.clear();
                    }
                    Err(e) => s.status = e,
                }
            }
        }
        let (secs, hour) = now();
        let fields = self.settings.fields(view, secs, hour);
        let pot = {
            let mut s = self.shared.lock().unwrap();
            if s.pump() {
                self.cq.store(true, Ordering::SeqCst);
            }
            if let Some(k) = s.keyer.as_mut() {
                let mut q = self.queue.lock().unwrap();
                // Another panel cleared the queue (a function key, SO2R turned
                // off): stop the message of the queue being sent.
                match self.playing {
                    Some(g) if g != q.generation() => {
                        k.abort();
                        self.playing = None;
                    }
                    Some(_) if !k.busy() => self.playing = None,
                    _ => {}
                }
                if play_next(&mut q, k.as_mut()) {
                    self.playing = Some(q.generation());
                }
            }
            s.fields = fields;
            s.pot_wpm.take()
        };
        if let Some(w) = pot {
            self.settings.timing.wpm = w.clamp(morse::MIN_WPM, morse::MAX_WPM);
        }
    }

    fn start_cq(&mut self, view: &View<'_>) {
        let (secs, hour) = now();
        let fields = self.settings.fields(view, secs, hour);
        self.start_cq_with(fields);
    }

    /// Start the CQ repeat of message F1 of the current bank with the entry row's
    /// macro values `fields`; nothing without a keyer or with an empty message.
    fn start_cq_with(&mut self, fields: Fields) {
        self.stop();
        let template = self.settings.banks[self.bank][0].clone();
        let mut first = fields.clone();
        let (secs, hour) = now();
        self.settings.stamp(&mut first, secs, hour);
        let text = expand(&template, &first, &self.settings.abbrev);
        if text.trim().is_empty() || !self.configured() {
            return;
        }
        self.shared.lock().unwrap().fields = fields;
        let stop = Arc::new(AtomicBool::new(false));
        self.cq = stop.clone();
        let shared = self.shared.clone();
        let interval = self.settings.cq_interval();
        let max = self.settings.cq_max.max(1);
        let settings = self.settings.clone();
        let make = move |s: &Shared| {
            let mut f = s.fields.clone();
            let (secs, hour) = now();
            settings.stamp(&mut f, secs, hour);
            expand(&template, &f, &settings.abbrev)
        };
        thread::spawn(move || cq_loop(shared, make, interval, max, stop));
    }

    /// Whether the CQ repeat is running.
    fn cq_running(&self) -> bool {
        !self.cq.load(Ordering::SeqCst)
    }

    /// The toolbar's CQ and Stop buttons (zLog's CQ repeat and `actionCQAbort`
    /// buttons of the function bar), usable with a keyer.
    fn toolbar_actions(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = |k| texts().get(language, k);
        let running = self.cq_running();
        let state = t(if running {
            "toolbar.cq.running"
        } else {
            "toolbar.cq.stopped"
        });
        let mut b = vec![
            zlog_ui::Command::action("cq", t("toolbar.cq"), t("toolbar.cq.tip"))
                .icon(zlog_ui::icon::MEGAPHONE)
                .state(state)
                .selected(running),
            zlog_ui::Command::action("stop", t("toolbar.stop"), t("toolbar.stop.tip"))
                .icon(zlog_ui::icon::STOP)
                .state(state),
        ];
        if !self.configured() {
            b = b
                .into_iter()
                .map(|b| b.disabled_because(t("toolbar.no_keyer")))
                .collect();
        }
        b
    }

    fn send_text(&mut self, text: &str) {
        if let Some(k) = self.shared.lock().unwrap().keyer.as_mut() {
            k.send(text);
        }
    }

    fn set_wpm(&mut self, wpm: u32) {
        self.settings.timing.wpm = wpm.clamp(morse::MIN_WPM, morse::MAX_WPM);
        if let Some(k) = self.shared.lock().unwrap().keyer.as_mut() {
            k.set_wpm(self.settings.timing.wpm);
        }
    }

    fn device_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.horizontal(|ui| {
            ui.label(t("device"));
            egui::ComboBox::from_id_salt("keyer-device")
                .selected_text(t(self.settings.device.key()))
                .show_ui(ui, |ui| {
                    for d in Device::ALL {
                        ui.selectable_value(&mut self.settings.device, d, t(d.key()));
                    }
                });
            if matches!(self.settings.device, Device::WinKeyer | Device::Serial) {
                ui.label(t("port"));
                let combo = egui::ComboBox::from_id_salt("keyer-port")
                    .selected_text(self.settings.port.clone())
                    .show_ui(ui, |ui| {
                        for p in &self.ports {
                            ui.selectable_value(&mut self.settings.port, p.clone(), p);
                        }
                    });
                if combo.response.clicked() {
                    self.ports = serialport::available_ports()
                        .map(|v| v.into_iter().map(|p| p.port_name).collect())
                        .unwrap_or_default();
                }
                ui.add(egui::TextEdit::singleline(&mut self.settings.port).desired_width(120.0));
            }
            if self.opening.is_some() {
                ui.spinner();
            } else if self.configured() {
                if ui.button(t("disconnect")).clicked() {
                    self.disconnect();
                }
            } else if self.settings.device != Device::None && ui.button(t("connect")).clicked() {
                self.connect();
            }
        });
        if self.settings.device == Device::Serial {
            ui.horizontal(|ui| {
                ui.label(t("key_line"));
                ui.radio_value(&mut self.settings.key_line, serial::Line::Dtr, "DTR");
                ui.radio_value(&mut self.settings.key_line, serial::Line::Rts, "RTS");
                ui.checkbox(&mut self.settings.ptt, t("ptt_other_line"));
            });
        }
        if self.settings.device == Device::WinKeyer {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.settings.sidetone, t("sidetone"));
                ui.checkbox(&mut self.settings.paddle_swap, t("paddle_reverse"));
            });
        }
        if matches!(self.settings.device, Device::WinKeyer | Device::Serial) {
            ui.horizontal(|ui| {
                ui.label(t("ptt_delay"));
                ui.add(egui::DragValue::new(&mut self.settings.ptt_before_ms).range(1..=255));
                ui.add(egui::DragValue::new(&mut self.settings.ptt_after_ms).range(0..=255));
                ui.label("ms");
            });
        }
    }

    fn speed_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.horizontal(|ui| {
            let mut wpm = self.settings.timing.wpm;
            ui.label(t("speed"));
            if ui.button("-").clicked() {
                wpm = wpm.saturating_sub(1);
            }
            ui.add(egui::DragValue::new(&mut wpm).range(morse::MIN_WPM..=morse::MAX_WPM));
            if ui.button("+").clicked() {
                wpm += 1;
            }
            ui.label("WPM");
            if wpm != self.settings.timing.wpm {
                self.set_wpm(wpm);
            }
            let before = self.settings.timing;
            ui.label(t("weight"));
            ui.add(egui::DragValue::new(&mut self.settings.timing.weight).range(0..=100));
            ui.label(t("space_factor"));
            ui.add(egui::DragValue::new(&mut self.settings.timing.space_factor).range(1..=500));
            ui.label(t("ei_space_factor"));
            ui.add(egui::DragValue::new(&mut self.settings.timing.ei_space_factor).range(1..=500));
            if self.settings.timing != before {
                self.set_timing(self.settings.timing);
            }
        });
    }

    /// Apply new weight and space factors to the connected keyer at once.
    fn set_timing(&mut self, timing: Timing) {
        self.settings.timing = timing;
        if let Some(k) = self.shared.lock().unwrap().keyer.as_mut() {
            k.set_timing(timing);
        }
    }
}

impl KeyerPanel {
    /// On CW, F1 to F12 play the message of the current bank and Shift+F1 to F12 that
    /// of the other bank (`PlayMessage`: `mCW` only; `actionPlayMessageB01`); any
    /// function key stops CQ repeat. On other modes the keys are left to other
    /// panels (zLog plays voice there).
    fn function_key(&mut self, n: u8, shift: bool, view: &View<'_>) -> bool {
        if !(1..=12).contains(&n) || view.mode != Mode::Cw {
            return false;
        }
        if self.shared.lock().unwrap().pump() {
            self.cq.store(true, Ordering::SeqCst);
        }
        let bank = if shift { 1 - self.bank } else { self.bank };
        self.play(bank, n as usize - 1, view)
    }

    /// Esc stops CW and the CQ repeat (`actionCQAbort`); true when there was
    /// something to stop.
    fn escape(&mut self) -> bool {
        let running = !self.cq.load(Ordering::SeqCst);
        let busy = self
            .shared
            .lock()
            .unwrap()
            .keyer
            .as_ref()
            .is_some_and(|k| k.busy());
        self.stop();
        running || busy
    }
}

/// Send the next message of the SO2R queue when `keyer` is idle, dropping the one
/// it finished (`ContinueQue`).
/// Whether it sent one.
fn play_next(queue: &mut zlog_ui::queue::PlayQueue, keyer: &mut dyn Keyer) -> bool {
    match queue.next(false, !keyer.busy()).map(|m| m.play) {
        Some(zlog_ui::queue::Play::Text(text)) => {
            keyer.send(&text);
            true
        }
        _ => false,
    }
}

impl Panel for KeyerPanel {
    fn id(&self) -> &str {
        "keyer"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Cw, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        let mut b = vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::KEYBOARD)];
        b.extend(self.toolbar_actions(language));
        b
    }

    fn default_size(&self) -> [f32; 2] {
        [640.0, 520.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        self.update(view);
        let (busy, status) = {
            let s = self.shared.lock().unwrap();
            let busy = s.keyer.as_ref().is_some_and(|k| k.busy());
            (busy, s.status.clone())
        };
        self.device_ui(ui, lang);
        self.speed_ui(ui, lang);
        ui.separator();
        self.buttons_ui(ui, view);
        self.keyboard_ui(ui, lang);
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.editor_ui(ui, lang);
            self.pad_ui(ui, view);
        });
        let ctx = ui.ctx().clone();
        self.manager_ui(&ctx, lang);
        ui.separator();
        let state = if self.opening.is_some() {
            "opening"
        } else if !self.configured() {
            "not_configured"
        } else if busy {
            "sending"
        } else {
            "idle"
        };
        ui.horizontal(|ui| {
            ui.label(texts().get(lang, state));
            if !status.is_empty() {
                ui.colored_label(ui.visuals().warn_fg_color, status);
            }
        });
        if busy || self.opening.is_some() || !self.cq.load(Ordering::SeqCst) {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }

    fn tick(&mut self, view: &View<'_>) {
        self.update(view);
    }

    fn on_key(&mut self, key: EntryKey, view: &View<'_>) -> bool {
        match key {
            EntryKey::Function { n, shift } => self.function_key(n, shift, view),
            EntryKey::Escape => self.escape(),
            EntryKey::Typed => false,
        }
    }

    /// CQ starts the CQ repeat with the entry row of the last frame (kept by
    /// [`Panel::tick`]) or stops it; Stop stops CW and the CQ repeat as Esc does.
    fn run_command(&mut self, id: &str) {
        match id {
            "cq" if self.cq_running() => self.stop(),
            "cq" => {
                let fields = self.shared.lock().unwrap().fields.clone();
                self.start_cq_with(fields);
            }
            "stop" => self.stop(),
            _ => {}
        }
    }

    /// PTT for another panel (voice memories, `VoiceControl` keys the keyer's PTT).
    fn transmit(&mut self, on: bool) -> bool {
        match self.shared.lock().unwrap().keyer.as_mut() {
            Some(k) => {
                k.set_phone_ptt(on);
                true
            }
            None => false,
        }
    }

    /// Bytes of a device sharing the keyer's port (SO2R Neo), written when a
    /// keyer is connected; what they set is also kept for a WinKeyer opened later
    /// (taken then, so no warning while one is configured).
    fn keyer_port(&mut self, bytes: &[u8]) -> bool {
        self.port.take(bytes);
        match self.shared.lock().unwrap().keyer.as_mut() {
            Some(k) => k.write_port(bytes),
            None => self.settings.device == Device::WinKeyer,
        }
    }

    /// Restore the settings and reopen the keyer.
    fn load(&mut self, section: &Section) {
        self.settings = Settings::load(section);
        if self.settings.device != Device::None {
            self.connect();
        }
    }

    fn save(&self, section: &mut Section) {
        self.settings.save(section);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use zlog_core::{Band, Contest, ContestMode, Log, Power, Qso, ScoreSheet, Scoring, Station};

    pub(crate) struct Fixed;

    impl Contest for Fixed {
        fn name(&self) -> &str {
            "test"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::B50)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Cw
        }
        fn sent_number(&self, _band: Band, power: Power, station: &Station) -> String {
            format!("{}{}", station.prov, power.code().unwrap_or('?'))
        }
        fn valid_number(&self, _q: &Qso) -> bool {
            true
        }
        fn score(&mut self, _log: &Log) -> Scoring {
            Scoring {
                qsos: Vec::new(),
                sheet: ScoreSheet::default(),
            }
        }
    }

    /// A contest with serial numbers: 41 on 7 MHz, 1 elsewhere.
    pub(crate) struct Serials;

    impl Contest for Serials {
        fn name(&self) -> &str {
            "serials"
        }
        fn band_range(&self) -> (Band, Band) {
            Fixed.band_range()
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Cw
        }
        fn sent_number(&self, band: Band, power: Power, station: &Station) -> String {
            Fixed.sent_number(band, power, station)
        }
        fn next_serial_on(&self, band: Band) -> Option<i32> {
            Some(if band == Band::B7 { 41 } else { 1 })
        }
        fn valid_number(&self, _q: &Qso) -> bool {
            true
        }
        fn score(&mut self, log: &Log) -> Scoring {
            Fixed.score(log)
        }
    }

    pub(crate) fn with_view<R>(f: impl FnOnce(&View<'_>) -> R) -> R {
        with_mode(Mode::Cw, f)
    }

    pub(crate) fn with_mode<R>(mode: Mode, f: impl FnOnce(&View<'_>) -> R) -> R {
        with_contest(&Fixed, mode, f)
    }

    pub(crate) fn with_contest<R>(
        contest: &dyn Contest,
        mode: Mode,
        f: impl FnOnce(&View<'_>) -> R,
    ) -> R {
        let mut log = Log::default();
        log.qsos.push(Qso {
            callsign: "JA1YAA".into(),
            nr_rcvd: "11H".into(),
            ..Qso::default()
        });
        let sheet = ScoreSheet::default();
        let station = Station {
            callsign: "jr1abc".into(),
            prov: "10".into(),
            ..Station::default()
        };
        let view = View {
            station: &station,
            number: "25M",
            rst_sent: "",
            power: Power::P010,
            log: &log,
            contest,
            sheet: &sheet,
            callsign: "JA1ZLO",
            band: Band::B7,
            mode,
            language: "en",
            spots: &[],
            hz: None,
            rig: 1,
            cq: false,
            operator: "",
            period_hours: 0,
            pc_name: "",
            focus: None,
            editing: None,
            selected: None,
            period: &(),
        };
        f(&view)
    }

    /// Records what it is told; busy as set.
    #[derive(Clone, Default)]
    pub(crate) struct Recorder {
        pub(crate) calls: Arc<Mutex<Vec<String>>>,
        pub(crate) busy: Arc<AtomicBool>,
        pub(crate) events: Arc<Mutex<Vec<KeyerEvent>>>,
    }

    impl Keyer for Recorder {
        fn send(&mut self, text: &str) {
            self.calls.lock().unwrap().push(format!("send {text}"));
        }
        fn abort(&mut self) {
            self.busy.store(false, Ordering::SeqCst);
            self.calls.lock().unwrap().push("abort".into());
        }
        fn set_wpm(&mut self, _wpm: u32) {}
        fn wpm(&self) -> u32 {
            25
        }
        fn set_ptt(&mut self, _on: bool) {}
        fn busy(&self) -> bool {
            self.busy.load(Ordering::SeqCst)
        }
        fn poll(&mut self) -> Vec<KeyerEvent> {
            std::mem::take(&mut *self.events.lock().unwrap())
        }
    }

    pub(crate) fn recording_panel() -> (KeyerPanel, Recorder) {
        let p = KeyerPanel::default();
        let r = Recorder::default();
        p.shared.lock().unwrap().keyer = Some(Box::new(r.clone()));
        (p, r)
    }

    pub(crate) fn calls(r: &Recorder) -> Vec<String> {
        std::mem::take(&mut *r.calls.lock().unwrap())
    }

    /// PTT for other panels is taken only with a keyer connected.
    #[test]
    fn transmit_needs_a_keyer() {
        let mut p = KeyerPanel::default();
        assert!(!p.transmit(true));
        p.shared.lock().unwrap().keyer = Some(Box::new(SimKeyer::new(Timing::default())));
        assert!(p.transmit(true) && p.transmit(false));
    }

    /// Bytes of SO2R Neo are taken only with a keyer that has the port.
    #[test]
    fn keyer_port_needs_a_winkeyer() {
        let mut p = KeyerPanel::default();
        assert!(!p.keyer_port(&[0x90]));
        let w = crate::winkeyer::tests::FakeWire::default();
        p.shared.lock().unwrap().keyer =
            Some(Box::new(crate::winkeyer::WinKeyer::new(w.clone(), 25)));
        assert!(p.keyer_port(&[0x90]));
        assert_eq!(*w.written.lock().unwrap(), [0x90]);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn function_keys_need_a_keyer() {
        let mut p = KeyerPanel::default();
        assert!(!with_view(|v| p.function_key(1, false, v)));
        p.shared.lock().unwrap().keyer = Some(Box::new(SimKeyer::new(Timing::default())));
        assert!(with_view(|v| p.function_key(1, false, v)));
        assert!(with_view(|v| p.function_key(4, false, v)));
        assert!(!with_view(|v| p.function_key(13, false, v)));
        let mut s = p.shared.lock().unwrap();
        assert!(s.keyer.as_ref().unwrap().busy());
        s.keyer.as_mut().unwrap().abort();
    }

    /// With an SO2R box, a function key clears the queue, stopping what is sent,
    /// and its message waits there until the keyer has sent it.
    #[test]
    fn so2r_messages_go_through_the_queue() {
        let (mut p, r) = recording_panel();
        p.queue = zlog_ui::queue::Shared::default();
        p.queue.lock().unwrap().so2r = true;
        assert!(with_view(|v| p.function_key(1, false, v)));
        assert_eq!(calls(&r), ["abort", "send CQ TEST JR1ABC TEST"]);
        r.busy.store(true, Ordering::SeqCst);
        with_view(|v| p.tick(v));
        assert_eq!(
            p.queue.lock().unwrap().lines(),
            ["[1] [CW] CQ TEST JR1ABC TEST"]
        );
        assert!(calls(&r).is_empty());
        r.busy.store(false, Ordering::SeqCst);
        with_view(|v| p.tick(v));
        assert!(p.queue.lock().unwrap().is_empty());
        // Esc empties the queue as well.
        assert!(with_view(|v| p.function_key(2, false, v)));
        p.escape();
        assert!(p.queue.lock().unwrap().is_empty());
    }

    /// A message of the queue being sent stops when another panel (voice, the
    /// SO2R panel turning SO2R off) clears the queue.
    #[test]
    fn a_queued_message_stops_when_another_panel_clears_the_queue() {
        let (mut p, r) = recording_panel();
        p.queue = zlog_ui::queue::Shared::default();
        p.queue.lock().unwrap().so2r = true;
        assert!(with_view(|v| p.function_key(1, false, v)));
        r.busy.store(true, Ordering::SeqCst);
        calls(&r);
        with_view(|v| p.tick(v));
        assert!(calls(&r).is_empty());
        p.queue.lock().unwrap().clear();
        with_view(|v| p.tick(v));
        assert_eq!(calls(&r), ["abort"]);
        with_view(|v| p.tick(v));
        assert!(calls(&r).is_empty());
    }

    /// SO2R Neo bytes given before the WinKeyer has opened reach it when it opens,
    /// as the state they set; and again when it is reopened.
    #[test]
    fn keyer_port_state_reaches_a_keyer_opened_later() {
        let mut p = KeyerPanel::default();
        p.settings.device = Device::WinKeyer;
        assert!(p.keyer_port(&[0x90, 0x86]));
        assert!(p.keyer_port(&[0x95, 0x87, 0xc7, 0xcf]));
        for _ in 0..2 {
            let w = crate::winkeyer::tests::FakeWire::default();
            let (tx, rx) = mpsc::channel();
            let k: Box<dyn Keyer> = Box::new(crate::winkeyer::WinKeyer::new(w.clone(), 25));
            tx.send(Ok(k)).unwrap();
            p.opening = Some(rx);
            with_view(|v| p.tick(v));
            assert_eq!(*w.written.lock().unwrap(), [0x95, 0x87, 0xc7, 0xcf]);
            p.shared.lock().unwrap().keyer = None;
        }
    }

    #[test]
    fn a_message_stops_the_one_being_sent() {
        let (mut p, r) = recording_panel();
        assert!(with_view(|v| p.function_key(1, false, v)));
        assert!(with_view(|v| p.function_key(2, false, v)));
        assert_eq!(
            calls(&r),
            [
                "abort",
                "send CQ TEST JR1ABC TEST",
                "abort",
                "send JA1ZLO 5NN10H"
            ]
        );
    }

    #[test]
    fn an_empty_message_sends_nothing() {
        let (mut p, r) = recording_panel();
        // F4 and F10 are empty by default, bank B entirely; spaces count as empty.
        assert!(with_view(|v| p.function_key(4, false, v)));
        assert!(with_view(|v| p.function_key(10, true, v)));
        p.settings.banks[0][4] = "   ".into();
        assert!(with_view(|v| p.function_key(5, false, v)));
        assert!(calls(&r).is_empty());
    }

    #[test]
    fn function_keys_only_on_cw() {
        let (mut p, r) = recording_panel();
        for mode in [Mode::Ssb, Mode::Fm, Mode::Am, Mode::Rtty] {
            assert!(!with_mode(mode, |v| p.function_key(1, false, v)));
            assert!(!with_mode(mode, |v| p.function_key(1, true, v)));
        }
        assert!(calls(&r).is_empty());
        assert!(with_mode(Mode::Cw, |v| p.function_key(9, false, v)));
        assert_eq!(calls(&r), ["abort", "send TU"]);
    }

    #[test]
    fn function_key_numbers_out_of_range() {
        let (mut p, r) = recording_panel();
        for n in [0, 13, 255] {
            assert!(!with_view(|v| p.function_key(n, false, v)));
            assert!(!with_view(|v| p.function_key(n, true, v)));
        }
        assert!(calls(&r).is_empty());
    }

    #[test]
    fn shift_plays_the_other_bank() {
        let (mut p, r) = recording_panel();
        p.settings.banks[1][0] = "QRZ?".into();
        p.settings.banks[1][11] = "$M".into();
        assert!(with_view(|v| p.function_key(1, true, v)));
        assert!(with_view(|v| p.function_key(12, true, v)));
        assert_eq!(calls(&r), ["abort", "send QRZ?", "abort", "send JR1ABC"]);
        // With bank B selected, Shift plays bank A.
        p.bank = 1;
        assert!(with_view(|v| p.function_key(1, false, v)));
        assert!(with_view(|v| p.function_key(3, true, v)));
        assert_eq!(
            calls(&r),
            ["abort", "send QRZ?", "abort", "send TU JR1ABC TEST"]
        );
    }

    #[test]
    fn escape_stops_cw_and_cq_repeat() {
        let (mut p, r) = recording_panel();
        // Nothing to stop.
        assert!(!p.escape());
        r.busy.store(true, Ordering::SeqCst);
        assert!(p.escape());
        assert!(!r.busy());
        assert!(calls(&r).contains(&"abort".to_string()));
        // A CQ repeat running between CQs.
        p.cq = Arc::new(AtomicBool::new(false));
        let cq = p.cq.clone();
        assert!(p.escape());
        assert!(cq.load(Ordering::SeqCst));
        assert!(!p.escape());
        // Without a keyer.
        let mut q = KeyerPanel::default();
        assert!(!q.escape());
    }

    #[test]
    fn paddle_stops_message_and_cq_repeat() {
        let (mut p, r) = recording_panel();
        r.busy.store(true, Ordering::SeqCst);
        p.cq = Arc::new(AtomicBool::new(false));
        let cq = p.cq.clone();
        r.events.lock().unwrap().push(KeyerEvent::Paddle(true));
        with_view(|v| p.tick(v));
        assert!(cq.load(Ordering::SeqCst));
        assert!(!r.busy());
        assert_eq!(calls(&r), ["abort"]);
    }

    #[test]
    fn paddle_release_and_other_events_stop_nothing() {
        let (mut p, r) = recording_panel();
        r.busy.store(true, Ordering::SeqCst);
        p.cq = Arc::new(AtomicBool::new(false));
        let cq = p.cq.clone();
        r.events.lock().unwrap().extend([
            KeyerEvent::Paddle(false),
            KeyerEvent::Finished,
            KeyerEvent::Speed(30),
        ]);
        with_view(|v| p.tick(v));
        assert!(!cq.load(Ordering::SeqCst));
        assert!(r.busy());
        assert!(calls(&r).is_empty());
        // No events at all.
        with_view(|v| p.tick(v));
        assert!(!cq.load(Ordering::SeqCst));
    }

    #[test]
    fn paddle_once_among_many_events_aborts_once() {
        let (mut p, r) = recording_panel();
        r.events.lock().unwrap().extend([
            KeyerEvent::Paddle(true),
            KeyerEvent::Paddle(false),
            KeyerEvent::Paddle(true),
        ]);
        with_view(|v| p.tick(v));
        assert_eq!(calls(&r), ["abort"]);
    }

    #[test]
    fn tick_keeps_the_entry_row_for_cq_repeat() {
        let (mut p, _r) = recording_panel();
        with_view(|v| p.tick(v));
        let s = p.shared.lock().unwrap();
        assert_eq!(s.fields.callsign, "JA1ZLO");
        assert_eq!(s.fields.my_call, "JR1ABC");
    }

    fn action_ids(p: &KeyerPanel) -> Vec<(String, bool, bool)> {
        p.commands("en")
            .into_iter()
            .map(|b| (b.label, b.enabled, b.selected))
            .collect()
    }

    #[test]
    fn toolbar_cq_and_stop_need_a_keyer() {
        let p = KeyerPanel::default();
        assert_eq!(
            action_ids(&p)[1..],
            [("CQ".into(), false, false), ("Stop".into(), false, false)]
        );
    }

    /// CQ on the toolbar starts the CQ repeat with the entry row kept by tick and
    /// shows it pressed; CQ again or Stop stops it.
    #[test]
    fn toolbar_cq_toggles_and_stop_stops() {
        let mut p = KeyerPanel::default();
        let fast = Timing {
            wpm: 60,
            ..Timing::default()
        };
        p.shared.lock().unwrap().keyer = Some(Box::new(SimKeyer::new(fast)));
        p.settings.banks[0][0] = "CQ $M".into();
        p.settings.cq_repeat = 10.0;
        with_view(|v| p.tick(v));
        p.run_command("cq");
        assert!(p.cq_running());
        assert_eq!(action_ids(&p)[1], ("CQ".into(), true, true));
        p.run_command("cq");
        assert!(!p.cq_running());
        p.run_command("cq");
        assert!(p.cq_running());
        p.run_command("stop");
        assert!(!p.cq_running());
        assert_eq!(action_ids(&p)[1], ("CQ".into(), true, false));
        // Stop also aborts what is being sent.
        let (mut q, r) = recording_panel();
        r.busy.store(true, Ordering::SeqCst);
        q.run_command("stop");
        assert!(!r.busy());
        q.run_command("unknown");
    }

    /// The toolbar CQ sends the F1 message expanded with the entry row.
    #[test]
    fn toolbar_cq_sends_the_f1_message() {
        let (mut p, r) = recording_panel();
        p.settings.banks[0][0] = "CQ $M".into();
        p.settings.cq_max = 1;
        with_view(|v| p.tick(v));
        p.run_command("cq");
        for _ in 0..100 {
            if !p.cq_running() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(calls(&r).contains(&"send CQ JR1ABC".to_string()));
        // An empty message starts nothing.
        p.settings.banks[0][0].clear();
        p.run_command("cq");
        assert!(!p.cq_running());
    }
}
