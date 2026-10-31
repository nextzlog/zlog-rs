pub mod driver;
pub mod modem;
pub use haml::morse;
pub use zlog_macro as macros;
mod messages;
mod options;
mod playback;
mod settings;
pub mod sidetone;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use egui::{Key, KeyboardShortcut, Modifiers, Ui};
use zlog_core::Mode;
use zlog_ui::device::{DeviceDriver, DriverChoice, PortHub, NONE};
use zlog_ui::{EntryAction, EntryKey, Panel, Section, StatusItem, View};

pub use driver::{timing_of, with_timing, KeyerDrv};
pub use haml::{Keyer, KeyerCaps, KeyerEvent};

pub mod lines {
    pub use haml::keyer::line::{LineKeyer, LineSetup};
    pub use haml::ptt::{Line, Lines};
    pub use haml::sim::lines::SimLines;
}
pub use macros::{expand, Abbrev, Fields};
pub use morse::Timing;
pub use settings::{ptt_delay, Settings};

use crate::playback::{cq_loop, Opened, Shared};
use crate::settings::now;

zlog_core::texts!();

pub struct KeyerPanel {
    settings: Settings,
    shared: Arc<Mutex<Shared>>,
    opening: Option<Receiver<Opened>>,
    bank: usize,
    fkeys: bool,
    cq: Arc<AtomicBool>,
    keyboard: String,
    keyboard_last: String,
    queue: zlog_ui::queue::Shared,
    playing: Option<u64>,
    drivers: Arc<Vec<Box<dyn KeyerDrv>>>,
    ports: PortHub,
    device: DriverChoice,
    so2r_device: String,
    hold: bool,
    cw: bool,
    focus_keyboard: bool,
    actions: Vec<EntryAction>,
    reopened: u64,
    held: bool,
    loaded: bool,
    ptt: Section,
    sound: Section,
    test: bool,
}

pub const KEY_MAX_WPM: u32 = 50;

const KEYS: [(&str, Modifiers, Key); 5] = [
    ("cq_repeat", Modifiers::CTRL, Key::Z),
    ("cq_hold", Modifiers::SHIFT, Key::Z),
    ("speed_up", Modifiers::SHIFT, Key::Y),
    ("speed_down", Modifiers::SHIFT, Key::U),
    ("keyboard", Modifiers::ALT, Key::K),
];

pub const SO2R_KEY: &str = "so2r";

pub const SOUND_PREFIX: &str = "sound.";

impl Default for KeyerPanel {
    fn default() -> Self {
        KeyerPanel {
            settings: Settings::default(),
            shared: Arc::default(),
            opening: None,
            bank: 0,
            fkeys: true,
            cq: Arc::new(AtomicBool::new(true)),
            keyboard: String::new(),
            keyboard_last: String::new(),
            queue: zlog_ui::queue::shared(),
            playing: None,
            drivers: Arc::default(),
            ports: PortHub::new(),
            device: DriverChoice::new(NONE),
            so2r_device: String::new(),
            hold: false,
            cw: false,
            focus_keyboard: false,
            actions: Vec::new(),
            reopened: zlog_ui::device::reopened(),
            held: false,
            loaded: false,
            ptt: Section::new(),
            sound: Section::new(),
            test: false,
        }
    }
}

impl KeyerPanel {
    pub fn new(drivers: Vec<Box<dyn KeyerDrv>>, ports: PortHub) -> Self {
        KeyerPanel {
            drivers: Arc::new(drivers),
            ports,
            ..KeyerPanel::default()
        }
    }

    pub fn on(mut self, tx: zlog_ui::tx::TxHub) -> Self {
        self.shared = Arc::new(Mutex::new(Shared::on(tx)));
        self
    }

    fn driver_list(&self) -> Vec<&dyn DeviceDriver> {
        self.drivers
            .iter()
            .map(|d| &**d as &dyn DeviceDriver)
            .collect()
    }

    fn configured(&self) -> bool {
        self.shared.lock().unwrap().configured()
    }

    fn problem(&self, language: &str) -> Option<String> {
        let s = self.shared.lock().unwrap();
        let raw = s.status.clone();
        match (raw.is_empty(), s.configured()) {
            (true, _) => None,
            (false, true) => Some(sidetone::explain(language, &raw).unwrap_or(raw)),
            (false, false) => Some(zlog_ui::connection::explain(language, &raw)),
        }
    }

    fn connect(&mut self) {
        self.disconnect();
        let Some(i) = self.drivers.iter().position(|d| d.id() == self.device.id) else {
            return;
        };
        let mut settings = with_timing(&self.device.section(), self.settings.timing);
        settings.extend(self.ptt.clone());
        let delays = driver::ptt_delays(&self.ptt);
        self.shared.lock().unwrap().tx.set_delays(delays);
        self.so2r_device = self.queue.lock().unwrap().so2r_device.clone();
        settings.insert(SO2R_KEY.into(), self.so2r_device.clone());
        for (k, v) in &self.sound {
            settings.insert(format!("{SOUND_PREFIX}{k}"), v.clone());
        }
        let (drivers, ports) = (self.drivers.clone(), self.ports.clone());
        let (tx, rx) = mpsc::channel();
        // An open still under way holds the port (Windows ports are exclusive): wait
        // for it and close what it opened before opening again.
        let pending = self.opening.take();
        thread::spawn(move || {
            if let Some(old) = pending {
                drop(old.recv());
            }
            let opened: Opened = drivers[i].open(&settings, &ports);
            let _ = tx.send(opened);
            zlog_core::wake::wake();
        });
        self.opening = Some(rx);
    }

