//! The Z-Link window: connection settings (`TformZLinkTelnetSet`), the console
//! (`TZLinkForm`) and the chat (`TChatForm`); the stations' rig status is in
//! [`crate::freq`].

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{RichText, ScrollArea, Ui};
use zlog_core::{Band, Mode, Qso, QsoId};
use zlog_ui::{Panel, Section, View};

use crate::session::{LineBreak, OnConnect, Prompt, Session, State};
use crate::{protocol, texts};

/// `hh:nn:ss` of `now` as zLog's `CurrentTime`: UTC when the log records UTC,
/// local time otherwise.
fn freq_time(log: &zlog_core::Log, now: chrono::DateTime<chrono::Utc>) -> String {
    const FORMAT: &str = "%H:%M:%S";
    match log.utc_offset_minutes() {
        None => now.format(FORMAT).to_string(),
        Some(_) => now.with_timezone(&chrono::Local).format(FORMAT).to_string(),
    }
}

pub struct ZLinkPanel {
    session: Arc<Mutex<Session>>,
    input: String,
    notice: Option<&'static str>,
    /// Rig status (band, hertz, mode, CQ) last sent with `FREQ`, and when.
    freq_sent: Option<((Band, u64, Mode, bool), Instant)>,
    /// The QSO locked while the edit dialog is open on it.
    edit_lock: Option<Qso>,
}

impl Default for ZLinkPanel {
    fn default() -> Self {
        ZLinkPanel::new(crate::shared())
    }
}

impl ZLinkPanel {
    pub fn new(session: Arc<Mutex<Session>>) -> Self {
        ZLinkPanel {
            session,
            input: String::new(),
            notice: None,
            freq_sent: None,
            edit_lock: None,
        }
    }

    /// `TZLinkForm.SendRigStatus`: the entry row's band ordinal, the rig's band,
    /// frequency and mode, at most once a second, when they change (zLog sends on every rig poll), with the CQ/SP flag of
    /// the entry row.
    ///
    /// zLog's multiplier station (`IsMultiStation`: category Multi-Op Single-TX and
    /// TX# 1) sends `30` as band ordinal; the category is not in the view, so here a
    /// station with TX# 1 counts as the multiplier station (departure).
    fn send_freq(&mut self, s: &mut Session, view: &View<'_>) {
        let Some(hz) = view.hz else {
            return;
        };
        let now = Instant::now();
        let status = (view.band, hz, view.mode, view.cq);
        if let Some((sent, at)) = self.freq_sent {
            if sent == status || now.duration_since(at) < Duration::from_secs(1) {
                return;
            }
        }
        let time = freq_time(view.log, chrono::Utc::now());
        let pc = s.settings.pc_name.clone();
        let multi = view.station.tx_nr == 1;
        // The rig's band (`FCurrentRig.CurrentBand`); the entry row's when the
        // frequency is out of every band. The view has no rig mode, so the entry
        // row's mode stands for it.
        let rig_band = zlog_rig::sync::band_of(hz).unwrap_or(view.band);
        let line = protocol::freq(
            view.band, rig_band, multi, hz, view.mode, view.cq, &time, &pc,
        );
        // Shown here too (`MainForm.FreqList.ProcessFreqData`), then sent, which
        // does nothing while disconnected (`WriteData`).
        s.freqs
            .take(&line[protocol::HEADER.len() + " FREQ ".len()..]);
        s.write(&line);
        self.freq_sent = Some((status, now));
    }

