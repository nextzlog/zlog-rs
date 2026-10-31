use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use zlog_net::Fetch;
use zlog_ui::{Panel, View};

zlog_core::texts!();

const CITIES: &str = include_str!("../data/allcity.dat");

#[derive(Debug, Clone, PartialEq)]
pub struct License {
    pub callsign: String,
    pub location: String,
    pub class: String,
}

pub trait LicenseSource {
    fn name(&self, language: &str) -> &str;
    fn user_agent(&self) -> &'static str {
        zlog_net::USER_AGENT
    }
    fn notice(&self, language: &str) -> &str;
    fn url(&self, callsign: &str) -> String;
    fn parse(&self, body: &str) -> Result<Vec<License>, String>;
}

pub struct Mic;

impl LicenseSource for Mic {
    fn user_agent(&self) -> &'static str {
        zlog_net::BROWSER_AGENT
    }

    fn name(&self, language: &str) -> &str {
        texts().get(language, "mic.name")
    }

    fn notice(&self, language: &str) -> &str {
        texts().get(language, "mic.notice")
    }

    fn url(&self, callsign: &str) -> String {
        format!(
            "https://www.tele.soumu.go.jp/musen/list?ST=1&OF=2&DA=1&OW=AT&SK=2&DC=1&SC=1&MA={}",
            query_value(callsign)
        )
    }

    fn parse(&self, body: &str) -> Result<Vec<License>, String> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|_| "unexpected response (not JSON)".to_string())?;
        let list = v["musen"].as_array().cloned().unwrap_or_default();
        Ok(list
            .iter()
            .map(|m| {
                let d = &m["detailInfo"];
                let text = |k: &str| d[k].as_str().unwrap_or_default().trim().to_string();
                License {
                    callsign: text("identificationSignals"),
                    location: text("radioEuipmentLocation"),
                    class: text("radioSpec1"),
                }
            })
            .collect())
    }
}

pub fn number_of(location: &str) -> Option<&'static str> {
    CITIES.lines().find_map(|l| {
        l.split_once('\t')
            .filter(|(name, _)| *name == location)
            .map(|(_, n)| n)
    })
}

pub fn class_text(code: &str, language: &str) -> String {
    let fixed = match code.get(2..) {
        Some("F") => texts().get(language, "class.fixed"),
        Some("M") => texts().get(language, "class.mobile"),
        _ => return code.to_string(),
    };
    match code.get(..2) {
        Some(n @ ("1A" | "2A" | "3A" | "4A")) => {
            texts().format(language, "class.amateur", &[&&n[..1], &fixed])
        }
        _ => code.to_string(),
    }
}

pub struct Soumu {
    source: Box<dyn LicenseSource>,
    typed: String,
    typed_at: Instant,
    pending: Option<(String, Fetch)>,
    cache: HashMap<String, Vec<License>>,
    failed: Option<(String, String)>,
    pinned: Option<(String, String)>,
    entry: String,
}

impl Soumu {
    pub fn new(source: impl LicenseSource + 'static) -> Self {
        Soumu {
            source: Box::new(source),
            typed: String::new(),
            typed_at: Instant::now(),
            pending: None,
            cache: HashMap::new(),
            failed: None,
            pinned: None,
            entry: String::new(),
        }
    }
}

impl Soumu {
    fn select(&mut self, call: &str) {
        if call != self.typed {
            self.typed = call.to_string();
            self.typed_at = Instant::now();
            if self.failed.as_ref().is_some_and(|(c, _)| c != call) {
                self.failed = None;
            }
        }
    }

    fn wants_lookup(&self, call: &str) -> bool {
        call.len() >= 4
            && self.typed_at.elapsed() >= Duration::from_secs(1)
            && self.pending.is_none()
            && !self.cache.contains_key(call)
            && self.failed.as_ref().is_none_or(|(c, _)| c != call)
    }

    fn store(&mut self, asked: String, result: Result<Vec<License>, String>) {
        match result {
            Ok(list) => {
                self.cache.insert(asked, list);
            }
            Err(e) => self.failed = Some((asked, e)),
        }
    }
}

