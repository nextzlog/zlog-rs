use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::Section;

pub trait DeviceDriver {
    fn id(&self) -> &str;

    fn title(&self, language: &str) -> String;

    fn settings(&self, _language: &str) -> Vec<SettingField> {
        Vec::new()
    }

    fn simulated(&self) -> bool {
        false
    }

    fn status(&self, _language: &str) -> Option<String> {
        None
    }

    fn remote(&self) -> bool {
        false
    }
}

static REOPEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn reopen() {
    REOPEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    zlog_core::wake::wake();
}

pub fn reopened() -> u64 {
    REOPEN.load(std::sync::atomic::Ordering::SeqCst)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingField {
    pub key: &'static str,
    pub label: String,
    pub default: String,
    pub kind: SettingKind,
    pub advanced: bool,
}

impl SettingField {
    pub fn new(key: &'static str, label: impl Into<String>, kind: SettingKind) -> Self {
        SettingField {
            key,
            label: label.into(),
            default: String::new(),
            kind,
            advanced: false,
        }
    }

    pub fn advanced(mut self, advanced: bool) -> Self {
        self.advanced = advanced;
        self
    }

    pub fn default_value(mut self, value: impl Into<String>) -> Self {
        self.default = value.into();
        self
    }

    pub fn value<'a>(&'a self, section: &'a Section) -> &'a str {
        section.get(self.key).map_or(&self.default, |v| v)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingKind {
    Port(Vec<String>),
    Baud,
    Choice(Vec<(String, String)>),
    Bool,
    Int { min: i64, max: i64 },
    Text,
    File { filters: Vec<(String, Vec<String>)> },
}

pub const BAUDS: [u32; 8] = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200];

#[derive(Default)]
pub struct Drivers(Vec<Box<dyn Any>>);

impl Drivers {
    pub fn new() -> Self {
        Drivers::default()
    }

    pub fn add<T: ?Sized + 'static>(&mut self, driver: Box<T>) {
        self.0.push(Box::new(driver));
    }

    pub fn with<T: ?Sized + 'static>(mut self, driver: Box<T>) -> Self {
        self.add(driver);
        self
    }

    pub fn take<T: ?Sized + 'static>(&mut self) -> Vec<Box<T>> {
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.0)
            .into_iter()
            .partition(|d| d.is::<Box<T>>());
        self.0 = rest;
        mine.into_iter()
            .filter_map(|d| d.downcast::<Box<T>>().ok().map(|b| *b))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub const NONE: &str = "none";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DriverChoice {
    pub id: String,
    pub sections: BTreeMap<String, Section>,
    fields: Option<(String, String, Vec<SettingField>)>,
}

const DEVICE: &str = "device";

impl DriverChoice {
    pub fn new(id: &str) -> Self {
        DriverChoice {
            id: id.into(),
            ..DriverChoice::default()
        }
    }

