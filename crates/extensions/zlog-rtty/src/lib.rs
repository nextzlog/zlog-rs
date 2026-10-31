pub mod driver;
pub mod heard;
pub mod remote;
mod settings;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use egui::Ui;
use rtty_ui::Event;
use zlog_core::Mode;
use zlog_macro as macros;
use zlog_ui::device::{AudioDrv, AudioIo, DeviceDriver, DriverChoice, PortHub};
use zlog_ui::spot::{heard_hz, Sideband, SpotFeed};
use zlog_ui::tx::{Keying, PttLink, TxHub};
use zlog_ui::{EntryAction, EntryKey, Panel, Section, Spot, View};

pub use rtty;
pub use settings::{
    device_ui, load_panel, options_of, save_panel, RttyOptions, Settings, DEFAULT_MESSAGES,
    DEFAULT_SP_MESSAGES, SECTION, SOUND,
};

zlog_core::texts!();

pub const SOURCE: &str = "RTTY";

pub struct RttyPanel {
    inner: rtty_ui::RttyPanel,
    link: PttLink,
    lead: Option<Duration>,
    dial: Option<u64>,
    radio: u8,
    settings: Settings,
    fkeys: bool,
    drivers: Vec<Box<dyn AudioDrv>>,
    ports: PortHub,
    device: DriverChoice,
    io: Option<Box<dyn AudioIo>>,
    clock: rtty_ui::Clock,
    status: String,
    actions: Vec<EntryAction>,
    feed: SpotFeed,
    host: Option<remote::Host>,
    busy: bool,
    remote: Option<remote::Remote>,
}

impl Default for RttyPanel {
    fn default() -> Self {
        RttyPanel::on(TxHub::default())
    }
}

impl RttyPanel {
    pub fn on(hub: TxHub) -> Self {
        let mut inner = rtty_ui::RttyPanel::new("en");
        inner.host_sends = true;
        inner.host_call = true;
        inner.tools.settings = false;
        inner.tools.messages = false;
        let settings = Settings::default();
        settings.apply(&mut inner);
        load_panel(&mut inner, &Section::new());
        RttyPanel {
            inner,
            link: PttLink::new(hub),
            lead: None,
            dial: None,
            radio: 1,
            settings,
            fkeys: true,
            drivers: Vec::new(),
            ports: PortHub::new(),
            device: DriverChoice::new(SOUND),
            io: None,
            clock: rtty_ui::Clock::default(),
            status: String::new(),
            actions: Vec::new(),
            feed: SpotFeed::default(),
            host: None,
            busy: false,
            remote: None,
        }
    }
}

fn is_callsign(c: &str) -> bool {
    let body = c.split('/').max_by_key(|p| p.len()).unwrap_or("");
    (3..=12).contains(&c.len())
        && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '/')
        && body.chars().any(|ch| ch.is_ascii_digit())
        && body.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn now() -> (u64, u32) {
    use chrono::Timelike;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (secs, chrono::Local::now().hour())
}

pub fn expand(template: &str, view: &View<'_>, leading_zeros: bool) -> String {
    let (secs, hour) = now();
    let fields = macros::fields(view, secs, hour, leading_zeros, &macros::GREETING_HOURS);
    macros::expand_rtty(template, &fields)
}

