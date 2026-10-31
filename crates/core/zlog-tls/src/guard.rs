use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub attempts: u32,
    pub delay: Duration,
    pub max_delay: Duration,
    pub lockout: Duration,
    pub handshake: Duration,
    pub approval: Duration,
    pub max_conns: usize,
    pub max_line: usize,
    pub lines_per_sec: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            attempts: 5,
            delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(4),
            lockout: Duration::from_secs(600),
            handshake: Duration::from_secs(5),
            approval: Duration::from_secs(120),
            max_conns: 16,
            max_line: 4096,
            lines_per_sec: 200,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub net: IpAddr,
    pub prefix: u8,
}

impl Range {
    pub fn private() -> Vec<Range> {
        [
            "127.0.0.0/8",
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "169.254.0.0/16",
            "100.64.0.0/10",
            "::1/128",
            "fc00::/7",
            "fe80::/10",
        ]
        .iter()
        .filter_map(|r| r.parse().ok())
        .collect()
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let bits = |ip: IpAddr| match ip.to_canonical() {
            IpAddr::V4(v) => (u128::from(u32::from(v)), 32u32),
            IpAddr::V6(v) => (u128::from(v), 128u32),
        };
        let (net, width) = bits(self.net);
        let (ip, w) = bits(ip);
        let prefix = u32::from(self.prefix).min(width);
        if w != width {
            return false;
        }
        let shift = width - prefix;
        prefix == 0 || net >> shift == ip >> shift
    }

    pub fn allows(ranges: &[Range], ip: IpAddr) -> bool {
        ranges.iter().any(|r| r.contains(ip))
    }
}

impl FromStr for Range {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (ip, prefix) = s.split_once('/').unwrap_or((s, ""));
        let net: IpAddr = ip.parse().map_err(|_| format!("not an address: {ip}"))?;
        let max = if net.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            "" => max,
            p => p.parse().ok().filter(|p| *p <= max).ok_or("bad prefix")?,
        };
        Ok(Range { net, prefix })
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.net, self.prefix)
    }
}

#[derive(Default)]
pub(crate) struct Guard {
    failures: Mutex<HashMap<IpAddr, (u32, Option<Instant>)>>,
}

pub(crate) enum Verdict {
    Delay(Duration, u32),
    Lock(Duration),
}

impl Guard {
    pub fn locked(&self, ip: IpAddr) -> bool {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&ip) {
            Some((_, Some(until))) if *until > Instant::now() => true,
            Some((_, Some(_))) => {
                map.remove(&ip);
                false
            }
            _ => false,
        }
    }

    pub fn fail(&self, ip: IpAddr, limits: &Limits) -> Verdict {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if map.len() > 4096 {
            map.retain(|_, (_, until)| until.is_some_and(|u| u > now));
        }
        let entry = map.entry(ip).or_insert((0, None));
        entry.0 += 1;
        if entry.0 >= limits.attempts.max(1) {
            *entry = (0, Some(now + limits.lockout));
            return Verdict::Lock(limits.lockout);
        }
        let factor = 1u32 << (entry.0 - 1).min(16);
        Verdict::Delay(
            limits.delay.saturating_mul(factor).min(limits.max_delay),
            entry.0,
        )
    }

    pub fn succeed(&self, ip: IpAddr) {
        self.failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ip);
    }
}

pub(crate) struct Rate {
    per_sec: u32,
    window: Instant,
    count: u32,
}

impl Rate {
    pub fn new(per_sec: u32) -> Rate {
        Rate {
            per_sec,
            window: Instant::now(),
            count: 0,
        }
    }

    pub fn allow(&mut self) -> bool {
        if self.window.elapsed() >= Duration::from_secs(1) {
            self.window = Instant::now();
            self.count = 0;
        }
        self.count += 1;
        self.per_sec == 0 || self.count <= self.per_sec
    }
}
