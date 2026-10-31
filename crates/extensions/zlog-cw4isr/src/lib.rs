pub mod extension;
pub mod sim;

use std::cell::RefCell;
use std::rc::Rc;

use cw4i_ui::{Contact, Cw4i, Event, Pick, ToolbarItems};
use egui::Ui;
use extension::Extension;
use zlog_ui::device::{AudioDrv, AudioIo, DeviceDriver, DriverChoice, PortHub};
use zlog_ui::spot::SpotFeed;
use zlog_ui::{EntryAction, Panel, Section, View};

zlog_core::texts!();

pub const SOUND: &str = "sound";
pub const INPUT: &str = "sound.input";
pub const SIMULATOR: &str = "simulator";
pub const LEGACY_SIM: [&str; 2] = ["sim", "cw4i.sim"];

pub fn input_of(section: &Section) -> Option<String> {
    let old = section.get(INPUT).or_else(|| section.get("input")).cloned();
    let sim = LEGACY_SIM
        .iter()
        .any(|k| section.get(*k).is_some_and(|v| v == "true"));
    let old = old.or_else(|| sim.then(String::new));
    old.map(|v| if v == SIMULATOR { String::new() } else { v })
}
const TOOLS: ToolbarItems = ToolbarItems {
    start: true,
    wav: true,
    clear: true,
    waterfall: true,
    ..ToolbarItems::NONE
};

pub struct CwPanel {
    decoder: Cw4i,
    ctx: Option<egui::Context>,
    drivers: Vec<Box<dyn AudioDrv>>,
    ports: PortHub,
    driver: &'static str,
    device: DriverChoice,
    input: Option<Box<dyn AudioIo>>,
    error: Option<String>,
    contacts: Rc<RefCell<Vec<Contact>>>,
    extension: Extension,
    actions: Vec<EntryAction>,
    feed: SpotFeed,
}

impl Default for CwPanel {
    fn default() -> Self {
        let mut decoder = Cw4i::default();
        decoder.host_input = true;
        decoder.toolbar_items = TOOLS;
        let contacts = Rc::new(RefCell::new(Vec::new()));
        let queue = contacts.clone();
        decoder.on_event(move |e| {
            if let Event::Contact(c) = e {
                queue.borrow_mut().push(c.clone());
            }
        });
        CwPanel {
            decoder,
            ctx: None,
            drivers: Vec::new(),
            ports: PortHub::new(),
            driver: SOUND,
            device: DriverChoice::new(SOUND),
            input: None,
            error: None,
            contacts,
            extension: Extension::default(),
            actions: Vec::new(),
            feed: SpotFeed::default(),
        }
    }
}

pub fn action(pick: Pick) -> EntryAction {
    match pick {
        Pick::Callsign(c) => EntryAction::Callsign(c),
        Pick::Number(n) => EntryAction::Number(n),
    }
}

fn valid<'a>(c: &'a Contact, view: &'a View<'_>) -> impl Fn(&str) -> bool + 'a {
    move |n: &str| {
        view.contest.valid_number(&zlog_core::Qso {
            callsign: c.callsign.clone().unwrap_or_default(),
            nr_rcvd: n.into(),
            band: view.band,
            mode: view.mode,
            ..zlog_core::Qso::default()
        })
    }
}

fn cw4i_key(key: &str) -> String {
    format!("cw4i.{key}")
}

