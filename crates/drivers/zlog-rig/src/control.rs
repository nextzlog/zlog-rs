use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use haml::sim::{Impairments, Simulator};
use haml::{Cat, Command, Config, Family, Model, Rig, RigEvent, State, Status};
use zlog_core::{Band, Mode};

use crate::sync::{band_of, rig_mode, FreqMemory, LogMode, RigMode};
use zlog_ui::rig::{Exposed, RigEntry, RigSource};
use zlog_ui::Section;

pub type SimFactory = fn(Family) -> Simulator;

pub fn simulator(family: Family) -> Simulator {
    Simulator::new(family, Impairments::clean(), 0)
}

type Transport = Box<dyn haml::Transport + Send>;

#[derive(Clone, Default)]
pub struct Drivers {
    pub models: Vec<&'static Model>,
    pub simulator: Option<SimFactory>,
    pub sources: Vec<Arc<dyn RigSource>>,
}

impl Drivers {
    pub fn from_host(ctx: &mut zlog_ui::HostCtx<'_>) -> Self {
        Drivers {
            models: ctx
                .drivers
                .take::<&'static Model>()
                .into_iter()
                .map(|m| *m)
                .collect(),
            simulator: ctx
                .drivers
                .take::<SimFactory>()
                .into_iter()
                .next()
                .map(|f| *f),
            sources: ctx
                .drivers
                .take::<dyn RigSource>()
                .into_iter()
                .map(Arc::from)
                .collect(),
        }
    }

    pub fn ports(&self) -> Vec<String> {
        let mut ports = zlog_serial::port_names();
        for s in &self.sources {
            ports.extend(s.ports());
        }
        ports
    }

    pub fn owned(&self, port: &str) -> bool {
        self.sources.iter().any(|s| s.owns(port))
    }

    fn open_source(&self, port: &str) -> Option<Result<Box<dyn Rig>, String>> {
        self.sources.iter().find_map(|s| s.open(port))
    }

    pub fn model(&self, i: usize) -> Option<&'static Model> {
        self.models.get(i).copied()
    }

    pub fn model_name(&self, i: usize) -> &'static str {
        self.model(i).map_or("", |m| m.name)
    }
}

pub use zlog_serial::SIMULATOR;

pub const CHOSEN: &str = "port_chosen";

