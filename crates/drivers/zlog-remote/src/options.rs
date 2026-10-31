use std::sync::Arc;

use egui::Ui;
use zlog_tls::discovery::Browser;
use zlog_tls::Token;
use zlog_ui::{Section, SettingsPage};

use crate::{texts, Remote, APP, SECTION, TOKEN};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Use,
    Share,
}

pub struct RemoteOptions {
    remote: Arc<Remote>,
    side: Side,
    shown: Option<Token>,
    browser: Option<Browser>,
}

impl Default for RemoteOptions {
    fn default() -> Self {
        RemoteOptions::new(crate::shared())
    }
}

impl RemoteOptions {
    pub fn new(remote: Arc<Remote>) -> Self {
        RemoteOptions::side(remote, Side::Use)
    }

    pub fn side(remote: Arc<Remote>, side: Side) -> Self {
        RemoteOptions {
            remote,
            side,
            shown: None,
            browser: None,
        }
    }

    pub fn server_state(&self, language: &str) -> String {
        let t = texts();
        let s = self.remote.lock();
        if let Some(e) = &s.server_error {
            return t.format(language, "server.failed", &[e]);
        }
        let Some((_, srv)) = &s.server else {
            return t.get(language, "server.off").into();
        };
        match srv.peers().first() {
            Some(p) => t.format(language, "server.client", &[p]),
            None => t.format(language, "server.listening", &[&srv.addr()]),
        }
    }

    pub fn new_token(&mut self, section: &mut Section) -> Token {
        let token = Token::generate();
        section.insert(TOKEN.into(), token.hash().to_string());
        self.shown = Some(token.clone());
        token
    }

    fn token_ui(&mut self, ui: &mut Ui, section: &mut Section, language: &str) {
        let t = |k| texts().get(language, k);
        let set = section.get(TOKEN).is_some_and(|v| !v.is_empty());
        ui.label(t(if set { "token.set" } else { "token.none" }));
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(t("token.new"))
                .on_hover_text(t("token.new.tip"))
                .clicked()
            {
                self.new_token(section);
            }
            if set
                && ui
                    .button(t("token.clear"))
                    .on_hover_text(t("token.clear.tip"))
                    .clicked()
            {
                section.insert(TOKEN.into(), String::new());
                self.shown = None;
            }
        });
        if let Some(token) = &self.shown {
            ui.horizontal_wrapped(|ui| {
                ui.monospace(token.as_str());
                if ui
                    .button(t("token.copy"))
                    .on_hover_text(t("token.copy.tip"))
                    .clicked()
                {
                    ui.ctx().copy_text(token.to_string());
                }
            });
            ui.weak(t("token.once"));
        }
    }

    fn trust_ui(&mut self, ui: &mut Ui, language: &str) {
        let t = |k| texts().get(language, k);
        let trust = self.remote.lock().trust.clone();
        let Some((addr, fp)) = trust else {
            return;
        };
        ui.label(texts().format(language, "trust.ask", &[&addr]));
        ui.monospace(fp.to_string());
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(t("trust.yes"))
                .on_hover_text(t("trust.yes.tip"))
                .clicked()
            {
                self.remote.remember_server(&addr, fp);
                self.remote.lock().trust = None;
            }
            if ui.button(t("trust.no")).clicked() {
                self.remote.lock().trust = None;
            }
        });
    }

    fn search_ui(&mut self, ui: &mut Ui, section: &mut Section, language: &str) {
        zlog_ui::pairing::search_button(ui, &mut self.browser, APP, language);
        let Some(b) = self.browser.as_mut() else {
            return;
        };
        let rows: Vec<zlog_ui::pairing::FoundRow> = zlog_ui::pairing::sorted_found(b.poll(), &[])
            .into_iter()
            .map(|(found, at)| zlog_ui::pairing::FoundRow {
                found,
                at,
                warning: None,
            })
            .collect();
        if let Some(i) = zlog_ui::pairing::found_list(ui, &rows, language) {
            section.insert("connect_addr".into(), rows[i].at.clone());
            self.connect(section);
        }
    }

    fn connect(&self, section: &Section) {
        let s = crate::Settings::load(section);
        crate::panel::start_connect(
            &self.remote,
            &s.connect_addr,
            &s.name(),
            &s.client_token,
            false,
        );
    }

    fn client_state(&self, language: &str) -> String {
        let t = texts();
        let s = self.remote.lock();
        match (&s.client, s.connecting, &s.error) {
            (_, true, _) => t.get(language, "status.connecting").into(),
            (Some(c), _, _) if c.connected() => t.get(language, "use.connected").into(),
            (_, _, Some(e)) => zlog_ui::connection::explain_net(language, e),
            _ => t.get(language, "not_connected").into(),
        }
    }

    fn use_ui(&mut self, ui: &mut Ui, section: &mut Section, language: &str) {
        let t = |k| texts().get(language, k);
        ui.weak(t("use.note"));
        self.search_ui(ui, section, language);
        ui.separator();
        let servers = self.remote.lock().servers.clone();
        zlog_ui::dialog::settings_form_with(
            ui,
            &crate::client_fields(language),
            section,
            language,
            |ui| {
                ui.label(t("servers"));
                zlog_ui::pairing::peer_list(ui, &servers, language);
            },
        );
        let open = self.remote.client().is_some();
        ui.label(self.client_state(language));
        ui.horizontal_wrapped(|ui| {
            if !open {
                let ready = section
                    .get("connect_addr")
                    .is_some_and(|a| !a.trim().is_empty());
                if ui
                    .add_enabled(ready, egui::Button::new(t("use.connect")))
                    .on_hover_text(t("use.connect.tip"))
                    .clicked()
                {
                    self.connect(section);
                }
            } else {
                for n in [1u8, 2] {
                    let key = if n == 1 { "use.rig1" } else { "use.rig2" };
                    if ui.button(t(key)).on_hover_text(t("use.rig.tip")).clicked() {
                        self.remote.lock().use_rig = Some(n);
                    }
                }
                if ui.button(t("use.disconnect")).clicked() {
                    crate::client::disconnect(&self.remote);
                }
            }
        });
        self.trust_ui(ui, language);
    }

    fn share_ui(&mut self, ui: &mut Ui, section: &mut Section, language: &str) {
        let t = |k| texts().get(language, k);
        ui.weak(t("note"));
        let clients = self.remote.lock().clients.clone();
        zlog_ui::dialog::settings_form(ui, &crate::server_fields(language), section, language);
        egui::CollapsingHeader::new(t("token.title"))
            .id_salt("remote_token")
            .show(ui, |ui| self.token_ui(ui, section, language));
        ui.label(self.server_state(language));
        let tx = zlog_ui::tx::hub().transmitting();
        if let Some(r) = tx {
            ui.label(texts().format(language, "server.tx", &[&r]));
        }
        let asking = self.remote.requests().len();
        if asking > 0 {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                texts().format(language, "share.asking", &[&asking]),
            );
        }
        ui.label(t("clients"));
        zlog_ui::pairing::peer_list(ui, &clients, language);
    }
}

