use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use crate::crypto::{Fingerprint, TokenHash};
use crate::tls::Identity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PeerId {
    Cert(Fingerprint),
    Ip(IpAddr),
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeerId::Cert(fp) => write!(f, "cert:{fp}"),
            PeerId::Ip(ip) => write!(f, "ip:{ip}"),
        }
    }
}

impl FromStr for PeerId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s.split_once(':') {
            Some(("cert", fp)) => fp.parse().map(PeerId::Cert),
            Some(("ip", ip)) => ip.parse().map(PeerId::Ip).map_err(|_| "bad address".into()),
            _ => Err(format!("not a peer: {s}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub id: PeerId,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct Peers(Arc<Mutex<BTreeMap<PeerId, String>>>);

fn clean(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control() && *c != ';')
        .collect::<String>()
        .trim()
        .to_string()
}

impl Peers {
    pub fn list(&self) -> Vec<Peer> {
        self.lock()
            .iter()
            .map(|(id, name)| Peer {
                id: *id,
                name: name.clone(),
            })
            .collect()
    }

    pub fn name(&self, id: &PeerId) -> Option<String> {
        self.lock().get(id).cloned()
    }

    pub fn knows(&self, id: &PeerId) -> bool {
        self.lock().contains_key(id)
    }

    pub fn add(&self, id: PeerId, name: &str) {
        self.lock().insert(id, clean(name));
    }

    pub fn remove(&self, id: &PeerId) -> bool {
        self.lock().remove(id).is_some()
    }

    pub fn to_text(&self) -> String {
        self.lock()
            .iter()
            .map(|(id, name)| format!("{id} {name}"))
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn from_text(text: &str) -> Peers {
        let peers = Peers::default();
        for item in text.split(';') {
            let (id, name) = item.trim().split_once(' ').unwrap_or((item.trim(), ""));
            if let Ok(id) = id.parse() {
                peers.add(id, name);
            }
        }
        peers
    }

    pub fn replace(&self, other: &Peers) {
        let new = other.lock().clone();
        *self.lock() = new;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<PeerId, String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub trait Store {
    fn load(&self, key: &str) -> Option<String>;

    fn save(&mut self, key: &str, value: String);
}

impl Store for BTreeMap<String, String> {
    fn load(&self, key: &str) -> Option<String> {
        self.get(key).filter(|v| !v.is_empty()).cloned()
    }

    fn save(&mut self, key: &str, value: String) {
        self.insert(key.into(), value);
    }
}

pub fn load_identity(store: &mut dyn Store, key: &str) -> Result<Identity, String> {
    if let Some(id) = store.load(key).and_then(|t| Identity::from_text(&t)) {
        return Ok(id);
    }
    let id = Identity::generate()?;
    store.save(key, id.to_text());
    Ok(id)
}

pub fn load_token_hash(store: &dyn Store, key: &str) -> Option<TokenHash> {
    store.load(key).and_then(|t| t.parse().ok())
}

pub fn load_peers(store: &dyn Store, key: &str) -> Peers {
    Peers::from_text(&store.load(key).unwrap_or_default())
}
