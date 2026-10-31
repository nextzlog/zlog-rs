use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{RichText, ScrollArea, Ui};
use zlog_core::{Band, Mode, Qso, QsoId};
use zlog_tls::discovery::Browser;
use zlog_ui::{EntryAction, Panel, Section, View};

use crate::net::{Shared, StartError};
use crate::role::Role;
use crate::session::{ChatLine, LineBreak, OnConnect, Prompt, Session, State};
use crate::{protocol, texts};

fn freq_time(log: &zlog_core::Log, now: chrono::DateTime<chrono::Utc>) -> String {
    const FORMAT: &str = "%H:%M:%S";
    match log.utc_offset_minutes() {
        None => now.format(FORMAT).to_string(),
        Some(_) => now.with_timezone(&chrono::Local).format(FORMAT).to_string(),
    }
}

pub const CONNECT: &str = "connect";
pub const SETTINGS: &str = "settings";
pub const SHARE: &str = "share";

pub struct ZLinkPanel {
    session: Arc<Mutex<Session>>,
    net: Arc<Shared>,
    input: String,
    notice: Option<&'static str>,
    problem: Option<String>,
    freq_sent: Option<((Band, u64, Mode, bool), Instant)>,
    freq_due: Option<Instant>,
    edit_lock: Option<Qso>,
    split: f32,
    browser: Option<Browser>,
    contest: String,
    lang: String,
    actions: Vec<EntryAction>,
    auto_pending: bool,
    auto_done: bool,
}

const SPLIT: f32 = 0.4;

const FREQ_PACE: Duration = Duration::from_secs(1);

impl Default for ZLinkPanel {
    fn default() -> Self {
        ZLinkPanel::new(crate::shared())
    }
}

pub fn start_problem(e: &StartError, lang: &str) -> String {
    let t = texts();
    match e {
        StartError::Role(Role::Client) => t.get(lang, "role-busy").into(),
        StartError::Role(_) => t.get(lang, "role-busy").into(),
        StartError::Settings(s) => t.format(lang, "server.bad", &[s]),
        StartError::Bind { port, error, text } => {
            let hint = match (*port < 1024, error) {
                (true, std::io::ErrorKind::PermissionDenied) => {
                    t.format(lang, "server.privileged", &[port])
                }
                _ => String::new(),
            };
            format!("{} {hint}", t.format(lang, "server.bind", &[port, text]))
                .trim()
                .to_string()
        }
    }
}

pub use zlog_ui::pairing::sorted_found;

fn chat_view(ui: &mut Ui, id: &str, chat: &[ChatLine]) {
    ScrollArea::both()
        .id_salt(id)
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for c in chat {
                let text = RichText::new(&c.text).monospace();
                ui.label(if c.red {
                    text.color(zlog_ui::palette::error(ui))
                } else {
                    text
                });
            }
        });
}

impl ZLinkPanel {
    pub fn new(session: Arc<Mutex<Session>>) -> Self {
        ZLinkPanel::with(session, crate::net::shared())
    }

    pub fn with(session: Arc<Mutex<Session>>, net: Arc<Shared>) -> Self {
        ZLinkPanel {
            session,
            net,
            input: String::new(),
            notice: None,
            problem: None,
            freq_sent: None,
            freq_due: None,
            edit_lock: None,
            split: SPLIT,
            browser: None,
            contest: String::new(),
            lang: "en".into(),
            actions: Vec::new(),
            auto_pending: false,
            auto_done: false,
        }
    }

    fn role(&self) -> Role {
        self.session.lock().unwrap().settings.role
    }

    fn freq_line(&mut self, pc: &str, view: &View<'_>) -> Option<String> {
        let hz = view.hz?;
        let now = Instant::now();
        let status = (view.band, hz, view.mode, view.cq);
        if let Some((sent, at)) = self.freq_sent {
            if sent == status {
                self.freq_due = None;
                return None;
            }
            if now.duration_since(at) < FREQ_PACE {
                self.freq_due = Some(at + FREQ_PACE);
                return None;
            }
        }
        self.freq_due = None;
        let time = freq_time(view.log, chrono::Utc::now());
        let multi = view.station.tx_nr == 1;
        let rig_band = zlog_core::band_of(hz).unwrap_or(view.band);
        self.freq_sent = Some((status, now));
        Some(protocol::freq(
            view.band, rig_band, multi, hz, view.mode, view.cq, &time, pc,
        ))
    }

    fn send_freq(&mut self, s: &mut Session, view: &View<'_>) {
        let pc = s.settings.pc_name.clone();
        if let Some(line) = self.freq_line(&pc, view) {
            s.freqs
                .take(&line[protocol::HEADER.len() + " FREQ ".len()..]);
            s.write(&line);
        }
    }