impl SettingsPage for RemoteOptions {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        let key = match self.side {
            Side::Use => "options.use",
            Side::Share => "options.share",
        };
        texts().get(language, key).into()
    }

    fn section_kind(&self) -> zlog_ui::SectionKind {
        zlog_ui::SectionKind::Secret
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut Section, language: &str) {
        match self.side {
            Side::Use => self.use_ui(ui, section, language),
            Side::Share => self.share_ui(ui, section, language),
        }
    }
}

pub fn dialogs(remote: &Remote, ctx: &egui::Context, language: &str) -> bool {
    let mut again = false;
    let repair = remote.lock().repair.clone();
    if let Some(addr) = repair {
        if let Some(yes) = zlog_ui::pairing::repair_window(ctx, &addr, language) {
            remote.lock().repair = None;
            again = yes;
        }
    }
    let code = remote.lock().code.clone();
    if let Some(code) = code {
        zlog_ui::pairing::code_window(ctx, &code, language);
    }
    let Some(r) = remote.requests().into_iter().next() else {
        return again;
    };
    let answer = zlog_ui::pairing::approve_window(
        ctx,
        &r.name,
        &r.addr.to_string(),
        r.code.as_deref(),
        false,
        language,
    );
    if let Some(a) = answer {
        remote.decide(r.id, a);
    }
    again
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_tells_whether_it_listens() {
        let o = RemoteOptions::new(Arc::default());
        assert_eq!(o.server_state("en"), "Not shared");
        o.remote.lock().server_error = Some("busy".into());
        assert_eq!(o.server_state("ja"), "待ち受けできません: busy");
        assert_eq!(o.id(), "panel.remote");
    }

    #[test]
    fn two_role_pages_share_one_section() {
        let u = RemoteOptions::side(Arc::default(), Side::Use);
        let s = RemoteOptions::side(Arc::default(), Side::Share);
        assert_eq!(u.id(), s.id());
        assert_eq!(u.title("en"), "Remote operation");
        assert_eq!(s.title("en"), "Remote sharing");
        for lang in ["en", "ja"] {
            let adv: Vec<_> = crate::server_fields(lang)
                .into_iter()
                .chain(crate::client_fields(lang))
                .filter(|f| f.advanced)
                .map(|f| f.key)
                .collect();
            assert!(adv.contains(&"listen_addr") && adv.contains(&"client_token"));
            assert!(!texts().get(lang, "client_token").contains("VPN"));
        }
    }

    #[test]
    fn a_new_token_is_shown_once_and_stored_as_a_hash() {
        let mut o = RemoteOptions::new(Arc::default());
        let mut s = Section::new();
        let token = o.new_token(&mut s);
        let stored = &s[TOKEN];
        assert!(!stored.contains(token.as_str()));
        let settings = crate::Settings::load(&s);
        assert!(settings.token.unwrap().verify(token.as_str()));
    }
}
