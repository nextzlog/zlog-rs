use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ftxl::{Command, Decode, Notice, Parity, Pick, Proto, Qso, Setup, Spot, Warning};
use haml::remote::{Channel, Inbound, RemoteChannel};
use zlog_ui::device::AudioIo;
use zlog_ui::tx::{Keying, PttLink, TxHub};

pub const NAME: &str = "ft8";
pub const RIG: &str = "RIG";
pub const NO_RIG: &str = "no shared rig to key";
pub const RESET: &str = "RESET";

fn ms(t: SystemTime) -> String {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
        .to_string()
}

fn time(s: &str) -> Option<SystemTime> {
    Some(UNIX_EPOCH + Duration::from_millis(s.parse().ok()?))
}

fn opt<T: ToString>(v: &Option<T>) -> String {
    v.as_ref().map_or_else(|| "-".into(), T::to_string)
}

fn parse_opt<T: std::str::FromStr>(s: &str) -> Option<Option<T>> {
    match s {
        "-" => Some(None),
        s => s.parse().ok().map(Some),
    }
}

fn text(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

fn flag(b: bool) -> &'static str {
    if b {
        "1"
    } else {
        "0"
    }
}

fn parse_flag(s: &str) -> Option<bool> {
    match s {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

fn proto(s: &str) -> Option<Proto> {
    Proto::ALL.into_iter().find(|p| p.id() == s)
}

fn parity(p: Parity) -> &'static str {
    match p {
        Parity::Even => "even",
        Parity::Odd => "odd",
    }
}

fn parse_parity(s: &str) -> Option<Parity> {
    match s {
        "even" => Some(Parity::Even),
        "odd" => Some(Parity::Odd),
        _ => None,
    }
}

fn setup(s: &Setup) -> String {
    let o = &s.options;
    let f = &o.ft;
    let q = &s.seq;
    [
        s.proto.id().to_string(),
        s.rx_hz.to_string(),
        s.tx_hz.to_string(),
        parity(s.parity).into(),
        o.min_hz.to_string(),
        o.max_hz.to_string(),
        opt(&o.rx_hz),
        f.candidates.to_string(),
        f.passes.to_string(),
        f.iterations.to_string(),
        f.threshold.to_string(),
        f.min_sync.to_string(),
        f.rx_width.to_string(),
        flag(f.blanker).into(),
        flag(f.fading).into(),
        text(&q.my_call),
        text(&q.my_grid),
        text(&q.his_call),
        text(&q.his_grid),
        q.report.to_string(),
        q.rcvd.as_deref().map_or_else(|| "-".into(), text),
        text(&q.cq),
        text(&q.exchange),
        q.tx.to_string(),
        flag(q.enabled).into(),
        flag(q.auto).into(),
    ]
    .join("\t")
}

fn parse_setup(w: &[&str]) -> Option<Setup> {
    let [p, rx, tx, par, min, max, orx, cand, passes, it, th, sync, width, nb, fade, my_call, my_grid, his_call, his_grid, report, rcvd, cq, exchange, txn, en, auto] =
        w
    else {
        return None;
    };
    let mut s = Setup {
        proto: proto(p)?,
        rx_hz: rx.parse().ok()?,
        tx_hz: tx.parse().ok()?,
        parity: parse_parity(par)?,
        ..Setup::default()
    };
    s.options.min_hz = min.parse().ok()?;
    s.options.max_hz = max.parse().ok()?;
    s.options.rx_hz = parse_opt(orx)?;
    let f = &mut s.options.ft;
    f.candidates = cand.parse().ok()?;
    f.passes = passes.parse().ok()?;
    f.iterations = it.parse().ok()?;
    f.threshold = th.parse().ok()?;
    f.min_sync = sync.parse().ok()?;
    f.rx_width = width.parse().ok()?;
    f.blanker = parse_flag(nb)?;
    f.fading = parse_flag(fade)?;
    let q = &mut s.seq;
    q.my_call = my_call.to_string();
    q.my_grid = my_grid.to_string();
    q.his_call = his_call.to_string();
    q.his_grid = his_grid.to_string();
    q.report = report.parse().ok()?;
    q.rcvd = (*rcvd != "-").then(|| rcvd.to_string());
    q.cq = cq.to_string();
    q.exchange = exchange.to_string();
    q.tx = txn.parse().ok()?;
    q.enabled = parse_flag(en)?;
    q.auto = parse_flag(auto)?;
    Some(s)
}

fn decode(d: &Decode) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        opt(&d.slot.map(ms)),
        ms(d.at),
        d.snr,
        d.hz,
        opt(&d.dt),
        text(&d.text)
    )
}