    fn track_edit(&mut self, editing: Option<&Qso>) {
        match (&self.edit_lock, editing) {
            (None, Some(q)) => {
                self.session.lock().unwrap().lock_qso(q);
                self.edit_lock = Some(q.clone());
            }
            (Some(_), None) => {
                if let Some(q) = self.edit_lock.take() {
                    self.session.lock().unwrap().unlock_qso(&q);
                }
            }
            (Some(old), Some(q)) if QsoId::of(old) != QsoId::of(q) => {
                let mut s = self.session.lock().unwrap();
                s.unlock_qso(old);
                s.lock_qso(q);
                self.edit_lock = Some(q.clone());
            }
            _ => {}
        }
    }

    pub fn follow(&mut self, view: &View<'_>) {
        self.contest = view.contest.name().to_string();
        self.lang = view.language.to_string();
        let session = self.session.clone();
        let mut s = session.lock().unwrap();
        s.pump();
        s.entry_band = Some(view.band);
        s.entry_operator = view.operator.to_string();
        s.settings.pc_name = view.pc_name.to_string();
        if !view.log_path.is_empty() {
            s.backup = Some(view.log_path.into());
        }
        let role = s.settings.role;
        // zLog sends the rig status whenever Z-Link is set up (`_zlinkport`; here a
        // host, or a connection), connected or not, and lists it among the running frequencies.
        if role == Role::Client && (s.connected() || !s.settings.host.trim().is_empty()) {
            self.send_freq(&mut s, view);
        }
        if s.connected() {
            if s.band_sent.is_some_and(|b| b != view.band) {
                s.send_band(view.band);
            }
            if s.operator_sent
                .as_deref()
                .is_some_and(|o| o != view.operator)
            {
                s.send_operator();
            }
        }
        drop(s);
        if role == Role::Server {
            let running = self.net.lock().running();
            let line = match running {
                true => self.freq_line(view.pc_name, view),
                false => None,
            };
            let mut n = self.net.lock();
            n.hub.local_tx = Some(i32::from(view.station.tx_nr.min(15)));
            if let Some(line) = line {
                n.hub
                    .freqs
                    .take(&line[protocol::HEADER.len() + " FREQ ".len()..]);
                n.broadcast(&line);
            }
        }
    }

    fn settings_ui(ui: &mut Ui, s: &mut Session, lang: &str) {
        let t = |k| texts().get(lang, k);
        let labels = [
            t("host"),
            t("port"),
            t("tls"),
            t("on-connect"),
            t("line-break"),
        ];
        zlog_ui::form(ui, "zlink-settings", &labels, |ui, form| {
            ui.label(t("host"));
            form.text(ui, egui::TextEdit::singleline(&mut s.settings.host));
            ui.end_row();
            ui.label(t("port"));
            form.text(ui, egui::TextEdit::singleline(&mut s.settings.port));
            ui.end_row();
            ui.label(t("tls"));
            ui.checkbox(&mut s.settings.tls, "")
                .on_hover_text(t("tls.tip"));
            ui.end_row();
            ui.label(t("auto_connect"));
            ui.checkbox(&mut s.settings.auto_connect, "")
                .on_hover_text(t("auto_connect.tip"));
            ui.end_row();
            ui.label(t("auto_start"));
            ui.checkbox(&mut s.settings.auto_start, "")
                .on_hover_text(t("auto_start.tip"));
            ui.end_row();
            ui.label(t("on-connect"));
            form.fit(ui, |ui| {
                form.combo("zlink-on-connect")
                    .selected_text(t(s.settings.on_connect.key()))
                    .show_ui(ui, |ui| {
                        for o in OnConnect::ALL {
                            ui.selectable_value(&mut s.settings.on_connect, o, t(o.key()));
                        }
                    });
            });
            ui.end_row();
            ui.label(t("line-break"));
            form.fit(ui, |ui| {
                form.combo("zlink-line-break")
                    .selected_text(format!("{:?}", s.settings.line_break))
                    .show_ui(ui, |ui| {
                        for l in LineBreak::ALL {
                            ui.selectable_value(&mut s.settings.line_break, l, format!("{l:?}"));
                        }
                    });
            });
            ui.end_row();
        });
    }

    pub fn connect(&mut self) {
        self.problem = None;
        let net = self.net.clone();
        let mut s = self.session.lock().unwrap();
        self.notice = match s.settings.tls {
            true => s.connect_secure(net, crate::net::Pairing::Auto).err(),
            false => s.connect_tcp().err(),
        };
    }

    pub fn start(&mut self) {
        self.notice = None;
        let got = self.net.lock().start(&self.contest);
        if got.is_ok() {
            self.session.lock().unwrap().settings.role = Role::Server;
        }
        self.problem = got.err().map(|e| start_problem(&e, &self.lang));
    }

