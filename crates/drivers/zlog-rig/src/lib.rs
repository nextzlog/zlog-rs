pub mod control;
pub mod pair;
pub mod sync;
pub mod window;

use std::time::Instant;

use egui::{Key, KeyboardShortcut, Modifiers};
use zlog_core::Mode;
use zlog_ui::rig::{RigControl, RigEntry};
use zlog_ui::{icon, Command, EntryAction, MenuGroup, MenuSection, Panel, Sections, View};

pub use control::{simulator, Drivers, RigConfig, SimFactory, SIMULATOR};
pub use haml::{Model, MODELS};
pub use pair::{Rigs, SECTIONS};
pub use window::RigPage;

zlog_core::texts!();

pub fn text(language: &str, key: &str) -> String {
    texts().get(language, key).to_string()
}

pub fn pages(drivers: &Drivers) -> Vec<Box<dyn zlog_ui::SettingsPage>> {
    vec![
        Box::new(RigPage::new(1, drivers.clone())),
        Box::new(RigPage::new(2, drivers.clone())),
    ]
}

pub struct RigPanel {
    rigs: Rigs,
    configs: [RigConfig; 2],
    drivers: Drivers,
    language: String,
    actions: Vec<EntryAction>,
    held: [bool; 2],
    configured: [bool; 2],
    view: haml_ui::RigPanel,
}

impl RigPanel {
    pub fn new(drivers: Drivers) -> Self {
        let config = RigConfig::load(&Default::default(), &drivers);
        RigPanel {
            rigs: Rigs::new(drivers.clone()),
            configs: [config.clone(), config],
            drivers,
            language: "en".into(),
            actions: Vec::new(),
            held: [false; 2],
            configured: [false; 2],
            view: haml_ui::RigPanel::new("en"),
        }
    }

    fn connect_slot(&mut self, n: u8) {
        let i = usize::from(n == 2);
        self.held[i] = false;
        let config = self.configs[i].clone();
        let language = self.language.clone();
        self.rigs.slot_mut(n).connect(config, &language);
    }

    fn shown(&self, n: u8) -> bool {
        let rig = self.rigs.slot(n);
        self.configured[usize::from(n == 2)] || rig.connected() || rig.opening()
    }

    fn disconnect_slot(&mut self, n: u8) {
        self.held[usize::from(n == 2)] = true;
        self.rigs.slot_mut(n).disconnect();
    }

    fn link(&self, n: u8, language: &str) -> haml_ui::LinkState {
        use haml_ui::LinkState;
        let rig = self.rigs.slot(n);
        if rig.opening() {
            LinkState::Connecting
        } else if rig.problem(language).is_some() {
            LinkState::Failed
        } else if rig.connected() {
            LinkState::Connected
        } else {
            LinkState::Off
        }
    }

    pub fn rig_view(&self, language: &str) -> haml_ui::RigView {
        let n = self.rigs.number();
        let config = self.config().clone();
        let (info, _) = window::rig_summary(&self.rigs, &config, language);
        let rig = self.rigs.slot(n);
        haml_ui::RigView {
            info,
            link: self.link(n, language),
            problem: rig.problem(language),
            set_up: self.shown(n),
            number: n,
            rigs: 2,
            state: rig.connected().then(|| rig.state().unwrap_or_default()),
            caps: haml::RigCaps::NONE,
        }
    }

    fn rig_action(&mut self, a: haml_ui::RigAction) {
        use haml_ui::RigAction;
        let n = self.rigs.number();
        match a {
            RigAction::Connect => self.connect_slot(n),
            RigAction::Disconnect => self.disconnect_slot(n),
            RigAction::SetUp => self
                .actions
                .push(EntryAction::Options(SECTIONS[usize::from(n == 2)].into())),
            RigAction::Select(m) => self.actions.push(EntryAction::SelectRig(m)),
            RigAction::Tune(_) | RigAction::Mode(_) => {}
        }
    }

