pub mod driver;
pub mod options;
pub mod queue;

use egui::Ui;
use zlog_ui::device::{DeviceDriver, DriverChoice, PortHub, NONE};
use zlog_ui::tx::{TxCommand, TxHub};
use zlog_ui::{layout, Command, EntryAction, Panel, Section, View};

pub use crate::driver::{with_simulator, Rx, So2r, So2rCaps, So2rDrv, SIMULATOR};
pub use crate::queue::QueuePanel;

zlog_core::texts!();

pub const TOGGLE_TX_KEY: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::V);

pub struct So2rPanel {
    drivers: Vec<Box<dyn So2rDrv>>,
    ports: PortHub,
    device: DriverChoice,
    hub: TxHub,
    lease: Lease,
    reopened: u64,
    message: Option<String>,
    rig: Option<u8>,
    tx: u8,
    rx: Rx,
    blend: bool,
    ratio: u8,
    auto_rx: bool,
    cancel_rx: bool,
    ptt: bool,
    actions: Vec<EntryAction>,
    queue: zlog_ui::queue::Shared,
    view: haml_ui::So2rPanel,
}

impl Default for So2rPanel {
    fn default() -> Self {
        So2rPanel {
            drivers: Vec::new(),
            ports: PortHub::new(),
            device: DriverChoice::new(NONE),
            hub: TxHub::default(),
            lease: Lease::default(),
            reopened: zlog_ui::device::reopened(),
            message: None,
            rig: None,
            tx: 1,
            rx: Rx::Rig1,
            blend: false,
            ratio: 50,
            auto_rx: false,
            cancel_rx: false,
            ptt: false,
            actions: Vec::new(),
            queue: zlog_ui::queue::shared(),
            view: haml_ui::So2rPanel::new("en"),
        }
    }
}

impl So2rPanel {
    pub fn new(drivers: Vec<Box<dyn So2rDrv>>, ports: PortHub) -> Self {
        So2rPanel {
            drivers,
            ports,
            ..So2rPanel::default()
        }
    }

    pub fn on(mut self, hub: TxHub) -> Self {
        self.hub = hub;
        self
    }

    pub fn attach(&mut self, so2r: Box<dyn So2r>) {
        self.hub.set_so2r(Some(so2r));
        self.lease = Lease(Some(self.hub.clone()));
        self.rig = None;
        self.message = None;
    }

    fn connected(&self) -> bool {
        self.hub.has_so2r()
    }

    fn listen(&mut self, rx: Rx) {
        if self.connected() {
            self.hub.send(TxCommand::Listen(rx));
        }
    }

    pub fn tx(&self) -> u8 {
        self.tx
    }

    fn toggle_tx(&mut self) {
        self.tx = 3 - self.tx;
        self.queue.lock().unwrap().clear();
        self.actions.push(EntryAction::StopSending);
        if self.connected() {
            self.hub.transmit_on(self.tx);
        }
    }

    fn connect(&mut self) {
        // Close the box's port first: on Windows a port opens only once.
        self.hub.set_so2r(None);
        let Some(drv) = self.device.find(&self.drivers) else {
            return;
        };
        match drv.open(&self.device.section(), &self.ports) {
            Ok(b) => self.attach(b),
            Err(e) => self.message = Some(e),
        }
    }

    fn disconnect(&mut self) {
        self.hub.set_so2r(None);
    }

    fn show_rx(&mut self, rx: Rx) {
        self.rx = rx;
        self.blend = rx == Rx::Both;
        self.send_blend(self.blend);
    }

    fn send_blend(&mut self, on: bool) {
        if self.connected() {
            self.hub.send(TxCommand::Blend(on, self.ratio));
        }
    }

    fn press_rx(&mut self, rx: Rx, current: Option<u8>) {
        let rig = match rx {
            Rx::Rig1 => Some(1),
            Rx::Rig2 => Some(2),
            Rx::Both => None,
        };
        match rig {
            Some(n) if current != Some(n) => self.actions.push(EntryAction::SelectRig(n)),
            _ => {
                self.listen(rx);
                self.show_rx(rx);
            }
        }
    }

    fn toggle_blend(&mut self) {
        if self.rx == Rx::Both {
            self.blend = !self.blend;
            self.send_blend(self.blend);
        }
    }

    fn set_ratio(&mut self, ratio: u8) {
        if ratio != self.ratio {
            self.ratio = ratio;
            self.send_blend(self.blend && self.rx == Rx::Both);
        }
    }