pub fn chosen(s: &Section) -> bool {
    match s.get("port") {
        Some(p) => s.get(CHOSEN).is_some_and(|v| v == "true") || (!p.is_empty() && p != SIMULATOR),
        None => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RigConfig {
    pub model: usize,
    pub port: String,
    pub baud: u32,
    pub transverter_khz: u64,
    pub init_freq_on_mode: bool,
    pub ignore_rig_mode: bool,
    pub rtty_keeps_rig_mode: bool,
}

impl RigConfig {
    pub fn load(s: &Section, drivers: &Drivers) -> Self {
        let models = &drivers.models;
        let model = s
            .get("model")
            .map(|m| match m.as_str() {
                "K3/K3S/KX3" => "K3",
                m => m,
            })
            .and_then(|m| models.iter().position(|x| x.name == m))
            .unwrap_or(1)
            .min(models.len().saturating_sub(1));
        RigConfig {
            model,
            port: s.get("port").cloned().unwrap_or_else(|| SIMULATOR.into()),
            baud: s
                .get("baud")
                .and_then(|b| b.parse().ok())
                .unwrap_or(models.get(model).map_or(9600, |m| m.baud)),
            transverter_khz: s
                .get("transverter_khz")
                .and_then(|b| b.parse().ok())
                .unwrap_or(0),
            init_freq_on_mode: s.get("init_freq_on_mode").is_some_and(|v| v == "true"),
            ignore_rig_mode: s.get("ignore_rig_mode").is_some_and(|v| v == "true"),
            rtty_keeps_rig_mode: s.get("rtty_keeps_rig_mode").is_some_and(|v| v == "true"),
        }
    }

    pub fn save(&self, s: &mut Section, drivers: &Drivers) {
        s.insert("model".into(), drivers.model_name(self.model).into());
        s.insert("port".into(), self.port.clone());
        s.insert("baud".into(), self.baud.to_string());
        s.insert("transverter_khz".into(), self.transverter_khz.to_string());
        s.insert(
            "init_freq_on_mode".into(),
            self.init_freq_on_mode.to_string(),
        );
        s.insert("ignore_rig_mode".into(), self.ignore_rig_mode.to_string());
        s.insert(
            "rtty_keeps_rig_mode".into(),
            self.rtty_keeps_rig_mode.to_string(),
        );
    }
}

#[derive(Default)]
pub struct Control {
    pub drivers: Drivers,
    pub config: Option<RigConfig>,
    rig: Option<Box<dyn Rig>>,
    pub(crate) cat: Option<Config>,
    pub(crate) exposed: Option<Exposed>,
    language: String,
    offset: i64,
    pub sim: Option<Simulator>,
    memory: FreqMemory,
    synced: Option<(Band, Mode)>,
    expect: Option<Expect>,
    pub message: Option<String>,
    silent: bool,
    timeouts: u64,
    opening: Option<mpsc::Receiver<Result<zlog_serial::SerialPort, String>>>,
    retry: Option<Instant>,
    retries: u32,
}

const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);

impl Control {
    pub fn new(drivers: Drivers) -> Self {
        Control {
            drivers,
            ..Default::default()
        }
    }

    pub fn connected(&self) -> bool {
        self.rig.is_some()
    }

    pub fn opening(&self) -> bool {
        self.opening.is_some()
    }

    pub fn retrying(&self) -> bool {
        self.retry.is_some()
    }

    pub fn problem(&self, language: &str) -> Option<String> {
        let raw = match self.connected() {
            true => self.error(),
            false => self.message.clone(),
        }?;
        Some(zlog_ui::connection::explain(language, &raw))
    }

    pub fn state(&self) -> Option<State> {
        self.rig.as_ref().map(|r| r.state())
    }

    pub fn error(&self) -> Option<String> {
        let rig = self.rig.as_ref()?;
        match rig.error() {
            Some(e) => Some(e),
            None => self.silent.then(|| "NoReply".into()),
        }
    }

    fn expose(&self) {
        if let Some(e) = &self.exposed {
            let model = self
                .config
                .as_ref()
                .map(|c| self.drivers.model_name(c.model));
            let shown = self
                .rig
                .as_ref()
                .map(|r| (model.unwrap_or("").to_string(), r.caps()));
            e.set(shown);
        }
    }

    pub fn connect(&mut self, config: RigConfig, language: &str) {
        self.retry = None;
        self.retries = 0;
        self.start(config, language);
    }

    fn start(&mut self, config: RigConfig, language: &str) {
        self.sim = None;
        self.cat = None;
        self.language = language.into();
        self.offset = (config.transverter_khz * 1000) as i64;
        let Some(model) = self.drivers.model(config.model) else {
            self.rig = None;
            self.expose();
            self.message = Some(crate::text(language, "rig.no_model"));
            self.config = Some(config);
            return;
        };
        let pending = self.opening.take();
        let transport: Result<Transport, String> =
            if let Some(remote) = self.drivers.open_source(&config.port) {
                match remote {
                    Ok(rig) => {
                        self.rig = Some(rig);
                        self.message = None;
                    }
                    Err(e) => {
                        self.rig = None;
                        self.message = Some(e);
                    }
                }
                self.silent = false;
                self.timeouts = 0;
                self.synced = None;
                self.expect = None;
                self.config = Some(config);
                self.expose();
                return;
            } else if config.port == SIMULATOR {
                match self.drivers.simulator {
                    Some(make) => {
                        let sim = make(model.family);
                        let t = sim.transport();
                        self.sim = Some(sim);
                        Ok(Box::new(t))
                    }
                    None => Err(crate::text(language, "rig.no_simulator")),
                }
            } else {
                let (tx, rx) = mpsc::channel();
                let port = config.port.clone();
                let line = (config.baud, model.stop_bits);
                std::thread::spawn(move || {
                    if let Some(old) = pending {
                        drop(old.recv());
                    }
                    let _ = tx.send(zlog_serial::open_shared(&port, Some(line)));
                    zlog_core::wake::wake();
                });
                self.opening = Some(rx);
                self.rig = None;
                self.message = None;
                self.silent = false;
                self.timeouts = 0;
                self.synced = None;
                self.expect = None;
                self.config = Some(config);
                self.expose();
                return;
            };
        self.config = Some(config);
        self.install(transport);
        self.silent = false;
        self.timeouts = 0;
        self.synced = None;
        self.expect = None;
        self.expose();
    }

    fn install(&mut self, transport: Result<Transport, String>) {
        let found = self
            .config
            .as_ref()
            .and_then(|c| Some((self.drivers.model(c.model)?, c.baud)));
        let Some((model, baud)) = found else {
            return;
        };
        match transport {
            Ok(t) => {
                let config = Config {
                    offset: self.offset,
                    ..Config::for_baud(baud)
                };
                self.cat = Some(config);
                self.rig = Some(Box::new(Cat::new(model.protocol(), t, config)));
                self.message = None;
                self.retries = 0;
            }
            Err(e) => {
                self.rig = None;
                self.message = Some(e);
            }
        }
    }

    fn settle(&mut self, now: Instant) {
        if let Some(rx) = &self.opening {
            match rx.try_recv() {
                Ok(r) => {
                    self.opening = None;
                    let failed = r
                        .as_ref()
                        .err()
                        .map(|e| zlog_ui::connection::PortFault::of(e));
                    self.install(r.map(|p| Box::new(p) as Transport));
                    if failed.is_some_and(|f| f.retry()) {
                        let wait = RETRY_FIRST.saturating_mul(1 << self.retries.min(5));
                        self.retry = Some(now + wait.min(RETRY_MAX));
                        self.retries += 1;
                    }
                    self.expose();
                }
                Err(mpsc::TryRecvError::Disconnected) => self.opening = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.retry.is_some_and(|t| now >= t) {
            self.retry = None;
            if let Some(config) = self.config.clone() {
                let language = self.language.clone();
                self.start(config, &language);
            }
        }
    }

    pub fn reopen(&mut self) {
        let Some(config) = self.config.clone() else {
            return;
        };
        if self.drivers.owned(&config.port) {
            let language = self.language.clone();
            self.connect(config, &language);
        }
    }

    pub fn disconnect(&mut self) {
        self.opening = None;
        self.retry = None;
        self.retries = 0;
        self.message = None;
        self.rig = None;
        self.cat = None;
        self.sim = None;
        self.synced = None;
        self.expect = None;
        self.expose();
    }

    pub fn qsy(&mut self, hz: u64, mode: Option<Mode>, entry: &mut dyn RigEntry) {
        zlog_ui::rig::qsy_entry(entry, band_of(hz), mode);
        let Some(rig) = self.rig.as_mut() else {
            return;
        };
        let (band, mode) = (entry.band(), entry.mode());
        let now = Instant::now();
        set_freq(rig.as_mut(), hz, now);
        set_mode(rig.as_mut(), rig_mode(mode, band), now);
        self.synced = Some((band, mode));
        self.expect = Expect::new(band_of(hz), Some(mode), band);
    }

    pub fn poll(&mut self, now: Instant) {
        self.settle(now);
        let Some(rig) = self.rig.as_mut() else {
            return;
        };
        if let Some(sim) = &self.sim {
            sim.advance(now);
        }
        if let Some(e) = &self.exposed {
            for c in e.take() {
                rig.send(c, now);
            }
        }
        let events = rig.tick(now);
        let outcomes = rig.take_outcomes();
        let failed = outcomes.iter().any(|o| o.status == Status::Failed);
        if let Some(e) = &self.exposed {
            for ev in &events {
                e.publish(RigEvent::Event(*ev));
            }
            for o in outcomes {
                e.publish(RigEvent::Outcome(o));
            }
        }
        let timeouts = rig.timeouts();
        if !events.is_empty() {
            self.silent = false;
        } else if failed || timeouts > self.timeouts {
            self.silent = true;
        }
        self.timeouts = timeouts;
        let st = rig.state();
        if let (Some(hz), Some(m)) = (st.freq(), st.mode) {
            self.memory.remember(hz, m.to_log());
        }
    }

    pub fn follow(&mut self) {
        self.synced = None;
        self.expect = None;
    }

    pub fn follow_from(&mut self, typed: (Band, Mode)) {
        let reported = self
            .rig
            .as_ref()
            .is_some_and(|r| r.state().freq().is_some() && r.state().mode.is_some());
        self.follow();
        if !reported {
            self.synced = Some(typed);
        }
    }

    pub fn tick(&mut self, now: Instant, entry: &mut dyn RigEntry) -> bool {
        let model = self
            .config
            .as_ref()
            .and_then(|c| self.drivers.model(c.model));
        let option = |f: fn(&RigConfig) -> bool| self.config.as_ref().is_some_and(f);
        let init_freq_on_mode = option(|c| c.init_freq_on_mode);
        let ignore_rig_mode = option(|c| c.ignore_rig_mode);
        let rtty_keeps_rig_mode = option(|c| c.rtty_keeps_rig_mode);
        self.poll(now);
        let Some(rig) = self.rig.as_mut() else {
            return false;
        };
        let st = rig.state();
        let offset = self.offset;
        let busy = rig.busy();
        let from_rig = st.freq().and_then(band_of).zip(st.mode.map(|m| {
            if ignore_rig_mode {
                entry.mode()
            } else {
                m.to_log()
            }
        }));
        let typed = (entry.band(), entry.mode());
        match self.synced {
            Some(prev) if prev != typed => {
                let band_changed = prev.0 != typed.0;
                let covered = |hz: u64| {
                    let dial = (hz as i128 - offset as i128).clamp(0, u64::MAX as i128);
                    model.is_none_or(|m| m.covers(dial as u64))
                };
                let hold = !band_changed && typed.1 == Mode::Rtty && rtty_keeps_rig_mode;
                let mut tuned = None;
                if (band_changed || init_freq_on_mode) && !hold {
                    if let Some(hz) = self.memory.tune(typed.0, typed.1).filter(|hz| covered(*hz)) {
                        set_freq(rig.as_mut(), hz, now);
                        tuned = band_of(hz);
                    }
                }
                let mut moded = None;
                if (!band_changed && !hold) || typed.1 == Mode::Ssb {
                    set_mode(rig.as_mut(), rig_mode(typed.1, typed.0), now);
                    moded = Some(typed.1);
                }
                self.synced = Some(typed);
                self.expect = Expect::new(tuned, moded, typed.0);
                false
            }
            _ => match from_rig {
                Some(_) if busy => false,
                Some(seen) if self.waiting(seen, now) => false,
                Some(seen) if Some(seen) != self.synced && seen != typed => {
                    if entry.band() != seen.0 {
                        entry.set_band(seen.0);
                    }
                    entry.set_mode(seen.1);
                    self.synced = Some(seen);
                    true
                }
                Some(seen) => {
                    self.synced = Some(seen);
                    false
                }
                None => false,
            },
        }
    }
}

fn set_freq(rig: &mut dyn Rig, hz: u64, now: Instant) {
    let vfo = rig.state().vfo;
    rig.send(Command::SetFreq(vfo, hz), now);
}

fn set_mode(rig: &mut dyn Rig, mode: RigMode, now: Instant) {
    let vfo = rig.state().vfo;
    rig.send(Command::SetMode(vfo, mode), now);
}

#[derive(Debug, Clone, Copy)]
struct Expect {
    band: Option<Band>,
    mode: Option<Mode>,
    until: Option<Instant>,
}

impl Expect {
    fn new(band: Option<Band>, mode: Option<Mode>, row_band: Band) -> Option<Expect> {
        let mode =
            mode.filter(|m| !matches!(rig_mode(*m, row_band), RigMode::Data | RigMode::Other));
        (band.is_some() || mode.is_some()).then_some(Expect {
            band,
            mode,
            until: None,
        })
    }
}

impl Control {
    fn waiting(&mut self, seen: (Band, Mode), now: Instant) -> bool {
        let Some(e) = self.expect.as_mut() else {
            return false;
        };
        let until = *e
            .until
            .get_or_insert(now + std::time::Duration::from_secs(1));
        let arrived = e.band.is_none_or(|b| b == seen.0) && e.mode.is_none_or(|m| m == seen.1);
        if arrived || now >= until {
            self.expect = None;
            return false;
        }
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use zlog_core::Station;

    pub(crate) fn drivers() -> Drivers {
        Drivers {
            models: haml::MODELS.iter().collect(),
            simulator: Some(simulator),
            sources: Vec::new(),
        }
    }

    pub(crate) struct Entry {
        pub band: Band,
        pub mode: Mode,
        pub power: Power,
        pub rst_sent: String,
    }

    impl Entry {
        pub(crate) fn new(band: Band, mode: Mode, power: Power) -> Self {
            Entry {
                band,
                mode,
                power,
                rst_sent: rst(mode).into(),
            }
        }

        pub(crate) fn set_mode(&mut self, mode: Mode) {
            if self.mode != mode {
                self.mode = mode;
                self.rst_sent = rst(mode).into();
            }
        }
    }

    fn rst(mode: Mode) -> &'static str {
        match mode {
            Mode::Ssb | Mode::Fm | Mode::Am | Mode::Dv => "59",
            _ => "599",
        }
    }

    impl RigEntry for Entry {
        fn band(&self) -> Band {
            self.band
        }
        fn mode(&self) -> Mode {
            self.mode
        }
        fn set_band(&mut self, band: Band) {
            self.band = band;
            self.power = Station::default().power_for(band);
        }
        fn set_mode(&mut self, mode: Mode) {
            Entry::set_mode(self, mode);
        }
    }

    pub(crate) fn sim(rc: &Control) -> Simulator {
        rc.sim.clone().expect("simulated")
    }

    fn clear(rc: &Control) {
        let sim = sim(rc);
        let n = sim.stats().commands;
        MARK.with(|m| m.set(n));
    }

    thread_local! {
        static MARK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }
    use std::time::Duration;
    use zlog_core::Power;

    fn run(rc: &mut Control, t: &mut Instant, entry: &mut Entry, ms: u64) {
        for _ in 0..ms / 10 {
            *t += Duration::from_millis(10);
            rc.tick(*t, entry);
        }
    }

    struct Source(Simulator, std::sync::atomic::AtomicUsize);

    impl RigSource for Source {
        fn ports(&self) -> Vec<String> {
            vec!["remote:1 IC-7300".into()]
        }
        fn open(&self, port: &str) -> Option<Result<Box<dyn Rig>, String>> {
            let family = Family::Icom { address: 0x94 };
            if !self.owns(port) {
                return None;
            }
            let t = self.0.transport();
            self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let cat = Cat::new(family.protocol(), t, Config::for_baud(19200));
            self.owns(port).then(|| Ok(Box::new(cat) as Box<dyn Rig>))
        }
        fn owns(&self, port: &str) -> bool {
            port == "remote:1 IC-7300"
        }
    }

    #[test]
    fn a_rig_from_a_source_is_driven_like_a_local_one_and_exposed() {
        let sim = simulator(Family::Icom { address: 0x94 });
        let with_source = Drivers {
            sources: vec![Arc::new(Source(sim.clone(), Default::default()))],
            ..drivers()
        };
        assert!(with_source
            .ports()
            .contains(&"remote:1 IC-7300".to_string()));
        let mut rc = Control::new(with_source);
        let shown = Exposed::default();
        let events = shown.subscribe();
        rc.exposed = Some(shown.clone());
        let mut config = RigConfig::load(&Section::default(), &drivers());
        config.port = "remote:1 IC-7300".into();
        rc.connect(config, "en");
        assert!(rc.connected() && rc.sim.is_none() && rc.cat.is_none());
        assert_eq!(shown.rig().unwrap().1, haml::RigCaps::ALL);
        let mut entry = Entry::new(Band::B14, Mode::Ssb, Power::P010);
        let mut t = Instant::now();
        for _ in 0..60 {
            t += Duration::from_millis(10);
            sim.advance(t);
            rc.tick(t, &mut entry);
        }
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        shown.push(Command::SetFreq(haml::Vfo::A, 7_025_000));
        for _ in 0..60 {
            t += Duration::from_millis(10);
            sim.advance(t);
            rc.tick(t, &mut entry);
        }
        assert_eq!(sim.radio().freq_a, 7_025_000, "a remote client's command");
        assert!(events
            .try_iter()
            .any(|e| e == RigEvent::Event(haml::Event::Freq(haml::Vfo::A, 7_025_000))));
        rc.disconnect();
        assert!(shown.rig().is_none());
    }

    #[test]
    fn a_remote_rig_already_chosen_is_opened_again_after_a_reconnect() {
        let source = Arc::new(Source(
            simulator(Family::Icom { address: 0x94 }),
            Default::default(),
        ));
        let opens = || source.1.load(std::sync::atomic::Ordering::SeqCst);
        let with_source = Drivers {
            sources: vec![source.clone()],
            ..drivers()
        };
        let mut rigs = crate::pair::Rigs::new(with_source.clone());
        let mut config = RigConfig::load(&Section::default(), &with_source);
        config.port = "remote:1 IC-7300".into();
        rigs.slot_mut(1).connect(config, "ja");
        let mut entry = Entry::new(Band::B7, Mode::Cw, Power::P010);
        let mut t = Instant::now();
        rigs.tick(t, &mut entry);
        let before = opens();
        zlog_ui::device::reopen();
        t += Duration::from_millis(10);
        rigs.tick(t, &mut entry);
        assert_eq!(opens(), before + 1, "reopened once");
        rigs.tick(t, &mut entry);
        assert_eq!(opens(), before + 1);
        assert!(rigs.slot(1).connected());
        let mut local = Control::new(with_source);
        local.connect(RigConfig::load(&Section::default(), &drivers()), "en");
        local.reopen();
        assert_eq!(opens(), before + 1, "a local rig is left alone");
    }

    #[test]
    fn entry_follows_the_simulated_rig_and_back() {
        let mut rc = Control::new(drivers());
        rc.connect(
            RigConfig {
                model: 1,
                port: SIMULATOR.into(),
                baud: 19200,
                transverter_khz: 0,
                init_freq_on_mode: false,
                ignore_rig_mode: false,
                rtty_keeps_rig_mode: false,
            },
            "en",
        );
        let mut entry = Entry::new(Band::B14, Mode::Ssb, Power::P010);
        let mut t = Instant::now();
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));

        sim(&rc).press_mode(RigMode::Lsb);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.mode, entry.rst_sent.as_str()), (Mode::Ssb, "59"));
        sim(&rc).press_mode(RigMode::Cw);
        run(&mut rc, &mut t, &mut entry, 600);

        sim(&rc).turn_dial(21_030_000);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B21);

        entry.band = Band::B28;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(sim(&rc).with(|r| r.freq_a), 28_050_000);
        assert_eq!(entry.band, Band::B28);

        entry.band = Band::B21;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(sim(&rc).with(|r| r.freq_a), 21_030_000);
    }

    fn model(name: &str) -> usize {
        drivers()
            .models
            .iter()
            .position(|m| m.name == name)
            .unwrap()
    }

    fn settled(name: &str, init_freq_on_mode: bool) -> (Control, Entry, Instant) {
        settled_with(name, |c| c.init_freq_on_mode = init_freq_on_mode)
    }

    fn settled_with(name: &str, f: impl FnOnce(&mut RigConfig)) -> (Control, Entry, Instant) {
        let mut rc = Control::new(drivers());
        let mut config = RigConfig {
            model: model(name),
            port: SIMULATOR.into(),
            baud: 19200,
            transverter_khz: 0,
            init_freq_on_mode: false,
            ignore_rig_mode: false,
            rtty_keeps_rig_mode: false,
        };
        f(&mut config);
        rc.connect(config, "en");
        let mut entry = Entry::new(Band::B7, Mode::Cw, Power::P010);
        let mut t = Instant::now();
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        clear(&rc);
        (rc, entry, t)
    }

    fn received(rc: &Control) -> Vec<Vec<u8>> {
        let sim = sim(rc);
        let all = sim.received();
        let new = (sim.stats().commands - MARK.with(|m| m.get())) as usize;
        assert!(new <= all.len(), "more commands than kept");
        all[all.len() - new..].to_vec()
    }

    fn sets(rc: &Control) -> Vec<Vec<u8>> {
        received(rc)
            .into_iter()
            .filter(|c| match c.first() {
                Some(0xFE) => matches!(c.get(4), Some(0x05 | 0x06)),
                _ => c.len() > 4,
            })
            .collect()
    }

    fn icom_freq_sets(cmds: &[Vec<u8>]) -> usize {
        cmds.iter().filter(|c| c[4] == 0x05).count()
    }

    fn icom_mode_sets(cmds: &[Vec<u8>]) -> usize {
        cmds.iter().filter(|c| c[4] == 0x06).count()
    }

    #[test]
    fn a_mode_change_sets_only_the_mode() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (0, 1));
        let sim = sim(&rc);
        assert_eq!(sim.with(|r| (r.freq_a, r.mode)), (7_012_000, RigMode::Lsb));
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Ssb));
    }

    #[test]
    fn a_mode_change_tunes_with_the_option() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", true);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
        assert_eq!(sim(&rc).with(|r| r.freq_a), 7_060_000);
    }

    #[test]
    fn a_band_change_sets_the_mode_only_for_ssb() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.band = Band::B14;
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 0));
        assert_eq!(sim(&rc).with(|r| r.freq_a), 14_050_000);

        sim(&rc).press_mode(RigMode::CwR);
        run(&mut rc, &mut t, &mut entry, 600);
        entry.band = Band::B21;
        run(&mut rc, &mut t, &mut entry, 600);
        let sim = sim(&rc);
        assert_eq!(sim.with(|r| r.mode), RigMode::CwR);

        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        clear(&rc);
        entry.band = Band::B3_5;
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
        assert_eq!(sim.with(|r| (r.freq_a, r.mode)), (3_535_000, RigMode::Lsb));
    }

    #[test]
    fn a_band_the_rig_does_not_cover_is_not_tuned_and_the_row_comes_back() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.band = Band::B144;
        run(&mut rc, &mut t, &mut entry, 1000);
        assert!(sets(&rc).is_empty());
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        sim(&rc).turn_dial(21_030_000);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B21);
        entry.band = Band::B28;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(sim(&rc).with(|r| r.freq_a), 28_050_000);
        assert_eq!(entry.band, Band::B28);
    }

    #[test]
    fn an_uncovered_band_in_ssb_still_sends_the_mode() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        let sim = sim(&rc);
        clear(&rc);
        entry.band = Band::B144;
        run(&mut rc, &mut t, &mut entry, 1000);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (0, 1));
        assert_eq!(sim.with(|r| (r.freq_a, r.mode)), (7_012_000, RigMode::Usb));
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Ssb));
    }

    #[test]
    fn the_lowest_band_of_a_vhf_rig() {
        let mut rc = Control::new(drivers());
        rc.connect(
            RigConfig {
                model: model("IC-9700"),
                port: SIMULATOR.into(),
                baud: 19200,
                transverter_khz: 0,
                init_freq_on_mode: false,
                ignore_rig_mode: false,
                rtty_keeps_rig_mode: false,
            },
            "en",
        );
        sim(&rc).with(|r| r.freq_a = 144_050_000);
        let mut entry = Entry::new(Band::B144, Mode::Cw, Power::P010);
        let mut t = Instant::now();
        run(&mut rc, &mut t, &mut entry, 600);
        clear(&rc);
        entry.band = Band::B50;
        run(&mut rc, &mut t, &mut entry, 600);
        assert!(sets(&rc).is_empty());
        assert_eq!(entry.band, Band::B144);
        entry.band = Band::B430;
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(sim(&rc).with(|r| r.freq_a), 430_050_000);
    }

    #[test]
    fn transverter_offset_from_the_settings() {
        let mut rc = Control::new(drivers());
        rc.connect(
            RigConfig {
                model: model("IC-705"),
                port: SIMULATOR.into(),
                baud: 19200,
                transverter_khz: 1_150_000,
                init_freq_on_mode: false,
                ignore_rig_mode: false,
                rtty_keeps_rig_mode: false,
            },
            "en",
        );
        sim(&rc).with(|r| r.freq_a = 144_050_000);
        let mut entry = Entry::new(Band::B7, Mode::Cw, Power::P010);
        let mut t = Instant::now();
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B1200);
        assert_eq!(rc.state().unwrap().freq(), Some(1_294_050_000));
    }

    #[test]
    fn data_modes_leave_the_rig_alone() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.set_mode(Mode::Ft8);
        run(&mut rc, &mut t, &mut entry, 600);
        assert!(sets(&rc).is_empty());
    }

    #[test]
    fn text_rig_band_change_goes_to_the_vfo_in_use() {
        let (mut rc, mut entry, mut t) = settled("TS-590", false);
        sim(&rc).with(|r| r.vfo = haml::Vfo::B);
        run(&mut rc, &mut t, &mut entry, 600);
        entry.band = Band::B14;
        run(&mut rc, &mut t, &mut entry, 600);
        let sim = sim(&rc);
        assert_eq!(sim.with(|r| (r.freq_a, r.freq_b)), (7_012_000, 14_050_000));
        assert_eq!(sets(&rc), vec![b"FB00014050000;".to_vec()]);
    }

    #[test]
    fn ignore_rig_mode_keeps_the_row_mode() {
        let (mut rc, mut entry, mut t) = settled_with("IC-7300", |c| c.ignore_rig_mode = true);
        let sim = sim(&rc);
        sim.press_mode(RigMode::Lsb);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        sim.turn_dial(14_030_000);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B14, Mode::Cw));
        clear(&rc);
        entry.set_mode(Mode::Fm);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(icom_mode_sets(&sets(&rc)), 1);
        assert_eq!(sim.with(|r| r.mode), RigMode::Fm);
        assert_eq!(entry.mode, Mode::Fm);
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        self::sim(&rc).press_mode(RigMode::Lsb);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Ssb);
    }

    #[test]
    fn rtty_keeps_rig_mode_option() {
        let (mut rc, mut entry, mut t) = settled_with("IC-7300", |c| {
            c.rtty_keeps_rig_mode = true;
        });
        entry.set_mode(Mode::Rtty);
        run(&mut rc, &mut t, &mut entry, 600);
        assert!(sets(&rc).is_empty());
        assert_eq!(entry.mode, Mode::Cw);
        let (mut rc, mut entry, mut t) = settled_with("IC-7300", |c| {
            c.rtty_keeps_rig_mode = true;
            c.ignore_rig_mode = true;
            c.init_freq_on_mode = true;
        });
        entry.set_mode(Mode::Rtty);
        run(&mut rc, &mut t, &mut entry, 600);
        assert!(sets(&rc).is_empty());
        assert_eq!(entry.mode, Mode::Rtty);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.set_mode(Mode::Rtty);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(icom_mode_sets(&sets(&rc)), 1);
        assert_eq!(sim(&rc).with(|r| r.mode), RigMode::Rtty);
    }

    #[test]
    fn qsy_with_a_rig() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        rc.qsy(21_250_000, Some(Mode::Ssb), &mut entry);
        assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Ssb));
        assert_eq!(entry.rst_sent, "59");
        for _ in 0..60 {
            t += Duration::from_millis(10);
            rc.tick(t, &mut entry);
            assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Ssb));
        }
        let sim = sim(&rc);
        assert_eq!(sim.with(|r| (r.freq_a, r.mode)), (21_250_000, RigMode::Usb));
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));

        clear(&rc);
        rc.qsy(7_150_000, None, &mut entry);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Ssb));
        run(&mut rc, &mut t, &mut entry, 600);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
        assert_eq!(sim.with(|r| (r.freq_a, r.mode)), (7_150_000, RigMode::Lsb));

        clear(&rc);
        rc.qsy(7_020_000, Some(Mode::Cw), &mut entry);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!((entry.band, entry.mode), (Band::B7, Mode::Cw));
        assert_eq!(sim.with(|r| r.freq_a), 7_020_000);
        let cmds = sets(&rc);
        assert_eq!((icom_freq_sets(&cmds), icom_mode_sets(&cmds)), (1, 1));
    }

    #[test]
    fn a_band_change_does_not_bounce() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        entry.band = Band::B14;
        for _ in 0..60 {
            t += Duration::from_millis(10);
            rc.tick(t, &mut entry);
            assert_eq!(entry.band, Band::B14);
        }
        let sim = sim(&rc);
        sim.with(|r| r.power = false);
        entry.band = Band::B21;
        run(&mut rc, &mut t, &mut entry, 300);
        sim.with(|r| {
            r.power = true;
            r.freq_a = 7_020_000;
        });
        for _ in 0..250 {
            t += Duration::from_millis(10);
            rc.tick(t, &mut entry);
            assert_eq!(entry.band, Band::B21);
        }
        assert_eq!(sim.with(|r| r.freq_a), 21_050_000);
        assert_eq!(rc.error(), None);
    }

    #[test]
    fn a_rig_switched_off_is_reported_and_recovers() {
        let (mut rc, mut entry, mut t) = settled("IC-7300", false);
        sim(&rc).with(|r| r.power = false);
        run(&mut rc, &mut t, &mut entry, 1500);
        assert_eq!(rc.error().as_deref(), Some("NoReply"));
        sim(&rc).with(|r| r.power = true);
        run(&mut rc, &mut t, &mut entry, 1500);
        assert_eq!(rc.error(), None);
    }

    #[test]
    fn every_model_follows_and_tunes_the_simulator() {
        for (i, m) in drivers().models.iter().enumerate() {
            let mut rc = Control::new(drivers());
            let mut config = RigConfig::load(&Section::default(), &drivers());
            config.model = i;
            config.baud = m.baud;
            rc.connect(config, "en");
            let hz = if m.covers(7_012_000) {
                7_012_000
            } else {
                145_012_000
            };
            sim(&rc).with(|r| r.freq_a = hz);
            let mut entry = Entry::new(Band::B14, Mode::Ssb, Power::P010);
            let mut t = Instant::now();
            run(&mut rc, &mut t, &mut entry, 1000);
            let name = m.name;
            assert_eq!(entry.mode, Mode::Cw, "{name}");
            assert_eq!(rc.state().unwrap().freq(), Some(hz), "{name}");
            rc.qsy(hz + 13_000, Some(Mode::Ssb), &mut entry);
            run(&mut rc, &mut t, &mut entry, 1500);
            let want = rig_mode(Mode::Ssb, entry.band);
            assert_eq!(sim(&rc).radio().freq(), hz + 13_000, "{name}");
            assert_eq!(sim(&rc).radio().mode, want, "{name}");
            assert_eq!(rc.error(), None, "{name}");
        }
    }

    #[test]
    fn the_simulator_follows_the_tick_clock_and_needs_a_driver() {
        let mut rc = Control::new(drivers());
        rc.connect(RigConfig::load(&Section::default(), &drivers()), "en");
        let t = Instant::now() + Duration::from_secs(3600);
        rc.poll(t);
        rc.poll(t + Duration::from_millis(10));
        assert_eq!(rc.state().unwrap().freq(), Some(7_012_000));
        let config = rc.cat.unwrap();
        assert_eq!((config.interval.as_millis(), config.verify), (75, true));
        let mut config = RigConfig::load(&Section::default(), &drivers());
        config.baud = 9600;
        config.transverter_khz = 1_150_000;
        rc.connect(config.clone(), "en");
        let config2 = rc.cat.unwrap();
        assert_eq!(
            (config2.interval.as_millis(), config2.offset),
            (150, 1_150_000_000)
        );
        rc.disconnect();
        assert!(rc.sim.is_none());
        let mut rc = Control::new(Drivers {
            simulator: None,
            ..drivers()
        });
        rc.connect(config, "en");
        assert!(!rc.connected() && rc.sim.is_none());
        assert_eq!(rc.message, Some(crate::text("en", "rig.no_simulator")));
    }

    #[test]
    fn old_k3_settings_load_as_k3() {
        let mut s = Section::default();
        s.insert("model".into(), "K3/K3S/KX3".into());
        let c = RigConfig::load(&s, &drivers());
        assert_eq!(drivers().model_name(c.model), "K3");
        assert_eq!(c.baud, 38400);
        c.save(&mut s, &drivers());
        assert_eq!(s.get("model").map(String::as_str), Some("K3"));
        s.insert("model".into(), "KX3".into());
        assert_eq!(
            drivers().model_name(RigConfig::load(&s, &drivers()).model),
            "KX3"
        );
    }

    #[test]
    fn qsy_without_a_rig() {
        let mut rc = Control::new(drivers());
        let st = Station::default();
        let mut entry = Entry::new(Band::B7, Mode::Cw, Power::P010);
        rc.qsy(14_250_000, Some(Mode::Ssb), &mut entry);
        assert_eq!((entry.band, entry.mode), (Band::B14, Mode::Ssb));
        assert_eq!(entry.power, st.power_for(Band::B14));
        rc.qsy(21_030_000, None, &mut entry);
        assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Ssb));
        rc.qsy(8_500_000, Some(Mode::Cw), &mut entry);
        assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Cw));
        rc.qsy(0, None, &mut entry);
        assert_eq!((entry.band, entry.mode), (Band::B21, Mode::Cw));
        assert!(!rc.connected());
    }

    #[test]
    fn dv_and_other_modes_from_and_to_an_icom() {
        let (mut rc, mut entry, mut t) = settled("IC-705", false);
        let sim = sim(&rc);
        sim.press_mode(RigMode::Dv);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Dv);
        sim.press_mode(RigMode::Other);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Other);
        sim.press_mode(RigMode::Cw);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Cw);
        clear(&rc);
        entry.set_mode(Mode::Dv);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(icom_mode_sets(&sets(&rc)), 1);
        assert_eq!(sim.with(|r| r.mode), RigMode::Dv);
        assert_eq!(entry.mode, Mode::Dv);
    }

    #[test]
    fn kenwood_cw_reverse_survives_a_mode_change() {
        let (mut rc, mut entry, mut t) = settled("TS-590", false);
        let sim = sim(&rc);
        sim.press_mode(RigMode::CwR);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.mode, Mode::Cw);
        entry.set_mode(Mode::Ssb);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(sim.with(|r| r.mode), RigMode::Lsb);
        entry.set_mode(Mode::Cw);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(sim.with(|r| r.mode), RigMode::CwR);
    }

    #[test]
    fn ftdx101_follows_the_receiving_vfo() {
        let (mut rc, mut entry, mut t) = settled("FTDX101", false);
        let sim = sim(&rc);
        sim.with(|r| r.freq_b = 14_100_000);
        sim.with(|r| r.vfo = haml::Vfo::B);
        run(&mut rc, &mut t, &mut entry, 600);
        assert_eq!(entry.band, Band::B14);
        assert!(sim.received().iter().any(|c| c == b"FR;"));
        assert!(sim.received().iter().all(|c| c != b"FT;"));
    }

    #[test]
    fn settings_round_trip_and_defaults() {
        let d = RigConfig::load(&Section::default(), &drivers());
        assert_eq!(drivers().model_name(d.model), "IC-7300");
        assert_eq!((d.transverter_khz, d.init_freq_on_mode), (0, false));
        assert_eq!((d.ignore_rig_mode, d.rtty_keeps_rig_mode), (false, false));
        assert_eq!(d.port, SIMULATOR);
        let c = RigConfig {
            model: model("FT-710"),
            port: "/dev/ttyUSB0".into(),
            baud: 38400,
            transverter_khz: 116_000,
            init_freq_on_mode: true,
            ignore_rig_mode: true,
            rtty_keeps_rig_mode: true,
        };
        let mut s = Section::default();
        c.save(&mut s, &drivers());
        assert_eq!(RigConfig::load(&s, &drivers()), c);
        s.insert("transverter_khz".into(), "x".into());
        s.insert("init_freq_on_mode".into(), "yes".into());
        s.insert("ignore_rig_mode".into(), "1".into());
        s.insert("rtty_keeps_rig_mode".into(), "".into());
        s.insert("model".into(), "nonexistent".into());
        let m = RigConfig::load(&s, &drivers());
        assert_eq!((m.transverter_khz, m.init_freq_on_mode), (0, false));
        assert_eq!((m.ignore_rig_mode, m.rtty_keeps_rig_mode), (false, false));
        assert_eq!(drivers().model_name(m.model), "IC-7300");
    }
}