    fn connection(&self, n: u8, language: &str) -> zlog_ui::connection::Connection {
        use zlog_ui::connection::{Connection, LinkState};
        let state = match self.link(n, language) {
            haml_ui::LinkState::Connecting => LinkState::Connecting,
            haml_ui::LinkState::Failed => LinkState::Failed,
            haml_ui::LinkState::Connected => LinkState::Connected,
            haml_ui::LinkState::Off => LinkState::Off,
        };
        let problem = self.rigs.slot(n).problem(language);
        let label = text(
            language,
            if n == 2 {
                "options.rig2"
            } else {
                "options.rig"
            },
        );
        Connection::new(SECTIONS[usize::from(n == 2)], label, state)
            .error(problem)
            .page(SECTIONS[usize::from(n == 2)])
            .commands(&format!("rig{n}.connect"), &format!("rig{n}.disconnect"))
    }

    fn config(&self) -> &RigConfig {
        &self.configs[usize::from(self.rigs.number() == 2)]
    }
}

fn cmd(lang: &str, id: &str, label: &str) -> Command {
    Command::new(id, text(lang, label), text(lang, &format!("cmd.{id}.tip")))
}

pub fn commands(connected: bool, lang: &str) -> Vec<Command> {
    let mut connect = cmd(lang, "device.connect", "menu.device.connect").icon(icon::PLUGS);
    if connected {
        connect = connect.disabled_because(text(lang, "cmd.device.connect.busy"));
    }
    let mut disconnect =
        cmd(lang, "device.disconnect", "menu.device.disconnect").icon(icon::PLUGS_CONNECTED);
    if !connected {
        disconnect = disconnect.disabled_because(text(lang, "cmd.device.disconnect.none"));
    }
    let mut window = Command::window(
        text(lang, "menu.rig.window"),
        text(lang, "cmd.device.rig_window.tip"),
    )
    .icon(icon::RADIO)
    .shortcut(KeyboardShortcut::new(Modifiers::ALT, Key::T));
    window.toolbar = false;
    let toggle = cmd(lang, "device.toggle_rig", "menu.device.toggle_rig")
        .key_hint(KeyboardShortcut::new(Modifiers::SHIFT, Key::X))
        .icon(icon::SWAP);
    let connect = connect.section(MenuSection::Rig, 0);
    let disconnect = disconnect.section(MenuSection::Rig, 1);
    let window = window.section(MenuSection::Rig, 2);
    let toggle = toggle.section(MenuSection::Rig, 3);
    vec![
        window,
        connect.toolbar_key("device.connect"),
        disconnect.toolbar_key("device.disconnect"),
        toggle,
    ]
}

impl Panel for RigPanel {
    fn migrations(&self) -> Vec<zlog_ui::migration::Migration> {
        [
            ("device/device.connect", "rig/device.connect"),
            ("device/device.disconnect", "rig/device.disconnect"),
            ("device/device.rig_window", "rig/window"),
            ("device/device.toggle_rig", "rig/device.toggle_rig"),
        ]
        .map(|(old, new)| zlog_ui::migration::Migration::MovedCommand(old, new))
        .to_vec()
    }

    fn id(&self) -> &str {
        "rig"
    }

    fn title(&self, language: &str) -> String {
        text(language, "window.rig")
    }

    fn default_size(&self) -> [f32; 2] {
        [380.0, 330.0]
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Device
    }

    fn section(&self) -> (MenuSection, i32) {
        (MenuSection::Rig, 0)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        commands(self.rigs.connected(), language)
    }

    fn run_command(&mut self, id: &str) {
        match id {
            "device.connect" => self.connect_slot(self.rigs.number()),
            "device.disconnect" => self.disconnect_slot(self.rigs.number()),
            "rig1.connect" => self.connect_slot(1),
            "rig2.connect" => self.connect_slot(2),
            "rig1.disconnect" => self.disconnect_slot(1),
            "rig2.disconnect" => self.disconnect_slot(2),
            "device.toggle_rig" => {
                let other = 3 - self.rigs.number();
                if self.rigs.slot(other).connected() {
                    self.actions.push(EntryAction::SelectRig(other));
                }
            }
            _ => {}
        }
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        [1, 2]
            .iter()
            .map(|n| self.rigs.slot(*n))
            .any(|r| r.connected() || r.opening() || r.retrying())
            .then_some(std::time::Duration::from_millis(250))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.language = view.language.to_string();
    }

