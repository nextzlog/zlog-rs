pub mod parse;
pub mod telnet;

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::{ScrollArea, TextEdit, Ui};
use zlog_ui::{Panel, Section, Spot, View};

use crate::parse::{parse_line, spot_command};
use crate::telnet::{Connection, Event};

zlog_core::texts!();

pub const PRESETS: &[(&str, &str, u16)] = &[
    ("RBN CW/RTTY", "telnet.reversebeacon.net", 7000),
    ("RBN FT8/FT4", "telnet.reversebeacon.net", 7001),
];

const MAX_LINES: usize = 1000;

const RETRY_INTERVAL: Duration = Duration::from_secs(180);
const RETRY_MAX: u32 = 10;

pub struct Cluster {
    host: String,
    port: String,
    login: String,
    mycall: String,
    auto_login: bool,
    auto_reconnect: bool,
    local_echo: bool,
    connection: Option<Connection>,
    connected: bool,
    lines: VecDeque<String>,
    command: String,
    comment: String,
    spots: Vec<Spot>,
    retry_at: Option<Instant>,
    retries: u32,
}

impl Default for Cluster {
    fn default() -> Self {
        Cluster {
            host: PRESETS[0].1.into(),
            port: PRESETS[0].2.to_string(),
            login: String::new(),
            mycall: String::new(),
            auto_login: true,
            auto_reconnect: true,
            local_echo: false,
            connection: None,
            connected: false,
            lines: VecDeque::new(),
            command: String::new(),
            comment: String::new(),
            spots: Vec::new(),
            retry_at: None,
            retries: 0,
        }
    }
}

fn login_name(login: &str, mycall: &str, auto_login: bool) -> Option<String> {
    let name = match login.trim() {
        "" => mycall.trim(),
        l => l,
    };
    (auto_login && !name.is_empty()).then(|| name.to_string())
}

const SEND_SPOT: &str = "send-spot";

fn qso_hz(qso: &zlog_core::Qso) -> Option<u64> {
    let khz: f64 = qso.freq.trim().parse().ok()?;
    (khz > 0.0).then(|| (khz * 1000.0).round() as u64)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Cluster {
    fn connect(&mut self) {
        let Ok(port) = self.port.trim().parse() else {
            self.push_line(format!("bad port {}", self.port));
            return;
        };
        let login = login_name(&self.login, &self.mycall, self.auto_login);
        self.connection = Some(Connection::open(self.host.trim(), port, login));
        self.push_line(format!("connecting to {}:{port}", self.host.trim()));
    }

    fn disconnect(&mut self) {
        self.connection = None;
        self.connected = false;
        self.retry_at = None;
        self.retries = 0;
    }

    fn push_line(&mut self, line: String) {
        self.lines.push_back(line);
        while self.lines.len() > MAX_LINES {
            self.lines.pop_front();
        }
    }

    fn send(&mut self, line: String) {
        if let Some(c) = &self.connection {
            c.send(&line);
            if self.local_echo {
                self.push_line(line);
            }
        }
    }

    fn pump(&mut self) {
        let events = self
            .connection
            .as_ref()
            .map(|c| c.poll())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Connected => {
                    self.connected = true;
                    self.retries = 0;
                }
                Event::Line(line) => {
                    if let Some(spot) = parse_line(&line, now()) {
                        self.spots.push(spot);
                    }
                    self.push_line(line);
                }
                Event::Closed(reason) => {
                    self.push_line(format!("disconnected: {reason}"));
                    self.connection = None;
                    self.connected = false;
                    if self.auto_reconnect && self.retries < RETRY_MAX {
                        self.retry_at = Some(Instant::now() + RETRY_INTERVAL);
                    }
                }
            }
        }
        if self.retry_at.is_some_and(|t| Instant::now() >= t) {
            self.retry_at = None;
            self.retries += 1;
            self.connect();
        }
    }
}

impl Cluster {
    fn toolbar_actions(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = |k| texts().get(language, k);
        let busy = self.connection.is_some();
        let label = match (busy, self.connected) {
            (false, _) if self.retry_at.is_some() => t("disconnect"),
            (false, _) => t("connect"),
            (true, false) => t("connecting"),
            (true, true) => t("disconnect"),
        };
        let state = match (busy, self.connected) {
            (false, _) if self.retry_at.is_some() => t("toolbar.state.retrying"),
            (false, _) => t("toolbar.state.disconnected"),
            (true, false) => t("toolbar.state.connecting"),
            (true, true) => t("toolbar.state.connected"),
        };
        let ready = busy || self.retry_at.is_some() || !self.host.trim().is_empty();
        let icon = if busy {
            zlog_ui::icon::PLUGS_CONNECTED
        } else {
            zlog_ui::icon::PLUGS
        };
        let mut b = zlog_ui::Command::action("connect", label, t("toolbar.connect.tip"))
            .icon(icon)
            .state(state)
            .selected(busy);
        b.menu = true;
        b.checkable = true;
        b = b
            .menu_label(t("menu.connect"))
            .section(zlog_ui::MenuSection::Spots, 1);
        if !ready {
            b = b.disabled_because(t("toolbar.no_host"));
        }
        vec![b]
    }

