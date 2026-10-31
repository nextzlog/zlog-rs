use std::io;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

pub use haml::ports::{Link, PortInfo, UsbId};
pub use haml::ptt::{Line, Lines};
pub use haml::transport::{Boxed, Emulated, Emulator, Loop, Peer, Shared, Slot, Transport};
pub use haml::SerialTransport;
pub use serialport::StopBits;

type Scan = fn() -> Result<Vec<PortInfo>, String>;

struct Listed {
    ports: Result<Vec<PortInfo>, String>,
    scanned: bool,
    watching: bool,
}

impl Listed {
    const fn new() -> Self {
        Listed {
            ports: Ok(Vec::new()),
            scanned: false,
            watching: false,
        }
    }
}

static LISTED: Mutex<Listed> = Mutex::new(Listed::new());

const INTERVAL: Duration = Duration::from_secs(2);

pub struct Ports {
    pub ports: Result<Vec<PortInfo>, String>,
    pub scanning: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn update(listed: &Mutex<Listed>, ports: Result<Vec<PortInfo>, String>) -> bool {
    let mut l = lock(listed);
    let changed = !l.scanned || l.ports != ports;
    l.ports = ports;
    l.scanned = true;
    changed
}

fn cached(listed: &'static Mutex<Listed>, scan: Scan, interval: Duration) -> Ports {
    let mut l = lock(listed);
    if !l.watching {
        l.watching = true;
        std::thread::spawn(move || loop {
            if update(listed, scan()) {
                zlog_core::wake::wake();
            }
            std::thread::sleep(interval);
        });
    }
    Ports {
        ports: l.ports.clone(),
        scanning: !l.scanned,
    }
}

fn names(p: Ports) -> Vec<String> {
    p.ports
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.name)
        .collect()
}

pub fn ports() -> Ports {
    cached(&LISTED, haml::ports::serial_ports, INTERVAL)
}

pub fn port_names() -> Vec<String> {
    names(ports())
}

pub const SIMULATOR: &str = "simulator";

pub fn port_names_with_simulator() -> Vec<String> {
    std::iter::once(SIMULATOR.to_string())
        .chain(port_names())
        .collect()
}

#[derive(Clone)]
pub struct SerialPort(Arc<Mutex<SerialTransport>>);

impl SerialPort {
    pub fn same(&self, other: &SerialPort) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn shared(&self) -> Shared {
        Shared::new(Box::new(self.clone()))
    }
}

impl Transport for SerialPort {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        lock(&self.0).write(bytes)
    }

    fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        lock(&self.0).read(buf)
    }

    fn maintain(&mut self, now: Instant) -> Link {
        lock(&self.0).maintain(now)
    }
}

impl Lines for SerialPort {
    fn set(&mut self, line: Line, on: bool) -> io::Result<()> {
        lock(&self.0).set(line, on)
    }
}

type Opened = (String, Weak<Mutex<SerialTransport>>);

static OPEN: Mutex<Vec<Opened>> = Mutex::new(Vec::new());

fn stop_bits(n: u8) -> StopBits {
    if n == 2 {
        StopBits::Two
    } else {
        StopBits::One
    }
}

pub fn open_shared(path: &str, line: Option<(u32, u8)>) -> Result<SerialPort, String> {
    let mut open = lock(&OPEN);
    open.retain(|(_, w)| w.strong_count() > 0);
    if let Some(port) = open
        .iter()
        .find(|(p, _)| p == path)
        .and_then(|(_, w)| w.upgrade())
    {
        if let Some((baud, stop)) = line {
            let mut t = lock(&port);
            t.port().set_baud_rate(baud).map_err(|e| e.to_string())?;
            t.port()
                .set_stop_bits(stop_bits(stop))
                .map_err(|e| e.to_string())?;
        }
        return Ok(SerialPort(port));
    }
    let (baud, stop) = line.unwrap_or((9600, 1));
    let mut port = serialport::new(path, baud)
        .stop_bits(stop_bits(stop))
        .data_bits(serialport::DataBits::Eight)
        .timeout(Duration::from_millis(1))
        .open()
        .map_err(|e| e.to_string())?;
    let _ = port.write_data_terminal_ready(false);
    let _ = port.write_request_to_send(false);
    let port = Arc::new(Mutex::new(SerialTransport::from_port(port)));
    open.push((path.into(), Arc::downgrade(&port)));
    Ok(SerialPort(port))
}

pub fn open(path: &str, baud: u32, stop: u8) -> Result<SerialTransport, String> {
    serialport::new(path, baud)
        .stop_bits(stop_bits(stop))
        .data_bits(serialport::DataBits::Eight)
        .timeout(Duration::from_millis(1))
        .open()
        .map(SerialTransport::from_port)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: Duration = Duration::from_millis(5);

    fn settle(listed: &'static Mutex<Listed>, scan: Scan, want: &[&str]) {
        let start = std::time::Instant::now();
        while names(cached(listed, scan, FAST)) != want {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(FAST);
        }
    }

    #[test]
    fn port_names_never_scan_on_the_calling_thread() {
        static SCANNED: Mutex<Vec<std::thread::ThreadId>> = Mutex::new(Vec::new());
        static FAKE: Mutex<Listed> = Mutex::new(Listed::new());
        fn scan() -> Result<Vec<PortInfo>, String> {
            SCANNED.lock().unwrap().push(std::thread::current().id());
            Ok(vec![PortInfo::named("COM9")])
        }
        let first = cached(&FAKE, scan, FAST);
        assert!(first.scanning, "returns at once");
        assert!(names(first).is_empty());
        settle(&FAKE, scan, &["COM9"]);
        assert!(!cached(&FAKE, scan, FAST).scanning);
        let me = std::thread::current().id();
        assert!(SCANNED.lock().unwrap().iter().all(|t| *t != me));
    }

    #[test]
    fn new_ports_appear_without_a_rescan() {
        static FOUND: Mutex<Vec<&str>> = Mutex::new(Vec::new());
        static FAKE: Mutex<Listed> = Mutex::new(Listed::new());
        fn scan() -> Result<Vec<PortInfo>, String> {
            Ok(FOUND
                .lock()
                .unwrap()
                .iter()
                .map(|n| PortInfo::named(n))
                .collect())
        }
        FOUND.lock().unwrap().push("COM1");
        settle(&FAKE, scan, &["COM1"]);
        FOUND.lock().unwrap().push("COM4");
        settle(&FAKE, scan, &["COM1", "COM4"]);
    }

    #[test]
    fn only_a_change_is_reported() {
        static FAKE: Mutex<Listed> = Mutex::new(Listed::new());
        let one = || Ok(vec![PortInfo::named("COM1")]);
        assert!(update(&FAKE, one()), "first result");
        assert!(!update(&FAKE, one()), "same ports");
        let two = Ok(vec![PortInfo::named("COM1"), PortInfo::named("COM2")]);
        assert!(update(&FAKE, two));
        assert!(update(&FAKE, Err("gone".into())));
        assert!(!update(&FAKE, Err("gone".into())));
    }

    #[test]
    fn the_simulator_is_listed_first() {
        assert_eq!(port_names_with_simulator()[0], SIMULATOR);
    }

    #[test]
    fn a_missing_port_is_an_error() {
        assert!(open_shared("/dev/zlog-no-such-port", None).is_err());
        assert!(open("/dev/zlog-no-such-port", 9600, 1).is_err());
    }
}
