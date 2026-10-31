use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use haml::device::{Device, Driver};
use haml::remote::{Lent, Limits, Server, Station};
use haml::tx::Delays;
use haml::{Arbiter, Command, RigCaps, RigEvent};
use zlog_tls::discovery::{Announcer, Service};
use zlog_tls::{Identity, Peers, ServerConfig};
use zlog_ui::rig::Exposed;
use zlog_ui::tx::TxHub;

use crate::link::Accept;
use crate::{Settings, APP};

pub struct Listening {
    pub haml: Server,
    pub tls: Arc<zlog_tls::Server>,
    pub audit: Receiver<zlog_tls::Audit>,
    pub tx_audit: Receiver<haml::remote::Audit>,
    _announcer: Option<Announcer>,
    settings: Settings,
    announced: (String, bool),
}

pub fn shares(settings: &Settings) -> String {
    let mut v: Vec<&str> = Vec::new();
    for (n, on) in ["rig1", "rig2"].iter().zip(settings.expose_rigs) {
        if on {
            v.push(n);
        }
    }
    if settings.expose_tx {
        v.push("keyer");
    }
    if settings.modems() {
        v.push("modems");
    }
    v.join(",")
}

fn service(settings: &Settings, port: u16, call: &str, busy: bool) -> Service {
    Service {
        kind: APP.into(),
        name: settings.name(),
        port,
        txt: vec![
            ("name".into(), settings.name()),
            ("call".into(), call.into()),
            ("tls".into(), "1".into()),
            (
                "token".into(),
                u8::from(settings.token.is_some()).to_string(),
            ),
            ("shares".into(), shares(settings)),
            ("busy".into(), u8::from(busy).to_string()),
        ],
    }
}

impl Listening {
    pub fn announce(&mut self, call: &str) {
        let busy = !self.peers().is_empty();
        let now = (call.to_string(), busy);
        if !self.settings.announce || now == self.announced {
            return;
        }
        self._announcer = None;
        self._announcer =
            Announcer::start(&service(&self.settings, self.addr().port(), call, busy)).ok();
        self.announced = now;
    }

    pub fn addr(&self) -> SocketAddr {
        self.tls.addr()
    }

    pub fn peers(&self) -> Vec<String> {
        self.haml.peers()
    }
}

struct RigBridge {
    exposed: Exposed,
    events: Receiver<RigEvent>,
}

impl Driver for RigBridge {
    type Command = Command;
    type Event = RigEvent;

    fn handle(&mut self, command: Command, _now: Instant, _out: &mut Vec<RigEvent>) {
        self.exposed.push(command);
    }

    fn tick(&mut self, _now: Instant, out: &mut Vec<RigEvent>) {
        out.extend(self.events.try_iter());
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(10)
    }
}

pub fn rig(exposed: Exposed) -> Device<Command, RigEvent> {
    let events = exposed.subscribe();
    Device::spawn("zlog-remote-rig", RigBridge { exposed, events })
}

fn describe(n: usize, exposed: &Exposed) -> (String, RigCaps) {
    match exposed.rig() {
        Some((model, caps)) => (format!("RIG{n} {model}"), caps),
        None => (format!("RIG{n}"), RigCaps::NONE),
    }
}

fn lending(slots: &[(usize, Exposed)], tx: Option<&TxHub>) -> Lent {
    let mut devices = Vec::new();
    if let Some(tx) = tx {
        if tx.has_keyer() {
            devices.push("keyer".into());
        }
        if tx.has_so2r() {
            devices.push("SO2R".into());
        }
    }
    Lent {
        rigs: slots.iter().map(|(n, e)| describe(*n, e)).collect(),
        tx: devices,
    }
}

pub fn start(
    settings: &Settings,
    identity: Identity,
    clients: Peers,
    rigs: [Exposed; 2],
    tx: &TxHub,
) -> Result<Listening, String> {
    let bind = settings
        .listener()
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let slots: Vec<(usize, Exposed)> = (1..)
        .zip(rigs)
        .zip(settings.expose_rigs)
        .filter(|(_, on)| *on)
        .map(|(slot, _)| slot)
        .collect();
    crate::modem::set_lent(slots.iter().map(|(n, _)| *n as u8).collect());
    let hub = settings.expose_tx.then(|| tx.clone());
    let now = lending(&slots, hub.as_ref());
    let lent = slots
        .iter()
        .zip(now.rigs)
        .map(|((_, e), (name, caps))| (name, caps, rig(e.clone())))
        .collect();
    let device = match &hub {
        Some(tx) => tx.device(),
        None => Arbiter::new(2, Delays::default()).spawn(),
    };
    let station = Station {
        rigs: lent,
        tx: device,
        tx_devices: now.tx,
        ports: None,
        lent: Some(Box::new(move || lending(&slots, hub.as_ref()))),
        channels: match settings.modems() {
            true => crate::modem::offered(),
            false => Vec::new(),
        },
    };
    let config = ServerConfig {
        tls: Some(identity),
        token: settings.token,
        ..ServerConfig::new(APP, bind)
    };
    let tls = Arc::new(zlog_tls::Server::bind(config, clients).map_err(|e| e.to_string())?);
    let audit = tls.audit();
    let haml = Server::start(Accept(tls.clone()), station, Limits::default())
        .map_err(|e| e.to_string())?;
    let tx_audit = haml.audit();
    let announcer = settings
        .announce
        .then(|| Announcer::start(&service(settings, tls.addr().port(), "", false)));
    let _announcer = match announcer {
        Some(Err(e)) => return Err(e),
        Some(Ok(a)) => Some(a),
        None => None,
    };
    Ok(Listening {
        haml,
        tls,
        audit,
        tx_audit,
        _announcer,
        settings: settings.clone(),
        announced: (String::new(), false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modems_are_shared_only_with_the_tx() {
        let mut s = Settings {
            expose_modems: true,
            ..Settings::default()
        };
        assert_eq!(shares(&s), "rig1,rig2,keyer,modems");
        s.expose_tx = false;
        assert_eq!(shares(&s), "rig1,rig2");
        assert!(!Settings::default().modems());
    }
}