    fn toggle_connection(&mut self) {
        if self.connection.is_some() || self.retry_at.is_some() {
            self.disconnect();
        } else {
            self.connect();
        }
    }
}

impl Panel for Cluster {
    fn id(&self) -> &str {
        "cluster"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Network
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Spots, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        let mut b = vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::BROADCAST)
        .shortcut(egui::KeyboardShortcut::new(
            egui::Modifiers::ALT,
            egui::Key::P,
        ))];
        b.extend(self.toolbar_actions(language));
        b
    }

    fn default_size(&self) -> [f32; 2] {
        [640.0, 420.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        self.pump();
        if self.mycall != view.station.callsign {
            self.mycall = view.station.callsign.clone();
        }
        let t = |k| texts().get(view.language, k);
        let busy = self.connection.is_some();
        let retry = self.retry_at.map(|at| {
            let secs = at.saturating_duration_since(Instant::now()).as_secs();
            texts().format(view.language, "retry", &[&secs])
        });
        let spot = view.hz.filter(|_| !view.callsign.is_empty());
        let mut send = false;
        let status = [(retry.as_deref().unwrap_or(""), false)];
        zlog_ui::layout::toolbar(ui, &status, |ui| {
            let (label, glyph) = match (busy, self.connected) {
                (false, _) => (t("connect"), zlog_ui::icon::PLUGS),
                (true, false) => (t("connecting"), zlog_ui::icon::PLUGS),
                (true, true) => (t("disconnect"), zlog_ui::icon::PLUGS_CONNECTED),
            };
            if zlog_ui::layout::toggle(ui, glyph, label, busy).clicked() {
                self.toggle_connection();
            }
            ui.add_enabled_ui(busy && spot.is_some(), |ui| {
                send = zlog_ui::layout::tool(ui, zlog_ui::icon::PAPER_PLANE_TILT, t("send_spot"))
                    .clicked();
            });
        });
        ui.add_enabled(
            busy && spot.is_some(),
            TextEdit::singleline(&mut self.comment)
                .hint_text(t("comment"))
                .desired_width(f32::INFINITY),
        );
        if send {
            if let Some(hz) = spot {
                let line = spot_command(hz, view.callsign, &self.comment);
                self.send(line);
                self.comment.clear();
            }
        }
        let edit = ui.add_enabled(
            busy,
            TextEdit::singleline(&mut self.command)
                .hint_text(t("command"))
                .desired_width(f32::INFINITY),
        );
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let line = std::mem::take(&mut self.command);
            self.send(line);
            edit.request_focus();
        }
        ScrollArea::both()
            .stick_to_bottom(true)
            .auto_shrink(false)
            .show(ui, |ui| {
                for line in &self.lines {
                    ui.monospace(line);
                }
            });
        if self.retry_at.is_some() {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
    }

    fn qso_menu(&self, language: &str, qso: &zlog_core::Qso) -> Vec<zlog_ui::QsoMenuItem> {
        let ready = self.connection.is_some() && qso_hz(qso).is_some();
        vec![
            zlog_ui::QsoMenuItem::new(SEND_SPOT, texts().get(language, "send_spot")).enabled(ready),
        ]
    }

    fn on_qso_menu(&mut self, id: &str, qso: &zlog_core::Qso) -> bool {
        match qso_hz(qso).filter(|_| id == SEND_SPOT) {
            Some(hz) => {
                let line = spot_command(hz, qso.callsign.trim(), &self.comment);
                self.send(line);
                self.comment.clear();
                true
            }
            None => false,
        }
    }

    fn run_command(&mut self, id: &str) {
        if id == "connect" {
            self.toggle_connection();
        }
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        self.retry_at
            .map(|t| t.saturating_duration_since(Instant::now()))
    }

    fn tick(&mut self, view: &View<'_>) {
        if self.mycall != view.station.callsign {
            self.mycall = view.station.callsign.clone();
        }
        self.pump();
    }

    fn take_actions(&mut self) -> Vec<zlog_ui::EntryAction> {
        self.pump();
        let spots = std::mem::take(&mut self.spots);
        spots.into_iter().map(zlog_ui::EntryAction::Spot).collect()
    }

    fn load(&mut self, section: &Section) {
        let get = |k: &str| section.get(k).cloned();
        if let Some(v) = get("host") {
            self.host = v;
        }
        if let Some(v) = get("port") {
            self.port = v;
        }
        if let Some(v) = get("login") {
            self.login = v;
        }
        if let Some(v) = get("auto_login") {
            self.auto_login = v == "true";
        }
        if let Some(v) = get("auto_reconnect") {
            self.auto_reconnect = v == "true";
        }
        if let Some(v) = get("local_echo") {
            self.local_echo = v == "true";
        }
    }

    fn save(&self, section: &mut Section) {
        section.insert("host".into(), self.host.clone());
        section.insert("port".into(), self.port.clone());
        section.insert("login".into(), self.login.clone());
        section.insert("auto_login".into(), self.auto_login.to_string());
        section.insert("auto_reconnect".into(), self.auto_reconnect.to_string());
        section.insert("local_echo".into(), self.local_echo.to_string());
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.page(400, |_| ClusterOptions);
    b.panel(470, |_| Cluster::default());
}

pub struct ClusterOptions;

impl zlog_ui::SettingsPage for ClusterOptions {
    fn id(&self) -> &str {
        "panel.cluster"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut Section, lang: &str) {
        let mut c = Cluster::default();
        c.load(section);
        c.settings_ui(ui, lang);
        c.save(section);
    }
}

impl Cluster {
    fn settings_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        let mut preset = None;
        let labels = [t("presets"), t("host"), t("port"), t("login")];
        zlog_ui::form(ui, "cluster-options", &labels, |ui, form| {
            ui.label(t("presets"));
            form.fit(ui, |ui| {
                form.combo("cluster-preset")
                    .selected_text(t("presets"))
                    .show_ui(ui, |ui| {
                        for (name, h, p) in PRESETS {
                            if ui.selectable_label(false, *name).clicked() {
                                preset = Some((*h, *p));
                            }
                        }
                    });
            });
            ui.end_row();
            for (key, value) in [
                ("host", &mut self.host),
                ("port", &mut self.port),
                ("login", &mut self.login),
            ] {
                ui.label(t(key));
                form.text(ui, TextEdit::singleline(value));
                ui.end_row();
            }
        });
        if let Some((h, p)) = preset {
            self.host = h.into();
            self.port = p.to_string();
        }
        ui.checkbox(&mut self.auto_login, t("auto_login"));
        ui.checkbox(&mut self.auto_reconnect, t("auto_reconnect"));
        ui.checkbox(&mut self.local_echo, t("local_echo"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_qso_is_spotted_on_its_recorded_frequency() {
        let mut q = zlog_core::Qso {
            callsign: "JA1ZLO".into(),
            freq: "7012.5".into(),
            ..Default::default()
        };
        assert_eq!(qso_hz(&q), Some(7_012_500));
        let c = Cluster::default();
        let items = c.qso_menu("en", &q);
        assert_eq!(items[0].id, SEND_SPOT);
        assert!(!items[0].enabled);
        q.freq.clear();
        assert_eq!(qso_hz(&q), None);
    }

    #[test]
    fn login_case_is_kept() {
        assert_eq!(login_name("ja1zlo", "JA1RL", true), Some("ja1zlo".into()));
        assert_eq!(login_name("  ", " ja1rl ", true), Some("ja1rl".into()));
        assert_eq!(login_name("", "", true), None);
        assert_eq!(login_name("JA1ZLO", "", false), None);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let a = Cluster {
            host: "dx.example.org".into(),
            port: "8000".into(),
            login: "JA1ZLO".into(),
            auto_reconnect: false,
            ..Cluster::default()
        };
        let mut section = Section::new();
        a.save(&mut section);
        let mut b = Cluster::default();
        b.load(&section);
        assert_eq!(b.host, "dx.example.org");
        assert_eq!(b.port, "8000");
        assert_eq!(b.login, "JA1ZLO");
        assert!(!b.auto_reconnect);
        assert!(b.auto_login);
        assert!(!b.local_echo);
    }

    fn offline(local_echo: bool) -> Cluster {
        Cluster {
            connection: Some(Connection::open("127.0.0.1", 1, None)),
            local_echo,
            ..Cluster::default()
        }
    }

    #[test]
    fn sent_lines_are_not_echoed_by_default() {
        assert!(!Cluster::default().local_echo);
        let mut c = offline(false);
        c.send("SH/DX".into());
        assert!(c.lines.is_empty());
    }

    #[test]
    fn sent_lines_are_echoed_with_local_echo() {
        let mut c = offline(true);
        c.send("SH/DX".into());
        c.send(String::new());
        assert_eq!(c.lines, ["SH/DX", ""]);
    }

    #[test]
    fn nothing_is_sent_or_echoed_without_connection() {
        let mut c = Cluster {
            local_echo: true,
            ..Cluster::default()
        };
        c.send("SH/DX".into());
        assert!(c.lines.is_empty());
    }

    #[test]
    fn local_echo_round_trip_and_bad_value() {
        let mut section = Section::new();
        offline(true).save(&mut section);
        let mut b = Cluster::default();
        b.load(&section);
        assert!(b.local_echo);
        section.insert("local_echo".into(), "yes".into());
        b.load(&section);
        assert!(!b.local_echo);
        b.load(&Section::new());
        assert!(!b.local_echo);
    }

    fn wake_counter() -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        let woken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        zlog_core::wake::set(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        woken
    }

    fn woken_past(woken: &std::sync::atomic::AtomicUsize, n: usize) -> bool {
        let end = Instant::now() + std::time::Duration::from_secs(3);
        while woken.load(std::sync::atomic::Ordering::SeqCst) <= n && Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        woken.load(std::sync::atomic::Ordering::SeqCst) > n
    }

    #[test]
    fn a_spot_from_the_server_wakes_the_window_without_polling() {
        if !zlog_core::wake::alone(concat!(
            module_path!(),
            "::a_spot_from_the_server_wakes_the_window_without_polling"
        )) {
            return;
        }
        use std::io::Write;
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let woken = wake_counter();
        let mut c = Cluster {
            host: "127.0.0.1".into(),
            port: server.local_addr().unwrap().port().to_string(),
            auto_login: false,
            ..Cluster::default()
        };
        c.connect();
        let (mut peer, _) = server.accept().unwrap();
        assert!(woken_past(&woken, 0), "connected");
        c.pump();
        assert!(c.connected);
        assert_eq!(c.tick_after(), None, "idle while connected");
        let n = woken.load(std::sync::atomic::Ordering::SeqCst);
        peer.write_all(b"DX de JA1ZLO:     7010.0  JA1YAA       cq  0123Z\r\n")
            .unwrap();
        assert!(woken_past(&woken, n), "spot");
        c.pump();
        assert!(
            c.lines.iter().any(|l| l.contains("JA1YAA")),
            "{:?}",
            c.lines
        );
        let n = woken.load(std::sync::atomic::Ordering::SeqCst);
        drop(peer);
        assert!(woken_past(&woken, n), "closed");
    }

    #[test]
    fn a_pending_retry_ticks_when_it_is_due() {
        let c = Cluster {
            retry_at: Some(Instant::now() + RETRY_INTERVAL),
            ..Cluster::default()
        };
        let wait = c.tick_after().unwrap();
        assert!(
            wait > RETRY_INTERVAL / 2 && wait <= RETRY_INTERVAL,
            "{wait:?}"
        );
        assert_eq!(Cluster::default().tick_after(), None);
    }

    fn toolbar_action(c: &Cluster) -> (String, bool, bool) {
        let b = c.commands("en").remove(1);
        assert_eq!(b.id, "connect");
        (b.label, b.enabled, b.selected)
    }

    #[test]
    fn toolbar_connect_follows_the_connection() {
        let mut c = Cluster::default();
        assert_eq!(toolbar_action(&c), ("Connect".into(), true, false));
        c.host.clear();
        assert_eq!(toolbar_action(&c), ("Connect".into(), false, false));
        c.retry_at = Some(Instant::now() + RETRY_INTERVAL);
        assert_eq!(toolbar_action(&c), ("Disconnect".into(), true, false));
        c.run_command("connect");
        assert!(c.retry_at.is_none() && c.connection.is_none());
        let mut c = offline(false);
        assert_eq!(toolbar_action(&c), ("Connecting…".into(), true, true));
        c.connected = true;
        assert_eq!(toolbar_action(&c), ("Disconnect".into(), true, true));
        c.run_command("other");
        assert!(c.connection.is_some());
        c.run_command("connect");
        assert!(c.connection.is_none() && !c.connected);
        assert_eq!(toolbar_action(&c), ("Connect".into(), true, false));
    }

    #[test]
    fn connect_is_also_a_network_menu_toggle() {
        let p = Cluster::default();
        for lang in ["en", "ja"] {
            let c = p
                .commands(lang)
                .into_iter()
                .find(|c| c.id == "connect")
                .unwrap();
            assert!(c.menu && c.toolbar && c.checkable && !c.selected);
            assert_eq!(c.section, zlog_ui::MenuSection::Spots);
            assert_ne!(c.menu_text(), c.label);
        }
    }
}