    fn disconnect(&mut self) {
        self.stop();
        self.shared.lock().unwrap().install(None);
    }

    fn stop(&mut self) {
        self.cq.store(true, Ordering::SeqCst);
        self.queue.lock().unwrap().clear();
        self.shared.lock().unwrap().abort();
    }

    fn aim(&self, view: &View<'_>) {
        let mut s = self.shared.lock().unwrap();
        let so2r = s.tx.has_so2r();
        s.radio = s.tx.on_air().filter(|_| so2r).unwrap_or(view.rig);
    }

    fn play(&mut self, bank: usize, n: usize, view: &View<'_>) -> bool {
        self.aim(view);
        self.cq.store(true, Ordering::SeqCst);
        let text = self.expanded(bank, n, view);
        let mut s = self.shared.lock().unwrap();
        if !s.configured() {
            return false;
        }
        if text.trim().is_empty() {
            return true;
        }
        s.abort();
        let mut q = self.queue.lock().unwrap();
        if !q.so2r {
            s.send(&text);
            return true;
        }
        q.clear();
        q.add(zlog_ui::queue::Queued {
            tx: view.rig,
            mode: Mode::Cw,
            play: zlog_ui::queue::Play::Text(text),
        });
        if play_next(&mut q, &mut s) {
            self.playing = Some(q.generation());
        }
        true
    }

    fn expanded(&self, bank: usize, n: usize, view: &View<'_>) -> String {
        let (secs, hour) = now();
        let f = self.settings.fields(view, secs, hour);
        expand(&self.settings.banks[bank][n], &f, &self.settings.abbrev)
    }