fn parse_decode(w: &[&str]) -> Option<Decode> {
    let [slot, at, snr, hz, dt, t] = w else {
        return None;
    };
    Some(Decode {
        slot: match *slot {
            "-" => None,
            s => Some(time(s)?),
        },
        at: time(at)?,
        snr: snr.parse().ok()?,
        hz: hz.parse().ok()?,
        dt: parse_opt(dt)?,
        text: t.to_string(),
        fields: ftxl::fields(t),
    })
}

pub fn command(c: &Command) -> Option<String> {
    Some(match c {
        Command::Start => "START".into(),
        Command::Stop => "STOP".into(),
        Command::Halt => "HALT".into(),
        Command::Setup(s) => format!("SETUP\t{}", setup(s)),
        Command::Select(d) => format!("SELECT\t{}", decode(d)),
        Command::Dial(hz) => format!("DIAL\t{hz}"),
        _ => return None,
    })
}

pub fn parse_command(s: &str) -> Option<Command> {
    let w: Vec<&str> = s.split('\t').collect();
    Some(match w.as_slice() {
        ["START"] => Command::Start,
        ["STOP"] => Command::Stop,
        ["HALT"] => Command::Halt,
        ["SETUP", rest @ ..] => Command::Setup(Box::new(parse_setup(rest)?)),
        ["SELECT", rest @ ..] => Command::Select(Box::new(parse_decode(rest)?)),
        ["DIAL", hz] => Command::Dial(hz.parse().ok()?),
        _ => return None,
    })
}

pub fn notice(n: &Notice) -> Option<String> {
    Some(match n {
        Notice::Running(on) => format!("RUNNING\t{}", flag(*on)),
        Notice::Setup(s) => format!("SETUP\t{}", setup(s)),
        Notice::Decode(d) => format!("DECODE\t{}", decode(d)),
        Notice::Spot(s) => format!(
            "SPOT\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            s.mode.id(),
            text(&s.callsign),
            s.grid.as_deref().map_or_else(|| "-".into(), text),
            s.snr,
            s.hz,
            opt(&s.dial_hz),
            ms(s.at)
        ),
        Notice::Pick(p) => format!(
            "PICK\t{}\t{}\t{}",
            text(&p.callsign),
            p.grid.as_deref().map_or_else(|| "-".into(), text),
            p.hz
        ),
        Notice::QsoCompleted(q) => format!(
            "QSO\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            q.mode.id(),
            text(&q.callsign),
            text(&q.grid),
            text(&q.sent),
            text(&q.rcvd),
            q.hz,
            opt(&q.dial_hz),
            ms(q.at)
        ),
        Notice::Halt => "HALT".into(),
        Notice::Ptt(on) => format!("PTT\t{}", flag(*on)),
        Notice::Tune(hz) => format!("TUNE\t{hz}"),
        Notice::Warnings(w) => {
            let w: Vec<String> = w
                .iter()
                .map(|w| match w {
                    Warning::ClockSkew(s) => format!("skew:{s}"),
                    Warning::Latency(d) => format!("latency:{}", d.as_millis()),
                })
                .collect();
            format!("WARNINGS\t{}", w.join(" "))
        }
        Notice::Error(e) => format!("ERROR\t{}", text(e)),
        Notice::Burst(_) => return None,
    })
}

fn grid(s: &str) -> Option<String> {
    (s != "-").then(|| s.to_string())
}

pub fn parse_notice(s: &str) -> Option<Notice> {
    let w: Vec<&str> = s.split('\t').collect();
    Some(match w.as_slice() {
        ["RUNNING", on] => Notice::Running(parse_flag(on)?),
        ["SETUP", rest @ ..] => Notice::Setup(Box::new(parse_setup(rest)?)),
        ["DECODE", rest @ ..] => Notice::Decode(parse_decode(rest)?),
        ["SPOT", m, call, g, snr, hz, dial, at] => Notice::Spot(Spot {
            mode: proto(m)?,
            callsign: call.to_string(),
            grid: grid(g),
            snr: snr.parse().ok()?,
            hz: hz.parse().ok()?,
            dial_hz: parse_opt(dial)?,
            at: time(at)?,
        }),
        ["PICK", call, g, hz] => Notice::Pick(Pick {
            callsign: call.to_string(),
            grid: grid(g),
            hz: hz.parse().ok()?,
        }),
        ["QSO", m, call, g, sent, rcvd, hz, dial, at] => Notice::QsoCompleted(Qso {
            mode: proto(m)?,
            callsign: call.to_string(),
            grid: g.to_string(),
            sent: sent.to_string(),
            rcvd: rcvd.to_string(),
            hz: hz.parse().ok()?,
            dial_hz: parse_opt(dial)?,
            at: time(at)?,
        }),
        ["HALT"] => Notice::Halt,
        ["PTT", on] => Notice::Ptt(parse_flag(on)?),
        ["TUNE", hz] => Notice::Tune(hz.parse().ok()?),
        ["WARNINGS", list] => Notice::Warnings(
            list.split(' ')
                .filter(|w| !w.is_empty())
                .filter_map(|w| match w.split_once(':')? {
                    ("skew", s) => Some(Warning::ClockSkew(s.parse().ok()?)),
                    ("latency", m) => {
                        Some(Warning::Latency(Duration::from_millis(m.parse().ok()?)))
                    }
                    _ => None,
                })
                .collect(),
        ),
        ["ERROR", e] => Notice::Error(e.to_string()),
        _ => return None,
    })
}