impl Default for Soumu {
    fn default() -> Self {
        Soumu::new(Mic)
    }
}

impl Panel for Soumu {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

    fn id(&self) -> &str {
        "soumu"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::LookUp, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::IDENTIFICATION_CARD)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [460.0, 180.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        self.entry = license_call(view.callsign);
        if self.pinned.as_ref().is_some_and(|(_, e)| *e != self.entry) {
            self.pinned = None;
        }
        let call = match &self.pinned {
            Some((c, _)) => c.clone(),
            None => self.entry.clone(),
        };
        self.select(&call);
        let settled = self.typed_at.elapsed() >= Duration::from_secs(1);
        if self.wants_lookup(&call) {
            self.pending = Some((
                call.clone(),
                Fetch::get_as(self.source.url(&call), self.source.user_agent()),
            ));
        }
        if let Some((asked, fetch)) = self.pending.as_mut() {
            if let Some(r) = fetch.poll() {
                let parsed = r.clone().and_then(|body| self.source.parse(&body));
                let asked = asked.clone();
                self.pending = None;
                self.store(asked, parsed);
            }
        }
        if self.pending.is_some() || (call.len() >= 4 && !settled) {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }

        ui.label(
            RichText::new(if call.is_empty() {
                "-".to_string()
            } else {
                call.clone()
            })
            .monospace()
            .strong(),
        );
        let failed = self.failed.as_ref().filter(|(c, _)| *c == call);
        match self
            .cache
            .get(&call)
            .map(Ok)
            .or(failed.map(|(_, e)| Err(e)))
        {
            Some(Ok(list)) if list.is_empty() => {
                ui.label(texts().get(lang, "none"));
            }
            Some(Ok(list)) => {
                egui::Grid::new("licences").striped(true).show(ui, |ui| {
                    for l in list {
                        ui.label(RichText::new(&l.callsign).monospace());
                        ui.add(egui::Label::new(&l.location).wrap());
                        ui.label(RichText::new(number_of(&l.location).unwrap_or("?")).monospace());
                        ui.label(class_text(&l.class, lang));
                        ui.end_row();
                    }
                });
            }
            Some(Err(e)) => {
                ui.colored_label(zlog_ui::palette::error(ui), e);
            }
            None if self.pending.is_some() => {
                ui.spinner();
            }
            None => {}
        }
        ui.separator();
        ui.weak(self.source.name(lang));
        ui.weak(self.source.notice(lang));
    }

    fn qso_menu(&self, language: &str, qso: &zlog_core::Qso) -> Vec<zlog_ui::QsoMenuItem> {
        vec![
            zlog_ui::QsoMenuItem::new(LOOK_UP, texts().get(language, "look_up"))
                .enabled(license_call(&qso.callsign).len() >= 4),
        ]
    }

    fn on_qso_menu(&mut self, id: &str, qso: &zlog_core::Qso) -> bool {
        if id != LOOK_UP {
            return false;
        }
        let call = license_call(&qso.callsign);
        if self.failed.as_ref().is_some_and(|(c, _)| *c == call) {
            self.failed = None;
        }
        self.typed = call.clone();
        self.typed_at = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        self.pinned = Some((call, self.entry.clone()));
        true
    }
}

fn license_call(typed: &str) -> String {
    let q = zlog_core::Qso {
        callsign: typed.trim().to_uppercase(),
        ..zlog_core::Qso::default()
    };
    q.core_call().to_string()
}

fn query_value(value: &str) -> String {
    const SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');
    percent_encoding::utf8_percent_encode(value, SET).to_string()
}