    fn ui(&mut self, ui: &mut egui::Ui, view: &View<'_>) {
        let lang = view.language;
        self.language = lang.into();
        self.view.language = lang.into();
        let rig = self.rig_view(lang);
        let problem = rig.problem.clone().unwrap_or_default();
        let status = [(rig.info.as_str(), false), (problem.as_str(), true)];
        let mut actions = zlog_ui::layout::toolbar(ui, &status, |ui| self.view.tools(ui, &rig));
        actions.extend(self.view.body(ui, &rig));
        for a in actions {
            self.rig_action(a);
        }
        window::sim_ui(ui, &self.rigs, lang);
    }

    fn status(&self, view: &View<'_>) -> Vec<zlog_ui::StatusItem> {
        let n = self.rigs.number();
        match self.rigs.problem(view.language) {
            Some(p) if self.shown(n) => {
                let label = text(
                    view.language,
                    if n == 2 {
                        "options.rig2"
                    } else {
                        "options.rig"
                    },
                );
                vec![zlog_ui::StatusItem::new(format!("{label}: {p}")).warning(true)]
            }
            _ => Vec::new(),
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &zlog_ui::Section) {
        for (i, name) in SECTIONS.iter().enumerate() {
            self.held[i] = section
                .get(&format!("{name}.held"))
                .is_some_and(|v| v == "true");
        }
    }

    fn save(&self, section: &mut zlog_ui::Section) {
        for (i, name) in SECTIONS.iter().enumerate() {
            section.insert(format!("{name}.held"), self.held[i].to_string());
        }
    }

    fn capability(&self, want: zlog_ui::Want<'_>) -> Option<zlog_ui::Capability<'_>> {
        match want {
            zlog_ui::Want::Rig => Some(zlog_ui::Capability::Rig(self)),
            zlog_ui::Want::MenuNote(language) => {
                Some(zlog_ui::Capability::MenuNote(self.menu_note(language)))
            }
            zlog_ui::Want::Connections(language) => Some(zlog_ui::Capability::Connections(
                [1, 2]
                    .into_iter()
                    .filter(|n| self.shown(*n))
                    .map(|n| self.connection(n, language))
                    .collect(),
            )),
        }
    }

    fn capability_mut(&mut self, want: zlog_ui::WantMut) -> Option<zlog_ui::CapabilityMut<'_>> {
        match want {
            zlog_ui::WantMut::Rig => Some(zlog_ui::CapabilityMut::Rig(self)),
            zlog_ui::WantMut::Buttons => None,
        }
    }
}

impl RigPanel {
    fn menu_note(&self, language: &str) -> String {
        let config = self.config();
        window::rig_status(
            self.drivers.model_name(config.model),
            &config.port,
            self.rigs.connected(),
            self.rigs.problem(language),
            language,
        )
    }
}

impl RigControl for RigPanel {
    fn number(&self) -> u8 {
        self.rigs.number()
    }

    fn hz(&self) -> Option<u64> {
        self.rigs.state().and_then(|s| s.freq())
    }

    fn connected(&self) -> bool {
        self.rigs.connected()
    }

    fn select(&mut self, n: u8) -> bool {
        self.rigs.select(n)
    }

    fn toggle(&mut self) -> bool {
        self.rigs.toggle()
    }

    fn follow(&mut self, now: Instant, entry: &mut dyn RigEntry) {
        self.rigs.tick(now, entry);
    }

    fn qsy(&mut self, hz: u64, mode: Option<Mode>, entry: &mut dyn RigEntry) {
        self.rigs.qsy(hz, mode, entry);
    }

    fn sections(&self) -> Vec<String> {
        SECTIONS.iter().map(|s| s.to_string()).collect()
    }

    fn use_port(&mut self, n: u8, port: &str, section: &mut zlog_ui::Section) {
        if !(1..=2).contains(&n) {
            return;
        }
        let i = usize::from(n == 2);
        let mut config = RigConfig::load(section, &self.drivers);
        config.port = port.into();
        config.save(section, &self.drivers);
        section.insert(control::CHOSEN.into(), "true".into());
        self.configs[i] = config;
        self.configured[i] = true;
        self.connect_slot(n);
    }

