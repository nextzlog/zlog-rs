//! Band scope window (`UBandScope2.pas`).
//!
//! Lists the stations spotted on a band ([`zlog_ui::View::spots`]) and those the
//! operator memorised, sorted by frequency and coloured as zLog does: black when
//! worked, red for a new multiplier, green otherwise; bold where the rig is tuned.
//! Entries fade as they age and leave after the expiry time. Clicking one tunes the
//! rig to it and puts its callsign and known number into the entry row; clicking a
//! worked one clears the entry row.
//!
//! zLog opens one window per band; here one window follows the entry row's band or
//! shows a chosen band or all bands.

pub mod scope;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use egui::{ComboBox, DragValue, RichText, ScrollArea, Ui};
use zlog_core::Band;
use zlog_ui::{EntryAction, Panel, Section, View};

use crate::scope::{
    click, freshness, is_near, mark, memorised, Mark, Scope, DEFAULT_EXPIRE_MINUTES,
};

zlog_core::texts!();

/// The band scope window.
pub struct BandScope {
    scope: Scope,
    /// Band shown; `None` follows the entry row.
    band: Option<Band>,
    all_bands: bool,
    expire_minutes: i64,
    actions: Vec<EntryAction>,
}

impl Default for BandScope {
    fn default() -> Self {
        BandScope {
            scope: Scope::default(),
            band: None,
            all_bands: false,
            expire_minutes: DEFAULT_EXPIRE_MINUTES,
            actions: Vec::new(),
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl BandScope {
    /// Take in new spots and drop expired entries.
    fn refresh(&mut self, view: &View<'_>, now: i64) {
        self.scope.ingest(view.spots, now, self.expire_minutes);
        self.scope.expire(now, self.expire_minutes);
    }
}

impl Panel for BandScope {
    fn id(&self) -> &str {
        "scope"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Report
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Progress, 1)
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

    fn default_size(&self) -> [f32; 2] {
        [300.0, 500.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        let now = now();
        self.refresh(view, now);

        ui.horizontal(|ui| {
            let shown = if self.all_bands {
                t("all_bands").to_string()
            } else {
                match self.band {
                    None => format!("{} ({})", t("entry_band"), view.band),
                    Some(b) => format!("{b} MHz"),
                }
            };
            ComboBox::from_id_salt("scope-band")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(false, t("entry_band")).clicked() {
                        (self.band, self.all_bands) = (None, false);
                    }
                    if ui.selectable_label(false, t("all_bands")).clicked() {
                        (self.band, self.all_bands) = (None, true);
                    }
                    let (low, high) = view.contest.band_range();
                    for b in Band::range(low, high) {
                        if b.is_warc() && !view.contest.uses_warc() {
                            continue;
                        }
                        if ui.selectable_label(false, format!("{b} MHz")).clicked() {
                            (self.band, self.all_bands) = (Some(b), false);
                        }
                    }
                });
            // Memorise the station in the entry row at the rig's frequency (`AddSelfSpot`).
            let memo = view.hz.filter(|_| !view.callsign.is_empty());
            let button = ui.add_enabled(memo.is_some(), egui::Button::new(t("memorise")));
            if let (true, Some(hz)) = (button.clicked(), memo) {
                if let Some(item) = memorised(view.callsign, view.number, hz, view.mode, now) {
                    self.scope.add(item);
                }
            }
            ui.label(t("expire"));
            ui.add(
                DragValue::new(&mut self.expire_minutes)
                    .range(1..=24 * 60)
                    .suffix(" min"),
            );
        });
        ui.separator();

        let band = if self.all_bands {
            None
        } else {
            Some(self.band.unwrap_or(view.band))
        };
        let mut remove = None;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for item in self.scope.rows(band) {
                let m = mark(item, view.log, view.contest, view.mode);
                let (color, number) = match &m {
                    Mark::Worked => (ui.visuals().text_color(), String::new()),
                    Mark::NewMulti(n) => (zlog_ui::palette::error(ui), n.clone()),
                    Mark::Number(n) => (zlog_ui::palette::good(ui), n.clone()),
                    Mark::Unknown => (zlog_ui::palette::good(ui), String::new()),
                };
                // Older entries fade (zLog shows a freshness icon instead).
                let alpha = 0.4 + 0.15 * freshness(item, now, self.expire_minutes) as f32;
                let mode = item.mode.map(|m| m.label()).unwrap_or("");
                let memo = if item.memory { "*" } else { " " };
                let text = format!(
                    "{:>9.1}{memo} {:<11} {:<4} {}",
                    item.hz as f64 / 1000.0,
                    item.callsign,
                    mode,
                    number
                );
                let mut rich = RichText::new(text)
                    .monospace()
                    .color(color.gamma_multiply(alpha));
                if is_near(item, view.hz) {
                    rich = rich
                        .strong()
                        .background_color(ui.visuals().selection.bg_fill);
                }
                if matches!(m, Mark::Worked) {
                    rich = rich.strikethrough();
                }
                let row = ui.selectable_label(false, rich);
                let row = if item.spotter.is_empty() {
                    row
                } else {
                    row.on_hover_text(format!("{} {}", t("spotter"), item.spotter))
                };
                if row.clicked() {
                    self.actions.extend(click(item, &m));
                }
                row.context_menu(|ui| {
                    if ui.button(t("delete")).clicked() {
                        remove = Some((item.callsign.clone(), item.band));
                        ui.close();
                    }
                });
            }
        });
        if let Some((call, band)) = remove {
            self.scope
                .items
                .retain(|i| !(i.callsign == call && i.band == band));
        }
        // Ageing goes on without input.
        ui.ctx().request_repaint_after(Duration::from_secs(5));
    }

