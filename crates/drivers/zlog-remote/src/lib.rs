pub mod client;
pub mod link;
pub mod modem;
pub mod options;
pub mod panel;
pub mod server;

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use haml::remote::{Client, Listing};
use zlog_tls::{Fingerprint, Identity, PeerId, Peers, Request, Store, TokenHash};
use zlog_ui::device::{SettingField, SettingKind};
use zlog_ui::Section;

pub use client::{RemoteKeyerDrv, RemoteRigs, RemoteSo2rDrv};
pub use options::RemoteOptions;
pub use panel::RemotePanel;

zlog_core::texts!();

pub const SECTION: &str = "panel.remote";

pub const DEFAULT_PORT: u16 = 6500;

pub const APP: &str = "zlog-remote";

pub const IDENTITY: &str = "identity";
pub const CLIENTS: &str = "clients";
pub const SERVERS: &str = "servers";
pub const TOKEN: &str = "token_hash";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub listen: bool,
    pub listen_addr: String,
    pub listen_port: u16,
    pub announce: bool,
    pub expose_rigs: [bool; 2],
    pub expose_tx: bool,
    pub expose_modems: bool,
    pub station_name: String,
    pub token: Option<TokenHash>,
    pub connect_addr: String,
    pub client_token: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::load(&Section::new())
    }
}

