//! SO2R, single operator two radios (`USo2rNeoCp`, the SO2R part of `UzLogKeyer`):
//! the SO2R Neo control panel, and an SO2R box told which rig transmits and which
//! one the operator hears.
//!
//! - The rig in use is the application's (RIG1 or RIG2, [`View::rig`], changed with
//!   Shift+X). When it changes, the box is switched to it, TX and RX, and the panel
//!   shows that rig with the AF blend off (`SwitchRig`, `DispRig1State`).
//! - RIG1 and RIG2 in the panel select that rig ([`EntryAction::SelectRig`]), or,
//!   when it is already in use, listen to it again; Both listens to both rigs and
//!   turns the AF blend on (`DispRigBothState`).
//! - The AF blend can be changed only while listening to both; moving the ratio
//!   (0 %, 50 %, 100 % or the slider) always sends the blend on with the ratio
//!   (`trackBlendRatioChange`).
//! - With RX Auto Select on, keying the transmitter listens to the other rig, and
//!   unkeying it to both unless Cancel RX Select was chosen, which lasts one
//!   transmission (`So2rNeoReverseRx`, `So2rNeoNormalRx`). zLog does this around CW
//!   messages sent by WinKeyer; here it follows the PTT requests of the panels
//!   ([`Panel::transmit`]), which the CW keyer does not make.
//!
//! The box has a port of its own: OTRSP at 9600 bps, SO2R Neo at WinKeyer's
//! 1200 bps. zLog sends the SO2R Neo commands on the WinKeyer port, shared with
//! the keyer; "SO2R Neo on the keyer port" does that, the panel handing its bytes
//! to the keyer ([`EntryAction::KeyerPort`], [`KeyerPort`]).

//!
//! While a box is connected, the messages of the keyer and the voice memories go
//! through the play queue ([`zlog_ui::queue`]), shown in the window of [`queue`].

pub mod neo;
pub mod otrsp;
pub mod queue;
pub mod switch;

use egui::{Color32, Ui};
use zlog_ui::{Command, EntryAction, Panel, Section, View};

use crate::neo::Rx;
pub use crate::queue::QueuePanel;
pub use crate::switch::{Device, Switch};

zlog_core::texts!();