    fn tick(&mut self, view: &View<'_>) {
        // Every frame, also while closed, so spots are not missed (zLog's band
        // scope takes spots whether its window is shown or not).
        self.refresh(view, now());
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &Section) {
        if let Some(m) = section.get("expire_minutes").and_then(|v| v.parse().ok()) {
            self.expire_minutes = m;
        }
        self.all_bands = section.get("all_bands").is_some_and(|v| v == "true");
    }

    fn save(&self, section: &mut Section) {
        section.insert("expire_minutes".into(), self.expire_minutes.to_string());
        section.insert("all_bands".into(), self.all_bands.to_string());
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
    fn settings_round_trip() {
        let a = BandScope {
            expire_minutes: 30,
            all_bands: true,
            ..BandScope::default()
        };
        let mut section = Section::new();
        a.save(&mut section);
        let mut b = BandScope::default();
        b.load(&section);
        assert_eq!(b.expire_minutes, 30);
        assert!(b.all_bands);
    }

    fn with_view<R>(spots: &[zlog_ui::Spot], f: impl FnOnce(&View<'_>) -> R) -> R {
        let log = zlog_core::Log::default();
        let sheet = zlog_core::ScoreSheet::default();
        let station = zlog_core::Station::default();
        let view = View {
            station: &station,
            number: "",
            rst_sent: "",
            power: zlog_core::Power::P010,
            log: &log,
            contest: &crate::scope::tests::Test,
            sheet: &sheet,
            callsign: "",
            band: Band::B7,
            mode: zlog_core::Mode::Cw,
            language: "en",
            spots,
            hz: None,
            rig: 1,
            cq: false,
            operator: "",
            period_hours: 0,
            pc_name: "",
            focus: None,
            editing: None,
            selected: None,
            period: &(),
        };
        f(&view)
    }

    /// C3: spots arrive through `tick`, which runs while the window is closed.
    #[test]
    fn tick_ingests_while_closed() {
        let mut b = BandScope::default();
        let spots = [crate::scope::tests::spot("JA1YAA", 7_010_000, now())];
        with_view(&spots, |v| b.tick(v));
        assert_eq!(b.scope.items.len(), 1);
        // Ticking again with the same spots adds nothing.
        with_view(&spots, |v| b.tick(v));
        assert_eq!(b.scope.items.len(), 1);
    }

    #[test]
    fn tick_with_no_spots_and_expired_spots() {
        let mut b = BandScope::default();
        with_view(&[], |v| b.tick(v));
        assert!(b.scope.items.is_empty());
        let old = [crate::scope::tests::spot("JA1YAA", 7_010_000, 0)];
        with_view(&old, |v| b.tick(v));
        assert!(b.scope.items.is_empty());
    }

    #[test]
    fn load_ignores_malformed_expiry() {
        let mut section = Section::new();
        section.insert("expire_minutes".into(), "abc".into());
        let mut b = BandScope::default();
        b.load(&section);
        assert_eq!(b.expire_minutes, DEFAULT_EXPIRE_MINUTES);
        assert!(!b.all_bands);
    }
}