fn get<T: std::str::FromStr>(s: &Section, key: &str, default: T) -> T {
    s.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn text(s: &Section, key: &str, default: &str) -> String {
    s.get(key).cloned().unwrap_or_else(|| default.into())
}

impl Settings {
    pub fn load(s: &Section) -> Self {
        Settings {
            listen: get(s, "listen", false),
            listen_addr: text(s, "listen_addr", "0.0.0.0"),
            listen_port: get(s, "listen_port", DEFAULT_PORT),
            announce: get(s, "announce", false),
            expose_rigs: [get(s, "expose_rig1", true), get(s, "expose_rig2", true)],
            expose_tx: get(s, "expose_tx", true),
            expose_modems: get(s, "expose_modems", false),
            station_name: text(s, "station_name", ""),
            token: zlog_tls::load_token_hash(s, TOKEN),
            connect_addr: text(s, "connect_addr", ""),
            client_token: text(s, "client_token", ""),
        }
    }

    pub fn listener(&self) -> String {
        format!("{}:{}", self.listen_addr, self.listen_port)
    }

    pub fn modems(&self) -> bool {
        self.expose_tx && self.expose_modems
    }

    pub fn name(&self) -> String {
        match self.station_name.trim() {
            "" => "zLog".into(),
            n => n.into(),
        }
    }
}

fn field_makers(
    language: &str,
) -> (
    impl Fn(&'static str, &str) -> SettingField + '_,
    impl Fn(&'static str, &str) -> SettingField + '_,
) {
    let t = move |k: &str| texts().get(language, k).to_string();
    let flag = move |key: &'static str, default: &str| {
        SettingField::new(key, t(key), SettingKind::Bool).default_value(default)
    };
    let line = move |key: &'static str, default: &str| {
        SettingField::new(key, texts().get(language, key), SettingKind::Text).default_value(default)
    };
    (flag, line)
}

pub fn client_fields(language: &str) -> Vec<SettingField> {
    let (_, line) = field_makers(language);
    vec![
        line("connect_addr", ""),
        line("station_name", "").advanced(true),
        line("client_token", "").advanced(true),
    ]
}

pub fn server_fields(language: &str) -> Vec<SettingField> {
    let (flag, line) = field_makers(language);
    vec![
        flag("listen", "false"),
        flag("expose_rig1", "true"),
        flag("expose_rig2", "true"),
        flag("expose_tx", "true"),
        flag("expose_modems", "false"),
        line("station_name", ""),
        flag("announce", "false"),
        line("listen_addr", "0.0.0.0").advanced(true),
        SettingField::new(
            "listen_port",
            texts().get(language, "listen_port"),
            SettingKind::Int { min: 1, max: 65535 },
        )
        .default_value(DEFAULT_PORT.to_string())
        .advanced(true),
    ]
}

pub fn fields(language: &str) -> Vec<SettingField> {
    let mut all = server_fields(language);
    for f in client_fields(language) {
        if !all.iter().any(|a| a.key == f.key) {
            all.push(f);
        }
    }
    all
}

#[derive(Default)]
pub struct State {
    pub client: Option<Client>,
    pub listing: Option<Listing>,
    pub connecting: bool,
    pub error: Option<String>,
    pub code: Option<String>,
    pub trust: Option<(String, Fingerprint)>,
    pub repair: Option<String>,
    pub server: Option<(Settings, server::Listening)>,
    pub server_error: Option<String>,
    pub identity: Option<Identity>,
    pub clients: Peers,
    pub servers: Peers,
    pub loaded: bool,
    pub audit: Vec<String>,
    pub use_rig: Option<u8>,
}

#[derive(Default)]
pub struct Remote(Mutex<State>);

impl Remote {
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn client(&self) -> Option<Client> {
        self.lock().client.clone().filter(Client::connected)
    }

    pub fn identity(&self) -> Result<Identity, String> {
        let mut s = self.lock();
        if let Some(id) = &s.identity {
            return Ok(id.clone());
        }
        let id = Identity::generate()?;
        s.identity = Some(id.clone());
        Ok(id)
    }

    pub fn requests(&self) -> Vec<Request> {
        let s = self.lock();
        s.server
            .as_ref()
            .map(|(_, l)| l.tls.requests())
            .unwrap_or_default()
    }

    pub fn decide(&self, id: u64, approve: bool) {
        if let Some((_, l)) = &self.lock().server {
            l.tls.decide(id, approve);
        }
    }

    pub fn pin(&self, addr: &str) -> Option<Fingerprint> {
        let s = self.lock();
        s.servers.list().into_iter().find_map(|p| match p.id {
            PeerId::Cert(fp) if p.name == addr => Some(fp),
            _ => None,
        })
    }

    pub fn remember_server(&self, addr: &str, fp: Fingerprint) {
        let s = self.lock();
        for p in s.servers.list() {
            if p.name == addr {
                s.servers.remove(&p.id);
            }
        }
        s.servers.add(PeerId::Cert(fp), addr);
    }

    pub fn load(&self, section: &Section) {
        let mut s = self.lock();
        if s.loaded {
            return;
        }
        s.loaded = true;
        s.identity = section.load(IDENTITY).and_then(|t| Identity::from_text(&t));
        s.clients.replace(&zlog_tls::load_peers(section, CLIENTS));
        s.servers.replace(&zlog_tls::load_peers(section, SERVERS));
    }

    pub fn save(&self, section: &mut Section) {
        let s = self.lock();
        if let Some(id) = &s.identity {
            section.save(IDENTITY, id.to_text());
        }
        section.save(CLIENTS, s.clients.to_text());
        section.save(SERVERS, s.servers.to_text());
    }
}

pub fn shared() -> Arc<Remote> {
    static REMOTE: OnceLock<Arc<Remote>> = OnceLock::new();
    REMOTE.get_or_init(Arc::default).clone()
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(620, |_| RemotePanel::default());
    b.page(530, |_| RemoteOptions::side(shared(), options::Side::Use));
    b.page(531, |_| RemoteOptions::side(shared(), options::Side::Share));
    b.driver::<dyn zlog_ui::rig::RigSource>(50, || Box::new(RemoteRigs::new(shared())));
    b.driver::<dyn zlog_keyer::KeyerDrv>(50, || Box::new(RemoteKeyerDrv::new(shared())));
    b.driver::<dyn zlog_so2r::So2rDrv>(50, || Box::new(RemoteSo2rDrv::new(shared())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listener_is_off_until_asked_and_every_field_has_a_text() {
        let s = Settings::default();
        assert!(!s.listen && !s.announce && s.expose_tx && s.expose_rigs == [true, true]);
        assert_eq!(s.listener(), "0.0.0.0:6500");
        assert_eq!(s.token, None);
        for lang in ["en", "ja"] {
            let f = fields(lang);
            assert_eq!(f.len(), 11);
            assert_eq!(client_fields(lang).len() + server_fields(lang).len(), 12);
            assert!(f.iter().all(|f| !f.label.is_empty()));
            let mut section = Section::new();
            for f in &f {
                section.insert(f.key.into(), f.default.clone());
            }
            assert_eq!(Settings::load(&section), s);
        }
    }

    #[test]
    fn identity_and_remembered_peers_survive_a_save_and_load() {
        let r = Remote::default();
        let id = r.identity().unwrap();
        r.lock()
            .clients
            .add(PeerId::Cert(id.fingerprint()), "JA1ZLO");
        r.remember_server("192.168.0.2:6500", id.fingerprint());
        let mut section = Section::new();
        r.save(&mut section);
        let again = Remote::default();
        again.load(&section);
        assert_eq!(again.identity().unwrap(), id);
        assert_eq!(again.lock().clients.list(), r.lock().clients.list());
        assert_eq!(again.pin("192.168.0.2:6500"), Some(id.fingerprint()));
        assert_eq!(again.pin("192.168.0.3:6500"), None);
        again.load(&Section::new());
        assert_eq!(
            again.identity().unwrap(),
            id,
            "memory wins after the first load"
        );
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