pub struct Host {
    channel: Channel,
    engine: ftxl::Engine,
    io: Option<Box<dyn AudioIo>>,
    link: PttLink,
    clock: ftxl_ui::Clock,
    radio: Option<u8>,
    rigs: Rigs,
    peer: Option<String>,
}

type Rigs = Box<dyn Fn(usize) -> Option<u8> + Send>;

impl Host {
    pub fn new(channel: Channel, hub: TxHub) -> Self {
        Host {
            channel,
            engine: ftxl::Engine::new(),
            io: None,
            link: PttLink::new(hub),
            clock: ftxl_ui::Clock::default(),
            radio: None,
            rigs: Box::new(zlog_remote::modem::lent_slot),
            peer: None,
        }
    }

    pub fn with_rigs(mut self, rigs: impl Fn(usize) -> Option<u8> + Send + 'static) -> Self {
        self.rigs = Box::new(rigs);
        self
    }

    pub fn radio(&self) -> Option<u8> {
        self.radio
    }

    pub fn peer(&self) -> Option<&str> {
        self.peer.as_deref()
    }

    pub fn active(&self) -> bool {
        self.peer.is_some() || self.link.active()
    }

    fn reset(&mut self) {
        self.engine = ftxl::Engine::new();
        self.engine.handle(Command::PttLead(Some(self.link.lead())));
        if let Some(io) = &self.io {
            self.engine.handle(Command::OutputRate(io.sample_rate()));
        }
        self.engine.take();
    }

    fn close(&mut self) {
        self.engine.handle(Command::Halt);
        self.engine.handle(Command::Stop);
        self.engine.take();
        self.link.ptt(false, self.radio.unwrap_or(1));
        self.radio = None;
        if let Some(io) = self.io.as_mut() {
            io.stop();
        }
        self.io = None;
        self.peer = None;
    }

    pub fn poll(
        &mut self,
        open: impl FnOnce() -> Result<Box<dyn AudioIo>, String>,
        now: SystemTime,
    ) {
        let mut open = Some(open);
        for i in self.channel.take() {
            match i {
                Inbound::Open(p) => {
                    self.close();
                    self.peer = Some(p);
                    self.reset();
                }
                Inbound::Closed => self.close(),
                Inbound::Line(_) if self.peer.is_none() => {}
                Inbound::Line(l) if l == RESET => self.reset(),
                Inbound::Line(l) => {
                    if let Some(i) = l
                        .strip_prefix(RIG)
                        .and_then(|n| n.trim().parse::<usize>().ok())
                    {
                        self.radio = (self.rigs)(i);
                        continue;
                    }
                    let Some(c) = parse_command(&l) else {
                        continue;
                    };
                    if matches!(c, Command::Start) && self.io.is_none() {
                        match open.take().map(|f| f()) {
                            Some(Ok(mut io)) => {
                                io.record(&mut Vec::new());
                                self.engine.handle(Command::OutputRate(io.sample_rate()));
                                self.clock = ftxl_ui::Clock::default();
                                self.io = Some(io);
                            }
                            Some(Err(e)) => {
                                self.channel
                                    .say(&notice(&Notice::Error(e)).unwrap_or_default());
                                continue;
                            }
                            None => {}
                        }
                    }
                    self.engine.handle(c);
                }
            }
        }
        if self.peer.is_none() && !self.link.active() {
            return;
        }
        if let Some(io) = self.io.as_mut() {
            let mut samples = Vec::new();
            io.record(&mut samples);
            if !samples.is_empty() {
                let rate = io.sample_rate();
                let at = self.clock.stamp(samples.len(), rate, now);
                self.engine.handle(Command::Samples { rate, samples, at });
            }
        }
        self.engine.handle(Command::Tick(now));
        loop {
            for k in self.link.poll() {
                self.engine.handle(match k {
                    Keying::Keyed(on) => Command::Keyed(on),
                    Keying::Failed(e) => Command::KeyFailed(e),
                    Keying::Halt => Command::Halt,
                });
            }
            let notices = self.engine.take();
            if notices.is_empty() {
                break;
            }
            for n in notices {
                match &n {
                    Notice::Burst(b) => {
                        if let Some(io) = self.io.as_mut() {
                            io.play(&b.lead_in(SystemTime::now()));
                        }
                    }
                    Notice::Halt => {
                        if let Some(io) = self.io.as_mut() {
                            io.stop();
                        }
                    }
                    Notice::Ptt(true) if self.radio.is_none() => {
                        self.engine.handle(Command::KeyFailed(NO_RIG.into()));
                    }
                    Notice::Ptt(on) => self.link.ptt(*on, self.radio.unwrap_or(1)),
                    _ => {}
                }
                if self.peer.is_some() {
                    if let Some(l) = notice(&n) {
                        self.channel.say(&l);
                    }
                }
            }
        }
    }
}