    pub fn load(section: &Section, default: &str, drivers: &[&dyn DeviceDriver]) -> Self {
        let mut sections = BTreeMap::new();
        for d in drivers {
            let prefix = format!("{}.", d.id());
            let mut own: Section = section
                .iter()
                .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_string(), v.clone())))
                .collect();
            for f in d.settings("en") {
                if let (false, Some(v)) = (own.contains_key(f.key), section.get(f.key)) {
                    own.insert(f.key.into(), v.clone());
                }
            }
            sections.insert(d.id().to_string(), own);
        }
        DriverChoice {
            id: section
                .get(DEVICE)
                .cloned()
                .unwrap_or_else(|| default.into()),
            sections,
            fields: None,
        }
    }

    pub fn save(&self, section: &mut Section) {
        section.insert(DEVICE.into(), self.id.clone());
        for (id, s) in &self.sections {
            for (k, v) in s {
                section.insert(format!("{id}.{k}"), v.clone());
            }
        }
    }

    pub fn section(&self) -> Section {
        self.sections.get(&self.id).cloned().unwrap_or_default()
    }

    pub fn is_none(&self) -> bool {
        self.id == NONE
    }

    pub fn find<'a, T: ?Sized + DeviceDriver>(&self, drivers: &'a [Box<T>]) -> Option<&'a T> {
        drivers.iter().find(|d| d.id() == self.id).map(|d| &**d)
    }

    pub fn refresh(&mut self) {
        self.fields = None;
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        language: &str,
        label: &str,
        none: &str,
        drivers: &[&dyn DeviceDriver],
    ) -> bool {
        let before = self.id.clone();
        let title = |id: &str| {
            drivers
                .iter()
                .find(|d| d.id() == id)
                .map_or_else(|| none.to_string(), |d| d.title(language))
        };
        crate::form(ui, ("device_form", label), &[label], |ui, form| {
            ui.label(label);
            let combo = form.fit(ui, |ui| {
                form.combo(("device", label))
                    .selected_text(title(&self.id))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.id, NONE.to_string(), none);
                        for d in drivers {
                            ui.selectable_value(
                                &mut self.id,
                                d.id().to_string(),
                                d.title(language),
                            );
                        }
                    })
            });
            if combo.response.clicked() {
                self.refresh();
            }
            ui.end_row();
        });
        let stale = self
            .fields
            .as_ref()
            .is_none_or(|(id, lang, _)| *id != self.id || lang != language);
        if stale {
            let fields = drivers
                .iter()
                .find(|d| d.id() == self.id)
                .map(|d| d.settings(language))
                .unwrap_or_default();
            self.fields = Some((self.id.clone(), language.into(), fields));
        }
        let Some((id, _, fields)) = self.fields.as_ref() else {
            return before != self.id;
        };
        let mut changed = false;
        let status = drivers
            .iter()
            .find(|d| d.id() == id)
            .and_then(|d| d.status(language));
        let status = |ui: &mut egui::Ui| {
            if let Some(s) = status {
                ui.weak(s);
            }
        };
        if !fields.is_empty() {
            let section = self.sections.entry(id.clone()).or_default();
            changed = crate::dialog::settings_form_with(ui, fields, section, language, status);
        } else {
            status(ui);
        }
        changed || before != self.id
    }
}

pub fn page_ui(
    ui: &mut egui::Ui,
    slot: &mut Option<DriverChoice>,
    section: &mut Section,
    default: &str,
    labels: (&str, &str, &str),
    drivers: &[&dyn DeviceDriver],
) {
    let (language, label, none) = labels;
    let saved = |d: &DriverChoice| {
        let mut s = Section::new();
        d.save(&mut s);
        s
    };
    let stale = slot
        .as_ref()
        .is_none_or(|d| saved(d).iter().any(|(k, v)| section.get(k) != Some(v)));
    if stale {
        *slot = Some(DriverChoice::load(section, default, drivers));
    }
    let d = slot.as_mut().expect("loaded");
    d.ui(ui, language, label, none, drivers);
    d.save(section);
}

pub const SOUND_SECTION: &str = "sound";

pub const OWN_SOUND: &str = "own_sound";

pub fn own_sound(own: &Section, shared: Option<&Section>) -> bool {
    own.get(OWN_SOUND).is_some_and(|v| v == "true")
        || (shared.is_none_or(|s| !s.contains_key(DEVICE)) && own.contains_key(DEVICE))
}

pub fn sound_section(own: &Section, others: &crate::Sections) -> Section {
    let shared = others.get(SOUND_SECTION);
    match (own_sound(own, shared), shared) {
        (false, Some(s)) => s.clone(),
        _ => own.clone(),
    }
}

pub fn own_sound_ui(
    ui: &mut egui::Ui,
    section: &mut Section,
    language: &str,
    body: impl FnOnce(&mut egui::Ui, &mut Section),
) {
    let t = |k| crate::texts().get(language, k);
    let mut own = section.get(OWN_SOUND).is_some_and(|v| v == "true");
    egui::CollapsingHeader::new(t("settings.advanced"))
        .id_salt("own_sound")
        .default_open(own)
        .show(ui, |ui| {
            ui.checkbox(&mut own, t("sound.own"))
                .on_hover_text(t("sound.own.tip"));
            if own {
                body(ui, section);
            }
        });
    if !own {
        ui.weak(t("sound.shared"));
    }
    section.insert(OWN_SOUND.into(), own.to_string());
}

pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs())).min(1.0)
}

pub fn level_meter(ui: &mut egui::Ui, level: f32, language: &str) -> egui::Response {
    let t = crate::texts();
    let db = 20.0 * level.max(1e-5).log10();
    ui.add(egui::ProgressBar::new(level.clamp(0.0, 1.0)).text(format!("{db:.0} dB")))
        .on_hover_text(t.get(language, "sound.level.tip"))
}

