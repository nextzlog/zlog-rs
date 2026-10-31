use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

pub const MAX_TXT: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub kind: String,
    pub name: String,
    pub port: u16,
    pub txt: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub addrs: Vec<IpAddr>,
    pub port: u16,
    pub txt: Vec<(String, String)>,
}

impl Found {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.txt
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn endpoints(&self) -> Vec<SocketAddr> {
        let mut addrs = self.addrs.clone();
        addrs.sort_by_key(|a| (!a.is_ipv4(), *a));
        addrs
            .into_iter()
            .map(|a| SocketAddr::new(a, self.port))
            .collect()
    }
}

pub fn service_type(kind: &str) -> String {
    let kind: String = kind
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(15)
        .collect();
    format!("_{}._tcp.local.", kind.to_ascii_lowercase())
}

pub fn encode_txt(fields: &[(String, String)]) -> Vec<(String, String)> {
    fields
        .iter()
        .filter_map(|(k, v)| {
            let key: String = k
                .chars()
                .filter(|c| c.is_ascii_graphic() && *c != '=')
                .collect::<String>()
                .to_ascii_lowercase();
            if key.is_empty() {
                return None;
            }
            let mut value = String::new();
            for c in v.chars().filter(|c| !c.is_control()) {
                if key.len() + 1 + value.len() + c.len_utf8() > MAX_TXT {
                    break;
                }
                value.push(c);
            }
            Some((key, value))
        })
        .collect()
}

pub fn decode_txt(entries: &[String]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter_map(|e| {
            let (k, v) = e.split_once('=').unwrap_or((e, ""));
            (!k.is_empty()).then(|| (k.to_ascii_lowercase(), v.into()))
        })
        .collect()
}

fn host(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() {
            true => c.to_ascii_lowercase(),
            false => '-',
        })
        .take(40)
        .collect();
    format!("{}.local.", slug.trim_matches('-').max("host"))
}

pub struct Announcer {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Announcer {
    pub fn start(service: &Service) -> Result<Announcer, String> {
        let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
        let txt = encode_txt(&service.txt);
        let props: Vec<(&str, &str)> = txt.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let info = ServiceInfo::new(
            &service_type(&service.kind),
            &service.name,
            &host(&service.name),
            (),
            service.port,
            &props[..],
        )
        .map_err(|e| e.to_string())?
        .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon.register(info).map_err(|e| e.to_string())?;
        Ok(Announcer { daemon, fullname })
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

pub struct Browser {
    daemon: ServiceDaemon,
    events: mdns_sd::Receiver<ServiceEvent>,
    found: BTreeMap<String, Found>,
}

impl Browser {
    pub fn start(kind: &str) -> Result<Browser, String> {
        let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
        let events = daemon
            .browse(&service_type(kind))
            .map_err(|e| e.to_string())?;
        Ok(Browser {
            daemon,
            events,
            found: BTreeMap::new(),
        })
    }

    pub fn poll(&mut self) -> Vec<Found> {
        for e in self.events.try_iter() {
            match e {
                ServiceEvent::ServiceResolved(s) => {
                    let name = s
                        .fullname
                        .strip_suffix(&format!(".{}", s.ty_domain))
                        .unwrap_or(&s.fullname)
                        .replace("\\", "");
                    let entries: Vec<String> = s
                        .txt_properties
                        .iter()
                        .map(|p| format!("{}={}", p.key(), p.val_str()))
                        .collect();
                    let mut addrs: Vec<IpAddr> =
                        s.addresses.iter().map(|a| a.to_ip_addr()).collect();
                    addrs.sort();
                    self.found.insert(
                        s.fullname.clone(),
                        Found {
                            name,
                            addrs,
                            port: s.port,
                            txt: decode_txt(&entries),
                        },
                    );
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    self.found.remove(&fullname);
                }
                _ => {}
            }
        }
        self.found.values().cloned().collect()
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}
