use std::sync::Arc;
use std::time::Duration;

use zlog_ui::tx::{TxCommand, TxHub};
use zlog_ui::{Command, EntryAction, MenuGroup, MenuSection, Panel, Section, StatusItem, View};

use crate::{client, server, texts, Remote, Settings};

pub const CONNECT: &str = "connect";
pub const DISCONNECT: &str = "disconnect";
pub const STOP: &str = "stop";

pub struct RemotePanel {
    remote: Arc<Remote>,
    tx: TxHub,
    rigs: [zlog_ui::rig::Exposed; 2],
    settings: Settings,
    actions: Vec<EntryAction>,
    language: String,
    was_open: bool,
}

pub const AUDIT_KEEP: usize = 50;

pub const USE_RIG1: &str = "use.rig1";
pub const USE_RIG2: &str = "use.rig2";

pub fn start_connect(remote: &Arc<Remote>, addr: &str, name: &str, token: &str, again: bool) {
    {
        let mut s = remote.lock();
        if s.connecting {
            return;
        }
        s.connecting = true;
        s.error = None;
        s.trust = None;
    }
    let (remote, addr, name, token) = (
        remote.clone(),
        addr.to_string(),
        name.to_string(),
        token.to_string(),
    );
    std::thread::spawn(move || {
        let got = match again {
            true => client::pair_again(&remote, &addr, &name),
            false => client::connect(&remote, &addr, &name, &token),
        };
        let mut s = remote.lock();
        s.connecting = false;
        s.error = got.err();
        drop(s);
        zlog_core::wake::wake();
    });
}

pub fn remote_port(remote: &Remote, n: u8) -> Option<String> {
    remote.client()?;
    let s = remote.lock();
    let rigs = &s.listing.as_ref()?.rigs;
    let i = usize::from(n.max(1) - 1).min(rigs.len().checked_sub(1)?);
    Some(format!("{}{} {}", client::PREFIX, i + 1, rigs[i].0))
}

impl Default for RemotePanel {
    fn default() -> Self {
        RemotePanel::new(
            crate::shared(),
            zlog_ui::tx::hub(),
            [zlog_ui::rig::exposed(1), zlog_ui::rig::exposed(2)],
        )
    }
}

impl RemotePanel {
    pub fn new(remote: Arc<Remote>, tx: TxHub, rigs: [zlog_ui::rig::Exposed; 2]) -> Self {
        RemotePanel {
            remote,
            tx,
            rigs,
            settings: Settings::default(),
            actions: Vec::new(),
            language: "en".into(),
            was_open: false,
        }
    }

    pub fn listen(&mut self) {
        let want = self.settings.listen.then(|| self.settings.clone());
        let mut s = self.remote.lock();
        let same = match (&s.server, &want) {
            (Some((old, _)), Some(new)) => old == new,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        s.server = None;
        s.server_error = None;
        let Some(settings) = want else {
            return;
        };
        let clients = s.clients.clone();
        drop(s);
        let started = self
            .remote
            .identity()
            .and_then(|id| server::start(&settings, id, clients, self.rigs.clone(), &self.tx));
        let mut s = self.remote.lock();
        match started {
            Ok(srv) => s.server = Some((settings, srv)),
            Err(e) => s.server_error = Some(e),
        }
    }

    pub fn connect(&mut self) {
        self.connect_with(false);
    }

    pub fn pair_again(&mut self) {
        self.connect_with(true);
    }

    fn connect_with(&mut self, again: bool) {
        start_connect(
            &self.remote,
            &self.settings.connect_addr,
            &self.settings.name(),
            &self.settings.client_token,
            again,
        );
    }

    fn use_rig(&mut self, n: u8, language: &str) {
        let t = texts();
        match remote_port(&self.remote, n) {
            Some(port) => self.actions.push(EntryAction::UseRig(n, port)),
            None => self.actions.push(EntryAction::Say(
                StatusItem::new(t.get(language, "use.none")).warning(true),
            )),
        }
    }

    pub fn disconnect(&mut self) {
        client::disconnect(&self.remote);
    }

    pub fn stop(&mut self) {
        if let Some(c) = self.remote.client() {
            let _ = c.send(&TxCommand::Stop);
        }
        self.tx.send(TxCommand::Stop);
    }
}

impl RemotePanel {
    pub fn drain_audit(&mut self, lang: &str) {
        let t = texts();
        let mut lines = Vec::new();
        if let Some((_, l)) = &self.remote.lock().server {
            for a in l.audit.try_iter() {
                let who = match a.name.is_empty() {
                    true => a.addr.to_string(),
                    false => format!("{} ({})", a.name, a.addr),
                };
                let kind = format!("{:?}", a.kind);
                let key = format!("audit.{}", kind.split([' ', '{']).next().unwrap_or(""));
                lines.push((t.format(lang, &key, &[&who]), warn_tls(&a.kind)));
            }
            for a in l.tx_audit.try_iter() {
                let key = match a.kind {
                    haml::remote::AuditKind::TxStarted { .. } => "audit.TxStarted",
                    haml::remote::AuditKind::EmergencyStop => "audit.EmergencyStop",
                    haml::remote::AuditKind::IdleTimeout => "audit.IdleTimeout",
                    haml::remote::AuditKind::Busy => "audit.Busy",
                    haml::remote::AuditKind::Disconnected => "audit.Disconnected",
                    _ => continue,
                };
                lines.push((t.format(lang, key, &[&a.peer]), false));
            }
        }
        if lines.is_empty() {
            return;
        }
        let mut s = self.remote.lock();
        for (text, warning) in lines {
            s.audit.push(text.clone());
            self.actions
                .push(EntryAction::Say(StatusItem::new(text).warning(warning)));
        }
        let over = s.audit.len().saturating_sub(AUDIT_KEEP);
        s.audit.drain(..over);
    }

