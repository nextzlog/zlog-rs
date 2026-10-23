//! RTTY (`UTTYConsole`, `URttyOptions`, the RTTY settings of `UOptions`): a
//! console that decodes RTTY from the audio input, lists the callsigns heard and
//! puts a clicked one into the entry row, takes the number after a `599`, and sends
//! the RTTY messages of F1 to F12 and typed text as AFSK with PTT through the keyer.
//!
//! zLog has no demodulator of its own: it starts MMTTY and talks to it by Windows
//! messages (`UMMTTY`), and keys FSK itself on a serial line. Here the modem is
//! this crate's own ([`fsk`], [`afsk`]), so it runs on macOS and Windows alike.
//!
//! Not implemented: MMTTY and its window messages (Windows only), FSK keying of a
//! serial line's TXD (`UseFskKeying`; serial ports cannot send 45.45 baud
//! portably), a console per rig in SO2R, the callsign filter as a regular
//! expression (`CallsignFilter`; zLog's default one is built in), colours per
//! keyword of `ColorCoding` (one highlight colour here), and the shortcut that
//! grabs the next callsign of the list (`actionRttyGrab`).

pub mod afsk;
pub mod audio;
pub mod baudot;
mod console;
pub mod fsk;
pub mod modem;
pub mod sender;
mod settings;
pub mod terminal;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::Ui;
use zlog_core::Mode;
use zlog_keyer::Abbrev;
use zlog_ui::{EntryAction, EntryKey, Panel, Section, View};

pub use settings::{Settings, BAUDS, DEFAULT_MESSAGES, SCHEMES};

use crate::fsk::Fsk;
use crate::modem::{Demodulator, Params};
use crate::sender::Sender;
use crate::terminal::Terminal;

zlog_core::texts!();

/// Numbers as they are: RTTY messages do not cut them (`SetStrNoAbbrev`).
const NO_CUT: Abbrev = Abbrev {
    zero: '0',
    one: '1',
    nine: '9',
};

/// Reception: the input device and the demodulator of its rate.
struct Receiver {
    input: audio::Input,
    device: String,
    params: Params,
    fsk: Fsk,
}

/// The RTTY console.
pub struct RttyPanel {
    settings: Settings,
    terminal: Terminal,
    receiver: Option<Receiver>,
    sender: Sender<audio::Output>,
    /// Text typed to send.
    typed: String,
    /// The text being sent, shown under the console.
    sending: String,
    outputs: Vec<String>,
    inputs: Vec<String>,
    status: String,
    my_call: String,
    actions: Vec<EntryAction>,
}

impl Default for RttyPanel {
    fn default() -> Self {
        RttyPanel {
            settings: Settings::default(),
            terminal: Terminal::default(),
            receiver: None,
            sender: Sender::new(audio::Output::default()),
            typed: String::new(),
            sending: String::new(),
            outputs: Vec::new(),
            inputs: Vec::new(),
            status: String::new(),
            my_call: String::new(),
            actions: Vec::new(),
        }
    }
}

/// Seconds since the Unix epoch and the PC's local hour, for `$D` and `$G`.
fn now() -> (u64, u32) {
    use chrono::Timelike;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (secs, chrono::Local::now().hour())
}

/// Message `template` with its `$`-macros (those of the keyer) for the entry row.
pub fn expand(template: &str, view: &View<'_>) -> String {
    let (secs, hour) = now();
    let fields = zlog_keyer::Settings::default().fields(view, secs, hour);
    zlog_keyer::expand(template, &fields, &NO_CUT)
}

impl RttyPanel {
    fn report(&mut self, r: Result<(), String>) {
        match r {
            Ok(()) => self.status.clear(),
            Err(e) => self.status = e,
        }
    }

    pub(crate) fn receiving(&self) -> bool {
        self.receiver.is_some()
    }

    /// Start or stop taking audio from the input device.
    pub(crate) fn toggle_receive(&mut self) {
        if self.receiver.take().is_some() {
            return;
        }
        let r = self.start_receive();
        self.report(r);
    }

    fn start_receive(&mut self) -> Result<(), String> {
        let input = audio::Input::start(&self.settings.input)?;
        let params = self.settings.params();
        let mut fsk = Fsk::new(params, input.rate);
        fsk.squelch = self.settings.squelch;
        self.receiver = Some(Receiver {
            fsk,
            input,
            device: self.settings.input.clone(),
            params,
        });
        Ok(())
    }

