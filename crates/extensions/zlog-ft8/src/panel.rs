use std::time::{Duration, SystemTime};

use egui::Ui;
use ftxl_ui::Event;
use zlog_rtty::SOUND;
use zlog_ui::device::{AudioDrv, AudioIo, DeviceDriver, DriverChoice, PortHub};
use zlog_ui::spot::{heard_hz, Sideband, SpotFeed};
use zlog_ui::tx::{Keying, PttLink, TxHub};
use zlog_ui::{EntryAction, Panel, Section, SettingsPage, Spot, View};

use crate::driver::Proto;
use crate::engine::{delphi, load_panel, qso, save_panel, scope_of, SCOPE};
use crate::texts;

pub const SECTION: &str = "panel.ft8";

const AUTO_LOG: &str = "auto_log";

fn auto_log_of(s: &Section) -> bool {
    s.get(AUTO_LOG).is_none_or(|v| v != "false")
}

pub struct Ft8Panel {
    inner: ftxl_ui::FtPanel,
    link: PttLink,
    lead: Option<Duration>,
    dial: Option<u64>,
    radio: u8,
    scope: bool,
    auto_log: bool,
    last: Option<zlog_core::Qso>,
    drivers: Vec<Box<dyn AudioDrv>>,
    ports: PortHub,
    device: DriverChoice,
    io: Option<Box<dyn AudioIo>>,
    clock: ftxl_ui::Clock,
    status: String,
    actions: Vec<EntryAction>,
    feed: SpotFeed,
    host: Option<crate::remote::Host>,
    busy: bool,
    remote: Option<crate::remote::Remote>,
}

impl Default for Ft8Panel {
    fn default() -> Self {
        Ft8Panel::on(TxHub::default())
    }
}

impl Ft8Panel {
    pub fn on(hub: TxHub) -> Self {
        let mut inner = ftxl_ui::FtPanel::new("en");
        inner.host_call = true;
        inner.tools.settings = false;
        Ft8Panel {
            inner,
            link: PttLink::new(hub),
            lead: None,
            dial: None,
            radio: 1,
            scope: true,
            auto_log: true,
            last: None,
            drivers: Vec::new(),
            ports: PortHub::new(),
            device: DriverChoice::new(SOUND),
            io: None,
            clock: ftxl_ui::Clock::default(),
            status: String::new(),
            actions: Vec::new(),
            feed: SpotFeed::default(),
            host: None,
            busy: false,
            remote: None,
        }
    }
}

fn heard_call(de: &str, my_call: &str) -> Option<String> {
    let call = de.trim_start_matches('<').trim_end_matches('>');
    let ok = call.bytes().any(|b| b.is_ascii_digit())
        && call.bytes().any(|b| b.is_ascii_uppercase())
        && call
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase() || b == b'/');
    (ok && call != my_call).then(|| call.to_string())
}

pub fn spot_of(s: &ftxl::Spot, my_call: &str, now: i64) -> Option<Spot> {
    let proto = Proto::of(s.mode);
    Some(Spot {
        callsign: heard_call(&s.callsign, my_call)?,
        hz: heard_hz(s.dial_hz?, s.hz as f64, 0.0, Sideband::Upper),
        mode: Some(proto.mode()),
        time: now,
        source: proto.name().into(),
        ..Spot::default()
    })
}