pub struct Remote {
    channel: RemoteChannel,
    rig: Option<usize>,
}

impl Remote {
    pub fn new(channel: RemoteChannel) -> Self {
        let _ = channel.send(RESET);
        Remote { channel, rig: None }
    }

    pub fn connected(&self) -> bool {
        self.channel.connected()
    }

    pub fn exchange(&mut self, inner: &mut ftxl_ui::FtPanel, rig: Option<usize>) {
        if self.rig != rig {
            self.rig = rig;
            if let Some(i) = rig {
                let _ = self.channel.send(&format!("{RIG} {i}"));
            }
        }
        for c in inner.take_commands() {
            if let Some(l) = command(&c) {
                let _ = self.channel.send(&l);
            }
        }
        for l in self.channel.recv() {
            if let Some(n) = parse_notice(&l) {
                inner.notify(n);
            }
        }
    }
}

pub fn offer() -> Channel {
    zlog_remote::modem::offer(NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_notices_survive_the_wire() {
        let t = UNIX_EPOCH + Duration::from_millis(1_761_900_000_000);
        let mut s = Setup {
            proto: Proto::Ft4,
            parity: Parity::Odd,
            ..Setup::default()
        };
        s.seq.my_call = "JA1ZLO".into();
        s.seq.cq = "CQ TEST".into();
        s.seq.rcvd = Some("-12".into());
        s.options.rx_hz = Some(812.0);
        let d = Decode {
            slot: Some(t),
            at: t,
            snr: -12.0,
            hz: 1500.0,
            dt: Some(0.2),
            text: "CQ JA1YWX PM95".into(),
            fields: ftxl::fields("CQ JA1YWX PM95"),
        };
        for c in [
            Command::Start,
            Command::Stop,
            Command::Halt,
            Command::Dial(7_074_000),
            Command::Setup(Box::new(s.clone())),
            Command::Select(Box::new(d.clone())),
        ] {
            assert_eq!(parse_command(&command(&c).unwrap()), Some(c));
        }
        assert!(command(&Command::Keyed(true)).is_none(), "the host keys");
        for n in [
            Notice::Running(true),
            Notice::Setup(Box::new(s)),
            Notice::Decode(d),
            Notice::Spot(Spot {
                mode: Proto::Ft8,
                callsign: "JA1YWX".into(),
                grid: None,
                snr: -3.0,
                hz: 900.0,
                dial_hz: Some(14_074_000),
                at: t,
            }),
            Notice::Pick(Pick {
                callsign: "JA1YWX".into(),
                grid: Some("PM95".into()),
                hz: 900.0,
            }),
            Notice::QsoCompleted(Qso {
                mode: Proto::Ft8,
                callsign: "JA1YWX".into(),
                grid: "PM95".into(),
                sent: "-09".into(),
                rcvd: "R-14".into(),
                hz: 900.0,
                dial_hz: None,
                at: t,
            }),
            Notice::Halt,
            Notice::Ptt(false),
            Notice::Tune(7_074_000),
            Notice::Warnings(vec![
                Warning::ClockSkew(0.5),
                Warning::Latency(Duration::from_millis(800)),
            ]),
            Notice::Error("no sound".into()),
        ] {
            assert_eq!(parse_notice(&notice(&n).unwrap()), Some(n));
        }
    }

    #[derive(Clone, Default)]
    struct Keys(std::sync::Arc<std::sync::Mutex<Vec<bool>>>);

    impl zlog_ui::tx::Keyer for Keys {
        fn caps(&self) -> zlog_ui::tx::KeyerCaps {
            zlog_ui::tx::KeyerCaps::default()
        }
        fn send(&mut self, _text: &str) {}
        fn abort(&mut self) {}
        fn set_wpm(&mut self, _wpm: u32) {}
        fn wpm(&self) -> u32 {
            25
        }
        fn set_ptt(&mut self, on: bool) {
            self.0.lock().unwrap().push(on);
        }
        fn busy(&self) -> bool {
            false
        }
        fn poll(&mut self) -> Vec<zlog_ui::tx::KeyerEvent> {
            Vec::new()
        }
    }

    #[test]
    fn a_client_runs_ft4_on_the_server_rig_and_sound_card() {
        use ftxl::sim::{render, Band, Source, Station as Sender};
        use haml::remote::{pair, Client, Limits, Server, Station};
        use zlog_ui::device::{AudioDrv, PortHub};
        let hub = TxHub::default();
        let keys = Keys::default();
        hub.set_keyer(Some(Box::new(keys.clone())));
        let channel = Channel::new(NAME);
        let station = Station {
            rigs: Vec::new(),
            tx: hub.device(),
            tx_devices: Vec::new(),
            ports: None,
            lent: None,
            channels: vec![channel.clone()],
        };
        let server = Server::new(station, Limits::default());
        let (near, far) = pair("client", "server");
        server.attach(Box::new(far));
        let client = Client::attach(Box::new(near)).unwrap();
        let listing = client.list(Duration::from_secs(5)).unwrap();
        let i = listing.channels.iter().position(|c| c == NAME).unwrap();
        let drv = zlog_ui::loopback::LoopbackDrv::default();
        let mut section = zlog_ui::Section::new();
        section.insert("rate".into(), "12000".into());
        let mut host = Host::new(channel, hub.clone()).with_rigs(|i| [1, 2].get(i).copied());
        let mut inner = ftxl_ui::FtPanel::remote("en");
        inner.proto = Proto::Ft4;
        inner.my_call = "JA1ZLO".into();
        inner.my_grid = "PM95".into();
        let mut remote = Remote::new(client.channel(i));
        let period = Proto::Ft4.period();
        let base = ftxl::next_slot(SystemTime::now() + period, period, Parity::Odd);
        let t0 = std::time::Instant::now();
        let mut origin = base - period;
        let mut step = |host: &mut Host, inner: &mut ftxl_ui::FtPanel, origin: SystemTime| {
            remote.exchange(inner, Some(1));
            let now = origin + t0.elapsed();
            host.poll(|| drv.open(&section, &PortHub::new()), now);
            std::thread::sleep(Duration::from_millis(10));
            remote.exchange(inner, Some(1));
        };
        inner.start();
        while host.peer().is_none() || !inner.running() {
            assert!(t0.elapsed() < Duration::from_secs(5));
            step(&mut host, &mut inner, origin);
        }
        let band = Band::slot(Source::Ft4);
        let cq = Sender::new("CQ JA1YWX PM95", 1200.0, 0.0, -5.0);
        let audio = render(&band, Source::Ft4, &[cq], 3).unwrap();
        let fed = t0.elapsed();
        drv.feed.lock().unwrap().extend(audio);
        origin = base - fed;
        let mut heard = Vec::new();
        while !heard.iter().any(|t: &String| t.contains("CQ JA1YWX")) {
            assert!(t0.elapsed() < Duration::from_secs(30), "{heard:?}");
            step(&mut host, &mut inner, origin);
            for e in inner.take_events() {
                if let ftxl_ui::Event::Decode(d) = e {
                    heard.push(d.text);
                }
            }
        }
        assert_eq!(host.radio(), Some(2));
        inner.seq.enabled = true;
        let now = origin + t0.elapsed();
        let slot = ftxl::next_slot(now, period, inner.parity);
        origin += slot.duration_since(now).unwrap() - Duration::from_millis(100);
        while hub.transmitting().is_none() {
            assert!(t0.elapsed() < Duration::from_secs(40), "never keyed");
            step(&mut host, &mut inner, origin);
        }
        assert_eq!(hub.transmitting(), Some(2), "keyed the server's rig 2");
        assert!(keys.0.lock().unwrap().contains(&true));
        client.close();
        while host.active() || hub.transmitting().is_some() {
            assert!(t0.elapsed() < Duration::from_secs(50), "TX kept on");
            host.poll(|| Err("closed".into()), SystemTime::now());
            hub.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(keys.0.lock().unwrap().last(), Some(&false));
    }
}