/// The transport of SO2R Neo on the keyer's port: the bytes written wait here
/// until the panel hands them to the keyer.
#[derive(Clone, Default)]
pub struct KeyerPort(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl KeyerPort {
    /// The bytes written since the last call.
    pub fn take(&self) -> Vec<u8> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

impl zlog_rig::Transport for KeyerPort {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    }

    fn read(&mut self, _buf: &mut Vec<u8>) -> std::io::Result<()> {
        Ok(())
    }
}

/// The SO2R Neo Control Panel window (`TformSo2rNeoCp`).
pub struct So2rPanel {
    device: Device,
    port: String,
    ports: Vec<String>,
    switch: Option<Switch>,
    /// Why the port did not open.
    message: Option<String>,
    /// The rig in use last seen, to switch the box when it changes.
    rig: Option<u8>,
    /// RIG1, RIG2 or Both, as pressed in the panel (`GetRx`).
    rx: Rx,
    /// AF Blend pressed, and its ratio in percent (`trackBlendRatio`, 50 at first).
    blend: bool,
    ratio: u8,
    /// RX Auto Select (`ToggleSwitch1`, `So2rNeoUseRxSelect`), off at first.
    auto_rx: bool,
    /// Cancel RX Select (`So2rNeoCanRxSel`) for the next transmission.
    cancel_rx: bool,
    ptt: bool,
    actions: Vec<EntryAction>,
    /// What SO2R Neo on the keyer's port has to send.
    keyer_port: KeyerPort,
    /// The play queue of SO2R, used while a box is connected.
    queue: zlog_ui::queue::Shared,
}

impl Default for So2rPanel {
    fn default() -> Self {
        So2rPanel {
            device: Device::None,
            port: String::new(),
            ports: Vec::new(),
            switch: None,
            message: None,
            rig: None,
            rx: Rx::Rig1,
            blend: false,
            ratio: 50,
            auto_rx: false,
            cancel_rx: false,
            ptt: false,
            actions: Vec::new(),
            keyer_port: KeyerPort::default(),
            queue: zlog_ui::queue::shared(),
        }
    }
}

impl So2rPanel {
    /// Use a box on `transport` (a serial port, or a fake one in tests).
    pub fn attach(&mut self, device: Device, transport: Box<dyn zlog_rig::Transport>) {
        self.device = device;
        self.switch = Some(Switch::new(device, transport));
        // The box is told the rig in use at the next frame (zLog `SwitchRig(1)` at start).
        self.rig = None;
        self.message = None;
    }

    fn connect(&mut self) {
        // Close the box's port first: on Windows a port opens only once.
        self.switch = None;
        let (baud, stop) = match self.device {
            Device::None => return,
            Device::NeoKeyer => {
                let port = self.keyer_port.clone();
                return self.attach(Device::NeoKeyer, Box::new(port));
            }
            Device::Neo => (1200, zlog_rig::serial::StopBits::Two),
            Device::Otrsp => (otrsp::BAUD, zlog_rig::serial::StopBits::One),
        };
        match zlog_rig::serial::SerialTransport::open(&self.port, baud, stop) {
            Ok(t) => self.attach(self.device, Box::new(t)),
            Err(e) => self.message = Some(e),
        }
    }

    fn disconnect(&mut self) {
        self.switch = None;
    }

    /// Show `rx` pressed (`DispRig1State`, `DispRig2State`, `DispRigBothState`).
    fn show_rx(&mut self, rx: Rx) {
        self.rx = rx;
        self.blend = rx == Rx::Both;
        self.send_blend(self.blend);
    }

    fn send_blend(&mut self, on: bool) {
        let ratio = self.ratio;
        if let Some(s) = self.switch.as_mut() {
            s.blend(on, ratio);
        }
    }

    /// RIG1, RIG2 or Both pressed (`buttonRigClick`, `OnZLogSo2rSelRx`).
    fn press_rx(&mut self, rx: Rx, current: Option<u8>) {
        let rig = match rx {
            Rx::Rig1 => Some(1),
            Rx::Rig2 => Some(2),
            Rx::Both => None,
        };
        match rig {
            Some(n) if current != Some(n) => self.actions.push(EntryAction::SelectRig(n)),
            _ => {
                if let Some(s) = self.switch.as_mut() {
                    s.listen(rx);
                }
                self.show_rx(rx);
            }
        }
    }

    /// AF Blend pressed or released (`buttonAfBlendClick`); only while on Both.
    fn toggle_blend(&mut self) {
        if self.rx == Rx::Both {
            self.blend = !self.blend;
            self.send_blend(self.blend);
        }
    }

    /// The ratio moved (`trackBlendRatioChange`): the blend is sent on.
    fn set_ratio(&mut self, ratio: u8) {
        if ratio != self.ratio {
            self.ratio = ratio;
            self.send_blend(true);
        }
    }

    /// zLog's SO2R actions, which have no key by default: #135 to #137 (RIG1,
    /// RIG2, Both), #144 (RX Auto Select), #141 (Cancel RX Select), #168 (AF Blend).
    fn act(&mut self, id: &str) {
        let current = self.rig;
        match id {
            "rx1" => self.press_rx(Rx::Rig1, current),
            "rx2" => self.press_rx(Rx::Rig2, current),
            "both" => self.press_rx(Rx::Both, current),
            "auto_rx" => self.auto_rx = !self.auto_rx,
            "cancel_rx" => self.cancel_rx = !self.cancel_rx,
            "af_blend" => self.toggle_blend(),
            _ => {}
        }
    }

    fn device_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.horizontal(|ui| {
            ui.label(t("device"));
            egui::ComboBox::from_id_salt("so2r-device")
                .selected_text(t(self.device.key()))
                .show_ui(ui, |ui| {
                    for d in Device::ALL {
                        ui.selectable_value(&mut self.device, d, t(d.key()));
                    }
                });
            if !matches!(self.device, Device::None | Device::NeoKeyer) {
                ui.label(t("port"));
                let combo = egui::ComboBox::from_id_salt("so2r-port")
                    .selected_text(self.port.clone())
                    .show_ui(ui, |ui| {
                        for p in &self.ports {
                            ui.selectable_value(&mut self.port, p.clone(), p);
                        }
                    });
                if combo.response.clicked() {
                    self.ports = zlog_rig::serial::available_ports()
                        .map(|v| v.into_iter().map(|p| p.port_name).collect())
                        .unwrap_or_default();
                }
            }
            if self.switch.is_some() {
                if ui.button(t("disconnect")).clicked() {
                    self.disconnect();
                }
            } else if self.device != Device::None && ui.button(t("connect")).clicked() {
                self.connect();
            }
        });
        let error = self.switch.as_ref().and_then(|s| s.error.clone());
        if let Some(e) = error.or_else(|| self.message.clone()) {
            ui.colored_label(ui.visuals().warn_fg_color, e);
        }
    }
}