    pub fn auto_start(&mut self) {
        if !std::mem::take(&mut self.auto_pending) || self.auto_done {
            return;
        }
        self.auto_done = true;
        let (role, connect, serve, idle) = {
            let s = self.session.lock().unwrap();
            let st = &s.settings;
            (
                st.role,
                st.auto_connect,
                st.auto_start,
                s.state == State::Disconnected,
            )
        };
        match role {
            Role::Client if connect && idle => self.connect(),
            Role::Server if serve && !self.net.lock().running() => self.start(),
            _ => {}
        }
    }

    pub fn stop(&mut self) {
        self.net.lock().stop();
    }

    fn connection(&self, language: &str) -> Option<zlog_ui::connection::Connection> {
        use zlog_ui::connection::{Connection, LinkState};
        let t = texts();
        let notice = self.notice.map(|n| t.get(language, n).to_string());
        let (state, error) = match self.role() {
            Role::Server => match self.net.lock().running() {
                true => (LinkState::Connected, None),
                false => {
                    let error = notice.or_else(|| self.problem.clone());
                    let state = match error {
                        Some(_) => LinkState::Failed,
                        None => LinkState::Off,
                    };
                    (state, error)
                }
            },
            Role::Client => {
                let s = self.session.lock().unwrap();
                if s.settings.host.trim().is_empty() {
                    return None;
                }
                match (s.state, notice, &s.error) {
                    (State::Connected, _, _) => (LinkState::Connected, None),
                    (State::Connecting, _, _) => (LinkState::Connecting, None),
                    (_, Some(n), _) => (LinkState::Failed, Some(n)),
                    (_, None, Some(e)) => (
                        LinkState::Failed,
                        Some(zlog_ui::connection::explain_net(language, e)),
                    ),
                    _ => (LinkState::Off, None),
                }
            }
            _ => return None,
        };
        Some(
            Connection::new("link", t.get(language, "title"), state)
                .error(error)
                .page("panel.link")
                .commands("link.connect", "link.disconnect"),
        )
    }

    fn toolbar_actions(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = texts();
        let (state, host) = {
            let s = self.session.lock().unwrap();
            (s.state, !s.settings.host.trim().is_empty())
        };
        let running = self.net.lock().running();
        let label = match state {
            State::Disconnected => "join",
            State::Connecting => "connecting",
            State::Connected => "leave",
        };
        let (shown, icon) = match state {
            State::Disconnected => ("toolbar.state.disconnected", zlog_ui::icon::PLUGS),
            State::Connecting => ("toolbar.state.connecting", zlog_ui::icon::PLUGS),
            State::Connected => ("toolbar.state.connected", zlog_ui::icon::PLUGS_CONNECTED),
        };
        let mut join = zlog_ui::Command::action(
            CONNECT,
            t.get(language, label),
            t.get(language, "toolbar.connect.tip"),
        )
        .icon(icon)
        .state(t.get(language, shown))
        .selected(state != State::Disconnected)
        .menu_label(t.get(language, "menu.connect"));
        join.menu = true;
        join.checkable = true;
        join = join.section(zlog_ui::MenuSection::Link, 2);
        if running {
            join = join.disabled_because(t.get(language, "role-busy"));
        } else if !host && state == State::Disconnected {
            join = join.disabled_because(t.get(language, "toolbar.no_host"));
        }
        let (label, icon, shown) = match running {
            true => ("server.stop", zlog_ui::icon::STOP, "toolbar.state.serving"),
            false => (
                "server.start",
                zlog_ui::icon::BROADCAST,
                "toolbar.state.stopped",
            ),
        };
        let mut share = zlog_ui::Command::action(
            SHARE,
            t.get(language, label),
            t.get(language, "toolbar.server.tip"),
        )
        .icon(icon)
        .state(t.get(language, shown))
        .selected(running)
        .menu_label(t.get(language, "menu.server"));
        share.menu = true;
        share.checkable = true;
        share = share.section(zlog_ui::MenuSection::Link, 2);
        if state != State::Disconnected {
            share = share.disabled_because(t.get(language, "role-busy"));
        }
        let settings = zlog_ui::Command::new(
            SETTINGS,
            t.get(language, "menu.settings"),
            t.get(language, "menu.settings.tip"),
        )
        .icon(zlog_ui::icon::GEAR)
        .section(zlog_ui::MenuSection::Link, 3)
        .dialog();
        vec![join, share, settings]
    }

    fn input_row(&mut self, ui: &mut Ui, lang: &str, enabled: bool) -> Option<String> {
        let t = texts();
        let mut send = false;
        let edit = zlog_ui::field_row(
            ui,
            |_| {},
            |ui| {
                send = ui
                    .add_enabled(enabled, egui::Button::new(t.get(lang, "send")))
                    .clicked();
            },
            |ui| {
                ui.add_enabled(
                    enabled,
                    egui::TextEdit::singleline(&mut self.input)
                        .hint_text(t.get(lang, "chat"))
                        .desired_width(f32::INFINITY),
                )
            },
        );
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (enter || send) && !self.input.is_empty() {
            edit.request_focus();
            return Some(std::mem::take(&mut self.input));
        }
        None
    }