const LOOK_UP: &str = "look-up";

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(490, |_| Soumu::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_callsign_is_percent_encoded() {
        assert!(query_value("JA1/P&OW=x #").ends_with("JA1%2FP%26OW%3Dx%20%23"));
    }

    #[test]
    fn the_licensed_call_is_looked_up() {
        assert_eq!(license_call(" ja1zlo/1 "), "JA1ZLO");
        assert_eq!(license_call("JA1ZLO/P"), "JA1ZLO");
        assert_eq!(license_call("7k1/abc"), "7K1/ABC");
        assert_eq!(license_call("jr0abc"), "JR0ABC");
    }

    #[test]
    fn the_menu_pins_the_licensed_call() {
        let mut s = Soumu::default();
        let q = zlog_core::Qso {
            callsign: "ja1zlo/3".into(),
            ..zlog_core::Qso::default()
        };
        assert!(s.on_qso_menu(LOOK_UP, &q));
        assert_eq!(s.pinned.as_ref().unwrap().0, "JA1ZLO");
        assert_eq!(s.typed, "JA1ZLO");
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn class_in_english() {
        assert_eq!(class_text("2AM", "en"), "Amateur class 2, mobile");
    }

    #[test]
    fn response_as_soumu_api_reads_it() {
        let body = r#"{"musen":[{"detailInfo":{"identificationSignals":"JA1ZLO ","radioEuipmentLocation":"東京都文京区","radioSpec1":"1AF"}}]}"#;
        let list = Mic.parse(body).unwrap();
        assert_eq!(list[0].callsign, "JA1ZLO");
        assert_eq!(number_of(&list[0].location), Some("100105"));
        assert_eq!(class_text(&list[0].class, "ja"), "1アマ固定");
        assert!(Mic.parse("<!doctype html>").is_err());
    }

    #[test]
    fn failed_lookups_are_retried_when_typed_again() {
        let mut s = Soumu::default();
        let settle = |s: &mut Soumu| {
            s.typed_at = Instant::now() - Duration::from_secs(2);
        };
        s.select("JA1ZLO");
        settle(&mut s);
        assert!(s.wants_lookup("JA1ZLO"));
        s.store("JA1ZLO".into(), Err("timeout".into()));
        s.select("JA1ZLO");
        assert!(!s.wants_lookup("JA1ZLO"));
        s.select("JA1ZL");
        s.select("JA1ZLO");
        settle(&mut s);
        assert!(s.wants_lookup("JA1ZLO"));
        s.store("JA1ZLO".into(), Ok(Vec::new()));
        s.select("JA1Z");
        s.select("JA1ZLO");
        settle(&mut s);
        assert!(!s.wants_lookup("JA1ZLO"));
    }

    #[test]
    fn the_qso_menu_pins_the_callsign_until_the_entry_changes() {
        let mut s = Soumu::default();
        let qso = zlog_core::Qso {
            callsign: "ja1zlo".into(),
            ..Default::default()
        };
        assert!(s.qso_menu("en", &qso)[0].enabled);
        assert!(s.on_qso_menu(LOOK_UP, &qso));
        assert!(!s.on_qso_menu("other", &qso));
        assert_eq!(s.pinned, Some(("JA1ZLO".into(), String::new())));
    }

    #[test]
    fn the_qso_menu_retries_a_failed_lookup() {
        let mut s = Soumu::default();
        s.store("JA1ZLO".into(), Err("timeout".into()));
        let qso = zlog_core::Qso {
            callsign: "ja1zlo/1".into(),
            ..Default::default()
        };
        assert!(s.on_qso_menu(LOOK_UP, &qso));
        s.select("JA1ZLO");
        assert!(s.wants_lookup("JA1ZLO"));
    }

    #[test]
    fn the_qso_menu_is_enabled_by_the_licensed_call() {
        let qso = |c: &str| zlog_core::Qso {
            callsign: c.into(),
            ..Default::default()
        };
        let s = Soumu::default();
        assert!(!s.qso_menu("en", &qso(" ja1 "))[0].enabled);
        assert!(s.qso_menu("en", &qso("ja1zlo/p"))[0].enabled);
    }

    #[test]
    fn city_numbers() {
        assert_eq!(number_of("北海道札幌市中央区"), Some("010101"));
        assert_eq!(number_of("どこでもない"), None);
    }
}
