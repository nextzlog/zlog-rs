//! License data of the station being typed, with its JCC/JCG number when the
//! location names a city.
//!
//! Port of the soumuAPI plugin (MIT, jucky154), including its city table
//! (`data/allcity.dat`). The lookup goes through [`LicenseSource`], so another
//! service can replace the Ministry's Web-API without touching the window.
//! A request is made only while this window is open, once per callsign, after the
//! callsign has stayed the same for a second.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Color32, RichText, Ui};
use zlog_net::Fetch;
use zlog_ui::{Panel, View};

zlog_core::texts!();

const CITIES: &str = include_str!("../data/allcity.dat");

/// One licensed station.
#[derive(Debug, Clone, PartialEq)]
pub struct License {
    pub callsign: String,
    pub location: String,
    /// Station class code such as `1AF` (first class, fixed).
    pub class: String,
}

/// A service that finds licences by callsign.
pub trait LicenseSource {
    /// Name of the service, in `language` where the service offers it.
    fn name(&self, language: &str) -> &str;
    /// Text the terms of the service require to be shown, in `language` where allowed.
    fn notice(&self, language: &str) -> &str;
    fn url(&self, callsign: &str) -> String;
    fn parse(&self, body: &str) -> Result<Vec<License>, String>;
}

/// The radio station search Web-API of the Ministry of Internal Affairs and Communications.
///
/// The URL and response format follow soumuAPI. They could not be checked from
/// outside Japan, where the service refuses connections.
pub struct Mic;

impl LicenseSource for Mic {
    fn name(&self, language: &str) -> &str {
        texts().get(language, "mic.name")
    }

    fn notice(&self, language: &str) -> &str {
        texts().get(language, "mic.notice")
    }

    fn url(&self, callsign: &str) -> String {
        format!("https://www.tele.soumu.go.jp/musen/list?ST=1&OF=2&DA=1&OW=AT&SK=2&DC=1&SC=1&MA={callsign}")
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

/// JCC/JCG number for a location written as in the licence (prefecture, city and ward).
pub fn number_of(location: &str) -> Option<&'static str> {
    CITIES.lines().find_map(|l| {
        l.split_once('\t')
            .filter(|(name, _)| *name == location)
            .map(|(_, n)| n)
    })
}

/// Licence class in words in `language`, as soumuAPI shows it in Japanese.
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
    cache: HashMap<String, Result<Vec<License>, String>>,
}

impl Soumu {
    pub fn new(source: impl LicenseSource + 'static) -> Self {
        Soumu {
            source: Box::new(source),
            typed: String::new(),
            typed_at: Instant::now(),
            pending: None,
            cache: HashMap::new(),
        }
    }
}

impl Default for Soumu {
    fn default() -> Self {
        Soumu::new(Mic)
    }
}

impl Panel for Soumu {
    fn id(&self) -> &str {
        "soumu"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [460.0, 180.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let call = view.callsign.trim().to_string();
        if call != self.typed {
            self.typed = call.clone();
            self.typed_at = Instant::now();
        }
        // Ask once per callsign, after typing has paused for a second.
        let settled = self.typed_at.elapsed() >= Duration::from_secs(1);
        if call.len() >= 4 && settled && self.pending.is_none() && !self.cache.contains_key(&call) {
            self.pending = Some((call.clone(), Fetch::get(self.source.url(&call))));
        }
        if let Some((asked, fetch)) = self.pending.as_mut() {
            if let Some(r) = fetch.poll() {
                let parsed = r.clone().and_then(|body| self.source.parse(&body));
                self.cache.insert(asked.clone(), parsed);
                self.pending = None;
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
        match self.cache.get(&call) {
            Some(Ok(list)) if list.is_empty() => {
                ui.label(texts().get(lang, "none"));
            }
            Some(Ok(list)) => {
                egui::Grid::new("licences").striped(true).show(ui, |ui| {
                    for l in list {
                        ui.label(RichText::new(&l.callsign).monospace());
                        ui.label(&l.location);
                        ui.label(RichText::new(number_of(&l.location).unwrap_or("?")).monospace());
                        ui.label(class_text(&l.class, lang));
                        ui.end_row();
                    }
                });
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::RED, e);
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn city_numbers() {
        assert_eq!(number_of("北海道札幌市中央区"), Some("010101"));
        assert_eq!(number_of("どこでもない"), None);
    }
}
