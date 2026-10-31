use std::time::{Duration, SystemTime, UNIX_EPOCH};

use haml::remote::{Channel, Inbound, RemoteChannel};
use rtty::{Command, Decode, Notice, RttyOptions, Timing, Unit};
use zlog_ui::device::AudioIo;
use zlog_ui::tx::{Keying, PttLink, TxHub};

pub const NAME: &str = "rtty";
pub const RIG: &str = "RIG";
pub const NO_RIG: &str = "no shared rig to key";
pub const RESET: &str = "RESET";

fn ms(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

fn at(ms: &str) -> Option<SystemTime> {
    Some(UNIX_EPOCH + Duration::from_millis(ms.parse().ok()?))
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

fn unit(u: Unit) -> &'static str {
    match u {
        Unit::Message => "M",
        Unit::Word => "W",
        Unit::LineBreak => "L",
    }
}

fn parse_unit(s: &str) -> Option<Unit> {
    Some(match s {
        "M" => Unit::Message,
        "W" => Unit::Word,
        "L" => Unit::LineBreak,
        _ => return None,
    })
}

pub fn command(c: &Command) -> Option<String> {
    Some(match c {
        Command::Start => "START".into(),
        Command::Stop => "STOP".into(),
        Command::Halt => "HALT".into(),
        Command::Send { text, at } => format!("SEND\t{}\t{text}", ms(*at)),
        Command::Dial(hz) => format!("DIAL\t{hz}"),
        Command::Options(o) => format!(
            "OPTIONS\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            o.mark,
            o.shift,
            o.baud,
            flag(o.reverse),
            flag(o.uos),
            o.squelch,
            flag(o.atc),
            flag(o.blanker)
        ),
        Command::Timing(t) => format!(
            "TIMING\t{}\t{}\t{}",
            flag(t.ptt),
            t.before.as_millis(),
            t.after.as_millis()
        ),
        _ => return None,
    })
}

pub fn parse_command(s: &str) -> Option<Command> {
    let w: Vec<&str> = s.split('\t').collect();
    Some(match w.as_slice() {
        ["START"] => Command::Start,
        ["STOP"] => Command::Stop,
        ["HALT"] => Command::Halt,
        ["SEND", t, text] => Command::Send {
            text: text.to_string(),
            at: at(t)?,
        },
        ["DIAL", hz] => Command::Dial(hz.parse().ok()?),
        ["OPTIONS", mark, shift, baud, rev, uos, sq, atc, nb] => Command::Options(RttyOptions {
            mark: mark.parse().ok()?,
            shift: shift.parse().ok()?,
            baud: baud.parse().ok()?,
            reverse: parse_flag(rev)?,
            uos: parse_flag(uos)?,
            squelch: sq.parse().ok()?,
            atc: parse_flag(atc)?,
            blanker: parse_flag(nb)?,
        }),
        ["TIMING", ptt, before, after] => Command::Timing(Timing {
            ptt: parse_flag(ptt)?,
            before: Duration::from_millis(before.parse().ok()?),
            after: Duration::from_millis(after.parse().ok()?),
        }),
        _ => return None,
    })
}

pub fn notice(n: &Notice) -> Option<String> {
    Some(match n {
        Notice::Running(on) => format!("RUNNING\t{}", flag(*on)),
        Notice::Busy(on) => format!("BUSY\t{}", flag(*on)),
        Notice::Ptt(on) => format!("PTT\t{}", flag(*on)),
        Notice::Halt => "HALT".into(),
        Notice::Dial(hz) => format!("DIAL\t{hz}"),
        Notice::Error(e) => format!("ERROR\t{e}"),
        Notice::Decode(d) => format!(
            "DECODE\t{}\t{}\t{}\t{}\t{}",
            ms(d.at),
            d.snr,
            d.hz,
            unit(d.unit),
            d.text
        ),
        Notice::Burst(_) => return None,
    })
}

pub fn parse_notice(s: &str) -> Option<Notice> {
    let w: Vec<&str> = s.split('\t').collect();
    Some(match w.as_slice() {
        ["RUNNING", on] => Notice::Running(parse_flag(on)?),
        ["BUSY", on] => Notice::Busy(parse_flag(on)?),
        ["PTT", on] => Notice::Ptt(parse_flag(on)?),
        ["HALT"] => Notice::Halt,
        ["DIAL", hz] => Notice::Dial(hz.parse().ok()?),
        ["ERROR", e] => Notice::Error(e.to_string()),
        ["DECODE", t, snr, hz, u, text] => Notice::Decode(Decode {
            at: at(t)?,
            snr: snr.parse().ok()?,
            hz: hz.parse().ok()?,
            unit: parse_unit(u)?,
            text: text.to_string(),
        }),
        _ => return None,
    })
}

pub struct Host {
    channel: Channel,
    engine: rtty::Engine,
    io: Option<Box<dyn AudioIo>>,
    link: PttLink,
    clock: rtty_ui::Clock,
    radio: Option<u8>,
    rigs: Rigs,
    peer: Option<String>,
}

type Rigs = Box<dyn Fn(usize) -> Option<u8> + Send>;

impl Host {
    pub fn new(channel: Channel, hub: TxHub) -> Self {
        Host {
            channel,
            engine: rtty::Engine::new(),
            io: None,
            link: PttLink::new(hub),
            clock: rtty_ui::Clock::default(),
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
        self.engine = rtty::Engine::new();
        self.engine.handle(Command::PttLead(Some(self.link.lead())));
        self.engine.take();
        if let Some(io) = &self.io {
            self.engine.handle(Command::OutputRate(io.sample_rate()));
        }
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
                    let wants_audio = matches!(c, Command::Start | Command::Send { .. });
                    if wants_audio && self.io.is_none() {
                        match open.take().map(|f| f()) {
                            Some(Ok(mut io)) => {
                                io.record(&mut Vec::new());
                                self.engine.handle(Command::OutputRate(io.sample_rate()));
                                self.clock = rtty_ui::Clock::default();
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
            if self.engine.running() && !samples.is_empty() {
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

    pub fn exchange(&mut self, inner: &mut rtty_ui::RttyPanel, rig: Option<usize>) {
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
        let t = UNIX_EPOCH + Duration::from_millis(1_761_900_000_123);
        for c in [
            Command::Start,
            Command::Stop,
            Command::Halt,
            Command::Send {
                text: "CQ TEST JA1ZLO\tK".replace('\t', " "),
                at: t,
            },
            Command::Dial(14_080_000),
            Command::Options(RttyOptions {
                reverse: true,
                squelch: 0.25,
                ..RttyOptions::default()
            }),
            Command::Timing(Timing::default()),
        ] {
            assert_eq!(parse_command(&command(&c).unwrap()), Some(c));
        }
        assert!(command(&Command::Keyed(true)).is_none(), "the host keys");
        assert!(command(&Command::PttLead(None)).is_none());
        for n in [
            Notice::Running(true),
            Notice::Busy(false),
            Notice::Ptt(true),
            Notice::Halt,
            Notice::Dial(7_080_000),
            Notice::Error("no sound".into()),
            Notice::Decode(Decode {
                at: t,
                snr: 12.5,
                hz: 2125.0,
                text: "JA1ZLO 599".into(),
                unit: Unit::Word,
            }),
        ] {
            assert_eq!(parse_notice(&notice(&n).unwrap()), Some(n));
        }
        assert_eq!(parse_command("KEYED\t1"), None);
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
    fn a_client_runs_rtty_on_the_server_rig_and_sound_card() {
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
        section.insert("rate".into(), "8000".into());
        let mut host = Host::new(channel, hub.clone()).with_rigs(|i| [1, 2].get(i).copied());
        let mut inner = rtty_ui::RttyPanel::remote("en");
        inner.options.baud = 100.0;
        let mut remote = Remote::new(client.channel(i));
        inner.start();
        inner.send("CQ TEST JA1ZLO ");
        let mut heard = String::new();
        let start = std::time::Instant::now();
        let step = |host: &mut Host, inner: &mut rtty_ui::RttyPanel, remote: &mut Remote| {
            remote.exchange(inner, Some(1));
            host.poll(|| drv.open(&section, &PortHub::new()), SystemTime::now());
            std::thread::sleep(Duration::from_millis(10));
            remote.exchange(inner, Some(1));
        };
        while !heard.contains("CQ TEST JA1ZLO") {
            assert!(start.elapsed() < Duration::from_secs(20), "{heard:?}");
            step(&mut host, &mut inner, &mut remote);
            for e in inner.take_events() {
                if let rtty_ui::Event::Decode(d) = e {
                    heard.push_str(&d.text);
                    heard.push(' ');
                }
            }
        }
        assert!(host.peer().is_some());
        assert_eq!(host.radio(), Some(2), "lent rig 2 is the server's rig 2");
        assert!(
            keys.0.lock().unwrap().contains(&true),
            "the server rig was keyed"
        );
        inner.send(&"RYRYRYRYRY ".repeat(20));
        while hub.transmitting().is_none() {
            assert!(start.elapsed() < Duration::from_secs(30));
            step(&mut host, &mut inner, &mut remote);
        }
        assert_eq!(hub.transmitting(), Some(2));
        client.close();
        while host.active() || hub.transmitting().is_some() {
            assert!(start.elapsed() < Duration::from_secs(30), "TX kept on");
            host.poll(|| Err("closed".into()), SystemTime::now());
            hub.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(keys.0.lock().unwrap().last(), Some(&false));
        assert!(host.peer().is_none());
    }
}