    /// Decode what came in; a change of device or signal restarts reception.
    fn receive(&mut self) {
        let Some(r) = self.receiver.as_mut() else {
            return;
        };
        if r.device != self.settings.input {
            self.receiver = None;
            let r = self.start_receive();
            self.report(r);
            return;
        }
        if r.params != self.settings.params() {
            r.params = self.settings.params();
            r.fsk = Fsk::new(r.params, r.input.rate);
        }
        r.fsk.squelch = self.settings.squelch;
        let text = r.fsk.demodulate(&r.input.take());
        let listening = !self.sender.busy();
        for c in text.chars() {
            if let Some(n) = self.terminal.push(c, &self.my_call, listening) {
                self.actions.push(EntryAction::Number(n));
            }
        }
    }

    /// The clarity of the last character, 0 to 1, while receiving.
    pub(crate) fn level(&self) -> Option<f64> {
        self.receiver.as_ref().map(|r| r.fsk.level)
    }

    /// Send `text` on a line of its own (`SendStrNow`), after what is being sent.
    pub(crate) fn send(&mut self, text: &str) {
        let text = text.trim().to_uppercase();
        if text.is_empty() {
            return;
        }
        self.sender.sink.device = self.settings.output.clone();
        if !self.sender.busy() {
            self.sending.clear();
        }
        self.sending.push_str(&text);
        self.sending.push(' ');
        self.sender.send(
            &format!("\r\n{text}\r\n"),
            self.settings.timing(),
            Instant::now(),
        );
    }

    /// Send message `n` (0..12) expanded for the entry row; false when it is empty.
    pub(crate) fn play(&mut self, n: usize, view: &View<'_>) -> bool {
        let text = expand(&self.settings.messages[n], view);
        if text.trim().is_empty() {
            return false;
        }
        self.send(&text);
        true
    }

    pub(crate) fn stop(&mut self) -> bool {
        self.sending.clear();
        self.sender.abort()
    }

    pub(crate) fn pick(&mut self, callsign: &str) {
        self.actions.push(EntryAction::Callsign(callsign.into()));
    }
}

impl RttyPanel {
    /// In RTTY, F1 to F12 send their message, Shift too (zLog has one RTTY bank).
    fn function_key(&mut self, n: u8, view: &View<'_>) -> bool {
        (1..=12).contains(&n) && view.mode == Mode::Rtty && self.play(n as usize - 1, view)
    }

    fn escape(&mut self) -> bool {
        self.stop()
    }
}

impl Panel for RttyPanel {
    fn id(&self) -> &str {
        "rtty"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Digital, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::TERMINAL_WINDOW)]
    }

    /// Hidden until chosen: most stations run CW and phone.
    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [760.0, 520.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        self.console_ui(ui, view);
        if self.receiving() || self.sender.busy() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }

    /// Reception and sending go on while the window is closed.
    fn tick(&mut self, view: &View<'_>) {
        self.my_call = view.station.callsign.to_uppercase();
        // Out of RTTY (to CW, say), a transmission stops.
        if view.mode != Mode::Rtty && self.sender.busy() {
            self.stop();
        }
        self.receive();
        let r = self.sender.tick(
            Instant::now(),
            self.settings.timing(),
            self.settings.params(),
        );
        if r.is_err() {
            self.report(r);
        }
        if !self.sender.busy() {
            self.sending.clear();
        }
    }

    fn on_key(&mut self, key: EntryKey, view: &View<'_>) -> bool {
        match key {
            EntryKey::Function { n, .. } => self.function_key(n, view),
            EntryKey::Escape => self.escape(),
            EntryKey::Typed => false,
        }
    }

    /// Callsigns and numbers picked, and PTT on and off around each transmission,
    /// carried out by the keyer.
    fn take_actions(&mut self) -> Vec<EntryAction> {
        let mut a = std::mem::take(&mut self.actions);
        a.extend(
            self.sender
                .take_requests()
                .into_iter()
                .map(EntryAction::Transmit),
        );
        a
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
    fn escape_without_sending_does_nothing() {
        let mut p = RttyPanel::default();
        assert!(!p.escape());
        assert!(p.take_actions().is_empty());
    }

    #[test]
    fn messages_keep_their_numbers() {
        let f = zlog_keyer::Fields {
            callsign: "JA1ZLO".into(),
            rst_sent: "599".into(),
            sent: "100110M".into(),
            ..Default::default()
        };
        assert_eq!(
            zlog_keyer::expand(DEFAULT_MESSAGES[1], &f, &NO_CUT),
            "JA1ZLO 599 100110M 100110M JA1ZLO"
        );
    }
}