    fn act(&mut self, id: &str) {
        let current = self.rig;
        match id {
            "rx1" => self.press_rx(Rx::Rig1, current),
            "rx2" => self.press_rx(Rx::Rig2, current),
            "both" => self.press_rx(Rx::Both, current),
            "auto_rx" => self.auto_rx = !self.auto_rx,
            "cancel_rx" => self.cancel_rx = !self.cancel_rx,
            "af_blend" => self.toggle_blend(),
            "toggle_tx" => self.toggle_tx(),
            _ => {}
        }
    }

    pub fn so2r_view(&self, lang: &str) -> haml_ui::So2rView {
        let error = self
            .hub
            .so2r(|b| b.error())
            .flatten()
            .map(|e| match self.device.find(&self.drivers) {
                Some(d) => d.explain(&e, lang),
                None => e,
            })
            .or_else(|| self.message.clone());
        let link = match (self.connected(), &error) {
            (true, _) => haml_ui::LinkState::Connected,
            (false, Some(_)) => haml_ui::LinkState::Failed,
            (false, None) => haml_ui::LinkState::Off,
        };
        let status = match self.connected() {
            true => self.device.find(&self.drivers).and_then(|d| d.status(lang)),
            false => None,
        };
        haml_ui::So2rView {
            link,
            set_up: !self.device.is_none(),
            status: status.unwrap_or_default(),
            problem: error.unwrap_or_default(),
            rx: self.rx,
            auto_rx: self.auto_rx,
            blend: self.blend,
            cancel_rx: self.cancel_rx,
            ptt: self.ptt,
            tx: self.tx,
            ratio: self.ratio,
            message: false,
        }
    }

    fn so2r_action(&mut self, a: haml_ui::So2rAction, rig: u8) {
        use haml_ui::So2rAction;
        match a {
            So2rAction::Connect => self.connect(),
            So2rAction::Disconnect => self.disconnect(),
            So2rAction::SetUp => self
                .actions
                .push(EntryAction::Options(options::SECTION.into())),
            So2rAction::Rx(rx) => self.press_rx(rx, Some(rig)),
            So2rAction::AutoRx => self.act("auto_rx"),
            So2rAction::Blend => self.act("af_blend"),
            So2rAction::ToggleTx => self.act("toggle_tx"),
            So2rAction::CancelRx => self.act("cancel_rx"),
            So2rAction::Ratio(p) => self.set_ratio(p),
            So2rAction::Send(_) | So2rAction::Stop => {}
        }
    }
}