    fn load_sections(&mut self, sections: &Sections, changed: &[String]) {
        for (n, name) in SECTIONS.iter().enumerate() {
            let config = RigConfig::load(
                sections.get(*name).unwrap_or(&Default::default()),
                &self.drivers,
            );
            let section = sections.get(*name);
            self.configured[n] = section.is_some_and(control::chosen);
            self.configs[n] = config;
            let rig = self.rigs.slot(n as u8 + 1);
            let applied = changed.iter().any(|c| c == name);
            let start = changed.is_empty()
                && !self.held[n]
                && !rig.connected()
                && !rig.opening()
                && !rig.retrying();
            if self.configured[n] && (applied || start) {
                self.connect_slot(n as u8 + 1);
            }
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(zlog_ui::builder::order::RIG, |ctx| {
        RigPanel::new(Drivers::from_host(ctx))
    });
    b.pages(200, |ctx| pages(&Drivers::from_host(ctx)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> RigPanel {
        RigPanel::new(control::tests::drivers())
    }

    fn sections() -> zlog_ui::Sections {
        let d = control::tests::drivers();
        let mut rig = zlog_ui::Section::new();
        RigConfig::load(&rig.clone(), &d).save(&mut rig, &d);
        rig.insert("port".into(), SIMULATOR.into());
        rig.insert(control::CHOSEN.into(), "true".into());
        [("rig".to_string(), rig)].into_iter().collect()
    }

    #[test]
    fn a_port_given_by_another_device_is_saved_and_connected() {
        let mut p = panel();
        let mut section = zlog_ui::Section::new();
        p.use_port(2, SIMULATOR, &mut section);
        assert!(p.rigs.slot(2).connected());
        assert!(control::chosen(&section));
        assert_eq!(section["port"], SIMULATOR);
        assert_eq!(zlog_ui::connections_of(&p, "en").len(), 1);
    }

    #[test]
    fn a_page_only_viewed_and_applied_does_not_count_as_set_up() {
        let d = control::tests::drivers();
        let mut viewed = zlog_ui::Section::new();
        RigConfig::load(&viewed.clone(), &d).save(&mut viewed, &d);
        assert!(!control::chosen(&viewed), "default simulator");
        let mut p = panel();
        let secs: zlog_ui::Sections = [("rig2".to_string(), viewed.clone())].into_iter().collect();
        p.load_sections(&secs, &["rig2".into()]);
        assert!(!p.rigs.slot(2).connected());
        assert!(zlog_ui::connections_of(&p, "en").is_empty());
        viewed.insert("port".into(), "COM3".into());
        assert!(control::chosen(&viewed), "a real port");
    }

    #[test]
    fn a_configured_rig_connects_on_startup_and_on_apply() {
        let mut p = panel();
        p.load_sections(&sections(), &[]);
        assert!(p.rigs.slot(1).connected());
        assert!(!p.rigs.slot(2).connected(), "rig 2 is not configured");
        let mut p = panel();
        p.load_sections(&Default::default(), &[]);
        assert!(!p.rigs.slot(1).connected(), "nothing set up yet");
        p.load_sections(&sections(), &["rig".into()]);
        assert!(p.rigs.slot(1).connected(), "applied in Options");
    }

    #[test]
    fn a_manual_disconnect_is_remembered_until_connected_again() {
        let mut p = panel();
        p.load_sections(&sections(), &[]);
        p.run_command("device.disconnect");
        assert!(!p.rigs.slot(1).connected());
        let mut own = zlog_ui::Section::new();
        p.save(&mut own);
        let mut q = panel();
        q.load(&own);
        q.load_sections(&sections(), &[]);
        assert!(!q.rigs.slot(1).connected(), "held over a restart");
        q.run_command("device.connect");
        assert!(q.rigs.slot(1).connected(), "the menu command connects");
        q.save(&mut own);
        assert_eq!(own["rig.held"], "false");
        let mut r = panel();
        r.load(&own);
        r.load_sections(&sections(), &[]);
        assert!(r.rigs.slot(1).connected());
    }

    #[test]
    fn applying_options_connects_a_held_rig() {
        let mut p = panel();
        let mut own = zlog_ui::Section::new();
        own.insert("rig.held".into(), "true".into());
        p.load(&own);
        p.load_sections(&sections(), &[]);
        assert!(!p.rigs.slot(1).connected());
        p.load_sections(&sections(), &["rig".into()]);
        assert!(p.rigs.slot(1).connected());
    }

    #[test]
    fn the_indicator_lists_configured_rigs_and_runs_their_commands() {
        use zlog_ui::connection::LinkState;
        let mut p = panel();
        assert!(zlog_ui::connections_of(&p, "en").is_empty());
        p.load_sections(&sections(), &[]);
        let c = zlog_ui::connections_of(&p, "en");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].state, LinkState::Connected);
        assert_eq!(c[0].page.as_deref(), Some("rig"));
        p.run_command(c[0].disconnect.as_deref().unwrap());
        assert_eq!(zlog_ui::connections_of(&p, "en")[0].state, LinkState::Off);
        p.run_command(c[0].connect.as_deref().unwrap());
        assert_eq!(
            zlog_ui::connections_of(&p, "en")[0].state,
            LinkState::Connected
        );
    }

    #[test]
    fn a_serial_port_opens_off_the_ui_thread_and_retries_when_missing() {
        let mut p = panel();
        let mut secs = sections();
        secs.get_mut("rig")
            .unwrap()
            .insert("port".into(), "/dev/zlog-no-such-port".into());
        p.load_sections(&secs, &[]);
        assert!(p.rigs.slot(1).opening());
        let start = Instant::now();
        while p.rigs.slot(1).opening() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(5));
            p.rigs.slot_mut(1).poll(Instant::now());
        }
        assert!(!p.rigs.slot(1).connected());
        assert!(p.rigs.slot(1).retrying(), "a missing port is tried again");
        assert!(p.tick_after().is_some());
        let problem = p.rigs.slot(1).problem("en").unwrap();
        assert!(problem.contains("not found"), "{problem}");
        let c = zlog_ui::connections_of(&p, "en");
        assert_eq!(c[0].state, zlog_ui::connection::LinkState::Failed);
        p.run_command("device.disconnect");
        assert!(!p.rigs.slot(1).retrying());
    }