impl RttyPanel {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>, ports: PortHub, hub: TxHub) -> Self {
        RttyPanel {
            drivers,
            ports,
            ..RttyPanel::on(hub)
        }
    }

    pub fn hosting(mut self, channel: haml::remote::Channel, hub: TxHub) -> Self {
        self.host = Some(remote::Host::new(channel, hub));
        self
    }

    fn wants_remote(&self) -> bool {
        self.device.find(&self.drivers).is_some_and(|d| d.remote())
    }

    fn rebuild(&mut self, remote: bool) {
        let lang = self.inner.language.clone();
        let mut p = match remote {
            true => rtty_ui::RttyPanel::remote(&lang),
            false => rtty_ui::RttyPanel::new(&lang),
        };
        p.host_sends = true;
        p.host_call = true;
        p.tools = self.inner.tools;
        for (k, v) in self.inner.settings() {
            p.load_setting(k, &v);
        }
        self.settings.apply(&mut p);
        p.bank = self.inner.bank;
        p.my_call = self.inner.my_call.clone();
        self.inner = p;
        self.lead = None;
        self.dial = None;
    }

    fn follow_remote(&mut self) {
        let want = self.wants_remote();
        if self.remote.as_ref().is_some_and(|r| !r.connected()) {
            self.remote = None;
            self.rebuild(false);
        }
        match (want, self.remote.is_some()) {
            (true, false) => match zlog_remote::modem::open(&zlog_remote::shared(), remote::NAME) {
                Ok(ch) => {
                    self.io = None;
                    self.rebuild(true);
                    self.remote = Some(remote::Remote::new(ch));
                    self.status.clear();
                }
                Err(e) => self.status = e,
            },
            (false, true) => {
                self.remote = None;
                self.rebuild(false);
            }
            _ => {}
        }
    }

    fn open_io(&self) -> Result<Box<dyn AudioIo>, String> {
        let drv = self
            .device
            .find(&self.drivers)
            .filter(|d| !d.remote())
            .ok_or_else(|| texts().get("en", "no_device").to_string())?;
        drv.open(&self.device.section(), &self.ports)
    }

    fn serve(&mut self) {
        let Some(mut host) = self.host.take() else {
            return;
        };
        host.poll(|| self.open_io(), SystemTime::now());
        if let Some(peer) = host.peer() {
            let busy = texts().format(&self.inner.language, "remote_busy", &[&peer]);
            if self.io.is_some() || self.inner.running() {
                self.inner.halt();
                self.inner.stop();
                self.io = None;
            }
            self.status = busy;
            self.busy = true;
        } else if std::mem::take(&mut self.busy) {
            self.status.clear();
        }
        self.host = Some(host);
    }

    fn open(&mut self) -> Result<(), String> {
        if self.remote.is_some() {
            return Ok(());
        }
        if let Some(peer) = self.host.as_ref().and_then(|h| h.peer()) {
            return Err(texts().format(&self.inner.language, "remote_busy", &[&peer]));
        }
        if self.io.is_none() {
            let drv = self
                .device
                .find(&self.drivers)
                .ok_or_else(|| texts().get("en", "no_device").to_string())?;
            let mut io = drv.open(&self.device.section(), &self.ports)?;
            io.record(&mut Vec::new());
            self.inner.set_output_rate(io.sample_rate());
            self.clock = rtty_ui::Clock::default();
            self.io = Some(io);
        }
        Ok(())
    }

    fn report(&mut self, r: Result<(), String>) {
        match r {
            Ok(()) => self.status.clear(),
            Err(e) => self.status = e,
        }
    }

    fn sync(&mut self, view: &View<'_>) {
        self.inner.language = view.language.into();
        self.inner.my_call = view.station.callsign.trim().to_ascii_uppercase();
        self.radio = view.rig.max(1);
        self.follow_remote();
        self.serve();
        if let Some(r) = self.remote.as_mut() {
            if let Some(hz) = view.hz.filter(|hz| self.dial != Some(*hz)) {
                self.dial = Some(hz);
                self.inner.set_dial(hz);
            }
            let rig = zlog_remote::modem::lent_index(&zlog_remote::shared(), self.radio);
            r.exchange(&mut self.inner, rig);
            return;
        }
        let lead = Some(self.link.lead());
        if self.lead != lead {
            self.lead = lead;
            self.inner.set_ptt_lead(lead);
        }
        if let Some(hz) = view.hz.filter(|hz| self.dial != Some(*hz)) {
            self.dial = Some(hz);
            self.inner.set_dial(hz);
        }
        if self.inner.sending() && self.io.is_none() {
            let r = self.open();
            self.report(r);
        }
        let Some(io) = self.io.as_mut() else {
            return;
        };
        let mut samples = Vec::new();
        io.record(&mut samples);
        if self.inner.running() && !samples.is_empty() {
            let rate = io.sample_rate();
            let at = self.clock.stamp(samples.len(), rate, SystemTime::now());
            self.inner.push_samples(rate, &samples, at);
        }
    }

    fn keying(&mut self) {
        for k in self.link.poll() {
            match k {
                Keying::Keyed(on) => self.inner.keyed(on),
                Keying::Failed(e) => self.inner.key_failed(&e),
                Keying::Halt => self.inner.halt(),
            }
        }
    }

    fn handle(&mut self, view: &View<'_>) {
        self.keying();
        if let Some(r) = self.remote.as_mut() {
            let rig = zlog_remote::modem::lent_index(&zlog_remote::shared(), self.radio);
            r.exchange(&mut self.inner, rig);
        }
        loop {
            let events = self.inner.take_events();
            if events.is_empty() {
                return;
            }
            for e in events {
                self.event(e, view);
            }
        }
    }

    fn event(&mut self, e: Event, view: &View<'_>) {
        match e {
            Event::Running(true) if !self.inner.playing_wav() => {
                let r = self.open();
                if r.is_err() {
                    self.inner.stop();
                }
                self.report(r);
            }
            Event::Running(false) if !self.inner.sending() => self.io = None,
            Event::Message { text, .. } => {
                let text = expand(&text, view, self.settings.leading_zeros);
                let r = self.open();
                if r.is_ok() {
                    self.inner.send(&text);
                }
                self.report(r);
            }
            Event::Pick(p) => self.actions.push(EntryAction::Callsign(p.callsign)),
            Event::Number(n) => self.actions.push(EntryAction::Number(n)),
            Event::Heard(h) => self.heard(&h, unix_now()),
            Event::Ptt(_) | Event::Burst(_) | Event::Halt if self.remote.is_some() => {}
            Event::Running(_) if self.remote.is_some() => {}
            Event::Ptt(on) => {
                if on {
                    let r = self.open();
                    self.report(r);
                }
                self.link.ptt(on, self.radio);
            }
            Event::Burst(b) => {
                if let Some(io) = self.io.as_mut() {
                    io.play(&b.lead_in(SystemTime::now()));
                }
            }
            Event::Halt => {
                if let Some(io) = self.io.as_mut() {
                    io.stop();
                }
            }
            Event::OpenWav => {
                let kind = texts().get(view.language, "wav.kind");
                if let Some(p) = zlog_ui::dialog::import_file(kind, &["wav"]) {
                    let r = self.inner.load_wav(&p);
                    self.report(r);
                }
            }
            _ => {}
        }
    }

    fn heard(&mut self, h: &rtty_ui::Heard, now: i64) {
        if !self.settings.scope {
            return;
        }
        let (Some(dial), Ok(m)) = (h.dial_hz, self.settings.filters.matcher()) else {
            return;
        };
        let Some((callsign, number)) = m.pass(h) else {
            return;
        };
        let o = self.inner.options;
        let sideband = match o.reverse {
            true => Sideband::Upper,
            false => Sideband::Lower,
        };
        let spot = Spot {
            callsign,
            number,
            hz: heard_hz(dial, o.mark as f64, 0.0, sideband),
            mode: Some(Mode::Rtty),
            time: now,
            source: SOURCE.into(),
            ..Spot::default()
        };
        if let Some(spot) = self.feed.offer(spot) {
            self.actions.push(EntryAction::Spot(spot));
        }
    }

    fn messages(&self, view: &View<'_>) -> Vec<String> {
        let z = self.settings.leading_zeros;
        self.settings
            .bank(self.inner.bank)
            .iter()
            .map(|m| expand(m, view, z))
            .collect()
    }

    fn function_key(&mut self, n: u8, view: &View<'_>) -> bool {
        let n = n as usize;
        let ok = (1..=12).contains(&n)
            && view.mode == Mode::Rtty
            && !self.settings.bank(self.inner.bank)[n - 1].trim().is_empty();
        if ok {
            self.sync(view);
            self.inner.play(n - 1);
            self.handle(view);
            if self.settings.auto_log && n == usize::from(self.settings.end_key) {
                self.auto_log(view);
            }
        }
        ok
    }

    fn auto_log(&mut self, view: &View<'_>) {
        let callsign = view.callsign.trim().to_ascii_uppercase();
        let probe = zlog_core::Qso {
            callsign: callsign.clone(),
            nr_rcvd: view.number.trim().to_ascii_uppercase(),
            band: view.band,
            mode: view.mode,
            power: view.power,
            ..zlog_core::Qso::default()
        };
        if !is_callsign(&callsign) || probe.nr_rcvd.is_empty() || !view.contest.valid_number(&probe)
        {
            return;
        }
        if view.log.find_dupe(&probe).is_some() {
            let text = texts().format(view.language, "auto_log.dupe", &[&callsign]);
            let said = zlog_ui::StatusItem::new(text).warning(true);
            self.actions.push(EntryAction::Say(said));
            return;
        }
        self.actions.push(EntryAction::LogEntry);
    }

    fn escape(&mut self, view: &View<'_>) -> bool {
        let sending = self.inner.sending();
        self.inner.halt();
        self.handle(view);
        sending
    }

    fn running(&self) -> bool {
        self.inner.running()
            || self.inner.sending()
            || self.inner.playing_wav()
            || self.link.active()
            || self.remote.is_some()
            || self.host.as_ref().is_some_and(remote::Host::active)
    }
}