#[derive(Default)]
struct Lease(Option<TxHub>);

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(hub) = self.0.take() {
            hub.set_so2r(None);
        }
    }
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
        let action = |id: &str, glyph, selected| {
            let weight = 12
                + ["rx1", "rx2", "both"]
                    .iter()
                    .position(|r| *r == id)
                    .unwrap_or(0);
            let mut c = Command::action(
                id,
                texts().get(language, &format!("toolbar.{id}")),
                texts().get(language, &format!("toolbar.{id}.tip")),
            )
            .icon(glyph)
            .selected(selected)
            .toolbar_key(format!("so2r.{id}"))
            .menu_label(texts().get(language, &format!("menu.{id}")))
            .section(zlog_ui::MenuSection::Rig, weight as i32);
            c.menu = true;
            c.radio = true;
            c
        };
        let mut toggle_tx = Command::action(
            "toggle_tx",
            t("toolbar.toggle_tx"),
            t("toolbar.toggle_tx.tip"),
        )
        .icon(zlog_ui::icon::SWAP)
        .shortcut(TOGGLE_TX_KEY)
        .toolbar_key("so2r.toggle_tx")
        .menu_label(t("menu.toggle_tx"))
        .section(zlog_ui::MenuSection::Rig, 15);
        toggle_tx.menu = true;
        vec![
            Command::window(t("toolbar"), t("toolbar.tip")).icon(zlog_ui::icon::BROADCAST),
            action("rx1", zlog_ui::icon::NUMPAD, self.rx == Rx::Rig1),
            action("rx2", zlog_ui::icon::NUMPAD, self.rx == Rx::Rig2),
            action(
                "both",
                zlog_ui::icon::ARROWS_LEFT_RIGHT,
                self.rx == Rx::Both,
            ),
            toggle_tx,
        ]
    }

    fn run_command(&mut self, id: &str) {
        self.act(id);
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        self.view.language = lang.into();
        let v = self.so2r_view(lang);
        let status = [(v.status.as_str(), false), (v.problem.as_str(), true)];
        let mut actions = layout::toolbar(ui, &status, |ui| self.view.tools(ui, &v));
        actions.extend(self.view.body(ui, &v));
        for a in actions {
            self.so2r_action(a, view.rig);
        }
    }

    fn status(&self, view: &View<'_>) -> Vec<zlog_ui::StatusItem> {
        if !self.connected() {
            return Vec::new();
        }
        let t = |k| texts().get(view.language, k);
        let pickup = self.tx != view.rig;
        let tip = if pickup { t("tx_pickup") } else { t("tx.tip") };
        vec![zlog_ui::StatusItem::new(format!("TX{}", self.tx))
            .tooltip(tip)
            .warning(pickup)]
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        self.connected()
            .then_some(std::time::Duration::from_millis(100))
    }

    fn tick(&mut self, view: &View<'_>) {
        let epoch = zlog_ui::device::reopened();
        if epoch != self.reopened {
            self.reopened = epoch;
            if self.device.find(&self.drivers).is_some_and(|d| d.remote()) {
                self.connect();
            }
        }
        let so2r = self.connected();
        let mut q = self.queue.lock().unwrap();
        if q.so2r != so2r {
            q.so2r = so2r;
            q.clear();
        }
        let device = if so2r { self.device.id.as_str() } else { "" };
        if q.so2r_device != device {
            q.so2r_device = device.to_string();
        }
        let idle = q.is_empty();
        drop(q);
        if so2r {
            self.hub.tick();
            if idle && self.hub.transmitting().is_none() {
                self.hub.transmit_on(self.tx);
            }
        }
        if self.rig == Some(view.rig) {
            return;
        }
        self.rig = Some(view.rig);
        self.tx = view.rig;
        if so2r {
            self.hub.send(TxCommand::Focus(view.rig));
        }
        self.show_rx(Rx::of(view.rig));
    }

    fn transmit(&mut self, on: bool) -> bool {
        self.ptt = on;
        let (auto, cancel) = (self.auto_rx, self.cancel_rx);
        if self.hub.so2r(|b| b.caps().auto_rx).unwrap_or(false) {
            let tx = self.hub.on_air().unwrap_or(self.tx);
            match on {
                true if auto => self.listen(Rx::other(tx)),
                false if auto && !cancel => self.listen(Rx::Both),
                _ => {}
            }
        }
        if !on {
            self.cancel_rx = false;
        }
        false
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &Section) {
        let list: Vec<&dyn DeviceDriver> = self.drivers.iter().map(|d| &**d as _).collect();
        let device = DriverChoice::load(section, NONE, &list);
        let same = zlog_ui::device::same_device(&self.device, &device);
        self.device = device;
        if !same || !self.connected() {
            self.connect();
        }
    }

    fn save(&self, section: &mut Section) {
        self.device.save(section);
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.page(210, |ctx| {
        options::So2rOptions::new(ctx.new_drivers().take())
    });
    b.panels(zlog_ui::builder::order::SO2R, |ctx| {
        vec![
            Box::new(So2rPanel::new(ctx.drivers.take(), ctx.ports.clone()).on(zlog_ui::tx::hub())),
            Box::new(QueuePanel::default()),
        ]
    });
    b.driver::<dyn So2rDrv>(40, || Box::<driver::SimSo2rDrv>::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Rx::{Both, Rig1, Rig2};
    use crate::driver::SimCommand::{self, Blend, Rx, Tx};
    use crate::driver::SimSo2rDrv;
    use std::sync::{Arc, Mutex};
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
            memo: "",
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
            log_path: "",
            focus: None,
            editing: None,
            selected: None,
            period: &(),
            tab: Default::default(),
            changes: 0,
        };
        f(&view)
    }

    fn panel() -> (Arc<Mutex<Vec<SimCommand>>>, So2rPanel) {
        let drv = SimSo2rDrv::default();
        let log = drv.log.clone();
        let mut p = So2rPanel {
            queue: zlog_ui::queue::Shared::default(),
            ..Default::default()
        };
        p.device = DriverChoice::new("sim");
        p.attach(drv.open(&Section::new(), &PortHub::new()).unwrap());
        (log, p)
    }

    fn take(log: &Arc<Mutex<Vec<SimCommand>>>) -> Vec<SimCommand> {
        std::mem::take(&mut log.lock().unwrap())
    }

    struct NoPort;

    impl DeviceDriver for NoPort {
        fn id(&self) -> &str {
            "otrsp"
        }
        fn title(&self, _language: &str) -> String {
            "OTRSP".into()
        }
        fn settings(&self, _language: &str) -> Vec<zlog_ui::device::SettingField> {
            use zlog_ui::device::{SettingField, SettingKind};
            vec![SettingField::new(
                "port",
                "Port",
                SettingKind::Port(Vec::new()),
            )]
        }
    }

    impl So2rDrv for NoPort {
        fn open(&self, s: &Section, _ports: &PortHub) -> Result<Box<dyn So2r>, String> {
            Err(format!(
                "no port {}",
                s.get("port").map_or("", |p| p.as_str())
            ))
        }
    }

    #[test]
    fn reconnecting_closes_the_old_box_first() {
        let (_log, mut p) = panel();
        p.drivers = vec![Box::new(NoPort)];
        p.device = DriverChoice::new("otrsp");
        p.connect();
        assert!(!p.connected());
        assert_eq!(p.message.as_deref(), Some("no port "));
    }

    #[test]
    fn the_box_follows_the_rig_in_use() {
        let (log, mut p) = panel();
        with_view(1, |v| p.tick(v));
        assert_eq!(take(&log), [Tx(1), Tx(1), Rx(Rig1), Blend(false, 50)]);
        with_view(1, |v| p.tick(v));
        assert!(take(&log).is_empty());
        with_view(2, |v| p.tick(v));
        assert_eq!(take(&log), [Tx(2), Rx(Rig2), Blend(false, 50)]);
        assert_eq!(p.rx, crate::Rx::Rig2);
        assert!(p.queue.lock().unwrap().so2r);
        assert_eq!(p.queue.lock().unwrap().so2r_device, p.device.id);
        p.disconnect();
        with_view(2, |v| p.tick(v));
        assert!(!p.queue.lock().unwrap().so2r);
        assert_eq!(p.queue.lock().unwrap().so2r_device, "");
    }

    #[test]
    fn rig_buttons_select_the_rig_or_listen_to_it() {
        let (log, mut p) = panel();
        with_view(1, |v| p.tick(v));
        take(&log);
        p.act("rx2");
        assert_eq!(p.take_actions(), [EntryAction::SelectRig(2)]);
        assert!(take(&log).is_empty());
        p.act("both");
        assert_eq!(take(&log), [Rx(Both), Blend(true, 50)]);
        assert!(p.blend);
        p.act("af_blend");
        p.set_ratio(100);
        assert_eq!(take(&log), [Blend(false, 50), Blend(false, 100)]);
        assert!(!p.blend);
        p.act("af_blend");
        p.set_ratio(70);
        assert_eq!(take(&log), [Blend(true, 100), Blend(true, 70)]);
        p.act("af_blend");
        p.set_ratio(100);
        take(&log);
        p.act("rx1");
        assert!(p.take_actions().is_empty());
        assert_eq!(take(&log), [Rx(Rig1), Blend(false, 100)]);
        p.act("af_blend");
        assert!(take(&log).is_empty() && !p.blend);
    }

    #[test]
    fn rig_buttons_are_also_radio_items_in_the_rig_menu() {
        let p = So2rPanel::default();
        for lang in ["en", "ja"] {
            let list = p.commands(lang);
            for id in ["rx1", "rx2", "both"] {
                let c = list.iter().find(|c| c.id == id).unwrap();
                assert!(c.menu && c.toolbar && c.radio, "{id}");
                assert_eq!(c.section, zlog_ui::MenuSection::Rig);
                assert_ne!(c.menu_text(), c.label);
            }
        }
    }

    #[test]
    fn rx_auto_select_around_transmissions() {
        let (log, mut p) = panel();
        with_view(1, |v| p.tick(v));
        take(&log);
        assert!(!p.transmit(true));
        assert!(p.ptt);
        p.transmit(false);
        assert!(take(&log).is_empty());
        p.act("auto_rx");
        p.transmit(true);
        p.transmit(false);
        assert_eq!(take(&log), [Rx(Rig2), Rx(Both)]);
        p.act("cancel_rx");
        p.transmit(true);
        p.transmit(false);
        assert_eq!(take(&log), [Rx(Rig2)]);
        assert!(!p.cancel_rx);
    }

    #[test]
    fn shift_v_toggles_tx_between_the_rigs() {
        let (log, mut p) = panel();
        with_view(1, |v| p.tick(v));
        take(&log);
        let c = p.commands("en");
        let c = c.iter().find(|c| c.id == "toggle_tx").unwrap();
        assert_eq!(c.shortcut, Some(TOGGLE_TX_KEY));
        assert!(c.menu && c.toolbar && !c.radio);
        p.run_command("toggle_tx");
        assert_eq!(p.tx(), 2);
        assert_eq!(take(&log), [Tx(2)]);
        assert_eq!(p.take_actions(), [EntryAction::StopSending]);
        with_view(1, |v| p.tick(v));
        assert!(take(&log).is_empty(), "the rig in use stays on RX");
        p.run_command("toggle_tx");
        assert_eq!((p.tx(), take(&log)), (1, vec![Tx(1)]));
        p.run_command("toggle_tx");
        with_view(2, |v| p.tick(v));
        assert_eq!(p.tx(), 2);
        assert_eq!(take(&log), [Tx(2), Tx(2), Rx(Rig2), Blend(false, 50)]);
    }

    #[test]
    fn the_tx_indicator_shows_the_transmitting_rig() {
        let p = So2rPanel::default();
        assert!(with_view(1, |v| p.status(v)).is_empty());
        let (_log, mut p) = panel();
        with_view(1, |v| p.tick(v));
        let s = with_view(1, |v| p.status(v));
        assert_eq!((s[0].text.as_str(), s[0].warning), ("TX1", false));
        p.run_command("toggle_tx");
        let s = with_view(1, |v| p.status(v));
        assert_eq!((s[0].text.as_str(), s[0].warning), ("TX2", true));
        assert_eq!(
            s[0].tooltip.as_deref(),
            Some("TX is on the other rig (SO2R)")
        );
    }

    #[test]
    fn a_message_goes_out_on_its_rig_and_tx_then_returns() {
        let (log, mut p) = panel();
        with_view(1, |v| p.tick(v));
        p.run_command("toggle_tx");
        take(&log);
        let cw = |radio| TxCommand::Send {
            id: zlog_ui::tx::ticket(),
            radio,
            signal: zlog_ui::tx::Signal::Cw("TU".into()),
        };
        p.hub.send(cw(2));
        assert!(take(&log).is_empty(), "already on rig 2");
        p.hub.send(cw(1));
        assert_eq!(take(&log), [Tx(1)], "F-keys send on the rig in use");
        with_view(1, |v| p.tick(v));
        assert_eq!(take(&log), [Tx(2)], "TX back to the chosen rig");
    }

    #[test]
    fn the_haml_panel_drives_the_box() {
        let p = So2rPanel::default();
        let v = p.so2r_view("en");
        assert!(!v.set_up && v.link == haml_ui::LinkState::Off);
        let (log, mut p) = panel();
        with_view(1, |v| p.tick(v));
        take(&log);
        assert_eq!(p.so2r_view("en").link, haml_ui::LinkState::Connected);
        p.so2r_action(haml_ui::So2rAction::Rx(crate::Rx::Both), 1);
        p.so2r_action(haml_ui::So2rAction::Ratio(70), 1);
        assert_eq!(take(&log), [Rx(Both), Blend(true, 50), Blend(true, 70)]);
        p.so2r_action(haml_ui::So2rAction::ToggleTx, 1);
        assert_eq!(p.so2r_view("en").tx, 2);
        p.so2r_action(haml_ui::So2rAction::SetUp, 1);
        assert!(p
            .take_actions()
            .contains(&EntryAction::Options(options::SECTION.into())));
        p.so2r_action(haml_ui::So2rAction::Disconnect, 1);
        assert_eq!(p.so2r_view("en").link, haml_ui::LinkState::Off);
    }

    #[test]
    fn old_settings_choose_the_driver() {
        let mut old = Section::new();
        old.insert("device".into(), "otrsp".into());
        old.insert("port".into(), "COM7".into());
        let mut q = So2rPanel::new(vec![Box::new(NoPort)], PortHub::new());
        q.load(&old);
        assert_eq!(q.device.id, "otrsp");
        assert!(!q.connected());
        assert_eq!(q.message.as_deref(), Some("no port COM7"));
        let mut s = Section::new();
        q.save(&mut s);
        assert_eq!(
            (s["device"].as_str(), s["otrsp.port"].as_str()),
            ("otrsp", "COM7")
        );
        let mut r = So2rPanel::new(vec![Box::new(SimSo2rDrv::default())], PortHub::new());
        r.load(&Section::new());
        assert!(r.device.is_none() && !r.connected() && r.message.is_none());
        r.device = DriverChoice::new("sim");
        r.connect();
        assert!(r.connected());
    }

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
            "connect",
            "disconnect",
            "none",
            "device.sim",
            "cancel_rx",
            "toggle",
            "af",
            "af_blend",
            "rx_select",
            "on",
            "off",
            "tx_select",
            "tx.tip",
            "tx_pickup",
            "menu.toggle_tx",
            "toolbar.toggle_tx",
            "toolbar.toggle_tx.tip",
        ] {
            assert!(en.contains(&k.to_string()), "{k}");
        }
    }
}
