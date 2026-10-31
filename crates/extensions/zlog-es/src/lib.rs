use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use zlog_net::{html_text, Fetch};
use zlog_ui::{Panel, View};

const SIGNAL_URL: &str = "https://wdc.nict.go.jp/Ionosphere/realtime/ISDJ/ionospheric-signal.html";
const EVENT_URL: &str = "https://wdc.nict.go.jp/Ionosphere/realtime/ISDJ/ionospheric-event.html";
zlog_core::texts!();

const REFRESH: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Unknown,
    Quiet,
    Likely,
    Ongoing,
}

impl Signal {
    fn from_image(src: &str) -> Self {
        if src.contains("green") {
            Signal::Quiet
        } else if src.contains("yellow") {
            Signal::Likely
        } else if src.contains("red") {
            Signal::Ongoing
        } else {
            Signal::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EsEvent {
    pub period: String,
    pub fxes_mhz: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    pub name: String,
    pub es: Signal,
    pub latest: Option<EsEvent>,
}

fn cell_text(cell: &str) -> String {
    let inner = cell.split("</td>").next().unwrap_or(cell);
    html_text(&format!("<td{inner}")).trim().to_string()
}

pub fn parse_signal(html: &str) -> Vec<(String, Signal)> {
    let mut out = Vec::new();
    for row in html.split("<tr>").skip(1) {
        let cells: Vec<&str> = row.split("<td").skip(1).collect();
        if cells.len() != 4 {
            continue;
        }
        let name = cell_text(cells[0]);
        let Some(src) = cells[3].split("<img src=").nth(1).and_then(|s| {
            let quote = s.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
            s[1..].split(quote).next()
        }) else {
            continue;
        };
        if name.contains('(') && cells[3].contains("<a href") {
            out.push((name, Signal::from_image(src)));
        }
    }
    out
}

pub fn parse_events(html: &str) -> Vec<(String, EsEvent)> {
    let mut out = Vec::new();
    for section in html.split("==== ").skip(1) {
        let Some((name, rest)) = section.split_once(" ====") else {
            continue;
        };
        let name = name.trim_matches(|c| c == '=' || c == ' ').to_string();
        let Some((_, es_rows)) = rest.split_once("fxEs[MHz]") else {
            continue;
        };
        let last = es_rows
            .split("<tr>")
            .filter_map(|row| {
                let cells: Vec<String> = row.split("<td").skip(1).map(cell_text).collect();
                (cells.len() == 4).then_some(cells)
            })
            .filter_map(|c| {
                Some(EsEvent {
                    period: c[0].clone(),
                    fxes_mhz: c[3].parse().ok()?,
                })
            })
            .last();
        if let Some(event) = last {
            out.push((name, event));
        }
    }
    out
}

#[derive(Default)]
pub struct EsInfo {
    signal: Option<Fetch>,
    events: Option<Fetch>,
    fetched_at: Option<Instant>,
    stations: Vec<Station>,
    error: Option<String>,
    unreadable: bool,
}

impl EsInfo {
    fn refresh(&mut self) {
        self.signal = Some(Fetch::get(SIGNAL_URL));
        self.events = Some(Fetch::get(EVENT_URL));
        self.fetched_at = Some(Instant::now());
    }

    fn collect(&mut self) {
        let (Some(s), Some(e)) = (self.signal.as_mut(), self.events.as_mut()) else {
            return;
        };
        let (Some(s), Some(e)) = (s.poll().cloned(), e.poll().cloned()) else {
            return;
        };
        self.signal = None;
        self.events = None;
        match (s, e) {
            (Ok(s), Ok(e)) => {
                let events = parse_events(&e);
                self.stations = parse_signal(&s)
                    .into_iter()
                    .map(|(name, es)| {
                        let latest = events
                            .iter()
                            .find(|(n, _)| *n == name)
                            .map(|(_, ev)| ev.clone());
                        Station { name, es, latest }
                    })
                    .collect();
                self.unreadable = self.stations.is_empty();
                self.error = None;
            }
            (Err(err), _) | (_, Err(err)) => self.error = Some(err),
        }
    }
}

impl Panel for EsInfo {
    fn id(&self) -> &str {
        "es"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Network
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Spots, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::CLOUD_LIGHTNING)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [420.0, 200.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let due = self.fetched_at.is_none_or(|t| t.elapsed() >= REFRESH);
        if due && self.signal.is_none() {
            self.refresh();
        }
        self.collect();
        if self.signal.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }

        let problem = match (&self.error, self.unreadable) {
            (Some(e), _) => e.clone(),
            (None, true) => texts().get(lang, "unreadable").to_string(),
            (None, false) => String::new(),
        };
        let mut refresh = false;
        zlog_ui::layout::toolbar(ui, &[(&problem, true)], |ui| {
            let glyph = zlog_ui::icon::ARROW_CLOCKWISE;
            refresh = zlog_ui::layout::tool(ui, glyph, texts().get(lang, "refresh")).clicked();
            if self.signal.is_some() {
                ui.spinner();
            }
        });
        if refresh && self.signal.is_none() {
            self.refresh();
        }
        egui::Grid::new("es_grid")
            .striped(true)
            .min_col_width(ui.available_width() / 4.5)
            .show(ui, |ui| {
                for h in ["head.station", "head.es", "head.latest", "head.fxes"] {
                    ui.label(RichText::new(texts().get(lang, h)).strong());
                }
                ui.end_row();
                for st in &self.stations {
                    ui.label(&st.name);
                    let (text, color) = match st.es {
                        Signal::Ongoing => (
                            texts().get(lang, "signal.ongoing"),
                            zlog_ui::palette::error(ui),
                        ),
                        Signal::Likely => (
                            texts().get(lang, "signal.likely"),
                            zlog_ui::palette::warning(ui),
                        ),
                        Signal::Quiet => (
                            texts().get(lang, "signal.quiet"),
                            zlog_ui::palette::weak(ui),
                        ),
                        Signal::Unknown => ("?", zlog_ui::palette::weak(ui)),
                    };
                    ui.colored_label(color, text);
                    match &st.latest {
                        Some(ev) => {
                            ui.label(&ev.period);
                            ui.label(format!("{:.1} MHz", ev.fxes_mhz));
                        }
                        None => {
                            ui.label("-");
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        ui.weak(texts().get(lang, "credit"));
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(570, |_| EsInfo::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNAL: &str = include_str!("../tests/fixtures/ionospheric-signal.html");
    const EVENT: &str = include_str!("../tests/fixtures/ionospheric-event.html");

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn status_table() {
        let s = parse_signal(SIGNAL);
        let names: Vec<&str> = s.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["北見台(架空)", "中野原(架空)", "南浜(架空)", "西岬(架空)"]
        );
        let signals: Vec<Signal> = s.iter().map(|(_, sig)| *sig).collect();
        assert_eq!(
            signals,
            vec![
                Signal::Quiet,
                Signal::Ongoing,
                Signal::Likely,
                Signal::Unknown
            ]
        );
    }

    #[test]
    fn double_quoted_images_are_read() {
        let html = SIGNAL
            .replace("<img src='", "<img src=\"")
            .replace(".gif'", ".gif\"");
        assert_eq!(parse_signal(&html), parse_signal(SIGNAL));
    }

    fn answered(signal: &str) -> EsInfo {
        let mut es = EsInfo {
            signal: Some(Fetch::ready(Ok(signal.to_string()))),
            events: Some(Fetch::ready(Ok(EVENT.to_string()))),
            ..EsInfo::default()
        };
        es.collect();
        es
    }

    #[test]
    fn an_unreadable_page_is_reported() {
        let es = answered("<html><body>maintenance</body></html>");
        assert!(es.unreadable && es.stations.is_empty() && es.error.is_none());
        let es = answered(SIGNAL);
        assert!(!es.unreadable && es.stations.len() == 4);
    }

    #[test]
    fn latest_events() {
        let e = parse_events(EVENT);
        let wakkanai = e.iter().find(|(n, _)| n == "北見台(架空)").unwrap();
        assert_eq!(
            wakkanai.1,
            EsEvent {
                period: "2026/10/12 06:15 - 10/12 06:30".into(),
                fxes_mhz: 17.2
            }
        );
        assert_eq!(e.len(), 4);
    }
}