    #[test]
    fn the_haml_panel_offers_set_up_then_controls_the_rig() {
        let mut p = panel();
        let v = p.rig_view("en");
        assert!(!v.set_up && v.link == haml_ui::LinkState::Off);
        p.rig_action(haml_ui::RigAction::SetUp);
        assert_eq!(p.take_actions(), [EntryAction::Options("rig".into())]);
        p.load_sections(&sections(), &[]);
        let v = p.rig_view("en");
        assert!(v.set_up && v.link == haml_ui::LinkState::Connected && v.state.is_some());
        p.rig_action(haml_ui::RigAction::Disconnect);
        assert_eq!(p.rig_view("en").link, haml_ui::LinkState::Off);
        let mut own = zlog_ui::Section::new();
        p.save(&mut own);
        assert_eq!(own["rig.held"], "true", "remembered");
        p.rig_action(haml_ui::RigAction::Connect);
        assert_eq!(p.rig_view("en").link, haml_ui::LinkState::Connected);
        p.rig_action(haml_ui::RigAction::Select(2));
        assert_eq!(p.take_actions(), [EntryAction::SelectRig(2)]);
    }

    #[test]
    fn every_text_is_in_every_language() {
        assert!(texts().missing().is_empty(), "{:?}", texts().missing());
    }

    #[test]
    fn connect_and_disconnect_follow_the_connection() {
        let d = commands(true, "en");
        assert!(d[0].is_window());
        assert!(
            !d[1].opens_window,
            "connect runs, it does not open a window"
        );
        assert!(!d[1].enabled && d[2].enabled);
        let d = commands(false, "en");
        assert!(d[1].enabled && !d[2].enabled);
        assert!(d.iter().all(|c| c.section == MenuSection::Rig));
        let ids: Vec<&str> = d.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "window",
                "device.connect",
                "device.disconnect",
                "device.toggle_rig"
            ]
        );
    }
}