impl CwPanel {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>, ports: PortHub) -> Self {
        CwPanel {
            drivers,
            ports,
            ..CwPanel::default()
        }
    }

    pub fn listen(&mut self) {
        self.input = None;
        let opened = match self.device.find(&self.drivers) {
            Some(d) => d.open(&self.device.section(), &self.ports),
            None => Err(texts().get("en", "no_device").into()),
        };
        match opened {
            Ok(io) => {
                self.input = Some(io);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub fn stop(&mut self) {
        self.input = None;
        self.decoder.stop();
    }

    pub fn listening(&self) -> bool {
        self.input.is_some()
    }

    pub fn play_wav(&mut self, path: &std::path::Path) {
        self.input = None;
        self.error = None;
        self.decoder.open_file(path);
    }

    fn advance(&mut self) {
        if let Some(io) = self.input.as_mut() {
            let mut samples = Vec::new();
            io.record(&mut samples);
            let rate = io.sample_rate();
            self.decoder.push_samples(&self.device.id, rate, &samples);
        }
        self.decoder.process();
        if let Some(ctx) = self.ctx.as_ref().filter(|_| self.running()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

impl CwPanel {
    fn follow(&mut self, view: &View<'_>) {
        self.decoder.host_call = true;
        let call = view.station.callsign.trim().to_uppercase();
        if self.decoder.my_call != call {
            self.decoder.my_call = call;
        }
        let contacts = std::mem::take(&mut *self.contacts.borrow_mut());
        for c in contacts {
            let a = self.extension.fill(&c, view.callsign, &valid(&c, view));
            self.actions.extend(a);
        }
        if let Some(dial) = view.hz.filter(|_| self.extension.formats.scope) {
            let heard: Vec<Contact> = self
                .decoder
                .stations()
                .iter()
                .filter(|s| s.confirmed && s.alive)
                .filter_map(|s| self.decoder.contact(s.id))
                .collect();
            self.publish(&heard, dial, view);
        }
    }

    fn publish(&mut self, heard: &[Contact], dial: u64, view: &View<'_>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        for c in heard {
            let spot = self.extension.spot(c, dial, &valid(c, view), now);
            if let Some(s) = spot.and_then(|s| self.feed.offer(s)) {
                self.actions.push(EntryAction::Spot(s));
            }
        }
    }

    fn running(&self) -> bool {
        self.listening() || self.decoder.running()
    }

    fn top_ui(&mut self, ui: &mut Ui) {
        let error = self.error.clone().unwrap_or_default();
        zlog_ui::layout::toolbar(ui, &[(&error, true)], |ui| {
            self.decoder.host_running = self.running();
            self.decoder.tools(ui);
        });
    }
}

impl Panel for CwPanel {
    fn id(&self) -> &str {
        "cw-decoder"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Cw, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::EAR)]
    }

    fn default_size(&self) -> [f32; 2] {
        [720.0, 480.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        if self.decoder.language != view.language || self.decoder.empty_hint.is_none() {
            self.decoder.language = view.language.into();
            self.decoder.empty_hint = Some(texts().get(view.language, "hint").into());
        }
        self.ctx = Some(ui.ctx().clone());
        self.advance();
        let lang = view.language;
        self.top_ui(ui);
        for e in self.decoder.take_events() {
            match e {
                Event::Start => self.listen(),
                Event::Stop => self.stop(),
                Event::OpenWav => {
                    let kind = texts().get(lang, "wav.kind");
                    if let Some(p) = zlog_ui::dialog::import_file(kind, &["wav"]) {
                        self.play_wav(&p);
                    }
                }
                _ => {}
            }
        }
        self.decoder.body(ui);
        self.follow(view);
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        self.running()
            .then_some(std::time::Duration::from_millis(100))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.advance();
        self.follow(view);
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        let mut out: Vec<EntryAction> = self.decoder.take_picks().into_iter().map(action).collect();
        out.append(&mut self.actions);
        out
    }

    fn load(&mut self, section: &Section) {
        self.load_with(section, &zlog_ui::Sections::new());
    }

    fn reads(&self) -> Vec<String> {
        vec![extension::KEYER_SECTION.into()]
    }

    fn load_with(&mut self, section: &Section, others: &zlog_ui::Sections) {
        let list: Vec<&dyn DeviceDriver> = self.drivers.iter().map(|d| &**d as _).collect();
        let mut section = section.clone();
        if let Some(v) = input_of(&section) {
            section.insert(INPUT.into(), v);
        }
        let section = &section;
        let mut device = DriverChoice::load(section, SOUND, &list);
        device.id = self.driver.into();
        let (mut before, mut after) = (Section::new(), Section::new());
        self.device.save(&mut before);
        device.save(&mut after);
        if before != after {
            self.input = None;
        }
        self.device = device;
        for (k, _) in self.decoder.settings() {
            let skip = matches!(k, "device" | "my_call");
            if let Some(v) = section.get(&cw4i_key(k)).filter(|_| !skip) {
                self.decoder.load_setting(k, v);
            }
        }
        self.extension.formats =
            extension::Formats::load_with(section, extension::keyer_abbrev(others));
        let table = self.extension.formats.table.clone();
        self.decoder.set_cut_table(table);
    }

    fn save(&self, section: &mut Section) {
        for (k, v) in self.device.section() {
            section.insert(format!("{}.{k}", self.device.id), v);
        }
        for (k, v) in self.decoder.settings() {
            if !matches!(k, "device" | "my_call") {
                section.insert(cw4i_key(k), v);
            }
        }
        self.extension.formats.save(section);
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.page(300, |ctx| extension::Options::new(ctx.new_drivers().take()));
    b.panel(160, |ctx| {
        CwPanel::new(ctx.new_drivers().take(), ctx.ports.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_fill_the_entry_row() {
        assert_eq!(
            action(Pick::Callsign("JA1ZLO".into())),
            EntryAction::Callsign("JA1ZLO".into())
        );
        assert_eq!(
            action(Pick::Number("13M".into())),
            EntryAction::Number("13M".into())
        );
    }

    fn silent_wav(path: &std::path::Path, samples: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..samples {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn decoding_goes_on_while_the_window_is_closed() {
        let path = std::env::temp_dir().join(format!("zlog-cw4isr-{}.wav", std::process::id()));
        silent_wav(&path, 400);
        let mut p = CwPanel::default();
        p.decoder.open_file(&path);
        assert!(p.decoder.running());
        std::thread::sleep(std::time::Duration::from_millis(100));
        p.advance();
        assert!(!p.decoder.running(), "the file was decoded to its end");
        std::fs::remove_file(path).unwrap();
    }

    fn loopback_panel(text: &str) -> CwPanel {
        let drv = zlog_ui::loopback::LoopbackDrv::with_texts(zlog_audio::other_stations());
        let mut p = CwPanel::new(vec![Box::new(drv)], PortHub::new());
        p.driver = "loopback";
        let mut s = Section::new();
        for (k, v) in [
            ("loopback.rate", "8000"),
            ("loopback.source", "cw"),
            ("loopback.text", text),
        ] {
            s.insert(k.into(), v.into());
        }
        p.load(&s);
        p
    }

    #[test]
    fn the_loopback_text_source_is_decoded() {
        let mut p = loopback_panel("CQ TEST JA1ZLO");
        p.listen();
        assert!(p.listening(), "{:?}", p.error);
        let start = std::time::Instant::now();
        while !p
            .decoder
            .stations()
            .iter()
            .any(|s| s.text().contains("JA1ZLO"))
        {
            assert!(start.elapsed() < std::time::Duration::from_secs(20));
            std::thread::sleep(std::time::Duration::from_millis(50));
            p.advance();
        }
        p.stop();
        assert!(!p.listening() && !p.decoder.running());
    }

    #[test]
    fn changing_the_extension_options_keeps_listening() {
        let mut p = loopback_panel("E");
        p.listen();
        let mut s = Section::new();
        p.save(&mut s);
        s.insert("extension".into(), "true".into());
        p.load(&s);
        assert!(p.listening() && p.extension.formats.enabled);
        s.insert("loopback.rate".into(), "12000".into());
        p.load(&s);
        assert!(!p.listening(), "a new device setting stops the old input");
    }

    #[test]
    fn the_audio_input_is_one_of_the_registered_drivers() {
        let p = loopback_panel("E");
        let mut s = Section::new();
        p.save(&mut s);
        assert!(!s.contains_key("device"));
        assert_eq!(s.get("loopback.rate").map(String::as_str), Some("8000"));
        assert!(p.decoder.host_input);
        let mut none = CwPanel::default();
        none.listen();
        assert!(!none.listening() && none.error.is_some());
    }

    #[test]
    fn panel_settings_persist_without_the_device() {
        let mut p = CwPanel::default();
        p.decoder.load_setting("waterfall", "left");
        p.decoder.load_setting("device", "Mic");
        p.extension.formats.enabled = true;
        let mut s = Section::new();
        p.save(&mut s);
        assert!(!s.contains_key("cw4i.device"));
        let mut q = CwPanel::default();
        q.load(&s);
        assert_eq!(q.decoder.place, cw4i_ui::Place::Left);
        assert!(q.extension.formats.enabled);
        assert_eq!(q.decoder.device, None);
    }

    #[test]
    fn a_clicked_contact_becomes_entry_actions() {
        let mut p = CwPanel::default();
        p.extension.formats.enabled = true;
        p.extension.formats.number = extension::NumberKind::Free;
        p.contacts.borrow_mut().push(Contact {
            station: 1,
            callsign: Some("JA1ZLO".into()),
            report: Some("599".into()),
            exchange: vec!["1317M".into()],
            raw_exchange: vec!["1317M".into()],
            freq: 700.0,
            wpm: 25.0,
            p: 0.9,
            time: std::time::SystemTime::now(),
        });
        let contacts = std::mem::take(&mut *p.contacts.borrow_mut());
        for c in contacts {
            let a = p.extension.fill(&c, "", &|_| true);
            p.actions.extend(a);
        }
        assert_eq!(
            p.take_actions(),
            [
                EntryAction::Callsign("JA1ZLO".into()),
                EntryAction::RstRcvd(599),
                EntryAction::Number("1317M".into())
            ]
        );
    }

    struct Any;

    impl zlog_core::Contest for Any {
        fn name(&self) -> &str {
            "test"
        }
        fn band_range(&self) -> (zlog_core::Band, zlog_core::Band) {
            (zlog_core::Band::B1_9, zlog_core::Band::B50)
        }
        fn contest_mode(&self) -> zlog_core::ContestMode {
            zlog_core::ContestMode::Cw
        }
        fn sent_number(
            &self,
            _: zlog_core::Band,
            _: zlog_core::Power,
            _: &zlog_core::Station,
        ) -> String {
            String::new()
        }
        fn valid_number(&self, q: &zlog_core::Qso) -> bool {
            q.nr_rcvd.ends_with('M')
        }
        fn score(&mut self, _: &zlog_core::Log) -> zlog_core::Scoring {
            zlog_core::Scoring::default()
        }
    }

    fn with_rig<R>(hz: Option<u64>, f: impl FnOnce(&View<'_>) -> R) -> R {
        with_rig_in(hz, "en", f)
    }

    fn with_rig_in<R>(hz: Option<u64>, language: &str, f: impl FnOnce(&View<'_>) -> R) -> R {
        let log = zlog_core::Log::default();
        let sheet = zlog_core::ScoreSheet::default();
        let station = zlog_core::Station::default();
        let view = View {
            log: &log,
            contest: &Any,
            sheet: &sheet,
            callsign: "",
            station: &station,
            number: "",
            memo: "",
            rst_sent: "",
            power: zlog_core::Power::P010,
            band: zlog_core::Band::B7,
            mode: zlog_core::Mode::Cw,
            language,
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

    fn heard(call: &str, raw: &[&str]) -> Contact {
        Contact {
            station: 1,
            callsign: Some(call.into()),
            report: Some("599".into()),
            exchange: Vec::new(),
            raw_exchange: raw.iter().map(|s| s.to_string()).collect(),
            freq: 650.0,
            wpm: 25.0,
            p: 0.9,
            time: std::time::SystemTime::now(),
        }
    }

    fn spots(p: &mut CwPanel) -> Vec<zlog_ui::Spot> {
        p.take_actions()
            .into_iter()
            .filter_map(|a| match a {
                EntryAction::Spot(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn confirmed_stations_reach_the_scope_only_when_enabled() {
        let mut p = CwPanel::default();
        let heard = [
            heard("JA1ZLO", &["1317M"]),
            heard("JA1YWX", &["13"]),
            heard("JA1YAA", &[]),
            heard("5NN", &[]),
        ];
        with_rig(Some(7_010_000), |v| p.publish(&heard, 7_010_000, v));
        assert!(spots(&mut p).is_empty(), "off by default");
        let mut s = Section::new();
        p.save(&mut s);
        s.insert("extension.scope".into(), "true".into());
        p.load(&s);
        with_rig(Some(7_010_000), |v| p.publish(&heard, 7_010_000, v));
        let got: Vec<_> = spots(&mut p)
            .into_iter()
            .map(|s| (s.callsign, s.number, s.hz, s.source))
            .collect();
        assert_eq!(
            got,
            [
                ("JA1ZLO".into(), "1317M".into(), 7_010_050, "CW4I".into()),
                ("JA1YAA".into(), String::new(), 7_010_050, "CW4I".into())
            ]
        );
        with_rig(Some(7_010_000), |v| p.publish(&heard, 7_010_000, v));
        assert!(spots(&mut p).is_empty(), "throttled");
        p.feed = SpotFeed::default();
        s.insert("extension.callsign".into(), ".*".into());
        s.insert("extension.number".into(), "free".into());
        p.load(&s);
        with_rig(Some(7_010_000), |v| p.publish(&heard, 7_010_000, v));
        assert_eq!(spots(&mut p).len(), 4, "unfiltered");
    }

    #[test]
    fn the_cut_table_reaches_the_decoder() {
        let mut p = CwPanel::default();
        assert_eq!(p.decoder.cut_table(), &cw4i_ui::CutTable::default());
        let mut s = Section::new();
        s.insert("extension.cut.table".into(), "O0 A1 N9".into());
        p.load(&s);
        let t = cw4i_ui::CutTable::new([('O', '0'), ('A', '1'), ('N', '9')]);
        assert_eq!(p.decoder.cut_table(), &t);
        assert_eq!(p.extension.formats.table, t);
    }

    struct FakeSound(std::rc::Rc<RefCell<Option<String>>>);

    impl DeviceDriver for FakeSound {
        fn id(&self) -> &str {
            SOUND
        }

        fn title(&self, _language: &str) -> String {
            "Sound".into()
        }

        fn settings(&self, _language: &str) -> Vec<zlog_ui::device::SettingField> {
            let list = vec![
                (String::new(), "Default".to_string()),
                ("USB".to_string(), "USB".to_string()),
            ];
            vec![zlog_ui::device::SettingField::new(
                "input",
                "Input",
                zlog_ui::device::SettingKind::Choice(list),
            )]
        }
    }

    impl AudioDrv for FakeSound {
        fn open(&self, s: &Section, _ports: &PortHub) -> Result<Box<dyn AudioIo>, String> {
            *self.0.borrow_mut() = Some(s.get("input").cloned().unwrap_or_default());
            Err("opened".into())
        }
    }

    fn opened_with(section: &Section) -> Option<String> {
        let seen = std::rc::Rc::new(RefCell::new(None));
        let mut p = CwPanel::new(vec![Box::new(FakeSound(seen.clone()))], PortHub::new());
        p.load(section);
        p.listen();
        seen.take()
    }

    #[test]
    fn starting_uses_the_chosen_input_or_the_default() {
        assert_eq!(opened_with(&Section::new()).as_deref(), Some(""));
        let mut s = Section::new();
        s.insert(INPUT.into(), "USB".into());
        assert_eq!(opened_with(&s).as_deref(), Some("USB"));
        s.insert("device".into(), "loopback".into());
        assert_eq!(
            opened_with(&s).as_deref(),
            Some("USB"),
            "old loopback ignored"
        );
    }

    #[test]
    fn an_old_device_key_is_dropped_and_the_input_kept() {
        let seen = std::rc::Rc::new(RefCell::new(None));
        let mut p = CwPanel::new(vec![Box::new(FakeSound(seen))], PortHub::new());
        let mut old = Section::new();
        old.insert("device".into(), "sound".into());
        old.insert("sound.input".into(), "USB".into());
        old.insert("cw4i.my_call".into(), "JA1ZLO".into());
        p.load(&old);
        let mut s = Section::new();
        p.save(&mut s);
        assert!(!s.contains_key("device") && !s.contains_key("cw4i.my_call"));
        assert_eq!(s[INPUT], "USB");
    }

    fn options_page(section: Section, w: f32) -> (Vec<String>, Section, f32) {
        use egui_kittest::kittest::{NodeT, Queryable};
        use zlog_ui::SettingsPage;
        let seen = std::rc::Rc::new(RefCell::new(None));
        let mut page = extension::Options::new(vec![Box::new(FakeSound(seen))]);
        let mut h = egui_kittest::Harness::builder()
            .with_size([w, 900.0])
            .with_accessibility_check(false)
            .build_ui_state(|ui, s: &mut Section| page.ui(ui, s, "en"), section);
        h.run_steps(2);
        let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, 900.0));
        let mut labels = Vec::new();
        for n in h.root().children_recursive() {
            let a = n.accesskit_node();
            if let Some(l) = a.label().or(a.value()) {
                labels.push(l);
            }
            if matches!(
                a.role(),
                egui::accesskit::Role::TextInput | egui::accesskit::Role::ComboBox
            ) {
                assert!(
                    clip.contains_rect(n.rect()),
                    "{w}: {:?} {:?}",
                    a.label(),
                    n.rect()
                );
            }
        }
        h.get_all_by_role(egui::accesskit::Role::ComboBox)
            .next()
            .unwrap()
            .click();
        h.run_steps(2);
        for n in h.root().children_recursive() {
            if let Some(l) = n.accesskit_node().label() {
                labels.push(l);
            }
        }
        let field = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .map(|n| n.rect().width())
            .fold(0.0, f32::max);
        (labels, h.state().clone(), field)
    }

    #[test]
    fn the_options_page_offers_the_input_without_loopback() {
        let (labels, s, narrow) = options_page(Section::new(), 360.0);
        let (_, _, wide) = options_page(Section::new(), 720.0);
        assert!(narrow > 150.0 && wide > narrow + 300.0, "{narrow} {wide}");
        assert!(labels.iter().any(|l| l == "Audio input"), "{labels:?}");
        assert!(labels.iter().any(|l| l == "Default"), "{labels:?}");
        assert!(labels.iter().any(|l| l == "USB"), "{labels:?}");
        assert!(!labels.iter().any(|l| l == "Simulator"), "{labels:?}");
        assert!(!labels.iter().any(|l| l.contains("oopback")), "{labels:?}");
        assert_eq!(s[INPUT], "");
        let mut old = Section::new();
        old.insert(INPUT.into(), "USB".into());
        assert_eq!(options_page(old, 600.0).1[INPUT], "USB", "kept");
        let mut old = Section::new();
        old.insert(INPUT.into(), SIMULATOR.into());
        assert_eq!(
            options_page(old, 600.0).1[INPUT],
            "",
            "falls back to Default"
        );
        let mut legacy = Section::new();
        legacy.insert("input".into(), "USB".into());
        assert_eq!(options_page(legacy, 600.0).1[INPUT], "USB", "migrated");
        let mut toggle = Section::new();
        toggle.insert("sim".into(), "true".into());
        let s = options_page(toggle, 600.0).1;
        assert_eq!(s[INPUT], "");
        assert!(!s.contains_key("sim"));
    }

    type Window = egui_kittest::Harness<'static, (CwPanel, egui::Rect)>;

    fn window(input: &str, w: f32) -> Window {
        window_in(input, w, "en")
    }

    fn window_in(input: &str, w: f32, lang: &'static str) -> Window {
        let seen = std::rc::Rc::new(RefCell::new(None));
        let mut p = CwPanel::new(vec![Box::new(FakeSound(seen))], PortHub::new());
        let mut s = Section::new();
        s.insert(INPUT.into(), input.into());
        p.load(&s);
        let mut h = egui_kittest::Harness::builder()
            .with_size([w, 480.0])
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, s: &mut (CwPanel, egui::Rect)| {
                    ui.ctx()
                        .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                    s.1 = ui.clip_rect();
                    with_rig_in(None, lang, |v| s.0.ui(ui, v))
                },
                (p, egui::Rect::NOTHING),
            );
        h.run_steps(3);
        h
    }

    #[test]
    fn the_window_with_a_real_device_has_only_the_toolbar() {
        use egui_kittest::kittest::Queryable;
        for w in [280.0, 360.0, 600.0] {
            let h = window("USB", w);
            assert_eq!(
                h.query_all_by_role(egui::accesskit::Role::ComboBox).count(),
                0
            );
            assert!(h.query_by_label("Show candidates").is_none());
            assert!(h.query_by_label("Simulator").is_none(), "no toggle at {w}");
            assert!(h.query_by_label("Play").is_none());
            let inputs = h.query_all_by_role(egui::accesskit::Role::MultilineTextInput);
            assert_eq!(inputs.count(), 0, "no text box at {w}");
            assert!(h.query_by_label("Start").is_some(), "at {w}");
        }
    }

    fn decoded(h: &Window) -> String {
        let stations = h.state().0.decoder.stations();
        stations
            .iter()
            .map(|s| s.text())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_simulator_input_is_gone_and_falls_back_to_the_default() {
        use egui_kittest::kittest::Queryable;
        let h = window(SIMULATOR, 600.0);
        assert!(h.query_by_label("Play").is_none());
        let inputs = h.query_all_by_role(egui::accesskit::Role::MultilineTextInput);
        assert_eq!(inputs.count(), 0);
        assert!(h
            .query_by_label("Press Start, or open a WAV file.")
            .is_some());
        let drop = "Press Start, or drop a WAV file here.";
        assert!(h.query_by_label(drop).is_none());
        let mut s = Section::new();
        h.state().0.save(&mut s);
        assert_eq!(s.get(INPUT).map(String::as_str), Some(""));
        assert_eq!(
            opened_with(&s).as_deref(),
            Some(""),
            "Start opens the default"
        );
        let mut old = Section::new();
        old.insert("sim".into(), "true".into());
        assert_eq!(opened_with(&old).as_deref(), Some(""));
    }

    #[test]
    fn an_opened_wav_plays_in_real_time_and_stop_ends_it() {
        use egui_kittest::kittest::Queryable;
        let path =
            std::env::temp_dir().join(format!("zlog-cw4isr-open-{}.wav", std::process::id()));
        let audio = sim::signal("CQ JA1ZLO JA1ZLO", sim::RATE);
        sim::write_wav(&path, &audio, sim::RATE).unwrap();
        let mut h = window("USB", 600.0);
        zlog_ui::dialog::set_test_pick(Some(path.clone()));
        h.get_by_label("Open…").click();
        h.run_steps(3);
        zlog_ui::dialog::set_test_pick(None);
        assert!(h.state().0.decoder.running());
        assert!(h.query_by_label("Stop").is_some());
        let secs = audio.len() as f64 / f64::from(sim::RATE);
        let start = std::time::Instant::now();
        while h.state().0.decoder.running() {
            assert!(start.elapsed() < std::time::Duration::from_secs(30));
            std::thread::sleep(std::time::Duration::from_millis(50));
            h.run_steps(1);
        }
        let took = start.elapsed().as_secs_f64();
        assert!(took > secs * 0.8, "real time: {took} s for {secs} s");
        assert!(decoded(&h).contains("JA1ZLO"), "{}", decoded(&h));
        h.run_steps(1);
        zlog_ui::dialog::set_test_pick(Some(path.clone()));
        h.get_by_label("Open…").click();
        h.run_steps(3);
        zlog_ui::dialog::set_test_pick(None);
        assert!(h.state().0.decoder.running());
        h.get_by_label("Stop").click();
        h.run_steps(3);
        assert!(!h.state().0.decoder.running());
        assert!(h.query_by_label("Start").is_some());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_japanese_toolbar_fits() {
        use egui_kittest::kittest::Queryable;
        for w in [280.0, 360.0, 600.0] {
            let h = window_in("USB", w, "ja");
            assert!(h.query_by_label("開始").is_some(), "at {w}");
            assert!(h.query_by_label("開く…").is_some(), "at {w}");
            assert!(h
                .query_by_label("開始を押すか、WAV を開いてください。")
                .is_some());
        }
    }

    #[test]
    fn starting_without_an_input_points_to_options() {
        let mut p = CwPanel::default();
        p.listen();
        assert_eq!(p.error.as_deref(), Some(texts().get("en", "no_device")));
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
