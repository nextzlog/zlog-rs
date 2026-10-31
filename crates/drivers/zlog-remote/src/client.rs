use std::sync::Arc;
use std::time::Duration;

use haml::remote::Client;
use zlog_keyer::{Keyer, KeyerDrv};
use zlog_so2r::{So2r, So2rDrv};
use zlog_tls::{connect_tls, ClientConfig, ConnectError, LineConn, Login};
use zlog_ui::device::{DeviceDriver, PortHub};
use zlog_ui::rig::{Rig, RigSource};
use zlog_ui::Section;

use crate::link::Bridge;
use crate::{texts, Remote, APP};

pub const PREFIX: &str = "remote:";

pub const APPROVAL: Duration = Duration::from_secs(120);

fn login(
    remote: &Remote,
    addr: &str,
    name: &str,
    token: &str,
    again: bool,
) -> Result<LineConn, String> {
    let config = ClientConfig {
        identity: Some(remote.identity()?),
        ..ClientConfig::new(APP, name)
    };
    let pin = remote.pin(addr).filter(|_| !again);
    let hs = connect_tls(addr, &config, pin).map_err(|e| e.to_string())?;
    let fp = hs.fingerprint();
    let how = match (pin.is_some(), token.trim().is_empty() || again) {
        (_, false) => Login::Token(token.into()),
        (true, true) => Login::Known,
        (false, true) => {
            remote.lock().code = Some(hs.code().into());
            zlog_core::wake::wake();
            Login::Pair
        }
    };
    let pairing = how == Login::Pair;
    let got = hs.login(how, APPROVAL);
    remote.lock().code = None;
    match got {
        Ok(conn) => {
            if pairing {
                remote.remember_server(addr, fp);
            }
            Ok(conn)
        }
        Err(ConnectError::NotPaired) => {
            remote.lock().repair = Some(addr.into());
            zlog_core::wake::wake();
            Err(texts().format("en", "repair.error", &[&addr]))
        }
        Err(ConnectError::Unknown(fp)) => {
            remote.lock().trust = Some((addr.into(), fp));
            Err(texts().format("en", "trust.error", &[&fp]))
        }
        Err(e) => Err(e.to_string()),
    }
}

pub fn connect(remote: &Arc<Remote>, addr: &str, name: &str, token: &str) -> Result<(), String> {
    open(remote, addr, name, token, false)
}

pub fn pair_again(remote: &Arc<Remote>, addr: &str, name: &str) -> Result<(), String> {
    open(remote, addr, name, "", true)
}

fn open(
    remote: &Arc<Remote>,
    addr: &str,
    name: &str,
    token: &str,
    again: bool,
) -> Result<(), String> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Err(texts().get("en", "no_address").into());
    }
    remote.lock().repair = None;
    let conn = login(remote, addr, name, token, again)?;
    let client = Client::attach(Box::new(Bridge(conn))).map_err(|e| e.to_string())?;
    let listing = client
        .list(Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let mut s = remote.lock();
    if let Some(old) = s.client.replace(client) {
        old.close();
    }
    s.listing = Some(listing);
    s.error = None;
    drop(s);
    zlog_ui::device::reopen();
    Ok(())
}

pub fn relist(remote: &Remote) -> bool {
    let Some(c) = remote.client() else {
        return false;
    };
    let Ok(listing) = c.list(Duration::from_secs(5)) else {
        return false;
    };
    remote.lock().listing = Some(listing);
    zlog_ui::device::reopen();
    true
}

pub fn disconnect(remote: &Remote) {
    let mut s = remote.lock();
    if let Some(c) = s.client.take() {
        c.close();
    }
    s.listing = None;
}

fn missing(language: &str) -> String {
    texts().get(language, "not_connected").into()
}

pub struct RemoteRigs(Arc<Remote>);

impl RemoteRigs {
    pub fn new(remote: Arc<Remote>) -> Self {
        RemoteRigs(remote)
    }
}

impl RigSource for RemoteRigs {
    fn ports(&self) -> Vec<String> {
        let s = self.0.lock();
        let Some(l) = s.listing.as_ref() else {
            return Vec::new();
        };
        l.rigs
            .iter()
            .enumerate()
            .map(|(i, (name, _))| format!("{PREFIX}{} {name}", i + 1))
            .collect()
    }

    fn owns(&self, port: &str) -> bool {
        port.starts_with(PREFIX)
    }

    fn open(&self, port: &str) -> Option<Result<Box<dyn Rig>, String>> {
        let n: usize = port.strip_prefix(PREFIX)?.split(' ').next()?.parse().ok()?;
        let rig = match self.0.client() {
            Some(c) if n >= 1 => Ok(Box::new(c.rig(n - 1)) as Box<dyn Rig>),
            _ => Err(missing("en")),
        };
        Some(rig)
    }
}

pub struct RemoteKeyerDrv(Arc<Remote>);

impl RemoteKeyerDrv {
    pub fn new(remote: Arc<Remote>) -> Self {
        RemoteKeyerDrv(remote)
    }
}

impl DeviceDriver for RemoteKeyerDrv {
    fn id(&self) -> &str {
        "remote"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "device").into()
    }

    fn status(&self, language: &str) -> Option<String> {
        self.0.client().is_none().then(|| missing(language))
    }

    fn remote(&self) -> bool {
        true
    }
}

impl KeyerDrv for RemoteKeyerDrv {
    fn open(&self, _settings: &Section, _ports: &PortHub) -> Result<Box<dyn Keyer>, String> {
        let c = self.0.client().ok_or_else(|| missing("en"))?;
        Ok(Box::new(c.keyer_on_focus()))
    }
}

pub struct RemoteSo2rDrv(Arc<Remote>);

impl RemoteSo2rDrv {
    pub fn new(remote: Arc<Remote>) -> Self {
        RemoteSo2rDrv(remote)
    }
}

impl DeviceDriver for RemoteSo2rDrv {
    fn id(&self) -> &str {
        "remote"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "device").into()
    }

    fn status(&self, language: &str) -> Option<String> {
        self.0.client().is_none().then(|| missing(language))
    }

    fn remote(&self) -> bool {
        true
    }
}

impl So2rDrv for RemoteSo2rDrv {
    fn open(&self, _settings: &Section, _ports: &PortHub) -> Result<Box<dyn So2r>, String> {
        let c = self.0.client().ok_or_else(|| missing("en"))?;
        Ok(Box::new(c.so2r()))
    }
}