impl Ft8Panel {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>, ports: PortHub, hub: TxHub) -> Self {
        Ft8Panel {
            drivers,
            ports,
            ..Ft8Panel::on(hub)
        }
    }

    pub fn hosting(mut self, channel: haml::remote::Channel, hub: TxHub) -> Self {
        self.host = Some(crate::remote::Host::new(channel, hub));
        self
    }

    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    fn rebuild(&mut self, remote: bool) {
        let lang = self.inner.language.clone();
        let mut p = match remote {
            true => ftxl_ui::FtPanel::remote(&lang),
            false => ftxl_ui::FtPanel::new(&lang),
        };
        p.host_call = self.inner.host_call;
        p.tools = self.inner.tools;
        for (k, v) in self.inner.settings() {
            p.load_setting(k, &v);
        }
        p.my_call = self.inner.my_call.clone();
        self.inner = p;
        self.lead = None;
        self.dial = None;
    }

    fn follow_remote(&mut self) {
        let want = self.device.find(&self.drivers).is_some_and(|d| d.remote());
        if self.remote.as_ref().is_some_and(|r| !r.connected()) {
            self.remote = None;
            self.rebuild(false);
        }
        match (want, self.remote.is_some()) {
            (true, false) => {
                let name = crate::remote::NAME;
                match zlog_remote::modem::open(&zlog_remote::shared(), name) {
                    Ok(ch) => {
                        self.io = None;
                        self.rebuild(true);
                        self.remote = Some(crate::remote::Remote::new(ch));
                        self.status.clear();
                    }
                    Err(e) => self.status = e,
                }
            }
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

    pub fn inner(&self) -> &ftxl_ui::FtPanel {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut ftxl_ui::FtPanel {
        &mut self.inner
    }

    pub fn listening(&self) -> bool {
        self.io.is_some()
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
            self.clock = ftxl_ui::Clock::default();
            self.io = Some(io);
        }
        Ok(())
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
        let Some(io) = self.io.as_mut() else {
            return;
        };
        let mut samples = Vec::new();
        io.record(&mut samples);
        if !samples.is_empty() {
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
            Event::Running(true) if !self.inner.playing_wav() => match self.open() {
                Ok(()) => self.status.clear(),
                Err(e) => {
                    self.status = e;
                    self.inner.stop();
                }
            },
            Event::Running(false) => self.io = None,
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
            Event::Ptt(_) if self.remote.is_some() => {}
            Event::Ptt(on) => self.link.ptt(on, self.radio),
            Event::Tune(hz) => {
                let mode = Proto::of(self.inner.proto).mode();
                self.actions.push(EntryAction::Qsy(hz, Some(mode)));
            }
            Event::QsoCompleted(q) => self.log(&q, view),
            Event::Spot(s) => self.publish(&[s], view),
            Event::OpenWav => {
                let kind = texts().get(view.language, "wav.kind");
                if let Some(p) = zlog_ui::dialog::import_file(kind, &["wav"]) {
                    if let Err(e) = self.inner.load_wav(&p) {
                        self.status = e;
                    }
                }
            }
            _ => {}
        }
    }

    fn publish(&mut self, heard: &[ftxl::Spot], view: &View<'_>) {
        if !self.scope {
            return;
        }
        let my_call = view.station.callsign.to_ascii_uppercase();
        let now = chrono::Utc::now().timestamp();
        for s in heard {
            let spot = spot_of(s, &my_call, now);
            if let Some(s) = spot.and_then(|s| self.feed.offer(s)) {
                self.actions.push(EntryAction::Spot(s));
            }
        }
    }

    fn log(&mut self, q: &ftxl::Qso, view: &View<'_>) {
        let at = chrono::DateTime::<chrono::Utc>::from(q.at);
        let time = match view.log.utc_offset_minutes().is_some() {
            true => at.with_timezone(&chrono::Local).naive_local(),
            false => at.naive_utc(),
        };
        let q = qso(q, view.band, delphi(time), view.operator);
        let same = |l: &zlog_core::Qso| {
            (l.callsign.as_str(), l.band, l.mode) == (q.callsign.as_str(), q.band, q.mode)
        };
        if self.last.as_ref().is_some_and(same) {
            return;
        }
        if self.auto_log && view.log.find_dupe(&q).is_none() {
            self.actions.push(EntryAction::Log(Box::new(q.clone())));
        } else {
            self.actions.push(EntryAction::Callsign(q.callsign.clone()));
            self.actions.push(EntryAction::Number(q.nr_rcvd.clone()));
        }
        self.last = Some(q);
    }
}

impl Panel for Ft8Panel {
    fn id(&self) -> &str {
        "ft8"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Digital, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::WAVEFORM)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 520.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        self.sync(view);
        if !self.status.is_empty() {
            zlog_ui::layout::status(ui, "", Some(&self.status));
        }
        self.inner.ui(ui);
        self.handle(view);
    }

    fn tick_after(&self) -> Option<Duration> {
        if self.link.active() {
            return Some(Duration::from_millis(20));
        }
        if self.host.as_ref().is_some_and(crate::remote::Host::active) {
            return Some(Duration::from_millis(20));
        }
        let busy = self.inner.running() || self.inner.playing_wav() || self.remote.is_some();
        if busy {
            return Some(Duration::from_millis(100));
        }
        (self.host.is_some() && zlog_remote::modem::sharing()).then_some(Duration::from_millis(200))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.sync(view);
        self.inner.tick(SystemTime::now());
        self.handle(view);
    }

    fn on_key(&mut self, key: zlog_ui::EntryKey, view: &View<'_>) -> bool {
        match key {
            zlog_ui::EntryKey::Escape if self.inner.sending() => {
                self.inner.halt();
                self.handle(view);
                true
            }
            _ => false,
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
        load_panel(&mut self.inner, section);
        self.scope = scope_of(section);
        self.auto_log = auto_log_of(section);
        let list: Vec<&dyn DeviceDriver> = self.drivers.iter().map(|d| &**d as _).collect();
        let sound = zlog_ui::device::sound_section(section, others);
        let device = DriverChoice::load(&sound, SOUND, &list);
        let (mut before, mut after) = (Section::new(), Section::new());
        self.device.save(&mut before);
        device.save(&mut after);
        self.device = device;
        if before != after {
            self.inner.stop();
            self.io = None;
            self.inner.take_events();
        }
    }

    fn save(&self, section: &mut Section) {
        save_panel(&self.inner, section);
        section.insert(SCOPE.into(), self.scope.to_string());
        section.insert(AUTO_LOG.into(), self.auto_log.to_string());
        self.device.save(section);
    }
}

pub struct Ft8Options {
    drivers: Vec<Box<dyn AudioDrv>>,
    device: Option<DriverChoice>,
    panel: ftxl_ui::FtPanel,
}

impl Ft8Options {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>) -> Self {
        let mut panel = ftxl_ui::FtPanel::new("en");
        panel.host_call = true;
        Ft8Options {
            drivers,
            device: None,
            panel,
        }
    }
}