/// A lamp and its label (`TJvLED`).
fn led(ui: &mut Ui, on: bool, label: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        let colour = if on {
            Color32::from_rgb(0, 200, 0)
        } else {
            ui.visuals().weak_text_color()
        };
        ui.painter().circle_filled(rect.center(), 5.0, colour);
        if !label.is_empty() {
            ui.label(label);
        }
    });
}

impl Panel for So2rPanel {
    fn id(&self) -> &str {
        "so2r"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Rig, 10)
    }

    fn default_size(&self) -> [f32; 2] {
        [420.0, 280.0]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let t = |k| texts().get(language, k);
        vec![Command::window(t("toolbar"), t("toolbar.tip")).icon(zlog_ui::icon::BROADCAST)]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let t = |k| texts().get(lang, k);
        self.device_ui(ui, lang);
        ui.separator();
        ui.horizontal(|ui| {
            led(ui, self.ptt, "PTT");
            led(ui, self.cancel_rx, t("cancel_rx"));
            if ui.small_button(t("toggle")).clicked() {
                self.act("cancel_rx");
            }
        });
        ui.group(|ui| {
            ui.label(t("af"));
            ui.add_enabled_ui(self.rx == Rx::Both, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.blend, t("af_blend")).clicked() {
                        self.act("af_blend");
                    }
                    for p in [100, 0, 50] {
                        if ui.button(format!("{p}%")).clicked() {
                            self.set_ratio(p);
                        }
                    }
                });
                let mut ratio = self.ratio;
                ui.add(egui::Slider::new(&mut ratio, 0..=100).suffix("%"));
                self.set_ratio(ratio);
            });
        });
        ui.group(|ui| {
            ui.label(t("rx_select"));
            ui.horizontal(|ui| {
                for (rx, label) in [(Rx::Rig1, "RIG1"), (Rx::Rig2, "RIG2"), (Rx::Both, "Both")] {
                    ui.vertical(|ui| {
                        if ui.selectable_label(self.rx == rx, label).clicked() {
                            self.press_rx(rx, Some(view.rig));
                        }
                        led(ui, self.rx == rx, "");
                    });
                }
                ui.separator();
                let key = if self.auto_rx { "on" } else { "off" };
                if ui.selectable_label(self.auto_rx, t(key)).clicked() {
                    self.act("auto_rx");
                }
            });
        });
    }

    /// Switch the box when the rig in use changed; messages go through the play
    /// queue while a box is connected.
    fn tick(&mut self, view: &View<'_>) {
        let so2r = self.switch.is_some();
        let mut q = self.queue.lock().unwrap();
        if q.so2r != so2r {
            q.so2r = so2r;
            q.clear();
        }
        drop(q);
        if self.rig == Some(view.rig) {
            return;
        }
        self.rig = Some(view.rig);
        if let Some(s) = self.switch.as_mut() {
            s.select_rig(view.rig);
        }
        self.show_rx(Rx::of(view.rig));
    }

    /// Follow PTT (the lamp, and RX Auto Select of SO2R Neo); always left to the
    /// panel that drives the PTT line.
    fn transmit(&mut self, on: bool) -> bool {
        self.ptt = on;
        let (auto, cancel) = (self.auto_rx, self.cancel_rx);
        if let Some(s) = self.switch.as_mut().filter(|s| s.device == Device::Neo) {
            match on {
                true if auto => s.listen(neo::reverse_rx(s.tx)),
                false if auto && !cancel => s.listen(Rx::Both),
                _ => {}
            }
        }
        if !on {
            self.cancel_rx = false;
        }
        false
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        let bytes = self.keyer_port.take();
        if !bytes.is_empty() {
            self.actions.push(EntryAction::KeyerPort(bytes));
        }
        std::mem::take(&mut self.actions)
    }

    /// Restore the box and reopen it.
    fn load(&mut self, section: &Section) {
        self.device = Device::from_key(section.get("device").map_or("", |s| s.as_str()));
        self.port = section.get("port").cloned().unwrap_or_default();
        self.connect();
    }

    fn save(&self, section: &mut Section) {
        section.insert("device".into(), self.device.key().into());
        section.insert("port".into(), self.port.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::switch::tests::Wire;
    use zlog_core::{
        Band, Contest, ContestMode, Log, Mode, Power, Qso, ScoreSheet, Scoring, Station,
    };

    struct Nothing;

    impl Contest for Nothing {
        fn name(&self) -> &str {
            "test"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::B50)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Cw
        }
        fn sent_number(&self, _band: Band, _power: Power, _station: &Station) -> String {
            String::new()
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

    fn with_view<R>(rig: u8, f: impl FnOnce(&View<'_>) -> R) -> R {
        let log = Log::default();
        let sheet = ScoreSheet::default();
        let station = Station::default();
        let view = View {
            log: &log,
            contest: &Nothing,
            sheet: &sheet,
            callsign: "",
            station: &station,
            number: "",
            rst_sent: "599",
            power: Power::P010,
            band: Band::B7,
            mode: Mode::Cw,
            language: "en",
            spots: &[],
            hz: None,
            rig,
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

    fn panel(device: Device) -> (Wire, So2rPanel) {
        let w = Wire::default();
        let mut p = So2rPanel {
            queue: zlog_ui::queue::Shared::default(),
            ..Default::default()
        };
        p.attach(device, Box::new(w.clone()));
        (w, p)
    }

    #[test]
    fn reconnecting_closes_the_old_port_first() {
        let (_w, mut p) = panel(Device::Otrsp);
        p.port = "/nonexistent/zlog-so2r-port".into();
        p.connect();
        // The old box was dropped before the open, which failed.
        assert!(p.switch.is_none());
        assert!(p.message.is_some());
    }

    #[test]
    fn the_box_follows_the_rig_in_use() {
        let (w, mut p) = panel(Device::Neo);
        with_view(1, |v| p.tick(v));
        // TX and RX on rig 1, then the AF blend off.
        assert_eq!(w.take(), [0x90, 0x90, 0x86]);
        with_view(1, |v| p.tick(v));
        assert!(w.take().is_empty());
        with_view(2, |v| p.tick(v));
        assert_eq!(w.take(), [0x94, 0x95, 0x86]);
        assert_eq!(p.rx, Rx::Rig2);
        // Connected, messages go through the play queue.
        assert!(p.queue.lock().unwrap().so2r);
        p.disconnect();
        with_view(2, |v| p.tick(v));
        assert!(!p.queue.lock().unwrap().so2r);
    }

    #[test]
    fn rig_buttons_select_the_rig_or_listen_to_it() {
        let (w, mut p) = panel(Device::Neo);
        with_view(1, |v| p.tick(v));
        w.take();
        // RIG2 asks the application for rig 2; the box follows at the next tick.
        p.act("rx2");
        assert_eq!(p.take_actions(), [EntryAction::SelectRig(2)]);
        assert!(w.take().is_empty());
        // Both: listen to both rigs, with the AF blend at 50 %.
        p.act("both");
        assert_eq!(w.take(), [0x92, 0x87, 0xC7, 0xCF]);
        assert!(p.blend);
        // AF Blend released, then the ratio moved: the blend goes on again.
        p.act("af_blend");
        p.set_ratio(100);
        assert_eq!(w.take(), [0x86, 0x87, 0xCF, 0xCF]);
        // RIG1, the rig in use: listen to it again, blend off.
        p.act("rx1");
        assert!(p.take_actions().is_empty());
        assert_eq!(w.take(), [0x90, 0x86]);
        // The AF blend cannot be changed while on one rig.
        p.act("af_blend");
        assert!(w.take().is_empty() && !p.blend);
    }

    #[test]
    fn rx_auto_select_around_transmissions() {
        let (w, mut p) = panel(Device::Neo);
        with_view(1, |v| p.tick(v));
        w.take();
        // Off: PTT only lights the lamp.
        assert!(!p.transmit(true));
        assert!(p.ptt);
        p.transmit(false);
        assert!(w.take().is_empty());
        p.act("auto_rx");
        p.transmit(true);
        p.transmit(false);
        // Sending on rig 1, listen to rig 2; afterwards to both.
        assert_eq!(w.take(), [0x91, 0x92]);
        // Cancel RX Select keeps the other rig after one transmission.
        p.act("cancel_rx");
        p.transmit(true);
        p.transmit(false);
        assert_eq!(w.take(), [0x91]);
        assert!(!p.cancel_rx);
    }

    #[test]
    fn otrsp_gets_tx_and_rx() {
        let (w, mut p) = panel(Device::Otrsp);
        with_view(1, |v| p.tick(v));
        with_view(2, |v| p.tick(v));
        assert_eq!(w.take(), b"TX1\rRX1\rTX2\rRX2\r");
        p.act("both");
        p.act("auto_rx");
        p.transmit(true);
        assert!(w.take().is_empty());
    }

    /// SO2R Neo on the keyer's port sends its bytes through the keyer.
    #[test]
    fn neo_on_the_keyer_port_goes_through_the_keyer() {
        let mut p = So2rPanel {
            device: Device::NeoKeyer,
            queue: zlog_ui::queue::Shared::default(),
            ..Default::default()
        };
        p.connect();
        assert!(p.switch.is_some() && p.message.is_none());
        with_view(1, |v| p.tick(v));
        assert_eq!(
            p.take_actions(),
            [EntryAction::KeyerPort(vec![0x90, 0x90, 0x86])]
        );
        p.act("both");
        p.act("rx2");
        assert_eq!(
            p.take_actions(),
            [
                EntryAction::SelectRig(2),
                EntryAction::KeyerPort(vec![0x92, 0x87, 0xC7, 0xCF])
            ]
        );
        assert!(p.take_actions().is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let p = So2rPanel {
            device: Device::Otrsp,
            port: "COM7".into(),
            ..Default::default()
        };
        let mut s = Section::new();
        p.save(&mut s);
        let mut q = So2rPanel::default();
        // The port does not exist: the panel says so and stays unconnected.
        q.load(&s);
        assert_eq!((q.device, q.port.as_str()), (Device::Otrsp, "COM7"));
        assert!(q.switch.is_none() && q.message.is_some());
    }

    /// Both lang files have the same keys, and every key the panel uses is there.
    #[test]
    fn texts_exist_in_both_languages() {
        let keys = |text: &str| -> Vec<String> {
            let mut k: Vec<String> = text
                .lines()
                .filter(|l| !l.starts_with('#'))
                .filter_map(|l| l.split_once(" = ").map(|(k, _)| k.to_string()))
                .collect();
            k.sort();
            k
        };
        let en = keys(include_str!("../lang/en.lang"));
        assert_eq!(en, keys(include_str!("../lang/ja.lang")));
        for k in [
            "title",
            "toolbar",
            "toolbar.tip",
            "device",
            "port",
            "connect",
            "disconnect",
            "none",
            "neo",
            "neo_keyer",
            "otrsp",
            "cancel_rx",
            "toggle",
            "af",
            "af_blend",
            "rx_select",
            "on",
            "off",
        ] {
            assert!(en.contains(&k.to_string()), "{k}");
        }
    }
}