impl Panel for RttyPanel {
    fn id(&self) -> &str {
        "rtty"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Digital, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        let t = |k| texts().get(language, k);
        let action = |id: &str, glyph, weight| {
            let mut c = zlog_ui::Command::action(
                id,
                texts().get(language, &format!("toolbar.{id}")),
                texts().get(language, &format!("toolbar.{id}.tip")),
            )
            .icon(glyph)
            .toolbar_key(format!("rtty.{id}"))
            .menu_label(texts().get(language, &format!("menu.{id}")))
            .section(zlog_ui::MenuSection::Digital, weight);
            c.menu = true;
            c
        };
        let mut send = action("send", zlog_ui::icon::PAPER_PLANE_TILT, 1);
        if self.inner.typed().trim().is_empty() {
            send = send.disabled_because(t("toolbar.send.none"));
        }
        vec![
            zlog_ui::Command::window(t("toolbar"), t("toolbar.tip"))
                .icon(zlog_ui::icon::TERMINAL_WINDOW),
            send,
            action("stop", zlog_ui::icon::STOP, 2),
        ]
    }

    fn run_command(&mut self, id: &str) {
        match id {
            "receive" if self.inner.running() => self.inner.stop(),
            "receive" => self.inner.start(),
            "send" => {
                let _ = self.open();
                self.inner.send_typed();
            }
            "stop" => self.inner.halt(),
            "clear" => self.inner.clear(),
            _ => {}
        }
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [640.0, 480.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        self.sync(view);
        if !self.status.is_empty() {
            zlog_ui::layout::status(ui, "", Some(&self.status));
        }
        if self.fkeys {
            let messages = self.messages(view);
            if let Some(n) = zlog_ui::fkeys::message_keys(ui, &messages) {
                self.function_key(n, view);
            }
        }
        self.inner.ui(ui);
        self.handle(view);
    }

    fn key_messages(&self, view: &View<'_>) -> Option<Vec<String>> {
        (view.mode == Mode::Rtty).then(|| self.messages(view))
    }

    fn tick_after(&self) -> Option<Duration> {
        if self.host.as_ref().is_some_and(remote::Host::active) {
            return Some(Duration::from_millis(20));
        }
        if self.running() {
            return Some(Duration::from_millis(50));
        }
        (self.host.is_some() && zlog_remote::modem::sharing()).then_some(Duration::from_millis(200))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.sync(view);
        if view.mode != Mode::Rtty && self.inner.sending() {
            self.inner.halt();
        }
        self.inner.tick(SystemTime::now());
        self.handle(view);
    }

    fn on_key(&mut self, key: EntryKey, view: &View<'_>) -> bool {
        match key {
            EntryKey::Function { n, .. } | EntryKey::Reserved { n } => self.function_key(n, view),
            EntryKey::Escape => self.escape(view),
            EntryKey::Typed => false,
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &Section) {
        self.load_with(section, &zlog_ui::Sections::new());
    }

    fn reads(&self) -> Vec<String> {
        vec![zlog_ui::device::SOUND_SECTION.into()]
    }

    fn load_with(&mut self, section: &Section, others: &zlog_ui::Sections) {
        self.settings = Settings::load(section);
        self.fkeys = zlog_ui::fkeys::shown(section);
        load_panel(&mut self.inner, section);
        self.settings.apply(&mut self.inner);
        let list: Vec<&dyn DeviceDriver> = self.drivers.iter().map(|d| &**d as _).collect();
        let sound = zlog_ui::device::sound_section(section, others);
        let device = DriverChoice::load(&sound, SOUND, &list);
        let (mut before, mut after) = (Section::new(), Section::new());
        self.device.save(&mut before);
        device.save(&mut after);
        self.device = device;
        if before != after {
            self.inner.halt();
            self.io = None;
            if self.inner.running() {
                let r = self.open();
                self.report(r);
            }
        }
    }

    fn save(&self, section: &mut Section) {
        self.settings.save(section);
        save_panel(&self.inner, section);
        self.device.save(section);
        zlog_ui::fkeys::set_shown(section, self.fkeys);
    }
}

pub fn with_remote(mut drivers: Vec<Box<dyn AudioDrv>>) -> Vec<Box<dyn AudioDrv>> {
    drivers.push(Box::new(zlog_remote::modem::RemoteAudioDrv));
    drivers
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.driver::<dyn zlog_ui::device::ModemDrv>(10, || Box::new(driver::RttyModemDrv));
    b.page(310, |ctx| {
        RttyOptions::new(with_remote(ctx.new_drivers().take()))
    });
    b.panel(440, |ctx| {
        RttyPanel::new(
            with_remote(ctx.new_drivers().take()),
            ctx.ports.clone(),
            zlog_ui::tx::hub(),
        )
        .hosting(remote::offer(), zlog_ui::tx::hub())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::{Band, Contest, ContestMode, Log, Power, Qso, ScoreSheet, Scoring, Station};

    struct Nothing;

    impl Contest for Nothing {
        fn name(&self) -> &str {
            "test"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::B50)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Rtty
        }
        fn sent_number(&self, _band: Band, _power: Power, _station: &Station) -> String {
            "100110M".into()
        }
        fn valid_number(&self, _q: &Qso) -> bool {
            true
        }
        fn score(&mut self, _log: &Log) -> Scoring {
            Scoring {
                qsos: Vec::new(),
                sheet: ScoreSheet::default(),
            }
        }
    }

    fn with_view<R>(hz: Option<u64>, f: impl FnOnce(&View<'_>) -> R) -> R {
        with_mode(hz, Mode::Rtty, f)
    }

    fn with_mode<R>(hz: Option<u64>, mode: Mode, f: impl FnOnce(&View<'_>) -> R) -> R {
        with_entry(hz, mode, "", &Log::default(), f)
    }

    fn with_entry<R>(
        hz: Option<u64>,
        mode: Mode,
        number: &str,
        log: &Log,
        f: impl FnOnce(&View<'_>) -> R,
    ) -> R {
        let sheet = ScoreSheet::default();
        let station = Station {
            callsign: "JA1ZLO".into(),
            ..Station::default()
        };
        let view = View {
            log,
            contest: &Nothing,
            sheet: &sheet,
            callsign: "JA1YWX",
            station: &station,
            number,
            memo: "",
            rst_sent: "599",
            power: Power::P010,
            band: Band::B14,
            mode,
            language: "en",
            spots: &[],
            hz,
            rig: 1,
            cq: false,
            operator: "",
            period_hours: 0,
            pc_name: "",
            log_path: "",
            focus: None,
            editing: None,
            selected: None,
            period: &(),
            tab: Default::default(),
            changes: 0,
        };
        f(&view)
    }

    fn loopback() -> RttyPanel {
        let mut p = RttyPanel::new(
            vec![Box::new(zlog_ui::loopback::LoopbackDrv::default())],
            PortHub::new(),
            TxHub::default(),
        );
        let mut s = Section::new();
        for (k, v) in [
            ("device", "loopback"),
            ("loopback.rate", "8000"),
            ("baud", "100"),
            ("ptt", "false"),
        ] {
            s.insert(k.into(), v.into());
        }
        p.load(&s);
        p
    }

    fn run_until(p: &mut RttyPanel, done: impl Fn(&RttyPanel) -> bool) {
        let start = std::time::Instant::now();
        while !done(p) {
            assert!(start.elapsed() < Duration::from_secs(10), "too slow");
            with_view(None, |v| p.tick(v));
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn lines(p: &RttyPanel) -> Vec<String> {
        let t = p.inner.terminal();
        t.all_lines().map(|l| l.trim_end().to_string()).collect()
    }

    #[test]
    fn the_console_decodes_what_it_sends_through_the_loopback() {
        let mut p = loopback();
        p.inner.start();
        with_view(None, |v| p.tick(v));
        assert!(p.io.is_some() && p.inner.running(), "{}", p.status);
        p.inner.send("cq test de jr1abc");
        run_until(&mut p, |p| {
            lines(p).iter().any(|l| l == "CQ TEST DE JR1ABC")
        });
        assert!(lines(&p).iter().all(|l| !l.starts_with(' ')));
    }

    #[test]
    fn f_keys_expand_the_message_and_escape_halts() {
        let mut p = loopback();
        p.inner.start();
        with_view(None, |v| {
            p.tick(v);
            assert!(p.on_key(EntryKey::Function { n: 2, shift: false }, v));
            assert!(p.inner.sending());
            assert!(!p.on_key(EntryKey::Function { n: 4, shift: false }, v));
            assert!(p.on_key(EntryKey::Escape, v));
            assert!(!p.inner.sending());
        });
        let mut q = loopback();
        q.inner.start();
        with_view(None, |v| q.tick(v));
        with_view(None, |v| {
            q.on_key(EntryKey::Function { n: 2, shift: false }, v)
        });
        run_until(&mut q, |p| !p.inner.sending());
        let heard = lines(&q).join(" ");
        assert!(
            heard.contains("JA1YWX 599 100110M 100110M JA1YWX"),
            "{heard}"
        );
        let mut off = RttyPanel::default();
        with_mode(None, Mode::Cw, |v| {
            assert!(!off.on_key(EntryKey::Function { n: 1, shift: false }, v));
            assert!(!off.on_key(EntryKey::Escape, v));
        });
        assert!(off.take_actions().is_empty());
    }

    #[test]
    fn events_become_entry_actions() {
        let mut p = RttyPanel::default();
        with_view(None, |v| {
            for e in [
                Event::Pick(rtty_ui::Pick {
                    callsign: "JA1YWX".into(),
                    hz: 2125.0,
                    dial_hz: Some(14_085_000),
                }),
                Event::Number("100110M".into()),
                Event::Ptt(true),
                Event::Ptt(false),
            ] {
                p.event(e, v);
            }
        });
        assert_eq!(
            p.take_actions(),
            [
                EntryAction::Callsign("JA1YWX".into()),
                EntryAction::Number("100110M".into()),
            ]
        );
    }

    fn spots(p: &mut RttyPanel) -> Vec<Spot> {
        p.take_actions()
            .into_iter()
            .filter_map(|a| match a {
                EntryAction::Spot(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn hear(p: &mut RttyPanel, text: &str, dial: Option<u64>) {
        let words: Vec<String> = text.split_whitespace().map(String::from).collect();
        let my_call = p.inner.my_call.clone();
        let mut h = rtty_ui::heard::parse(&words, &my_call);
        h.dial_hz = dial;
        with_view(None, |v| p.event(Event::Heard(h), v));
    }

    #[test]
    fn received_lines_go_to_the_band_scope_when_enabled() {
        let mut p = RttyPanel::default();
        p.inner.my_call = "JA1ZLO".into();
        hear(&mut p, "CQ TEST DE JA1YWX JA1YWX", Some(14_085_000));
        assert!(spots(&mut p).is_empty(), "off by default");
        p.settings.scope = true;
        hear(&mut p, "CQ TEST DE JA1YWX JA1YWX", None);
        assert!(spots(&mut p).is_empty(), "no rig frequency");
        hear(&mut p, "CQ TEST DE JA1YWX JA1YWX", Some(14_085_000));
        let s = spots(&mut p);
        assert_eq!(s.len(), 1);
        assert_eq!(
            (s[0].callsign.as_str(), s[0].number.as_str(), s[0].hz),
            ("JA1YWX", "", 14_082_875)
        );
        assert_eq!(
            (s[0].mode, s[0].source.as_str()),
            (Some(Mode::Rtty), "RTTY")
        );
        hear(&mut p, "JA1YWX 599 001 001", Some(14_085_000));
        assert_eq!(
            spots(&mut p).last().map(|s| s.number.clone()),
            Some("001".into())
        );
        p.inner.my_call = "JA1YWX".into();
        p.inner.options.reverse = true;
        hear(&mut p, "JA1ZLO 599 002", Some(14_085_000));
        let s = spots(&mut p);
        assert_eq!(
            s.last()
                .map(|s| (s.callsign.as_str(), s.number.as_str(), s.hz)),
            Some(("JA1ZLO", "002", 14_087_125))
        );
    }

    #[test]
    fn the_rtty_filters_drop_spots() {
        let mut p = RttyPanel::default();
        p.inner.my_call = "JA1ZLO".into();
        p.settings.scope = true;
        p.settings.filters.number = "[0-9]{2}[A-Z]".into();
        hear(&mut p, "JA1YWX 599 001", Some(7_040_000));
        assert!(spots(&mut p).is_empty(), "no number in the format");
        p.settings.filters.callsign = "[A-Z".into();
        hear(&mut p, "JA1YWX 599 13M", Some(7_040_000));
        assert!(
            spots(&mut p).is_empty(),
            "an invalid pattern publishes nothing"
        );
        p.settings.filters.callsign = String::new();
        hear(&mut p, "JA1YWX 599 13M", Some(7_040_000));
        assert_eq!(spots(&mut p)[0].number, "13M");
    }

    struct Sound;

    impl zlog_ui::device::DeviceDriver for Sound {
        fn id(&self) -> &str {
            "sound"
        }
        fn title(&self, _language: &str) -> String {
            "Sound".into()
        }
        fn settings(&self, _language: &str) -> Vec<zlog_ui::device::SettingField> {
            let text = || zlog_ui::device::SettingKind::Text;
            vec![
                zlog_ui::device::SettingField::new("output", "Output", text()),
                zlog_ui::device::SettingField::new("input", "Input", text()),
            ]
        }
    }

    impl AudioDrv for Sound {
        fn open(&self, _settings: &Section, _ports: &PortHub) -> Result<Box<dyn AudioIo>, String> {
            Err("no sound device in tests".into())
        }
    }

    #[test]
    fn old_device_names_reach_the_sound_driver_and_old_keys_migrate() {
        let mut old = Section::new();
        for (k, v) in [
            ("input", "USB Audio"),
            ("theme", "2"),
            ("ptt_after", "250"),
            ("F4", "QRZ?"),
        ] {
            old.insert(k.into(), v.into());
        }
        let mut p = RttyPanel::new(vec![Box::new(Sound)], PortHub::new(), TxHub::default());
        p.load(&old);
        assert_eq!(p.device.id, "sound");
        assert_eq!(p.device.section()["input"], "USB Audio");
        assert_eq!(p.inner.messages[3].label, "F4");
        let mut saved = Section::new();
        p.save(&mut saved);
        assert_eq!(saved["sound.input"], "USB Audio");
        assert!(
            !saved.contains_key("ptt_after"),
            "the transmitter keeps the delays"
        );
        assert!(!saved.contains_key("my_call") && !saved.contains_key("theme"));
        p.inner.start();
        with_view(None, |v| p.tick(v));
        assert!(!p.inner.running() && p.status.contains("no sound device"));
    }

    #[test]
    fn an_opened_wav_is_decoded() {
        let options = rtty::RttyOptions::default();
        let band = rtty::sim::Band::audio(8000, 6.0);
        let text = "\r\nCQ TEST DE JA1YWX\r\n";
        let station = rtty::sim::Station::new(text, options.mark, 0.0, 15.0);
        let audio = rtty::sim::render(&band, rtty::sim::Source::Rtty(options), &[station], 1);
        let path = std::env::temp_dir().join(format!("zlog-rtty-{}.wav", std::process::id()));
        rtty_ui::wav::write(&path, &audio.unwrap(), 8000).unwrap();
        let mut p = RttyPanel::new(vec![Box::new(Sound)], PortHub::new(), TxHub::default());
        zlog_ui::dialog::set_test_pick(Some(path.clone()));
        with_view(None, |v| p.event(Event::OpenWav, v));
        zlog_ui::dialog::set_test_pick(None);
        assert!(p.inner.playing_wav() && p.status.is_empty(), "{}", p.status);
        let t = SystemTime::now();
        p.inner.tick(t);
        p.inner.tick(t + Duration::from_secs(10));
        with_view(None, |v| p.handle(v));
        std::fs::remove_file(&path).unwrap();
        assert!(
            lines(&p).iter().any(|l| l == "CQ TEST DE JA1YWX"),
            "{:?}",
            lines(&p)
        );
        assert_eq!(p.inner.terminal().calls, ["JA1YWX"]);
    }

    fn auto(on: bool) -> RttyPanel {
        let mut p = RttyPanel::default();
        let mut s = Section::new();
        s.insert("auto_log".into(), on.to_string());
        p.load(&s);
        p
    }

    fn press(p: &mut RttyPanel, n: u8, number: &str, log: &Log) -> Vec<EntryAction> {
        with_entry(None, Mode::Rtty, number, log, |v| {
            p.on_key(EntryKey::Function { n, shift: false }, v)
        });
        p.take_actions()
            .into_iter()
            .filter(|a| matches!(a, EntryAction::LogEntry | EntryAction::Say(_)))
            .collect()
    }

    #[test]
    fn the_end_key_logs_a_complete_entry_once() {
        let log = Log::default();
        assert!(
            press(&mut auto(false), 3, "100110M", &log).is_empty(),
            "off by default"
        );
        let mut p = auto(true);
        assert_eq!(p.settings.end_key, 3);
        assert!(p.settings.messages[2].contains("TU"));
        assert!(press(&mut p, 2, "100110M", &log).is_empty());
        assert!(press(&mut p, 3, "", &log).is_empty(), "no number");
        assert_eq!(press(&mut p, 3, "100110M", &log), [EntryAction::LogEntry]);
    }

    #[test]
    fn the_end_key_does_not_log_a_dupe_or_a_bad_callsign() {
        let mut log = Log::default();
        log.qsos.push(Qso {
            callsign: "JA1YWX".into(),
            band: Band::B14,
            mode: Mode::Rtty,
            ..Qso::default()
        });
        let mut p = auto(true);
        let said = press(&mut p, 3, "100110M", &log);
        assert!(
            matches!(said.as_slice(), [EntryAction::Say(s)] if s.warning),
            "{said:?}"
        );
        for bad in ["7025", "TU", "JA1YWX?"] {
            assert!(!is_callsign(bad), "{bad}");
        }
        assert!(is_callsign("JA1YWX/1") && is_callsign("W1AW"));
    }

    #[test]
    fn the_options_page_holds_the_device_and_the_rtty_settings() {
        use egui_kittest::kittest::Queryable;
        use zlog_ui::SettingsPage;
        let mut page = RttyOptions::new(vec![Box::new(Sound)]);
        let mut h = egui_kittest::Harness::builder()
            .with_size([480.0, 900.0])
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, s: &mut Section| {
                    egui::ScrollArea::vertical().show(ui, |ui| page.ui(ui, s, "en"));
                },
                Section::new(),
            );
        h.run_steps(2);
        for label in ["Options > Sound", "Mark", "band scope", "Leading zeros"] {
            assert!(h.query_by_label_contains(label).is_some(), "{label}");
        }
        assert!(h.query_by_label_contains("PTT before").is_none());
        assert!(h.query_by_label("My call").is_none());
        let s = h.state();
        assert_eq!(s["own_sound"], "false");
        assert!(!s.contains_key("ptt_before"));
        assert_eq!(s["F1"], DEFAULT_MESSAGES[0]);
        assert_eq!(s["sp.F1"], DEFAULT_SP_MESSAGES[0]);
        for label in ["CQ messages", "S&P messages"] {
            assert!(h.query_by_label(label).is_some(), "{label}");
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

    fn route(p: &mut RttyPanel, events: Vec<Event>) {
        with_view(None, |v| {
            for e in events {
                p.event(e, v);
            }
        });
    }

    #[test]
    fn rtty_keys_through_the_arbiter_and_waits_for_the_release() {
        let hub = TxHub::default();
        let keys = Keys::default();
        hub.set_keyer(Some(Box::new(keys.clone())));
        let mut p = RttyPanel::on(hub.clone());
        p.inner.set_output_rate(8000);
        with_view(None, |v| p.sync(v));
        p.inner.send("TEST");
        let asked = p.inner.take_events();
        assert_eq!(asked, [Event::Ptt(true)]);
        route(&mut p, asked);
        assert_eq!(*keys.0.lock().unwrap(), [true]);
        p.keying();
        let sent = p.inner.take_events();
        let Some(Event::Burst(b)) = sent.first() else {
            panic!("{sent:?}")
        };
        p.inner.tick(b.end() + Duration::from_millis(10));
        let done = p.inner.take_events();
        assert_eq!(done, [Event::Ptt(false)]);
        assert!(p.inner.sending(), "busy until the release");
        route(&mut p, done);
        p.keying();
        assert!(!p.inner.sending() && !p.link.active());
        assert_eq!(*keys.0.lock().unwrap(), [true, false]);
        p.inner.send("TEST");
        let asked = p.inner.take_events();
        route(&mut p, asked);
        hub.send(zlog_ui::tx::TxCommand::Stop);
        p.keying();
        assert!(!p.inner.sending() && !p.link.active());
    }

    #[test]
    fn the_dial_of_the_rig_reaches_the_heard_lines() {
        let mut p = RttyPanel::default();
        p.settings.scope = true;
        p.inner.my_call = "JA1ZLO".into();
        with_view(Some(14_085_000), |v| p.sync(v));
        p.inner
            .simulate("CQ TEST DE JA1YWX JA1YWX", SystemTime::now());
        p.inner.simulate("RYRY", SystemTime::now());
        with_view(None, |v| p.handle(v));
        let s = spots(&mut p);
        assert_eq!(
            s.iter()
                .map(|s| (s.callsign.as_str(), s.hz))
                .collect::<Vec<_>>(),
            [("JA1YWX", 14_082_875)]
        );
    }

    #[test]
    fn f_keys_play_the_shown_bank() {
        let mut p = RttyPanel::default();
        p.settings.sp_messages[0] = "$M".into();
        p.settings.apply(&mut p.inner);
        p.inner.bank = rtty_ui::Bank::Sp;
        with_view(None, |v| {
            p.sync(v);
            p.inner.play(0);
            let e = p.inner.take_events();
            assert!(
                matches!(&e[0], Event::Message { bank: rtty_ui::Bank::Sp, text, .. } if text == "$M")
            );
        });
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn send_and_stop_are_also_in_the_rtty_menu() {
        let p = RttyPanel::default();
        for lang in ["en", "ja"] {
            let list = p.commands(lang);
            for id in ["send", "stop"] {
                let c = list.iter().find(|c| c.id == id).unwrap();
                assert!(c.menu && c.toolbar, "{id}");
                assert_eq!(c.section, zlog_ui::MenuSection::Digital);
                assert_ne!(c.menu_text(), c.label);
            }
            assert!(!list.iter().find(|c| c.id == "send").unwrap().enabled);
        }
    }

    #[test]
    fn messages_keep_their_numbers() {
        let f = macros::Fields {
            callsign: "JA1ZLO".into(),
            rst_sent: "599".into(),
            sent_template: "100110M".into(),
            ..Default::default()
        };
        assert_eq!(
            macros::expand_rtty(DEFAULT_MESSAGES[1], &f),
            "JA1ZLO 599 100110M 100110M JA1ZLO"
        );
    }

    #[test]
    fn a_remote_session_takes_the_sound_card_from_the_local_window() {
        use haml::remote::{pair, Channel, Client, Limits, Server, Station};
        let hub = TxHub::default();
        let channel = Channel::new(remote::NAME);
        let mut p = loopback().hosting(channel.clone(), hub.clone());
        p.inner.start();
        run_until(&mut p, |p| p.io.is_some());
        let station = Station {
            rigs: Vec::new(),
            tx: hub.device(),
            tx_devices: Vec::new(),
            ports: None,
            lent: None,
            channels: vec![channel],
        };
        let server = Server::new(station, Limits::default());
        let (near, far) = pair("JA1YWX", "server");
        server.attach(Box::new(far));
        let client = Client::attach(Box::new(near)).unwrap();
        run_until(&mut p, |p| p.io.is_none());
        assert!(!p.inner.running());
        assert_eq!(p.status, "In use by remote station JA1YWX");
        p.inner.start();
        with_view(None, |v| p.tick(v));
        assert!(p.io.is_none(), "the local window waits");
        client.close();
        run_until(&mut p, |p| !p.busy);
        assert!(p.status.is_empty());
        p.inner.start();
        run_until(&mut p, |p| p.io.is_some());
    }

    #[test]
    fn only_the_modem_windows_offer_the_remote_station() {
        let list = with_remote(Vec::new());
        assert!(list[0].remote());
    }
}