    fn discovery_ui(&mut self, ui: &mut Ui, lang: &str) {
        let Some(b) = self.browser.as_mut() else {
            return;
        };
        let t = texts();
        let recent = self.session.lock().unwrap().settings.recent.clone();
        let rows: Vec<zlog_ui::pairing::FoundRow> = sorted_found(b.poll(), &recent)
            .into_iter()
            .map(|(found, at)| {
                let contest = found.get("contest").unwrap_or("").to_string();
                let warning = (!contest.is_empty() && contest != self.contest)
                    .then(|| t.format(lang, "discover.other", &[&contest]));
                zlog_ui::pairing::FoundRow { found, at, warning }
            })
            .collect();
        if let Some(i) = zlog_ui::pairing::found_list(ui, &rows, lang) {
            let tls = rows[i].found.get("tls") == Some("1");
            if let Ok(a) = rows[i].at.parse::<std::net::SocketAddr>() {
                let mut s = self.session.lock().unwrap();
                s.settings.host = a.ip().to_string();
                s.settings.port = a.port().to_string();
                s.settings.tls = tls;
            }
            self.browser = None;
        }
    }

    fn client_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = texts();
        let lang = view.language;
        let session = self.session.clone();
        let mut s = session.lock().unwrap();
        s.pump();
        let mode = match (s.state, s.secure) {
            (State::Disconnected, _) => String::new(),
            (_, true) => "TLS".into(),
            (_, false) => t.get(lang, "plaintext").into(),
        };
        let (state, problem) = match s.state {
            State::Disconnected => (
                String::new(),
                match (self.notice, &s.error) {
                    (Some(n), _) => t.get(lang, n).to_string(),
                    (None, Some(e)) => format!(
                        "{}: {}",
                        t.get(lang, "failed"),
                        zlog_ui::connection::explain_net(lang, e)
                    ),
                    (None, None) => String::new(),
                },
            ),
            State::Connecting => (t.get(lang, "connecting").to_string(), String::new()),
            State::Connected => (t.get(lang, "connected").to_string(), String::new()),
        };
        let plain = s.state != State::Disconnected && !s.secure;
        let mut discover = false;
        let mut connect = false;
        let mut share = false;
        zlog_ui::layout::toolbar(
            ui,
            &[(&state, false), (&mode, plain), (&problem, true)],
            |ui| {
                if s.state == State::Disconnected {
                    let glyph = zlog_ui::icon::PLUGS;
                    connect = zlog_ui::layout::tool(ui, glyph, t.get(lang, "join"))
                        .on_hover_text(t.get(lang, "toolbar.connect.tip"))
                        .clicked();
                    discover = zlog_ui::layout::toggle(
                        ui,
                        zlog_ui::icon::MAGNIFYING_GLASS,
                        &zlog_ui::pairing::search_label(lang),
                        self.browser.is_some(),
                    )
                    .on_hover_text(zlog_ui::pairing::search_tip(lang))
                    .clicked();
                    share = zlog_ui::layout::tool(
                        ui,
                        zlog_ui::icon::BROADCAST,
                        t.get(lang, "server.start"),
                    )
                    .on_hover_text(t.get(lang, "toolbar.server.tip"))
                    .clicked();
                } else {
                    if zlog_ui::layout::tool(
                        ui,
                        zlog_ui::icon::PLUGS_CONNECTED,
                        t.get(lang, "leave"),
                    )
                    .clicked()
                    {
                        s.disconnect();
                    }
                    if s.state == State::Connecting {
                        ui.spinner();
                    }
                }
            },
        );
        let idle = s.state == State::Disconnected;
        let (mut host, mut port) = (s.settings.host.clone(), s.settings.port.clone());
        zlog_ui::field_row(
            ui,
            |_| {},
            |ui| {
                ui.add_enabled(
                    idle,
                    egui::TextEdit::singleline(&mut port).desired_width(48.0),
                )
                .on_hover_text(t.get(lang, "port"));
            },
            |ui| {
                ui.add_enabled(
                    idle,
                    egui::TextEdit::singleline(&mut host)
                        .hint_text(t.get(lang, "host"))
                        .desired_width(f32::INFINITY),
                )
            },
        );
        s.settings.host = host;
        s.settings.port = port;
        let connected = s.connected();
        let prompt_key = s.settings.prompt;
        let addr = s.settings.address();
        let freqs = s.freqs.lines();
        let chat = s.chat.clone();
        drop(s);
        if discover {
            zlog_ui::pairing::toggle_search(&mut self.browser, crate::net::APP);
        }
        if connect {
            self.connect();
        }
        if share {
            self.start();
        }
        self.discovery_ui(ui, lang);
        let mut prompt = prompt_key;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("zlink-prompt")
                .selected_text(t.get(lang, prompt.key()))
                .show_ui(ui, |ui| {
                    for p in Prompt::ALL {
                        ui.selectable_value(&mut prompt, p, t.get(lang, p.key()));
                    }
                });
        });
        if let Some(text) = self.input_row(ui, lang, connected) {
            let mut s = self.session.lock().unwrap();
            let p = prompt.text(
                view.band,
                &s.settings.pc_name,
                &s.entry_operator,
                &view.station.callsign,
            );
            let now = chrono::Local::now().format("%H:%M").to_string();
            s.send_chat(&now, &p, &text);
        }
        self.session.lock().unwrap().settings.prompt = prompt;
        zlog_ui::layout::split(
            ui,
            &mut self.split,
            |ui| {
                zlog_ui::layout::caption(ui, &format!("{} {addr}", t.get(lang, "peers.server")));
                ScrollArea::both()
                    .id_salt("zlink-peers")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        for l in &freqs {
                            ui.label(RichText::new(l).monospace().small());
                        }
                    });
            },
            |ui| chat_view(ui, "zlink-chat", &chat),
        );
    }

    fn server_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = texts();
        let lang = view.language;
        let (running, secure, count, addr) = {
            let n = self.net.lock();
            (
                n.running(),
                n.secure() == Some(true),
                n.hub.approved(),
                n.listener.as_ref().map(|l| l.addr().to_string()),
            )
        };
        let state = match running {
            true => t.format(lang, "server.clients", &[&count]),
            false => t.get(lang, "toolbar.state.stopped").to_string(),
        };
        let mode = match (running, secure) {
            (false, _) => String::new(),
            (true, true) => "TLS".into(),
            (true, false) => t.get(lang, "plaintext").into(),
        };
        let problem = match self.notice {
            Some(n) => t.get(lang, n).to_string(),
            None => self.problem.clone().unwrap_or_default(),
        };
        let mut toggle = false;
        let mut join = false;
        zlog_ui::layout::toolbar(
            ui,
            &[
                (&state, false),
                (&mode, running && !secure),
                (&problem, true),
            ],
            |ui| {
                let (icon, label) = match running {
                    true => (zlog_ui::icon::STOP, "server.stop"),
                    false => (zlog_ui::icon::BROADCAST, "server.start"),
                };
                toggle = zlog_ui::layout::tool(ui, icon, t.get(lang, label))
                    .on_hover_text(t.get(lang, "toolbar.server.tip"))
                    .clicked();
                if !running {
                    join = zlog_ui::layout::tool(ui, zlog_ui::icon::PLUGS, t.get(lang, "join"))
                        .on_hover_text(t.get(lang, "toolbar.connect.tip"))
                        .clicked();
                }
            },
        );
        if toggle {
            match running {
                true => self.stop(),
                false => self.start(),
            }
        }
        if join {
            self.session.lock().unwrap().settings.role = Role::Client;
            self.connect();
        }
        if let Some(text) = self.input_row(ui, lang, running) {
            let now = chrono::Local::now().format("%H:%M").to_string();
            if let Some((shown, line)) = protocol::chat(&now, "ZServer", &text) {
                self.net.lock().say(shown, &line, text.starts_with('!'));
            }
        }
        let n = self.net.lock();
        let mut peers: Vec<_> = n.hub.clients.values().cloned().collect();
        peers.sort_by_key(|p| p.number);
        let chat = n.hub.chat.clone();
        drop(n);
        zlog_ui::layout::split(
            ui,
            &mut self.split,
            |ui| {
                zlog_ui::layout::caption(ui, &addr.unwrap_or_default());
                ScrollArea::both()
                    .id_salt("zlink-peers")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        if peers.is_empty() {
                            ui.weak(t.get(lang, "peers.none"));
                        }
                        for p in &peers {
                            let band = p.band.map(protocol::band_string).unwrap_or_default();
                            let mut text =
                                format!("#{} {} {} {band}", p.number, p.pc_name, p.operator);
                            if !p.approved {
                                text = format!("{text} {}", t.get(lang, "peers.waiting"));
                            }
                            ui.label(RichText::new(text).monospace())
                                .on_hover_text(format!(
                                    "{} {}",
                                    p.addr,
                                    if p.tls { "TLS" } else { "" }
                                ));
                        }
                    });
            },
            |ui| chat_view(ui, "zlink-chat", &chat),
        );
    }
}