    pub fn status_items(&self, lang: &str) -> Vec<StatusItem> {
        let t = texts();
        let mut items = Vec::new();
        let asking = !self.remote.requests().is_empty();
        let s = self.remote.lock();
        match (&s.client, s.connecting) {
            (_, true) => items.push(StatusItem::new(t.get(lang, "status.connecting"))),
            (Some(c), _) if c.connected() => {
                let addr = &self.settings.connect_addr;
                items.push(
                    StatusItem::new(t.format(lang, "status.connected", &[addr])).tooltip(t.format(
                        lang,
                        "status.connected.tip",
                        &[addr],
                    )),
                );
            }
            (Some(_), _) => items.push(
                StatusItem::new(t.get(lang, "status.lost"))
                    .tooltip(t.get(lang, "status.lost.tip"))
                    .warning(true),
            ),
            (None, _) => {
                if let Some(e) = &s.error {
                    items.push(
                        StatusItem::new(t.get(lang, "status.failed"))
                            .tooltip(e.clone())
                            .warning(true),
                    );
                }
            }
        }
        if asking {
            items.push(StatusItem::new(t.get(lang, "status.pairing")).warning(true));
        }
        if let Some((_, srv)) = &s.server {
            if let Some(peer) = srv.peers().first() {
                let text = match self.tx.transmitting() {
                    Some(r) => t.format(lang, "status.client.tx", &[peer, &r]),
                    None => t.format(lang, "status.client", &[peer]),
                };
                items.push(StatusItem::new(text));
            }
        }
        items
    }
}

impl Panel for RemotePanel {
    fn id(&self) -> &str {
        "remote"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn section_kind(&self) -> zlog_ui::SectionKind {
        zlog_ui::SectionKind::Secret
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Device
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn section(&self) -> (MenuSection, i32) {
        (MenuSection::Link, 0)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let t = |k: &str| texts().get(language, k).to_string();
        let s = self.remote.lock();
        let open = s.client.as_ref().is_some_and(|c| c.connected());
        let busy = s.connecting;
        drop(s);
        let state = match open {
            true => self.settings.connect_addr.clone(),
            false => t("not_connected"),
        };
        let item = |id: &str, glyph, weight| {
            let mut c = Command::new(id, t(&format!("menu.{id}")), t(&format!("menu.{id}.tip")))
                .icon(glyph)
                .section(MenuSection::Link, weight)
                .state(state.clone())
                .on_toolbar(false);
            c.menu = true;
            c
        };
        let mut connect = item(CONNECT, zlog_ui::icon::PLUGS, 1);
        if open || busy || self.settings.connect_addr.trim().is_empty() {
            connect = connect.disabled_because(t(if open || busy {
                "status.connecting"
            } else {
                "no_address"
            }));
        }
        let mut disconnect = item(DISCONNECT, zlog_ui::icon::PLUGS_CONNECTED, 2);
        if !open {
            disconnect = disconnect.disabled_because(t("not_connected"));
        }
        let mut uses = Vec::new();
        for (id, weight) in [(USE_RIG1, 4), (USE_RIG2, 5)] {
            let mut c = item(id, zlog_ui::icon::RADIO, weight);
            if !open {
                c = c.disabled_because(t("not_connected"));
            }
            uses.push(c);
        }
        let mut all = vec![connect, disconnect, item(STOP, zlog_ui::icon::STOP, 3)];
        all.extend(uses);
        all
    }

    fn run_command(&mut self, id: &str) {
        match id {
            CONNECT => self.connect(),
            DISCONNECT => self.disconnect(),
            STOP => self.stop(),
            USE_RIG1 => self.use_rig(1, &self.language.clone()),
            USE_RIG2 => self.use_rig(2, &self.language.clone()),
            _ => {}
        }
    }

    fn status(&self, view: &View<'_>) -> Vec<StatusItem> {
        self.status_items(view.language)
    }

    fn capability(&self, want: zlog_ui::Want<'_>) -> Option<zlog_ui::Capability<'_>> {
        use zlog_ui::connection::{Connection, LinkState};
        let zlog_ui::Want::Connections(language) = want else {
            return None;
        };
        if self.settings.connect_addr.trim().is_empty() {
            return Some(zlog_ui::Capability::Connections(Vec::new()));
        }
        let t = texts();
        let s = self.remote.lock();
        let (state, error) = match (&s.client, s.connecting) {
            (_, true) => (LinkState::Connecting, None),
            (Some(c), _) if c.connected() => (LinkState::Connected, None),
            (Some(_), _) => (
                LinkState::Failed,
                Some(t.get(language, "status.lost.tip").into()),
            ),
            (None, _) => match &s.error {
                Some(e) => (
                    LinkState::Failed,
                    Some(zlog_ui::connection::explain_net(language, e)),
                ),
                None => (LinkState::Off, None),
            },
        };
        let c = Connection::new("remote", t.get(language, "title"), state)
            .error(error)
            .page(crate::SECTION)
            .commands(CONNECT, DISCONNECT);
        Some(zlog_ui::Capability::Connections(vec![c]))
    }

    fn tick_after(&self) -> Option<Duration> {
        let s = self.remote.lock();
        (s.client.is_some() || s.server.is_some() || s.connecting)
            .then_some(Duration::from_millis(500))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.language = view.language.to_string();
        self.drain_audit(view.language);
        let wanted = {
            let mut s = self.remote.lock();
            if let Some((_, srv)) = s.server.as_mut() {
                srv.announce(view.callsign);
            }
            s.use_rig.take()
        };
        if let Some(n) = wanted {
            self.use_rig(n, view.language);
        }
        let open = self.remote.client().is_some();
        if open && !self.was_open {
            let t = texts();
            self.actions.push(EntryAction::Say(StatusItem::new(
                t.get(view.language, "use.offer"),
            )));
        }
        self.was_open = open;
        let remote = self.remote.clone();
        if remote.client().is_some_and(|c| c.changed()) {
            std::thread::spawn(move || client::relist(&remote));
        }
    }

    fn load(&mut self, section: &Section) {
        self.remote.load(section);
        self.settings = Settings::load(section);
        self.listen();
    }

    fn save(&self, section: &mut Section) {
        self.remote.save(section);
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn dialogs(&mut self, ctx: &egui::Context, view: &View<'_>) {
        if crate::options::dialogs(&self.remote, ctx, view.language) {
            self.pair_again();
        }
    }
}

fn warn_tls(k: &zlog_tls::AuditKind) -> bool {
    use zlog_tls::AuditKind::*;
    matches!(
        k,
        OutOfRange
            | Full
            | Denied
            | Unknown
            | AuthFailed { .. }
            | LockedOut { .. }
            | Locked
            | LineTooLong
            | RateLimited
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> RemotePanel {
        RemotePanel::new(Arc::default(), TxHub::default(), Default::default())
    }

    #[test]
    fn using_the_remote_rig_needs_a_connection_and_names_the_slot() {
        let mut p = panel();
        let c = p.commands("en");
        let u = c.iter().find(|c| c.id == USE_RIG2).unwrap();
        assert!(!u.enabled && u.menu);
        p.run_command(USE_RIG1);
        assert!(matches!(&p.take_actions()[..], [EntryAction::Say(s)] if s.warning));
        assert_eq!(remote_port(&p.remote, 1), None);
    }

    #[test]
    fn connect_disconnect_and_stop_are_menu_commands_with_icons() {
        let mut p = panel();
        for lang in ["en", "ja"] {
            let c = p.commands(lang);
            let ids: Vec<_> = c.iter().map(|c| c.id.as_str()).collect();
            assert_eq!(ids, [CONNECT, DISCONNECT, STOP, USE_RIG1, USE_RIG2]);
            assert!(c
                .iter()
                .all(|c| c.menu && c.icon.is_some() && !c.toolbar_default));
            assert!(c.iter().all(|c| c.section == MenuSection::Link));
            assert!(!c[0].enabled, "no address yet");
            assert!(!c[1].enabled, "not connected");
            assert!(c[2].enabled, "an emergency stop always works");
        }
        let mut s = Section::new();
        s.insert("connect_addr".into(), "127.0.0.1:1".into());
        p.load(&s);
        assert!(p.commands("en")[0].enabled);
        assert!(p.remote.lock().server.is_none(), "the listener is off");
    }

    #[test]
    fn a_listener_starts_with_tls_and_audits_to_the_status_line() {
        let mut p = panel();
        let mut s = Section::new();
        for (k, v) in [
            ("listen", "true"),
            ("listen_addr", "127.0.0.1"),
            ("listen_port", "0"),
        ] {
            s.insert(k.into(), v.into());
        }
        p.load(&s);
        let addr = p.remote.lock().server.as_ref().unwrap().1.addr();
        assert!(p.remote.lock().identity.is_some(), "an identity was made");
        let mut saved = Section::new();
        p.save(&mut saved);
        assert!(saved.contains_key(crate::IDENTITY));
        let _ = std::net::TcpStream::connect(addr).unwrap();
        let start = std::time::Instant::now();
        while p.remote.lock().audit.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(10));
            p.drain_audit("ja");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(p.remote.lock().audit[0].contains("127.0.0.1"));
        assert!(matches!(p.take_actions()[0], EntryAction::Say(_)));
        assert!(p.status_items("en").is_empty(), "no lock is taken twice");
        s.insert("listen".into(), "false".into());
        p.load(&s);
        assert!(p.remote.lock().server.is_none(), "closed when turned off");
    }
}