    fn update(&mut self, view: &View<'_>) {
        self.cw = view.mode == Mode::Cw;
        self.aim(view);
        let epoch = zlog_ui::device::reopened();
        if epoch != self.reopened {
            self.reopened = epoch;
            if self.device.find(&self.drivers).is_some_and(|d| d.remote()) {
                self.connect();
            }
        }
        let so2r = self.queue.lock().unwrap().so2r_device.clone();
        if so2r != self.so2r_device && (self.configured() || self.opening.is_some()) {
            self.connect();
        }
        if let Some(rx) = &self.opening {
            if let Ok(r) = rx.try_recv() {
                self.opening = None;
                let mut s = self.shared.lock().unwrap();
                match r {
                    Ok(k) => {
                        s.install(Some(k));
                        s.status.clear();
                    }
                    Err(e) => s.status = e,
                }
            }
        }
        if self.test && self.configured() {
            self.test = false;
            self.send_text("TEST");
        }
        let (secs, hour) = now();
        let fields = self.settings.fields(view, secs, hour);
        let pot = {
            let mut s = self.shared.lock().unwrap();
            if s.pump() {
                self.cq.store(true, Ordering::SeqCst);
            }
            if s.configured() {
                let mut q = self.queue.lock().unwrap();
                match self.playing {
                    Some(g) if g != q.generation() => {
                        s.abort();
                        self.playing = None;
                    }
                    Some(_) if !s.busy() => self.playing = None,
                    _ => {}
                }
                if play_next(&mut q, &mut s) {
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

    fn start_cq_with(&mut self, fields: Fields) {
        self.stop();
        let template = self.settings.banks[0][0].clone();
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

    fn cq_running(&self) -> bool {
        !self.cq.load(Ordering::SeqCst)
    }

    fn toolbar_actions(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = |k| texts().get(language, k);
        let running = self.cq_running();
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
                KEYS.len() as i32 + 2,
            ),
            in_menu(
                zlog_ui::Command::action("stop", t("toolbar.stop"), t("toolbar.stop.tip"))
                    .icon(zlog_ui::icon::STOP)
                    .state(state),
                t("menu.stop"),
                KEYS.len() as i32 + 3,
            ),
        ];
        let wpm = format!("{} WPM", self.settings.timing.wpm);
        for (i, (id, m, k)) in KEYS.into_iter().enumerate() {
            let mut c = zlog_ui::Command::new(
                id,
                texts().get(language, &format!("key.{id}")).to_string(),
                texts().get(language, &format!("key.{id}.tip")).to_string(),
            )
            .key_hint(KeyboardShortcut::new(m, k))
            .role(id)
            .section(zlog_ui::MenuSection::Cw, i as i32 + 1);
            match id {
                "cq_repeat" | "cq_hold" if self.configured() && !self.cw => {
                    c = c.disabled_because(t("key.cw_only"));
                }
                "cq_repeat" | "cq_hold" => c = c.state(state).selected(running),
                "speed_up" | "speed_down" => c = c.state(wpm.clone()),
                _ => c = c.opens_window(),
            }
            b.push(c);
        }
        let bank = t(if self.bank == 0 { "bank_a" } else { "bank_b" });
        b.push(
            zlog_ui::Command::new("bank", t("toolbar.bank"), t("toolbar.bank.tip"))
                .key_hint(KeyboardShortcut::new(Modifiers::SHIFT, Key::F))
                .icon(zlog_ui::icon::SWAP)
                .role("bank")
                .section(zlog_ui::MenuSection::Cw, KEYS.len() as i32 + 1)
                .state(bank)
                .selected(self.bank == 1)
                .on_toolbar(false),
        );
        if !self.configured() {
            b = b
                .into_iter()
                .map(|b| match b.id.as_str() {
                    "speed_up" | "speed_down" | "keyboard" => b,
                    _ => b.disabled_because(t("toolbar.no_keyer")),
                })
                .collect();
        }
        b
    }

    fn step_speed(&mut self, up: bool) {
        let wpm = self.settings.timing.wpm;
        let next = if up { wpm + 1 } else { wpm.saturating_sub(1) };
        let max = if up { KEY_MAX_WPM } else { morse::MAX_WPM };
        if (morse::MIN_WPM..=max).contains(&next) {
            self.set_wpm(next);
        }
    }

    fn repeat_cq(&mut self, hold: bool) {
        if self.cq_running() {
            return;
        }
        let fields = self.shared.lock().unwrap().fields.clone();
        self.start_cq_with(fields);
        self.hold = hold;
    }

    fn send_text(&mut self, text: &str) {
        self.shared.lock().unwrap().send(text);
    }

    fn set_wpm(&mut self, wpm: u32) {
        self.settings.timing.wpm = wpm.clamp(morse::MIN_WPM, morse::MAX_WPM);
        let wpm = self.settings.timing.wpm;
        self.shared.lock().unwrap().set_wpm(wpm);
    }

    fn device_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        if self.opening.is_some() {
            ui.spinner();
        } else if self.configured() {
            if zlog_ui::layout::tool(ui, zlog_ui::icon::PLUGS_CONNECTED, t("disconnect")).clicked()
            {
                self.run_command("keyer.disconnect");
            }
        } else if !self.device.is_none()
            && zlog_ui::layout::tool(ui, zlog_ui::icon::PLUGS, t("connect")).clicked()
        {
            self.run_command("keyer.connect");
        }
    }

    fn speed_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.horizontal(|ui| {
            let mut wpm = self.settings.timing.wpm;
            if zlog_ui::layout::tool(ui, zlog_ui::icon::MINUS, "-")
                .on_hover_text(t("speed"))
                .clicked()
            {
                wpm = wpm.saturating_sub(1);
            }
            ui.add(
                egui::DragValue::new(&mut wpm)
                    .range(morse::MIN_WPM..=morse::MAX_WPM)
                    .suffix(" WPM"),
            )
            .on_hover_text(t("speed"));
            if zlog_ui::layout::tool(ui, zlog_ui::icon::PLUS, "+")
                .on_hover_text(t("speed"))
                .clicked()
            {
                wpm += 1;
            }
            if wpm != self.settings.timing.wpm {
                self.set_wpm(wpm);
            }
        });
    }

    fn set_timing(&mut self, timing: Timing) {
        self.settings.timing = timing;
        self.shared.lock().unwrap().set_timing(timing);
    }
}

impl KeyerPanel {
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

    fn reserved_key(&mut self, n: u8, view: &View<'_>) -> bool {
        if !(1..=12).contains(&n) || view.mode != Mode::Cw {
            return false;
        }
        if self.shared.lock().unwrap().pump() {
            self.cq.store(true, Ordering::SeqCst);
        }
        self.play(0, n as usize - 1, view)
    }