pub fn same_device(a: &DriverChoice, b: &DriverChoice) -> bool {
    let (mut x, mut y) = (Section::new(), Section::new());
    a.save(&mut x);
    b.save(&mut y);
    x == y
}

pub use zlog_serial::{Boxed, Shared as SharedPort, Slot, Transport as PortIo};

pub const KEYER_PORT: &str = "keyer";

#[derive(Clone, Default)]
pub struct PortHub(Arc<Mutex<BTreeMap<String, Slot>>>);

impl PortHub {
    pub fn new() -> Self {
        PortHub::default()
    }

    pub fn slot(&self, name: &str) -> Slot {
        self.lock().entry(name.into()).or_default().clone()
    }

    pub fn publish(&self, name: &str, port: Boxed) -> SharedPort {
        let shared = SharedPort::new(port);
        self.slot(name).publish(shared.clone());
        shared
    }

    pub fn open(&self, name: &str) -> Option<SharedPort> {
        self.lock().get(name).and_then(Slot::get)
    }

    pub fn withdraw(&self, name: &str) {
        if let Some(s) = self.lock().get(name) {
            s.withdraw();
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.lock()
            .iter()
            .filter(|(_, s)| s.get().is_some())
            .map(|(n, _)| n.clone())
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Slot>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub trait AudioIo: Send {
    fn sample_rate(&self) -> u32;
    fn play(&mut self, samples: &[f32]);
    fn stop(&mut self);
    fn fade_out(&mut self, _samples: usize) {
        self.stop();
    }
    fn playing(&self) -> bool;
    fn record(&mut self, out: &mut Vec<f32>);
}

pub trait AudioDrv: DeviceDriver {
    fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn AudioIo>, String>;
}

pub type Stamp = chrono::DateTime<chrono::Utc>;

pub fn stamp_after(at: Stamp, samples: usize, rate: u32) -> Stamp {
    let micros = samples as u128 * 1_000_000 / u128::from(rate.max(1));
    at + chrono::TimeDelta::microseconds(micros.min(i64::MAX as u128) as i64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModemKind {
    Stream,
    Slots { period: Duration },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parity {
    #[default]
    Any,
    Even,
    Odd,
}

impl ModemKind {
    fn period_ms(&self) -> Option<i64> {
        match self {
            ModemKind::Stream => None,
            ModemKind::Slots { period } => Some((period.as_millis() as i64).max(1)),
        }
    }

    pub fn slot_of(&self, at: Stamp) -> Option<Stamp> {
        let p = self.period_ms()?;
        let ms = at.timestamp_millis().div_euclid(p) * p;
        chrono::DateTime::from_timestamp_millis(ms)
    }

    pub fn parity_of(&self, slot: Stamp) -> Option<Parity> {
        let p = self.period_ms()?;
        Some(match slot.timestamp_millis().div_euclid(p) % 2 {
            0 => Parity::Even,
            _ => Parity::Odd,
        })
    }

    pub fn next_slot(&self, now: Stamp, parity: Parity) -> Stamp {
        let Some(p) = self.period_ms() else {
            return now;
        };
        let mut n = (now.timestamp_millis() + p - 1).div_euclid(p);
        let odd = n.rem_euclid(2) == 1;
        if matches!((parity, odd), (Parity::Even, true) | (Parity::Odd, false)) {
            n += 1;
        }
        chrono::DateTime::from_timestamp_millis(n * p).unwrap_or(now)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Decode {
    pub text: String,
    pub hz: f32,
    pub snr: Option<f32>,
    pub dt: Option<f32>,
    pub at: Stamp,
    pub slot: Option<Stamp>,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    pub at: Stamp,
    pub samples: Vec<f32>,
}

pub trait Modem: Send {
    fn kind(&self) -> ModemKind;
    fn sample_rate(&self) -> u32;
    fn rx(&mut self, samples: &[f32], at: Stamp) -> Vec<Decode>;
    fn tx(&mut self, text: &str, hz: f32) -> Result<Vec<f32>, String>;

    fn schedule(
        &mut self,
        text: &str,
        hz: f32,
        now: Stamp,
        parity: Parity,
    ) -> Result<Burst, String> {
        let at = self.kind().next_slot(now, parity);
        Ok(Burst {
            at,
            samples: self.tx(text, hz)?,
        })
    }

    fn options(&self, _language: &str) -> Vec<SettingField> {
        Vec::new()
    }

    fn set_option(&mut self, key: &str, _value: &str) -> Result<(), String> {
        Err(format!("unknown option: {key}"))
    }
}

pub trait ModemDrv: DeviceDriver {
    fn mode(&self) -> zlog_core::Mode;
    fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn Modem>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_published_port_is_shared_by_name() {
        let hub = PortHub::new();
        assert!(hub.open("keyer").is_none());
        let (port, peer) = zlog_serial::Loop::pair();
        let mut mine = hub.publish("keyer", Box::new(port));
        let mut other = hub.clone().open("keyer").expect("published");
        other.write(&[0x90]).unwrap();
        assert_eq!(peer.read_written(), [0x90]);
        peer.reply(&[0x91]);
        let mut got = Vec::new();
        mine.read(&mut got).unwrap();
        assert_eq!(got, [0x91], "both hold the same port");
        assert_eq!(hub.names(), ["keyer"]);
        assert!(mine.same(&hub.slot("keyer").get().unwrap()));
        hub.withdraw("keyer");
        assert!(hub.open("keyer").is_none());
        assert!(hub.names().is_empty());
    }

    struct Loop;

    impl Modem for Loop {
        fn kind(&self) -> ModemKind {
            ModemKind::Stream
        }
        fn sample_rate(&self) -> u32 {
            8000
        }
        fn rx(&mut self, samples: &[f32], at: Stamp) -> Vec<Decode> {
            let text = samples.iter().map(|s| (*s * 128.0) as u8 as char).collect();
            vec![Decode {
                text,
                hz: 1500.0,
                at,
                ..Default::default()
            }]
        }
        fn tx(&mut self, text: &str, _hz: f32) -> Result<Vec<f32>, String> {
            Ok(text.bytes().map(|b| b as f32 / 128.0).collect())
        }
    }

    struct LoopDrv;

    impl DeviceDriver for LoopDrv {
        fn id(&self) -> &str {
            "loop"
        }
        fn title(&self, _language: &str) -> String {
            "Loop".into()
        }
        fn simulated(&self) -> bool {
            true
        }
    }

    impl ModemDrv for LoopDrv {
        fn mode(&self) -> zlog_core::Mode {
            zlog_core::Mode::Rtty
        }
        fn open(&self, _s: &Section, _p: &PortHub) -> Result<Box<dyn Modem>, String> {
            Ok(Box::new(Loop))
        }
    }

    struct NullAudio;

    impl DeviceDriver for NullAudio {
        fn id(&self) -> &str {
            "null_audio"
        }
        fn title(&self, _language: &str) -> String {
            "None".into()
        }
    }

    impl AudioDrv for NullAudio {
        fn open(&self, _s: &Section, _p: &PortHub) -> Result<Box<dyn AudioIo>, String> {
            Err("no device".into())
        }
    }

    #[test]
    fn drivers_come_back_typed_by_kind_in_order() {
        let mut d = Drivers::new()
            .with::<dyn ModemDrv>(Box::new(LoopDrv))
            .with::<dyn AudioDrv>(Box::new(NullAudio));
        assert_eq!(d.len(), 2);
        let modems = d.take::<dyn ModemDrv>();
        assert_eq!(modems.len(), 1);
        assert_eq!(d.len(), 1, "the other kinds stay");
        let m = &modems[0];
        assert!(m.simulated() && m.settings("en").is_empty());
        let mut modem = m.open(&Section::new(), &PortHub::new()).unwrap();
        let audio = modem.tx("CQ", 1500.0).unwrap();
        let at = Stamp::default();
        assert_eq!(modem.rx(&audio, at)[0].text, "CQ");
        assert_eq!(modem.kind(), ModemKind::Stream);
        let burst = modem.schedule("CQ", 1500.0, at, Parity::Even).unwrap();
        assert_eq!((burst.at, burst.samples), (at, audio));
        assert!(modem.options("ja").is_empty() && modem.set_option("x", "1").is_err());
        let audio = d.take::<dyn AudioDrv>();
        assert!(audio[0].open(&Section::new(), &PortHub::new()).is_err());
        assert!(d.is_empty());
        assert!(d.take::<dyn ModemDrv>().is_empty());
    }

    struct Wk;

    impl DeviceDriver for Wk {
        fn id(&self) -> &str {
            "winkeyer"
        }
        fn title(&self, _language: &str) -> String {
            "WinKeyer".into()
        }
        fn settings(&self, _language: &str) -> Vec<SettingField> {
            vec![
                SettingField::new("port", "Port", SettingKind::Port(Vec::new())),
                SettingField::new("sidetone", "Sidetone", SettingKind::Bool).default_value("true"),
            ]
        }
    }

    #[test]
    fn old_flat_settings_reach_the_driver_and_are_saved_as_its_own() {
        let mut old = Section::new();
        for (k, v) in [("device", "winkeyer"), ("port", "COM3"), ("speed", "30")] {
            old.insert(k.into(), v.into());
        }
        let c = DriverChoice::load(&old, NONE, &[&Wk, &NullAudio]);
        assert_eq!(c.id, "winkeyer");
        assert_eq!(c.section()["port"], "COM3");
        assert!(
            !c.section().contains_key("speed"),
            "not a setting of the driver"
        );
        let mut saved = Section::new();
        c.save(&mut saved);
        assert_eq!(saved["winkeyer.port"], "COM3");
        assert_eq!(saved["device"], "winkeyer");
        saved.insert("port".into(), "COM9".into());
        let again = DriverChoice::load(&saved, NONE, &[&Wk]);
        assert_eq!(again.section()["port"], "COM3");
        assert_eq!(
            again,
            DriverChoice {
                fields: None,
                ..c.clone()
            }
            .without(&["null_audio"])
        );
        let fresh = DriverChoice::load(&Section::new(), "winkeyer", &[&Wk]);
        assert_eq!(fresh.id, "winkeyer");
        assert!(fresh.section().is_empty() && !fresh.is_none());
        assert!(DriverChoice::load(&Section::new(), NONE, &[]).is_none());
    }

    #[test]
    fn the_choice_draws_the_settings_of_the_driver_chosen() {
        let mut c = DriverChoice::new("winkeyer");
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let changed = c.ui(ui, "en", "Keyer", "None", &[&Wk, &NullAudio]);
                assert!(!changed);
            });
            out.textures_delta.clear();
        }
        let (id, _, fields) = c.fields.as_ref().unwrap();
        assert_eq!((id.as_str(), fields.len()), ("winkeyer", 2));
        c.refresh();
        assert!(c.fields.is_none());
    }

    impl DriverChoice {
        fn without(mut self, ids: &[&str]) -> Self {
            for id in ids {
                self.sections.remove(*id);
            }
            self
        }
    }

    fn at(text: &str) -> Stamp {
        text.parse().unwrap()
    }

    #[test]
    fn slots_are_aligned_to_the_epoch_and_alternate() {
        let k = ModemKind::Slots {
            period: Duration::from_secs(15),
        };
        let t = at("2026-10-25T00:00:07Z");
        assert_eq!(k.slot_of(t), Some(at("2026-10-25T00:00:00Z")));
        assert_eq!(k.parity_of(at("2026-10-25T00:00:15Z")), Some(Parity::Odd));
        assert_eq!(k.next_slot(t, Parity::Any), at("2026-10-25T00:00:15Z"));
        assert_eq!(k.next_slot(t, Parity::Odd), at("2026-10-25T00:00:15Z"));
        assert_eq!(k.next_slot(t, Parity::Even), at("2026-10-25T00:00:30Z"));
        let edge = at("2026-10-25T00:00:30Z");
        assert_eq!(k.next_slot(edge, Parity::Even), edge);
        assert_eq!(ModemKind::Stream.next_slot(t, Parity::Odd), t);
        assert_eq!(ModemKind::Stream.slot_of(t), None);
    }

    #[test]
    fn a_sample_offset_moves_the_stamp() {
        let t = at("2026-10-25T00:00:00Z");
        assert_eq!(stamp_after(t, 12000, 8000), at("2026-10-25T00:00:01.5Z"));
    }

    #[test]
    fn a_setting_reads_its_section_or_its_default() {
        let f = SettingField::new("baud", "Speed", SettingKind::Baud).default_value("1200");
        let mut s = Section::new();
        assert_eq!(f.value(&s), "1200");
        s.insert("baud".into(), "9600".into());
        assert_eq!(f.value(&s), "9600");
    }
}