impl SettingsPage for Ft8Options {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, lang: &str) {
        let t = |k| texts().get(lang, k);
        zlog_rtty::device_ui(ui, &mut self.device, &self.drivers, section, lang);
        ui.separator();
        self.panel.language = lang.into();
        load_panel(&mut self.panel, section);
        self.panel.settings_ui(ui);
        save_panel(&self.panel, section);
        let mut scope = scope_of(section);
        ui.checkbox(&mut scope, t("scope"))
            .on_hover_text(t("scope.tip"));
        section.insert(SCOPE.into(), scope.to_string());
        let mut auto = auto_log_of(section);
        ui.checkbox(&mut auto, t("auto_log"))
            .on_hover_text(t("auto_log.tip"));
        section.insert(AUTO_LOG.into(), auto.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::{
        Band, Contest, ContestMode, Log, Mode, Power, Qso, ScoreSheet, Scoring, Station,
    };

    struct Nothing;

    impl Contest for Nothing {
        fn name(&self) -> &str {
            "test"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::B50)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Cw
        }
        fn sent_number(&self, _band: Band, _power: Power, _station: &Station) -> String {
            String::new()
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

    fn with_view<R>(f: impl FnOnce(&View<'_>) -> R) -> R {
        with_rig(None, f)
    }

    fn with_rig<R>(hz: Option<u64>, f: impl FnOnce(&View<'_>) -> R) -> R {
        let log = Log::default();
        let sheet = ScoreSheet::default();
        let station = Station {
            callsign: "JA1ZLO".into(),
            ..Station::default()
        };
        let view = View {
            log: &log,
            contest: &Nothing,
            sheet: &sheet,
            callsign: "",
            station: &station,
            number: "",
            memo: "",
            rst_sent: "",
            power: Power::P010,
            band: Band::B7,
            mode: Mode::Ft8,
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

    fn loopback() -> Ft8Panel {
        let mut p = Ft8Panel::new(
            vec![Box::new(zlog_ui::loopback::LoopbackDrv::default())],
            PortHub::new(),
            TxHub::default(),
        );
        let mut s = Section::new();
        for (k, v) in [
            ("device", "loopback"),
            ("loopback.rate", "12000"),
            ("grid", "PM95"),
            ("auto_log", "true"),
        ] {
            s.insert(k.into(), v.into());
        }
        p.load(&s);
        p
    }

    fn logged(p: &mut Ft8Panel) -> Vec<zlog_core::Qso> {
        p.take_actions()
            .into_iter()
            .filter_map(|a| match a {
                EntryAction::Log(q) => Some(*q),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_clicked_report_to_me_is_logged_with_both_reports() {
        let mut p = loopback();
        with_view(|v| {
            p.sync(v);
            p.inner.start();
            p.handle(v);
            assert!(p.listening(), "{}", p.status);
            p.inner.simulate("JA1ZLO JA1YWX R-07", SystemTime::now());
            p.inner.select(0);
            p.handle(v);
        });
        let q = logged(&mut p);
        assert_eq!(q.len(), 1, "{q:?}");
        assert_eq!((q[0].callsign.as_str(), q[0].mode), ("JA1YWX", Mode::Ft8));
        assert_eq!(
            (q[0].nr_sent.as_str(), q[0].nr_rcvd.as_str()),
            ("-06", "-07")
        );
        assert_eq!(p.inner.seq.next(), "JA1YWX JA1ZLO RR73");
        assert_eq!(p.inner.my_grid, "PM95");
    }

    fn done(call: &str) -> ftxl::Qso {
        ftxl::Qso {
            mode: ftxl::Proto::Ft8,
            callsign: call.into(),
            grid: "PM95".into(),
            sent: "-06".into(),
            rcvd: "R-07".into(),
            hz: 1200.0,
            dial_hz: Some(7_041_000),
            at: SystemTime::now(),
        }
    }

    #[test]
    fn auto_log_is_on_by_default() {
        let mut p = Ft8Panel::default();
        assert!(p.auto_log);
        p.load(&Section::new());
        assert!(p.auto_log, "a missing key keeps it on");
        with_view(|v| p.log(&done("JA1YWX"), v));
        assert_eq!(logged(&mut p).len(), 1);
    }

    #[test]
    fn without_auto_log_a_completed_qso_fills_the_entry_row() {
        let mut p = Ft8Panel::default();
        let mut s = Section::new();
        s.insert("auto_log".into(), "false".into());
        p.load(&s);
        assert!(!p.auto_log);
        with_view(|v| p.log(&done("JA1YWX"), v));
        assert_eq!(
            p.take_actions(),
            [
                EntryAction::Callsign("JA1YWX".into()),
                EntryAction::Number("-07".into())
            ]
        );
    }

    #[test]
    fn a_repeated_completion_is_logged_once() {
        let mut p = Ft8Panel::default();
        let mut s = Section::new();
        p.save(&mut s);
        s.insert("auto_log".into(), "true".into());
        p.load(&s);
        with_view(|v| {
            p.log(&done("JA1YWX"), v);
            p.log(&done("JA1YWX"), v);
        });
        let q = logged(&mut p);
        assert_eq!(q.len(), 1, "{q:?}");
        assert_eq!(
            (q[0].nr_sent.as_str(), q[0].nr_rcvd.as_str()),
            ("-06", "-07")
        );
        assert_eq!((q[0].band, q[0].freq.as_str()), (Band::B7, "7041.0"));
    }

    #[test]
    fn a_full_contact_between_two_windows_ends_in_both_logs() {
        let mut zlo = Ft8Panel::default();
        let mut ywx = Ft8Panel::default();
        for p in [&mut zlo, &mut ywx] {
            p.inner.my_grid = "PM95".into();
            p.inner.set_output_rate(12000);
            p.inner.start();
            p.inner.take_events();
        }
        zlo.inner.my_call = "JA1ZLO".into();
        ywx.inner.my_call = "JA1YWX".into();
        zlo.inner.tx_hz = 1200.0;
        ywx.inner.tx_hz = 1700.0;
        zlo.inner.seq.enabled = true;
        zlo.auto_log = true;
        ywx.auto_log = true;
        let base = 1_793_000_010 / 30 * 30;
        let slot = |n: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(base + 15 * n);
        let air = |from: &mut Ft8Panel, to: &mut Ft8Panel, n: u64| {
            from.inner.tick(slot(n) - Duration::from_millis(50));
            let b = from
                .inner
                .take_events()
                .into_iter()
                .find_map(|e| match e {
                    Event::Burst(b) => Some(b),
                    _ => None,
                })
                .expect("a burst");
            let mut audio = vec![0.0f32; 16 * 12000];
            for (k, x) in b.samples.iter().enumerate() {
                if let Some(y) = audio.get_mut(6000 + k) {
                    *y += 0.3 * x;
                }
            }
            for (k, c) in audio.chunks(1200).enumerate() {
                let at = slot(n) + Duration::from_millis(100 * k as u64);
                to.inner.push_samples(12000, c, at);
            }
            from.inner.tick(b.end());
            with_view(|v| {
                from.handle(v);
                to.handle(v);
            });
        };
        air(&mut zlo, &mut ywx, 0);
        ywx.inner.select(ywx.inner.decodes().len() - 1);
        air(&mut ywx, &mut zlo, 1);
        air(&mut zlo, &mut ywx, 2);
        air(&mut ywx, &mut zlo, 3);
        air(&mut zlo, &mut ywx, 4);
        let (a, b) = (logged(&mut zlo), logged(&mut ywx));
        assert_eq!(
            a.iter().map(|q| q.callsign.as_str()).collect::<Vec<_>>(),
            ["JA1YWX"]
        );
        assert_eq!(
            b.iter().map(|q| q.callsign.as_str()).collect::<Vec<_>>(),
            ["JA1ZLO"]
        );
        assert_eq!((a[0].memo.as_str(), b[0].memo.as_str()), ("PM95", "PM95"));
    }

    fn spots(p: &mut Ft8Panel) -> Vec<Spot> {
        p.take_actions()
            .into_iter()
            .filter_map(|a| match a {
                EntryAction::Spot(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn spot(call: &str, dial_hz: Option<u64>) -> ftxl::Spot {
        ftxl::Spot {
            mode: ftxl::Proto::Ft8,
            callsign: call.into(),
            grid: None,
            snr: -12.0,
            hz: 900.0,
            dial_hz,
            at: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn heard_stations_go_to_the_band_scope_at_their_dial() {
        let mut p = Ft8Panel::default();
        assert!(p.scope, "on by default");
        let dial = Some(7_074_000);
        let heard = [
            spot("JA1YWX", dial),
            spot("JH1ABC", dial),
            spot("JA1ZLO", dial),
            spot("<JA1YWX/1>", dial),
            spot("JA1XYZ", None),
        ];
        with_rig(Some(14_074_000), |v| p.publish(&heard, v));
        let s = spots(&mut p);
        let got: Vec<_> = s.iter().map(|s| (s.callsign.as_str(), s.hz)).collect();
        assert_eq!(
            got,
            [
                ("JA1YWX", 7_074_900),
                ("JH1ABC", 7_074_900),
                ("JA1YWX/1", 7_074_900)
            ]
        );
        assert!(s
            .iter()
            .all(|s| s.source == "FT8" && s.mode == Some(Mode::Ft8) && s.number.is_empty()));
        with_rig(None, |v| p.publish(&heard, v));
        assert!(spots(&mut p).is_empty(), "throttled");
        p.scope = false;
        p.feed = SpotFeed::default();
        with_rig(None, |v| p.publish(&heard, v));
        assert!(spots(&mut p).is_empty(), "turned off");
    }

    #[test]
    fn ft4_spots_carry_their_tag() {
        let mut s = spot("JA1YWX", Some(14_080_000));
        s.mode = ftxl::Proto::Ft4;
        let s = spot_of(&s, "", 5).unwrap();
        assert_eq!(
            (s.hz, s.mode, s.source.as_str()),
            (14_080_900, Some(Mode::Ft4), "FT4")
        );
    }

    #[test]
    fn the_rig_dial_reaches_the_spots_through_the_engine() {
        let mut p = Ft8Panel::default();
        with_rig(Some(7_074_000), |v| {
            p.sync(v);
            p.inner.start();
            p.inner.simulate("CQ JA1YWX PM95", SystemTime::now());
            p.handle(v);
        });
        let s = spots(&mut p);
        assert_eq!(s.len(), 1);
        assert_eq!(
            (s[0].callsign.as_str(), s[0].source.as_str(), s[0].hz),
            ("JA1YWX", "FT8", 7_075_500)
        );
    }

    #[test]
    fn a_tune_from_the_engine_becomes_a_rig_qsy() {
        let mut p = Ft8Panel::default();
        with_rig(Some(7_074_000), |v| {
            p.sync(v);
            p.inner.proto = ftxl::Proto::Ft4;
            p.inner.tick(SystemTime::now());
            p.handle(v);
        });
        let qsy: Vec<_> = p
            .take_actions()
            .into_iter()
            .filter(|a| matches!(a, EntryAction::Qsy(..)))
            .collect();
        assert_eq!(qsy, [EntryAction::Qsy(7_047_500, Some(Mode::Ft4))]);
    }

    #[test]
    fn the_options_page_holds_the_device_the_ft_settings_and_the_scope() {
        use egui_kittest::kittest::Queryable;
        let mut page = Ft8Options::new(vec![Box::new(zlog_ui::loopback::LoopbackDrv::default())]);
        let mut old = Section::new();
        old.insert("grid".into(), "PM95".into());
        let mut h = egui_kittest::Harness::builder()
            .with_size([480.0, 800.0])
            .with_accessibility_check(false)
            .build_ui_state(|ui, s: &mut Section| page.ui(ui, s, "en"), old);
        h.run_steps(2);
        for label in [
            "Options > Sound",
            "My grid",
            "Decoding passes",
            "Band scope",
        ] {
            assert!(h.query_by_label_contains(label).is_some(), "{label}");
        }
        assert!(h.query_by_label("My call").is_none());
        h.get_by_label("Band scope").click();
        h.run_steps(2);
        let s = h.state();
        assert_eq!(
            (s["my_grid"].as_str(), s[SCOPE].as_str()),
            ("PM95", "false")
        );
        assert!(!s.contains_key("grid"));
    }

    #[test]
    fn the_window_has_the_toolbar_and_no_device_picker() {
        use egui_kittest::kittest::Queryable;
        let mut h = egui_kittest::Harness::builder()
            .with_size([360.0, 600.0])
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, p: &mut Ft8Panel| {
                    ui.ctx()
                        .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                    with_view(|v| p.ui(ui, v))
                },
                loopback(),
            );
        h.run_steps(2);
        for label in ["Start", "FT8", "FT4", "Even", "Odd", "Enable Tx", "Open…"] {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
        assert!(h.query_by_label("Audio").is_none());
        h.get_by_label("Start").click();
        h.run_steps(2);
        assert!(h.state().listening() && h.state().inner.running());
        h.get_by_label("Stop").click();
        h.run_steps(2);
        assert!(!h.state().listening());
    }

    #[test]
    fn open_wav_plays_the_chosen_file_into_the_decoder() {
        let mut band = ftxl::sim::Band::slot(ftxl::sim::Source::Ft8);
        band.rate = 12000;
        let station = ftxl::sim::Station::new("CQ JA1YWX PM95", 1500.0, 0.0, -5.0);
        let source = ftxl::sim::Source::Ft8;
        let mut audio = ftxl::sim::render(&band, source, &[station], 1).unwrap();
        audio.resize(15 * 12000, 0.0);
        let path = std::env::temp_dir().join(format!("zlog-ft8-{}.wav", std::process::id()));
        ftxl_ui::wav::write(&path, &audio, 12000).unwrap();
        let mut p = Ft8Panel::default();
        zlog_ui::dialog::set_test_pick(Some(path.clone()));
        with_rig(Some(7_074_000), |v| {
            p.sync(v);
            p.event(Event::OpenWav, v)
        });
        zlog_ui::dialog::set_test_pick(None);
        assert!(p.inner.playing_wav() && p.status.is_empty(), "{}", p.status);
        let t = SystemTime::now();
        p.inner.tick(t);
        p.inner.tick(t + Duration::from_secs(40));
        with_rig(Some(7_074_000), |v| p.handle(v));
        std::fs::remove_file(&path).unwrap();
        let texts: Vec<&str> = p.inner.decodes().iter().map(|d| d.text.as_str()).collect();
        assert_eq!(texts, ["CQ JA1YWX PM95"]);
        assert_eq!(spots(&mut p)[0].callsign, "JA1YWX");
        assert!(!p.listening());
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

    fn keyed(lead: Duration) -> (Ft8Panel, TxHub, Keys) {
        let hub = TxHub::default();
        let keys = Keys::default();
        hub.set_keyer(Some(Box::new(keys.clone())));
        hub.set_delays(zlog_ui::tx::Delays {
            before: lead,
            after: Duration::ZERO,
        });
        let mut p = Ft8Panel::on(hub.clone());
        p.inner.my_grid = "PM95".into();
        p.inner.set_output_rate(12000);
        p.inner.seq.enabled = true;
        p.inner.seq.his_call = "JA1YWX".into();
        p.inner.start();
        p.inner.take_events();
        with_view(|v| p.sync(v));
        (p, hub, keys)
    }

    fn bursts(events: &[Event]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, Event::Burst(_)))
            .count()
    }

    fn route(p: &mut Ft8Panel, events: Vec<Event>) {
        with_view(|v| {
            for e in events {
                p.event(e, v);
            }
        });
    }

    #[test]
    fn the_burst_starts_only_after_the_arbiter_keys() {
        let (mut p, hub, keys) = keyed(Duration::ZERO);
        let slot = ftxl::next_slot(SystemTime::now(), p.inner.proto.period(), p.inner.parity);
        p.inner.tick(slot - Duration::from_millis(50));
        let asked = p.inner.take_events();
        assert!(asked.contains(&Event::Ptt(true)), "{asked:?}");
        assert_eq!(bursts(&asked), 0, "no burst before the key");
        assert!(keys.0.lock().unwrap().is_empty());
        route(&mut p, asked);
        assert_eq!(*keys.0.lock().unwrap(), [true]);
        assert_eq!(hub.transmitting(), Some(1));
        p.keying();
        let sent = p.inner.take_events();
        assert_eq!(bursts(&sent), 1, "{sent:?}");
        let Some(Event::Burst(b)) = sent.iter().find(|e| matches!(e, Event::Burst(_))) else {
            unreachable!()
        };
        assert_eq!(b.at, slot + Duration::from_millis(500));
        p.inner.tick(b.end() + Duration::from_millis(10));
        let done = p.inner.take_events();
        assert!(done.contains(&Event::Ptt(false)), "{done:?}");
        route(&mut p, done);
        p.keying();
        assert_eq!(*keys.0.lock().unwrap(), [true, false]);
        assert!(!p.link.active() && hub.transmitting().is_none());
    }

    #[test]
    fn a_late_confirmation_skips_the_slot() {
        let (mut p, hub, keys) = keyed(Duration::from_secs(60));
        let slot = ftxl::next_slot(SystemTime::now(), p.inner.proto.period(), p.inner.parity);
        p.inner.tick(slot - Duration::from_millis(50));
        let asked = p.inner.take_events();
        route(&mut p, asked);
        p.keying();
        assert_eq!(bursts(&p.inner.take_events()), 0);
        p.inner.tick(slot + Duration::from_millis(600));
        let skipped = p.inner.take_events();
        assert!(skipped.contains(&Event::Ptt(false)), "{skipped:?}");
        assert_eq!(bursts(&skipped), 0);
        assert!(p.inner.status().contains("skipped"), "{}", p.inner.status());
        route(&mut p, skipped);
        assert!(p.link.active(), "waits for the arbiter to release");
        hub.send(zlog_ui::tx::TxCommand::Stop);
        p.keying();
        assert!(!p.link.active() && !hub.busy());
        assert_eq!(*keys.0.lock().unwrap(), [true, false]);
    }

    #[test]
    fn a_failed_key_and_an_emergency_stop_halt_the_engine() {
        let (mut p, hub, _) = keyed(Duration::ZERO);
        let slot = ftxl::next_slot(SystemTime::now(), p.inner.proto.period(), p.inner.parity);
        p.inner.tick(slot - Duration::from_millis(50));
        let asked = p.inner.take_events();
        route(&mut p, asked);
        p.keying();
        assert!(p.inner.sending());
        hub.send(zlog_ui::tx::TxCommand::Stop);
        p.keying();
        assert!(!p.inner.sending() && !p.inner.seq.enabled);
        p.radio = 3;
        p.inner.seq.enabled = true;
        let slot = slot + 2 * p.inner.proto.period();
        p.inner.tick(slot - Duration::from_millis(50));
        let asked = p.inner.take_events();
        route(&mut p, asked);
        p.keying();
        assert!(!p.inner.sending() && !p.inner.seq.enabled);
        assert_eq!(p.inner.status(), "no radio 3");
    }

    #[test]
    fn escape_halts_only_while_sending() {
        let mut p = loopback();
        with_view(|v| {
            assert!(!p.on_key(zlog_ui::EntryKey::Escape, v));
            p.inner.start();
            p.handle(v);
            p.inner.seq.enabled = true;
            p.inner.seq.his_call = "JA1YWX".into();
            let now = SystemTime::now();
            let next = ftxl::next_slot(now, p.inner.proto.period(), p.inner.parity);
            p.inner.tick(next - Duration::from_millis(50));
            p.handle(v);
            assert!(p.inner.sending() && p.link.active());
            assert!(p.on_key(zlog_ui::EntryKey::Escape, v));
            p.keying();
        });
        assert!(!p.inner.sending() && !p.inner.seq.enabled && !p.link.active());
        assert!(!p.take_actions().contains(&EntryAction::Transmit(true)));
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