    fn escape(&mut self) -> bool {
        let running = !self.cq.load(Ordering::SeqCst);
        let busy = self.shared.lock().unwrap().busy();
        self.stop();
        running || busy
    }
}

fn in_menu(mut c: zlog_ui::Command, label: &str, weight: i32) -> zlog_ui::Command {
    c.menu = true;
    c.menu_label(label)
        .section(zlog_ui::MenuSection::Cw, weight)
}

fn play_next(queue: &mut zlog_ui::queue::PlayQueue, shared: &mut Shared) -> bool {
    match queue.next(false, !shared.busy()) {
        Some(zlog_ui::queue::Queued {
            tx,
            play: zlog_ui::queue::Play::Text(text),
            ..
        }) => {
            let radio = std::mem::replace(&mut shared.radio, tx);
            shared.send(&text);
            shared.radio = radio;
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
        [420.0, 300.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        self.update(view);
        let busy = self.shared.lock().unwrap().busy();
        let status = self.problem(lang).unwrap_or_default();
        let state = if self.opening.is_some() {
            "opening"
        } else if !self.configured() {
            "not_configured"
        } else if busy {
            "sending"
        } else {
            "idle"
        };
        let state = texts().get(lang, state);
        zlog_ui::layout::toolbar(ui, &[(state, false), (&status, true)], |ui| {
            self.device_ui(ui, lang);
            self.speed_ui(ui, lang);
            self.bank_ui(ui, view);
        });
        if self.device.is_none()
            && ui
                .button(texts().get(lang, "set_up"))
                .on_hover_text(texts().get(lang, "set_up.tip"))
                .clicked()
        {
            self.actions
                .push(EntryAction::Options(options::SECTION.into()));
        }
        self.buttons_ui(ui, view);
        self.keyboard_ui(ui, lang);
        self.pad_ui(ui, view);
        if busy || !self.cq.load(Ordering::SeqCst) {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        let busy = self.shared.lock().unwrap().busy();
        (busy || self.cq_running()).then_some(std::time::Duration::from_millis(100))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.update(view);
    }

    fn key_messages(&self, view: &View<'_>) -> Option<Vec<String>> {
        (view.mode == Mode::Cw && self.configured())
            .then(|| (0..12).map(|n| self.expanded(self.bank, n, view)).collect())
    }

    fn on_key(&mut self, key: EntryKey, view: &View<'_>) -> bool {
        match key {
            EntryKey::Function { n, shift } => self.function_key(n, shift, view),
            EntryKey::Reserved { n } => self.reserved_key(n, view),
            EntryKey::Escape => self.escape(),
            EntryKey::Typed => {
                if self.cq_running() && !self.hold {
                    self.stop();
                }
                false
            }
        }
    }

    fn status(&self, view: &View<'_>) -> Vec<StatusItem> {
        if let Some(p) = self.problem(view.language).filter(|_| !self.configured()) {
            let label = texts().get(view.language, "title");
            return vec![StatusItem::new(format!("{label}: {p}")).warning(true)];
        }
        if !self.configured() || view.mode != Mode::Cw {
            return Vec::new();
        }
        let tip = texts().get(view.language, "speed");
        vec![StatusItem::new(format!("{} WPM", self.settings.timing.wpm)).tooltip(tip)]
    }

    fn capability(&self, want: zlog_ui::Want<'_>) -> Option<zlog_ui::Capability<'_>> {
        use zlog_ui::connection::{Connection, LinkState};
        let zlog_ui::Want::Connections(language) = want else {
            return None;
        };
        if self.device.is_none() {
            return Some(zlog_ui::Capability::Connections(Vec::new()));
        }
        let problem = self.problem(language);
        let state = if self.opening.is_some() {
            LinkState::Connecting
        } else if self.configured() {
            LinkState::Connected
        } else if problem.is_some() {
            LinkState::Failed
        } else {
            LinkState::Off
        };
        let label = texts().get(language, "title");
        let c = Connection::new("keyer", label, state)
            .error(problem)
            .page(options::SECTION)
            .commands("keyer.connect", "keyer.disconnect");
        Some(zlog_ui::Capability::Connections(vec![c]))
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn run_command(&mut self, id: &str) {
        match id {
            "cq" | "cq_hold" => self.repeat_cq(true),
            "cq_repeat" => self.repeat_cq(false),
            "speed_up" => self.step_speed(true),
            "speed_down" => self.step_speed(false),
            "keyboard" => self.focus_keyboard = true,
            "stop" => self.stop(),
            "bank" => self.bank = 1 - self.bank,
            "keyer.connect" if !self.device.is_none() => {
                self.held = false;
                self.connect();
            }
            "keyer.test" => self.test = true,
            "keyer.disconnect" => {
                self.held = true;
                self.disconnect();
                self.shared.lock().unwrap().status.clear();
            }
            _ => {}
        }
    }

    fn transmit(&mut self, on: bool) -> bool {
        self.shared.lock().unwrap().phone(on)
    }

    fn load(&mut self, section: &Section) {
        let mut others = zlog_ui::Sections::new();
        others.insert(zlog_ui::device::SOUND_SECTION.into(), self.sound.clone());
        self.load_with(section, &others);
    }

    fn reads(&self) -> Vec<String> {
        vec![zlog_ui::device::SOUND_SECTION.into()]
    }

    fn load_with(&mut self, section: &Section, others: &zlog_ui::Sections) {
        let sound = others
            .get(zlog_ui::device::SOUND_SECTION)
            .cloned()
            .unwrap_or_default();
        let sound_same = sound == self.sound || self.device.id != options::SIMULATED;
        self.sound = sound;
        self.settings = Settings::load(section);
        self.fkeys = zlog_ui::fkeys::shown(section);
        let device = DriverChoice::load(section, NONE, &self.driver_list());
        let ptt = driver::ptt_keys(section, &device.section());
        let same =
            zlog_ui::device::same_device(&self.device, &device) && ptt == self.ptt && sound_same;
        self.ptt = ptt;
        if std::mem::replace(&mut self.loaded, true) {
            self.held = false;
        } else {
            self.held = section.get("held").is_some_and(|v| v == "true");
        }
        self.device = device;
        let delays = driver::ptt_delays(&self.ptt);
        self.shared.lock().unwrap().tx.set_delays(delays);
        if same && (self.configured() || self.opening.is_some()) {
            self.set_timing(self.settings.timing);
        } else if !self.device.is_none() && !self.held {
            self.connect();
        } else {
            self.disconnect();
        }
    }

    fn save(&self, section: &mut Section) {
        self.settings.save(section);
        self.device.save(section);
        section.extend(self.ptt.clone());
        section.insert("held".into(), self.held.to_string());
        zlog_ui::fkeys::set_shown(section, self.fkeys);
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.driver::<dyn zlog_ui::device::ModemDrv>(20, || Box::new(modem::CwModemDrv));
    b.page(220, |ctx| {
        options::KeyerOptions::new(ctx.new_drivers().take())
    });
    b.panel(zlog_ui::builder::order::KEYER, |ctx| {
        KeyerPanel::new(ctx.drivers.take(), ctx.ports.clone()).on(zlog_ui::tx::hub())
    });
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
        fn sent_template(&self, _station: &Station) -> Option<String> {
            Some("$V$P".into())
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
            memo: "",
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

    #[derive(Clone, Default)]
    pub(crate) struct Recorder {
        pub(crate) calls: Arc<Mutex<Vec<String>>>,
        pub(crate) busy: Arc<AtomicBool>,
        pub(crate) events: Arc<Mutex<Vec<KeyerEvent>>>,
    }

    impl Keyer for Recorder {
        fn caps(&self) -> crate::KeyerCaps {
            crate::KeyerCaps::default()
        }
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
        p.shared.lock().unwrap().install(Some(Box::new(r.clone())));
        (p, r)
    }

    pub(crate) fn calls(r: &Recorder) -> Vec<String> {
        std::mem::take(&mut *r.calls.lock().unwrap())
    }

    #[test]
    fn zlog_keys_are_commands_with_roles() {
        let (mut p, _r) = recording_panel();
        with_view(|v| p.tick(v));
        let c = p.commands("ja");
        for (id, m, k) in KEYS {
            let c = c.iter().find(|c| c.id == id).expect(id);
            assert_eq!(c.role, Some(id));
            assert_eq!(c.key_hint, Some(KeyboardShortcut::new(m, k)));
            assert!(c.shortcut.is_none() && c.enabled, "{id}");
            assert!(c.label.chars().count() <= 40, "{id}");
        }
        with_mode(Mode::Ssb, |v| p.tick(v));
        let c = p.commands("en");
        assert!(!c.iter().find(|c| c.id == "cq_repeat").unwrap().enabled);
        assert!(c.iter().find(|c| c.id == "speed_up").unwrap().enabled);
    }

    #[test]
    fn speed_keys_step_one_wpm_within_zlog_limits() {
        let mut p = KeyerPanel::default();
        p.settings.timing.wpm = 25;
        p.run_command("speed_up");
        assert_eq!(p.settings.timing.wpm, 26);
        p.run_command("speed_down");
        p.run_command("speed_down");
        assert_eq!(p.settings.timing.wpm, 24);
        p.settings.timing.wpm = KEY_MAX_WPM;
        p.run_command("speed_up");
        assert_eq!(p.settings.timing.wpm, KEY_MAX_WPM);
        p.settings.timing.wpm = morse::MIN_WPM;
        p.run_command("speed_down");
        assert_eq!(p.settings.timing.wpm, morse::MIN_WPM);
        let s = with_view(|v| p.status(v));
        assert!(s.is_empty(), "no keyer");
        let (p, _r) = recording_panel();
        assert_eq!(with_view(|v| p.status(v))[0].text, "25 WPM");
    }

    #[test]
    fn speed_keys_across_the_five_to_sixty_range() {
        let cases: [(u32, &str, u32); 14] = [
            (4, "speed_up", 5),
            (5, "speed_down", 5),
            (5, "speed_up", 6),
            (6, "speed_down", 5),
            (49, "speed_up", 50),
            (50, "speed_up", 50),
            (50, "speed_down", 49),
            (51, "speed_up", 51),
            (51, "speed_down", 50),
            (55, "speed_up", 55),
            (55, "speed_down", 54),
            (60, "speed_up", 60),
            (60, "speed_down", 59),
            (61, "speed_down", 60),
        ];
        for (from, id, to) in cases {
            let mut p = KeyerPanel::default();
            p.settings.timing.wpm = from;
            p.run_command(id);
            assert_eq!(p.settings.timing.wpm, to, "{id} from {from}");
        }
        let mut p = KeyerPanel::default();
        p.settings.timing.wpm = morse::MAX_WPM;
        for _ in morse::MIN_WPM..morse::MAX_WPM + 5 {
            p.run_command("speed_down");
        }
        assert_eq!(p.settings.timing.wpm, morse::MIN_WPM);
    }

    fn repeating(id: &str) -> (KeyerPanel, Recorder) {
        let (mut p, r) = recording_panel();
        p.settings.cq_repeat = 0.0;
        p.settings.cq_max = 1000;
        r.busy.store(true, Ordering::SeqCst);
        with_view(|v| p.tick(v));
        p.run_command(id);
        thread::sleep(Duration::from_millis(100));
        (p, r)
    }

    #[test]
    fn ctrl_z_repeat_stops_when_typing_and_shift_z_only_on_escape() {
        let (mut p, r) = repeating("cq_repeat");
        assert!(p.cq_running());
        assert!(calls(&r).contains(&"send CQ TEST JR1ABC TEST".to_string()));
        p.run_command("cq_hold");
        assert!(!with_view(|v| p.on_key(EntryKey::Typed, v)));
        assert!(!p.cq_running());
        assert!(calls(&r).contains(&"abort".to_string()));

        let (mut p, _r) = repeating("cq_hold");
        assert!(p.cq_running());
        with_view(|v| p.on_key(EntryKey::Typed, v));
        assert!(p.cq_running(), "typing keeps Shift+Z");
        assert!(with_view(|v| p.on_key(EntryKey::Escape, v)));
        assert!(!p.cq_running());
    }

    #[test]
    fn alt_k_focuses_the_keyboard() {
        let mut p = KeyerPanel::default();
        p.run_command("keyboard");
        assert!(p.focus_keyboard);
        let c = p.commands("en");
        assert!(c
            .iter()
            .any(|c| c.id == "keyboard" && c.opens_window && c.enabled));
    }

    #[test]
    fn transmit_needs_a_keyer() {
        let mut p = KeyerPanel::default();
        assert!(!p.transmit(true));
        p.shared
            .lock()
            .unwrap()
            .install(Some(Box::new(Recorder::default())));
        assert!(p.transmit(true) && p.transmit(false));
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn function_keys_need_a_keyer() {
        let mut p = KeyerPanel::default();
        assert!(!with_view(|v| p.function_key(1, false, v)));
        let r = Recorder::default();
        p.shared.lock().unwrap().install(Some(Box::new(r.clone())));
        assert!(with_view(|v| p.function_key(1, false, v)));
        assert!(with_view(|v| p.function_key(4, false, v)));
        assert!(!with_view(|v| p.function_key(13, false, v)));
        assert_eq!(calls(&r), ["abort", "send CQ TEST JR1ABC TEST"]);
    }

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
        assert!(with_view(|v| p.function_key(2, false, v)));
        p.escape();
        assert!(p.queue.lock().unwrap().is_empty());
    }

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

    #[derive(Default)]
    struct RecorderDrv {
        keyer: Recorder,
        opened: Arc<Mutex<Vec<Section>>>,
    }

    impl DeviceDriver for RecorderDrv {
        fn id(&self) -> &str {
            "winkeyer"
        }
        fn title(&self, _language: &str) -> String {
            "Test".into()
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

    impl KeyerDrv for RecorderDrv {
        fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
            self.opened.lock().unwrap().push(settings.clone());
            Ok(Box::new(self.keyer.clone()))
        }
    }

    fn wait_opened(p: &mut KeyerPanel) {
        for _ in 0..200 {
            with_view(|v| p.tick(v));
            if p.opening.is_none() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("the keyer did not open");
    }

    #[test]
    fn an_opening_keyer_wakes_the_window_instead_of_polling() {
        if !zlog_core::wake::alone(concat!(
            module_path!(),
            "::an_opening_keyer_wakes_the_window_instead_of_polling"
        )) {
            return;
        }
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        zlog_core::wake::set(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let mut p = KeyerPanel::new(vec![Box::new(RecorderDrv::default())], PortHub::new());
        let mut old = Section::new();
        for (k, v) in [("device", "winkeyer"), ("port", "COM4")] {
            old.insert(k.into(), v.into());
        }
        p.load(&old);
        assert!(p.opening.is_some());
        assert_eq!(p.tick_after(), None, "opening");
        for _ in 0..600 {
            if woken.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            woken.load(std::sync::atomic::Ordering::SeqCst) > 0,
            "opened"
        );
        wait_opened(&mut p);
        assert_eq!(p.tick_after(), None, "idle");
    }

    #[test]
    fn old_settings_open_the_driver_of_the_device_saved() {
        let drv = RecorderDrv::default();
        let (keyer, opened) = (drv.keyer.clone(), drv.opened.clone());
        let mut p = KeyerPanel::new(vec![Box::new(drv)], PortHub::new());
        let mut old = Section::new();
        for (k, v) in [("device", "winkeyer"), ("port", "COM4"), ("speed", "33")] {
            old.insert(k.into(), v.into());
        }
        p.load(&old);
        wait_opened(&mut p);
        assert!(p.configured());
        let got = opened.lock().unwrap()[0].clone();
        assert_eq!(
            (got["port"].as_str(), got["speed"].as_str()),
            ("COM4", "33")
        );
        let mut saved = Section::new();
        p.save(&mut saved);
        assert_eq!(saved["device"], "winkeyer");
        assert_eq!(saved["winkeyer.port"], "COM4");
        assert_eq!(saved["speed"], "33");
        assert!(with_view(|v| p.function_key(9, false, v)));
        assert_eq!(calls(&keyer), ["abort", "send TU"]);
        let mut q = KeyerPanel::new(Vec::new(), PortHub::new());
        q.load(&old);
        assert!(q.opening.is_none() && !q.configured());
    }

    #[test]
    fn the_selected_so2r_device_reaches_the_driver_and_reopens_it() {
        let drv = RecorderDrv::default();
        let opened = drv.opened.clone();
        let mut p = KeyerPanel::new(vec![Box::new(drv)], PortHub::new());
        p.queue = Arc::default();
        let mut old = Section::new();
        old.insert("device".into(), "winkeyer".into());
        p.load(&old);
        wait_opened(&mut p);
        assert_eq!(opened.lock().unwrap()[0][SO2R_KEY], "");
        p.queue.lock().unwrap().so2r_device = "neo".into();
        with_view(|v| p.tick(v));
        wait_opened(&mut p);
        with_view(|v| p.tick(v));
        let got = opened.lock().unwrap();
        assert_eq!(got.len(), 2, "one reopen per change");
        assert_eq!(got[1][SO2R_KEY], "neo");
    }

    #[test]
    fn a_manual_disconnect_is_remembered_over_a_restart() {
        use zlog_ui::connection::LinkState;
        let new = || KeyerPanel::new(vec![Box::new(RecorderDrv::default())], PortHub::new());
        let mut p = new();
        let mut s = Section::new();
        s.insert("device".into(), "winkeyer".into());
        p.load(&s);
        wait_opened(&mut p);
        assert!(p.configured());
        let state = |p: &KeyerPanel| zlog_ui::connections_of(p, "en")[0].state;
        assert_eq!(state(&p), LinkState::Connected);
        p.run_command("keyer.disconnect");
        assert_eq!(state(&p), LinkState::Off);
        p.save(&mut s);
        let mut q = new();
        q.load(&s);
        assert!(q.opening.is_none() && !q.configured(), "held");
        q.run_command("keyer.connect");
        wait_opened(&mut q);
        assert!(q.configured());
        q.save(&mut s);
        assert_eq!(s["held"], "false");
    }

    #[test]
    fn applying_options_connects_a_held_keyer() {
        let mut p = KeyerPanel::new(vec![Box::new(RecorderDrv::default())], PortHub::new());
        let mut s = Section::new();
        s.insert("device".into(), "winkeyer".into());
        s.insert("held".into(), "true".into());
        p.load(&s);
        assert!(p.opening.is_none());
        s.insert("port".into(), "COM5".into());
        p.load(&s);
        assert!(p.opening.is_some(), "a later load is an apply");
        wait_opened(&mut p);
    }

    #[test]
    fn an_open_failure_is_explained_in_the_status_bar() {
        let mut p = KeyerPanel::default();
        p.shared.lock().unwrap().status = "Device or resource busy".into();
        p.device = DriverChoice::new("winkeyer");
        let items = with_view(|v| p.status(v));
        assert!(items[0].warning);
        assert!(
            items[0].text.contains("another program"),
            "{}",
            items[0].text
        );
        let c = zlog_ui::connections_of(&p, "en");
        assert_eq!(c[0].state, zlog_ui::connection::LinkState::Failed);
        assert_eq!(c[0].page.as_deref(), Some(options::SECTION));
    }

    #[test]
    fn the_keyer_device_sets_the_ptt_delays_of_the_transmitter() {
        let mut p = KeyerPanel::new(vec![Box::new(RecorderDrv::default())], PortHub::new());
        let mut s = Section::new();
        s.insert("device".into(), "winkeyer".into());
        s.insert("winkeyer.ptt_before".into(), "120".into());
        s.insert("winkeyer.ptt_after".into(), "40".into());
        p.load(&s);
        let d = p.shared.lock().unwrap().tx.delays();
        assert_eq!((d.before.as_millis(), d.after.as_millis()), (120, 40));
        wait_opened(&mut p);
    }

    #[derive(Default)]
    struct RemoteDrv(RecorderDrv);

    impl DeviceDriver for RemoteDrv {
        fn id(&self) -> &str {
            "remote"
        }
        fn title(&self, _language: &str) -> String {
            "Remote".into()
        }
        fn remote(&self) -> bool {
            true
        }
    }

    impl KeyerDrv for RemoteDrv {
        fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
            self.0.open(settings, ports)
        }
    }

    #[test]
    fn a_remote_keyer_already_chosen_is_opened_again_after_a_reconnect() {
        let remote = RemoteDrv::default();
        let opened = remote.0.opened.clone();
        let mut p = KeyerPanel::new(vec![Box::new(remote)], PortHub::new());
        let mut s = Section::new();
        s.insert("device".into(), "remote".into());
        p.load(&s);
        wait_opened(&mut p);
        let before = opened.lock().unwrap().len();
        zlog_ui::device::reopen();
        with_view(|v| p.tick(v));
        wait_opened(&mut p);
        with_view(|v| p.tick(v));
        assert_eq!(opened.lock().unwrap().len(), before + 1);
        let local = RecorderDrv::default();
        let opened = local.opened.clone();
        let mut q = KeyerPanel::new(vec![Box::new(local)], PortHub::new());
        let mut s = Section::new();
        s.insert("device".into(), "winkeyer".into());
        q.load(&s);
        wait_opened(&mut q);
        zlog_ui::device::reopen();
        with_view(|v| q.tick(v));
        assert_eq!(
            opened.lock().unwrap().len(),
            1,
            "a local keyer is left alone"
        );
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
                "send JA1ZLO 5NNAOH"
            ]
        );
    }

    #[test]
    fn an_empty_message_sends_nothing() {
        let (mut p, r) = recording_panel();
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
        p.bank = 1;
        assert!(with_view(|v| p.function_key(1, false, v)));
        assert!(with_view(|v| p.function_key(3, true, v)));
        assert_eq!(
            calls(&r),
            ["abort", "send QRZ?", "abort", "send TU JR1ABC TEST"]
        );
    }

    #[test]
    fn bank_command_switches_function_keys_but_not_reserved_ones() {
        let (mut p, r) = recording_panel();
        p.settings.banks[0][0] = "CQ".into();
        p.settings.banks[1][0] = "QRZ?".into();
        let bank = |p: &KeyerPanel| {
            let c = p
                .commands("en")
                .into_iter()
                .find(|c| c.id == "bank")
                .unwrap();
            assert!(c.enabled && c.menu && !c.toolbar_default);
            (c.state.unwrap(), c.selected)
        };
        assert_eq!(bank(&p), ("Bank A".into(), false));
        p.run_command("bank");
        assert_eq!(bank(&p), ("Bank B".into(), true));
        assert!(with_view(|v| p.function_key(1, false, v)));
        assert!(with_view(|v| p.function_key(1, true, v)));
        assert!(with_view(|v| p.reserved_key(1, v)));
        assert_eq!(
            calls(&r),
            ["abort", "send QRZ?", "abort", "send CQ", "abort", "send CQ"]
        );
        p.run_command("bank");
        assert_eq!(bank(&p), ("Bank A".into(), false));
    }

    #[test]
    fn escape_stops_cw_and_cq_repeat() {
        let (mut p, r) = recording_panel();
        assert!(!p.escape());
        r.busy.store(true, Ordering::SeqCst);
        assert!(p.escape());
        assert!(!r.busy());
        assert!(calls(&r).contains(&"abort".to_string()));
        p.cq = Arc::new(AtomicBool::new(false));
        let cq = p.cq.clone();
        assert!(p.escape());
        assert!(cq.load(Ordering::SeqCst));
        assert!(!p.escape());
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
            .filter(|b| b.toolbar)
            .map(|b| (b.label, b.enabled, b.selected))
            .collect()
    }

    #[test]
    fn cq_and_stop_are_also_in_the_cw_menu() {
        for lang in ["en", "ja"] {
            let list = KeyerPanel::default().commands(lang);
            for id in ["cq", "stop"] {
                let c = list.iter().find(|c| c.id == id).unwrap();
                assert!(c.menu && c.toolbar && c.toolbar_default, "{id}");
                assert_eq!(c.section, zlog_ui::MenuSection::Cw);
                assert_ne!(c.menu_text(), c.label);
            }
        }
    }

    #[test]
    fn toolbar_cq_and_stop_need_a_keyer() {
        let p = KeyerPanel::default();
        assert_eq!(
            action_ids(&p)[1..3],
            [("CQ".into(), false, false), ("Stop".into(), false, false)]
        );
    }

    #[test]
    fn toolbar_cq_holds_until_stop() {
        let mut p = KeyerPanel::default();
        p.shared
            .lock()
            .unwrap()
            .install(Some(Box::new(Recorder::default())));
        p.settings.banks[0][0] = "CQ $M".into();
        p.settings.cq_repeat = 10.0;
        with_view(|v| p.tick(v));
        p.run_command("cq");
        assert!(p.cq_running());
        assert_eq!(action_ids(&p)[1], ("CQ".into(), true, true));
        p.run_command("cq");
        assert!(p.cq_running(), "a second press does not stop it");
        with_view(|v| p.on_key(EntryKey::Typed, v));
        assert!(p.cq_running(), "typing does not stop it");
        p.run_command("stop");
        assert!(!p.cq_running());
        assert_eq!(action_ids(&p)[1], ("CQ".into(), true, false));
        let (mut q, r) = recording_panel();
        r.busy.store(true, Ordering::SeqCst);
        q.run_command("stop");
        assert!(!r.busy());
        q.run_command("unknown");
    }

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
        p.run_command("stop");
        p.settings.banks[0][0].clear();
        p.run_command("cq");
        assert!(!p.cq_running());
    }

    #[test]
    fn reserved_messages_and_cq_always_use_bank_a() {
        let (mut p, r) = recording_panel();
        p.settings.banks[1][0] = "QA".into();
        p.settings.banks[1][1] = "QB".into();
        p.settings.cq_max = 1;
        p.run_command("bank");
        assert_eq!(p.bank, 1);
        with_view(|v| p.tick(v));
        assert!(with_view(|v| p.on_key(EntryKey::Reserved { n: 2 }, v)));
        assert!(with_view(
            |v| p.on_key(EntryKey::Function { n: 2, shift: false }, v)
        ));
        assert_eq!(
            calls(&r),
            ["abort", "send JA1ZLO 5NNAOH", "abort", "send QB"]
        );
        for id in ["cq", "cq_repeat", "cq_hold"] {
            p.run_command(id);
            thread::sleep(Duration::from_millis(50));
            p.run_command("stop");
            let c = calls(&r);
            assert!(
                c.contains(&"send CQ TEST JR1ABC TEST".to_string()),
                "{id}: {c:?}"
            );
        }
        assert!(!with_mode(Mode::Ssb, |v| p.on_key(EntryKey::Reserved { n: 2 }, v)));
    }
}