#[derive(Default)]
pub struct LinkOptions {
    shown: Option<zlog_tls::Token>,
}

impl LinkOptions {
    pub fn shown_token(&self) -> Option<&str> {
        self.shown.as_ref().map(|t| t.as_str())
    }
}

impl zlog_ui::SettingsPage for LinkOptions {
    fn id(&self) -> &str {
        "panel.link"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut Section, lang: &str) {
        let t = |k| texts().get(lang, k);
        let mut p = ZLinkPanel::default();
        p.load(section);
        {
            let mut s = p.session.lock().unwrap();
            ZLinkPanel::settings_ui(ui, &mut s, lang);
        }
        p.save(section);
        ui.weak(t("role.note"));
        ui.separator();
        zlog_ui::layout::caption(ui, t("server.title"));
        zlog_ui::dialog::settings_form(ui, &crate::net::fields(lang), section, lang);
        ui.weak(t("server.note"));
        ui.separator();
        crate::secure::secrets_ui(ui, &p.net, &mut self.shown, lang);
    }
}

impl Panel for ZLinkPanel {
    fn migrations(&self) -> Vec<zlog_ui::migration::Migration> {
        vec![
            zlog_ui::migration::Migration::RenamedPanel("zserver", "link"),
            zlog_ui::migration::Migration::CopiedKey {
                from: ("zserver", "pc-name"),
                to: ("preferences", "pc_name"),
            },
        ]
    }