    /// Tell the other stations when the edit dialog opens on a QSO and closes
    /// (`TEditDialog.Init` locks the QSO, closing unlocks it, saved or cancelled);
    /// nothing is sent when not connected.
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
            // Another QSO opened without closing the first: release it, lock the new one.
            (Some(old), Some(q)) if QsoId::of(old) != QsoId::of(q) => {
                let mut s = self.session.lock().unwrap();
                s.unlock_qso(old);
                s.lock_qso(q);
                self.edit_lock = Some(q.clone());
            }
            _ => {}
        }
    }

    /// What zLog sends as the entry row changes, whether the window is open or not:
    /// `SendBand` on a band change, `SendOperator` on an operator change
    /// (`TMainForm.OpMenuClick`), and the rig status.
    pub fn follow(&mut self, view: &View<'_>) {
        let session = self.session.clone();
        let mut s = session.lock().unwrap();
        s.pump();
        s.entry_band = Some(view.band);
        s.entry_operator = view.operator.to_string();
        // The PC name is set in the Options (`Settings._pcname`), not in this panel.
        s.settings.pc_name = view.pc_name.to_string();
        // zLog sends the rig status whenever Z-Link is set up (`_zlinkport`; here a
        // host, or a connection), connected or not, and lists it among the running frequencies.
        if s.connected() || !s.settings.host.trim().is_empty() {
            self.send_freq(&mut s, view);
        }
        if !s.connected() {
            return;
        }
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

    fn settings_ui(ui: &mut Ui, s: &mut Session, lang: &str) {
        let t = texts();
        let editable = s.state == State::Disconnected;
        ui.add_enabled_ui(editable, |ui| {
            // The host takes what the labels and the port leave.
            let port_width = 60.0;
            let width =
                (zlog_ui::form_field_width(ui, &[t.get(lang, "host"), t.get(lang, "on-connect")])
                    - port_width
                    - 60.0)
                    .max(zlog_ui::FIELD_MIN);
            egui::Grid::new("zlink-settings").show(ui, |ui| {
                ui.label(t.get(lang, "host"));
                zlog_ui::sized_field(ui, egui::TextEdit::singleline(&mut s.settings.host), width);
                ui.label(t.get(lang, "port"));
                ui.add(egui::TextEdit::singleline(&mut s.settings.port).desired_width(port_width));
                ui.end_row();
                ui.label(t.get(lang, "on-connect"));
                egui::ComboBox::from_id_salt("zlink-on-connect")
                    .selected_text(t.get(lang, s.settings.on_connect.key()))
                    .show_ui(ui, |ui| {
                        for o in OnConnect::ALL {
                            ui.selectable_value(
                                &mut s.settings.on_connect,
                                o,
                                t.get(lang, o.key()),
                            );
                        }
                    });
                ui.label(t.get(lang, "line-break"));
                egui::ComboBox::from_id_salt("zlink-line-break")
                    .selected_text(format!("{:?}", s.settings.line_break))
                    .show_ui(ui, |ui| {
                        for l in LineBreak::ALL {
                            ui.selectable_value(&mut s.settings.line_break, l, format!("{l:?}"));
                        }
                    });
                ui.end_row();
            });
        });
    }
}

impl ZLinkPanel {
    /// The toolbar's Connect/Disconnect button, labelled by the session state.
    fn toolbar_actions(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = texts();
        let (state, host) = {
            let s = self.session.lock().unwrap();
            (s.state, !s.settings.host.trim().is_empty())
        };
        let label = match state {
            State::Disconnected => "connect",
            State::Connecting => "connecting",
            State::Connected => "disconnect",
        };
        let (shown, icon) = match state {
            State::Disconnected => ("toolbar.state.disconnected", zlog_ui::icon::PLUGS),
            State::Connecting => ("toolbar.state.connecting", zlog_ui::icon::PLUGS_CONNECTED),
            State::Connected => ("toolbar.state.connected", zlog_ui::icon::PLUGS_CONNECTED),
        };
        let mut b = zlog_ui::Command::action(
            "connect",
            t.get(language, label),
            t.get(language, "toolbar.connect.tip"),
        )
        .icon(icon)
        .state(t.get(language, shown))
        .selected(state != State::Disconnected);
        if !host && state == State::Disconnected {
            b = b.disabled_because(t.get(language, "toolbar.no_host"));
        }
        vec![b]
    }
}

