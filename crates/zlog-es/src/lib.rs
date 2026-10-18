//! Sporadic-E status at the four NICT ionosondes, in a window.
//!
//! Port of the infoEs plugin (MIT, jucky154). Its source page is gone; this reads
//! the current NICT pages instead: the status table (`ionospheric-signal.html`)
//! for whether Es is present now, and the event history (`ionospheric-event.html`)
//! for the latest Es and its fxEs. Nothing is fetched until the window is open.

use std::time::{Duration, Instant};

use egui::{Color32, RichText, Ui};
use zlog_net::{html_text, Fetch};
use zlog_ui::{Panel, View};

const SIGNAL_URL: &str = "https://wdc.nict.go.jp/Ionosphere/realtime/ISDJ/ionospheric-signal.html";
const EVENT_URL: &str = "https://wdc.nict.go.jp/Ionosphere/realtime/ISDJ/ionospheric-event.html";
zlog_core::texts!();

/// NICT updates both pages every 15 minutes.
const REFRESH: Duration = Duration::from_secs(15 * 60);

/// What the status table says about one kind of event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Unknown,
    Quiet,
    Likely,
    Ongoing,
}

impl Signal {
    /// From the traffic-light image of the table (`signal-green.gif`, ...).
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

/// The latest Es event of a station: when it ran and its largest fxEs.
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

/// Text of a table cell: the part before `</td>` without tags. The last cell of a
/// table is followed by whatever comes next on the page, which must not leak in.
fn cell_text(cell: &str) -> String {
    let inner = cell.split("</td>").next().unwrap_or(cell);
    html_text(&format!("<td{inner}")).trim().to_string()
}

/// Es status per station from the status table, in table order.
pub fn parse_signal(html: &str) -> Vec<(String, Signal)> {
    let mut out = Vec::new();
    for row in html.split("<tr>").skip(1) {
        let cells: Vec<&str> = row.split("<td").skip(1).collect();
        if cells.len() != 4 {
            continue;
        }
        let name = cell_text(cells[0]);
        let Some(src) = cells[3]
            .split("<img src='")
            .nth(1)
            .and_then(|s| s.split('\'').next())
        else {
            continue;
        };
        // Station rows only: their lights link to the station's data, while the last
        // row, NICT's overall assessment, has bare images.
        if name.contains('(') && cells[3].contains("<a href") {
            out.push((name, Signal::from_image(src)));
        }
    }
    out
}

/// The last Es event of each station from the 40-day history, keyed by station name.
pub fn parse_events(html: &str) -> Vec<(String, EsEvent)> {
    let mut out = Vec::new();
    for section in html.split("==== ").skip(1) {
        let Some((name, rest)) = section.split_once(" ====") else {
            continue;
        };
        let name = name.trim_matches(|c| c == '=' || c == ' ').to_string();
        // Es rows follow the Es header; storm rows come before it and have five cells.
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
}

impl EsInfo {
    fn refresh(&mut self) {
        self.signal = Some(Fetch::get(SIGNAL_URL));
        self.events = Some(Fetch::get(EVENT_URL));
        self.fetched_at = Some(Instant::now());
    }

    /// Combine both pages once both have arrived.
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

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
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

        if ui.button(texts().get(lang, "refresh")).clicked() && self.signal.is_none() {
            self.refresh();
        }
        if let Some(err) = &self.error {
            ui.colored_label(Color32::RED, err);
        }
        egui::Grid::new("es_grid").striped(true).show(ui, |ui| {
            for h in ["head.station", "head.es", "head.latest", "head.fxes"] {
                ui.label(RichText::new(texts().get(lang, h)).strong());
            }
            ui.end_row();
            for st in &self.stations {
                ui.label(&st.name);
                let (text, color) = match st.es {
                    Signal::Ongoing => (texts().get(lang, "signal.ongoing"), Color32::RED),
                    Signal::Likely => (
                        texts().get(lang, "signal.likely"),
                        Color32::from_rgb(200, 140, 0),
                    ),
                    Signal::Quiet => (texts().get(lang, "signal.quiet"), Color32::GRAY),
                    Signal::Unknown => ("?", Color32::GRAY),
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
            vec![
                "稚内(北海道)",
                "国分寺(東京)",
                "山川(鹿児島)",
                "大宜味(沖縄)"
            ]
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
    fn latest_events() {
        let e = parse_events(EVENT);
        let wakkanai = e.iter().find(|(n, _)| n == "稚内(北海道)").unwrap();
        assert_eq!(
            wakkanai.1,
            EsEvent {
                period: "2026/10/11 05:15 - 10/11 05:30".into(),
                fxes_mhz: 20.4
            }
        );
        assert_eq!(e.len(), 4);
    }
}
