use std::sync::{Mutex, OnceLock};

use haml::remote::{Channel, RemoteChannel};
use zlog_ui::device::{AudioDrv, AudioIo, DeviceDriver, PortHub};
use zlog_ui::Section;

use crate::{texts, Remote};

fn shelf() -> &'static Mutex<Vec<Channel>> {
    static SHELF: OnceLock<Mutex<Vec<Channel>>> = OnceLock::new();
    SHELF.get_or_init(Mutex::default)
}

pub fn offer(name: &str) -> Channel {
    let mut s = shelf().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = s.iter().find(|c| c.name() == name) {
        return c.clone();
    }
    let c = Channel::new(name).waking(zlog_core::wake::wake);
    s.push(c.clone());
    c
}

pub fn offered() -> Vec<Channel> {
    shelf().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn open(remote: &Remote, name: &str) -> Result<RemoteChannel, String> {
    let client = remote
        .client()
        .ok_or_else(|| texts().get("en", "not_connected").to_string())?;
    let s = remote.lock();
    let i = s
        .listing
        .as_ref()
        .and_then(|l| l.channels.iter().position(|c| c == name))
        .ok_or_else(|| texts().get("en", "modem.none").to_string())?;
    Ok(client.channel(i))
}

fn lent() -> &'static Mutex<Vec<u8>> {
    static LENT: OnceLock<Mutex<Vec<u8>>> = OnceLock::new();
    LENT.get_or_init(Mutex::default)
}

pub fn set_lent(slots: Vec<u8>) {
    *lent().lock().unwrap_or_else(|e| e.into_inner()) = slots;
}

pub fn lent_slot(index: usize) -> Option<u8> {
    lent()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(index)
        .copied()
}

pub fn lent_index(remote: &Remote, rig: u8) -> Option<usize> {
    let s = remote.lock();
    let n = s.listing.as_ref()?.rigs.len();
    (n > 0).then(|| usize::from(rig.max(1) - 1).min(n - 1))
}

pub fn sharing() -> bool {
    let s = crate::shared();
    let s = s.lock();
    s.server.as_ref().is_some_and(|(set, _)| set.modems())
}

pub const REMOTE: &str = "remote";

pub struct RemoteAudioDrv;

impl DeviceDriver for RemoteAudioDrv {
    fn id(&self) -> &str {
        REMOTE
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "modem.device").into()
    }

    fn remote(&self) -> bool {
        true
    }
}

impl AudioDrv for RemoteAudioDrv {
    fn open(&self, _settings: &Section, _ports: &PortHub) -> Result<Box<dyn AudioIo>, String> {
        Err(texts().get("en", "modem.device").into())
    }
}