impl Panel for ZLinkPanel {
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
        .icon(zlog_ui::icon::NETWORK)];
        b.extend(self.toolbar_actions(language));
        b
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 520.0]
    }

    /// Read the settings (zlog.ini `[Z-Link]`: `TELNEThost`, `Port`,
    /// `TELNETlinebreak`; the others are this client's). `PCName` is an application
    /// setting that comes in [`View::pc_name`]; the application reads this
    /// section's old `pc-name` key once when it has none of its own.
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
    }

    fn save(&self, section: &mut Section) {
        let s = self.session.lock().unwrap();
        let st = &s.settings;
        section.insert("host".into(), st.host.clone());
        section.insert("port".into(), st.port.clone());
        // Moved to the application's preferences; the application read it on start.
        section.remove("pc-name");
        section.insert("line-break".into(), st.line_break.index().to_string());
        section.insert("on-connect".into(), st.on_connect.key().into());
        section.insert("prompt".into(), st.prompt.key().into());
    }

    /// Connect as the window's Connect button does, or disconnect.
    fn run_command(&mut self, id: &str) {
        if id != "connect" {
            return;
        }
        let mut s = self.session.lock().unwrap();
        if s.state == State::Disconnected {
            self.notice = s.connect_tcp().err();
        } else {
            s.disconnect();
        }
    }

    fn tick(&mut self, view: &View<'_>) {
        self.follow(view);
        self.track_edit(view.editing);
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = texts();
        let lang = view.language;
        ui.ctx().request_repaint_after(Duration::from_millis(250));
        let session = self.session.clone();
        let mut s = session.lock().unwrap();
        s.pump();

        Self::settings_ui(ui, &mut s, lang);
        ui.horizontal(|ui| match s.state {
            State::Disconnected => {
                if ui.button(t.get(lang, "connect")).clicked() {
                    self.notice = s.connect_tcp().err();
                }
                if let Some(n) = self.notice {
                    ui.colored_label(zlog_ui::palette::error(ui), t.get(lang, n));
                } else if let Some(e) = &s.error {
                    ui.colored_label(
                        zlog_ui::palette::error(ui),
                        format!("{}: {e}", t.get(lang, "failed")),
                    );
                }
            }
            State::Connecting => {
                if ui.button(t.get(lang, "disconnect")).clicked() {
                    s.disconnect();
                }
                ui.spinner();
                ui.label(t.get(lang, "connecting"));
            }
            State::Connected => {
                if ui.button(t.get(lang, "disconnect")).clicked() {
                    s.disconnect();
                }
                ui.label(t.get(lang, "connected"));
            }
        });

        ui.separator();
        ui.label(RichText::new(t.get(lang, "chat")).strong());
        ScrollArea::vertical()
            .id_salt("zlink-chat")
            // The rest of the window but the chat line and the console.
            .max_height((ui.available_height() - 70.0).max(80.0))
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for c in &s.chat {
                    let text = RichText::new(&c.text).monospace();
                    ui.label(if c.red {
                        text.color(zlog_ui::palette::error(ui))
                    } else {
                        text
                    });
                }
            });
        let connected = s.connected();
        let mut send = false;
        let edit = zlog_ui::field_row(
            ui,
            |ui| {
                egui::ComboBox::from_id_salt("zlink-prompt")
                    .selected_text(t.get(lang, s.settings.prompt.key()))
                    .show_ui(ui, |ui| {
                        for p in Prompt::ALL {
                            ui.selectable_value(&mut s.settings.prompt, p, t.get(lang, p.key()));
                        }
                    });
            },
            |ui| {
                send = ui
                    .add_enabled(connected, egui::Button::new(t.get(lang, "send")))
                    .clicked();
            },
            |ui| {
                ui.add_enabled(
                    connected,
                    egui::TextEdit::singleline(&mut self.input).desired_width(f32::INFINITY),
                )
            },
        );
        {
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || send) && !self.input.is_empty() {
                let prompt = s.settings.prompt.text(
                    view.band,
                    &s.settings.pc_name,
                    &s.entry_operator,
                    &view.station.callsign,
                );
                let now = chrono::Local::now().format("%H:%M").to_string();
                s.send_chat(&now, &prompt, &self.input);
                self.input.clear();
                edit.request_focus();
            }
        }

        ui.separator();
        egui::CollapsingHeader::new(t.get(lang, "console"))
            .id_salt("zlink-console")
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .max_height(ui.available_height().max(80.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for l in &s.console {
                            ui.label(RichText::new(l).monospace().small());
                        }
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// N3: a UTC log sends UTC; a local-time log sends local time.
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

    /// A transport that never answers.
    struct Silent;

    impl crate::session::Transport for Silent {
        fn send(&mut self, _bytes: &[u8]) {}
        fn poll(&mut self) -> Vec<crate::session::NetEvent> {
            Vec::new()
        }
        fn close(&mut self) {}
    }

    /// A transport that keeps what was sent.
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

    /// C6: opening B while A is being edited unlocks A and locks B.
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

    fn toolbar_action(p: &ZLinkPanel) -> (String, bool, bool) {
        let b = p.commands("en").remove(1);
        assert_eq!(b.id, "connect");
        (b.label, b.enabled, b.selected)
    }

    /// The toolbar button follows the session state; no connection is opened here
    /// (only the disconnect path and a refused connect are pressed).
    #[test]
    fn toolbar_connect_follows_the_session() {
        let session = Arc::new(Mutex::new(Session::default()));
        let mut p = ZLinkPanel::new(session.clone());
        session.lock().unwrap().settings.host.clear();
        assert_eq!(toolbar_action(&p), ("Connect".into(), false, false));
        // Pressed anyway: refused as by the window's button, with its notice.
        p.run_command("connect");
        assert_eq!(session.lock().unwrap().state, State::Disconnected);
        assert!(p.notice.is_some());
        session.lock().unwrap().settings.host = "localhost".into();
        assert_eq!(toolbar_action(&p), ("Connect".into(), true, false));
        session.lock().unwrap().connect(Box::new(Silent));
        assert_eq!(toolbar_action(&p), ("Connecting...".into(), true, true));
        session.lock().unwrap().state = State::Connected;
        assert_eq!(toolbar_action(&p), ("Disconnect".into(), true, true));
        p.run_command("connect");
        assert_eq!(session.lock().unwrap().state, State::Disconnected);
    }
}