    fn id(&self) -> &str {
        "link"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Network
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Link, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        let mut b = vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::NETWORK)
        .shortcut(egui::KeyboardShortcut::new(
            egui::Modifiers::ALT,
            egui::Key::Z,
        ))];
        b.extend(self.toolbar_actions(language));
        b
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 520.0]
    }

    fn load(&mut self, section: &Section) {
        let get = |k: &str| section.get(k).cloned().unwrap_or_default();
        let mut s = self.session.lock().unwrap();
        let st = &mut s.settings;
        st.host = get("host");
        st.port = section
            .get("port")
            .cloned()
            .unwrap_or_else(|| "telnet".into());
        st.line_break = get("line-break")
            .parse::<usize>()
            .ok()
            .and_then(|i| LineBreak::ALL.get(i).copied())
            .unwrap_or_default();
        st.on_connect = OnConnect::ALL
            .into_iter()
            .find(|o| o.key() == get("on-connect"))
            .unwrap_or_default();
        st.prompt = Prompt::ALL
            .into_iter()
            .find(|p| p.key() == get("prompt"))
            .unwrap_or_default();
        st.role = Role::parse(&get("role"));
        st.tls = get("tls") == "true";
        st.recent = get("recent")
            .split(';')
            .filter(|r| !r.trim().is_empty())
            .map(String::from)
            .collect();
        st.auto_connect = get("auto_connect") == "true";
        st.auto_start = get("auto_start") == "true";
        drop(s);
        self.net.lock().settings = crate::net::ServerSettings::load(section);
        self.auto_pending = !self.auto_done;
        self.split = zlog_ui::layout::load_ratio(section, "split", SPLIT);
    }

    fn save(&self, section: &mut Section) {
        let s = self.session.lock().unwrap();
        let st = &s.settings;
        section.insert("host".into(), st.host.clone());
        section.insert("port".into(), st.port.clone());
        section.remove("pc-name");
        section.insert("line-break".into(), st.line_break.index().to_string());
        section.insert("on-connect".into(), st.on_connect.key().into());
        section.insert("prompt".into(), st.prompt.key().into());
        section.insert("role".into(), st.role.key().into());
        section.insert("tls".into(), st.tls.to_string());
        section.insert("recent".into(), st.recent.join(";"));
        section.insert("auto_connect".into(), st.auto_connect.to_string());
        section.insert("auto_start".into(), st.auto_start.to_string());
        zlog_ui::layout::save_ratio(section, "split", self.split);
    }

    fn run_command(&mut self, id: &str) {
        match id {
            CONNECT => {
                let state = self.session.lock().unwrap().state;
                match state {
                    State::Disconnected => self.connect(),
                    _ => self.session.lock().unwrap().disconnect(),
                }
            }
            SHARE if self.net.lock().running() => self.stop(),
            SHARE => self.start(),
            SETTINGS => self.actions.push(EntryAction::Options("panel.link".into())),
            "link.connect" => match self.role() {
                Role::Server => {
                    self.stop();
                    self.start();
                }
                _ => self.connect(),
            },
            "link.disconnect" => match self.role() {
                Role::Server => self.stop(),
                _ => self.session.lock().unwrap().disconnect(),
            },
            _ => {}
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn capability(&self, want: zlog_ui::Want<'_>) -> Option<zlog_ui::Capability<'_>> {
        let zlog_ui::Want::Connections(language) = want else {
            return None;
        };
        Some(zlog_ui::Capability::Connections(
            self.connection(language).into_iter().collect(),
        ))
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        self.freq_due
            .map(|t| t.saturating_duration_since(Instant::now()))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.follow(view);
        self.track_edit(view.editing);
        self.auto_start();
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        match self.role() {
            Role::Server => self.server_ui(ui, view),
            _ => self.client_ui(ui, view),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Shared;
    use chrono::TimeZone;

    #[test]
    fn freq_time_follows_the_log() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 10, 21, 3, 4, 5).unwrap();
        let mut log = zlog_core::Log::default();
        log.header.rst_sent = zlog_core::log::USE_UTC;
        assert_eq!(freq_time(&log, now), "03:04:05");
        let local = now
            .with_timezone(&chrono::Local)
            .format("%H:%M:%S")
            .to_string();
        log.header.rst_sent = (-540i16) as u16;
        assert!(log.utc_offset_minutes().is_some());
        assert_eq!(freq_time(&log, now), local);
    }

    struct Silent;

    impl crate::session::Transport for Silent {
        fn send(&mut self, _bytes: &[u8]) {}
        fn poll(&mut self) -> Vec<crate::session::NetEvent> {
            Vec::new()
        }
        fn close(&mut self) {}
    }

    struct Sent(Arc<Mutex<Vec<u8>>>);

    impl crate::session::Transport for Sent {
        fn send(&mut self, bytes: &[u8]) {
            self.0.lock().unwrap().extend_from_slice(bytes);
        }
        fn poll(&mut self) -> Vec<crate::session::NetEvent> {
            Vec::new()
        }
        fn close(&mut self) {}
    }

    #[test]
    fn edit_lock_moves_to_the_new_qso() {
        let session = Arc::new(Mutex::new(Session::default()));
        let mut p = ZLinkPanel::new(session.clone());
        let sent = Arc::new(Mutex::new(Vec::new()));
        session
            .lock()
            .unwrap()
            .connect(Box::new(Sent(sent.clone())));
        session.lock().unwrap().state = State::Connected;
        let qso = |call: &str, id| Qso {
            callsign: call.into(),
            reserve3: id * 100,
            ..Qso::default()
        };
        let (a, b) = (qso("JA1A", 1), qso("JA1B", 2));
        p.track_edit(Some(&a));
        sent.lock().unwrap().clear();
        p.track_edit(Some(&b));
        let text = String::from_utf8_lossy(&sent.lock().unwrap()).to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(
            lines[0].contains("UNLOCKQSO") && lines[0].contains("~JA1A~"),
            "{text}"
        );
        assert!(
            lines[1].contains(" LOCKQSO") && lines[1].contains("~JA1B~"),
            "{text}"
        );
        assert_eq!(p.edit_lock.as_ref().map(QsoId::of), Some(QsoId::of(&b)));
    }

    #[test]
    fn a_connected_idle_link_needs_no_tick() {
        let session = Arc::new(Mutex::new(Session::default()));
        let mut p = ZLinkPanel::new(session.clone());
        session.lock().unwrap().settings.host = "localhost".into();
        assert_eq!(p.tick_after(), None, "set up");
        session.lock().unwrap().connect(Box::new(Silent));
        session.lock().unwrap().state = State::Connected;
        assert_eq!(p.tick_after(), None, "connected");
        p.freq_due = Some(Instant::now() + Duration::from_millis(600));
        let wait = p.tick_after().unwrap();
        assert!(wait <= Duration::from_millis(600), "{wait:?}");
    }

    fn toolbar_action(p: &ZLinkPanel) -> (String, bool, bool) {
        let b = p.commands("en").remove(1);
        assert_eq!(b.id, "connect");
        (b.label, b.enabled, b.selected)
    }

    #[test]
    fn toolbar_connect_follows_the_session() {
        let session = Arc::new(Mutex::new(Session::default()));
        let mut p = ZLinkPanel::new(session.clone());
        session.lock().unwrap().settings.host.clear();
        assert_eq!(
            toolbar_action(&p),
            (texts().get("en", "join").into(), false, false)
        );
        p.run_command("connect");
        assert_eq!(session.lock().unwrap().state, State::Disconnected);
        assert!(p.notice.is_some());
        session.lock().unwrap().settings.host = "localhost".into();
        assert_eq!(
            toolbar_action(&p),
            (texts().get("en", "join").into(), true, false)
        );
        session.lock().unwrap().connect(Box::new(Silent));
        assert_eq!(toolbar_action(&p), ("Connecting…".into(), true, true));
        session.lock().unwrap().state = State::Connected;
        assert_eq!(
            toolbar_action(&p),
            (texts().get("en", "leave").into(), true, true)
        );
        p.run_command("connect");
        assert_eq!(session.lock().unwrap().state, State::Disconnected);
    }

    #[test]
    fn connect_is_also_a_network_menu_toggle() {
        let p = ZLinkPanel::new(Arc::new(Mutex::new(Session::default())));
        for lang in ["en", "ja"] {
            let c = p
                .commands(lang)
                .into_iter()
                .find(|c| c.id == "connect")
                .unwrap();
            assert!(c.menu && c.toolbar && c.checkable && !c.selected);
            assert_eq!(c.section, zlog_ui::MenuSection::Link);
            assert_ne!(c.menu_text(), c.label);
        }
    }

    fn role_panel(role: Role) -> ZLinkPanel {
        let session = Arc::new(Mutex::new(Session::default()));
        session.lock().unwrap().settings.role = role;
        ZLinkPanel::with(session, Arc::new(Shared::default()))
    }

    #[test]
    fn join_and_share_are_two_actions_that_exclude_each_other() {
        for lang in ["en", "ja"] {
            let p = role_panel(Role::Off);
            p.session.lock().unwrap().settings.host = "localhost".into();
            let c = p.commands(lang);
            let ids: Vec<_> = c.iter().map(|c| c.id.as_str()).collect();
            assert_eq!(ids, [zlog_ui::Command::WINDOW, CONNECT, SHARE, SETTINGS]);
            assert!(c[1].enabled, "join needs no role choice");
            assert!(c[2].enabled && c[2].label == texts().get(lang, "server.start"));
            assert!(c[3].enabled && c[3].menu && !c[3].toolbar);
            p.session.lock().unwrap().connect(Box::new(Silent));
            let c = p.commands(lang);
            assert_eq!(
                c[2].disabled_reason.as_deref(),
                Some(texts().get(lang, "role-busy"))
            );
        }
    }

    #[test]
    fn settings_open_the_one_z_link_page() {
        let mut p = role_panel(Role::Server);
        p.run_command(SETTINGS);
        assert_eq!(
            p.take_actions(),
            [EntryAction::Options("panel.link".into())]
        );
        let mut p = role_panel(Role::Client);
        p.run_command(SETTINGS);
        assert_eq!(
            p.take_actions(),
            [EntryAction::Options("panel.link".into())]
        );
    }

    #[test]
    fn role_tls_and_recent_servers_survive_save_and_load() {
        let mut p = role_panel(Role::Server);
        {
            let mut s = p.session.lock().unwrap();
            s.settings.tls = true;
            s.settings.recent = vec!["10.0.0.2:23".into(), "10.0.0.3:5000".into()];
        }
        let mut section = Section::new();
        p.save(&mut section);
        let mut q = role_panel(Role::Off);
        q.load(&section);
        let s = q.session.lock().unwrap();
        assert_eq!(s.settings.role, Role::Server);
        assert!(s.settings.tls);
        assert_eq!(s.settings.recent.len(), 2);
        drop(s);
        p.load(&Section::new());
        assert_eq!(p.role(), Role::Client, "the old default stays a client");
    }

    #[test]
    fn found_servers_list_the_last_used_first() {
        let found = |name: &str, ip: &str, port| zlog_tls::discovery::Found {
            name: name.into(),
            addrs: vec![ip.parse().unwrap()],
            port,
            txt: vec![
                ("contest".into(), "ALLJA1".into()),
                ("tls".into(), "0".into()),
            ],
        };
        let list = sorted_found(
            vec![
                found("A", "10.0.0.1", 23),
                found("B", "10.0.0.2", 23),
                found("C", "10.0.0.3", 23),
            ],
            &["10.0.0.3:23".into(), "10.0.0.2:23".into()],
        );
        let names: Vec<_> = list.iter().map(|(f, _)| f.name.as_str()).collect();
        assert_eq!(names, ["C", "B", "A"]);
        assert_eq!(list[0].0.get("contest"), Some("ALLJA1"));
    }

    #[test]
    fn startup_options_are_off_by_default_and_respect_the_role() {
        let _g = crate::role::serial();
        let mut p = role_panel(Role::Server);
        p.load(&Section::new());
        let s = p.session.lock().unwrap().settings.clone();
        assert!(!s.auto_connect && !s.auto_start);
        let mut section = Section::new();
        for (k, v) in [
            ("role", "server"),
            ("auto_start", "true"),
            ("auto_connect", "true"),
            ("server_port", "0"),
            ("server_bind", "127.0.0.1"),
        ] {
            section.insert(k.into(), v.into());
        }
        p.load(&section);
        p.auto_start();
        assert!(p.net.lock().running(), "started once at startup");
        assert_eq!(crate::role::active(), Role::Server);
        p.net.lock().stop();
        p.load(&section);
        p.auto_start();
        assert!(!p.net.lock().running(), "only at startup");
        let mut q = role_panel(Role::Client);
        section.insert("role".into(), "client".into());
        section.insert("host".into(), "127.0.0.1".into());
        section.insert("port".into(), "9".into());
        q.load(&section);
        crate::role::claim(Role::Server).unwrap();
        q.auto_start();
        assert_eq!(q.notice, Some("role-busy"), "the role lock still holds");
        crate::role::release(Role::Server);
    }
}
