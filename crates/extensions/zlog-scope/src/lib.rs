pub mod scope;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use egui::{ComboBox, DragValue, RichText, ScrollArea, Ui};
use egui::{Key, KeyboardShortcut, Modifiers};
use zlog_core::{Band, Mode};
use zlog_ui::{EntryAction, EntryField, Panel, Section, View};

use crate::scope::{
    click, freshness, is_near, mark, marker, memorised, Mark, Scope, DEFAULT_EXPIRE_MINUTES,
};

const MEMORISE: &str = "memorise";

zlog_core::texts!();

pub struct BandScope {
    scope: Scope,
    band: Option<Band>,
    all_bands: bool,
    expire_minutes: i64,
    actions: Vec<EntryAction>,
    pending: bool,
    prompt: Option<Prompt>,
    last_hz: Option<u64>,
}

struct Prompt {
    callsign: String,
    number: String,
    mode: Mode,
    text: String,
}

impl Default for BandScope {
    fn default() -> Self {
        BandScope {
            scope: Scope::default(),
            band: None,
            all_bands: false,
            expire_minutes: DEFAULT_EXPIRE_MINUTES,
            actions: Vec::new(),
            pending: false,
            prompt: None,
            last_hz: None,
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl BandScope {
    fn refresh(&mut self, view: &View<'_>, now: i64) {
        self.scope.ingest(view.spots, now, self.expire_minutes);
        self.scope.expire(now, self.expire_minutes);
    }

    fn memorise(&mut self, view: &View<'_>, now: i64) {
        if view.callsign.is_empty() {
            return;
        }
        match view.hz {
            Some(hz) => self.store(view.callsign, view.number, hz, view.mode, now),
            None => {
                self.prompt = Some(Prompt {
                    callsign: view.callsign.to_string(),
                    number: view.number.to_string(),
                    mode: view.mode,
                    text: String::new(),
                })
            }
        }
    }

    fn answer(&mut self, khz: Option<&str>, now: i64) {
        let Some(p) = self.prompt.take() else {
            return;
        };
        let Some(text) = khz else {
            return;
        };
        if let Some(khz) = text.trim().parse::<f64>().ok().filter(|k| *k > 1000.0) {
            if let Some(item) = memorised(
                &p.callsign,
                &p.number,
                (khz * 1000.0).round() as u64,
                p.mode,
                now,
            ) {
                self.scope.add(item);
            }
        }
        self.clear_entry();
    }

    fn store(&mut self, callsign: &str, number: &str, hz: u64, mode: Mode, now: i64) {
        if let Some(item) = memorised(callsign, number, hz, mode, now) {
            self.scope.add(item);
        }
        self.clear_entry();
    }

    fn clear_entry(&mut self) {
        self.actions.extend([
            EntryAction::Callsign(String::new()),
            EntryAction::Number(String::new()),
            EntryAction::Focus(EntryField::Call),
        ]);
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
        vec![
            zlog_ui::Command::window(
                texts().get(language, "toolbar"),
                texts().get(language, "toolbar.tip"),
            )
            .icon(zlog_ui::icon::WAVEFORM),
            zlog_ui::Command::new(
                MEMORISE,
                texts().get(language, "memorise"),
                texts().get(language, "memorise.tip"),
            )
            .shortcut(KeyboardShortcut::new(Modifiers::COMMAND, Key::N)),
        ]
    }

    fn run_command(&mut self, id: &str) {
        if id == MEMORISE {
            self.pending = true;
        }
    }

    fn default_size(&self) -> [f32; 2] {
        [300.0, 500.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        let now = now();
        self.refresh(view, now);

        let shown_band = if self.all_bands {
            None
        } else {
            Some(self.band.unwrap_or(view.band))
        };
        let count = texts().format(
            view.language,
            "count",
            &[&self.scope.rows(shown_band).len()],
        );
        zlog_ui::layout::toolbar(ui, &[(&count, false)], |ui| {
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
            let button = ui.add_enabled_ui(!view.callsign.is_empty(), |ui| {
                zlog_ui::layout::tool(ui, zlog_ui::icon::NOTE_PENCIL, t("memorise"))
            });
            if button.inner.on_hover_text(t("memorise.tip")).clicked() {
                self.memorise(view, now);
            }
        });

        let band = if self.all_bands {
            None
        } else {
            Some(self.band.unwrap_or(view.band))
        };
        let mut remove = None;
        let moved = view.hz != self.last_hz;
        self.last_hz = view.hz;
        let rows = self.scope.rows(band);
        let rig = marker(&rows, view.hz, band.unwrap_or(view.band));
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                let count = rows.len();
                for (n, item) in rows.into_iter().enumerate() {
                    let here = rig.filter(|m| m.row == n);
                    if let (Some(m), Some(hz)) = (here, view.hz) {
                        if !m.on {
                            rig_row(ui, hz, moved);
                        }
                    }
                    let on = here.is_some_and(|m| m.on);
                    let m = mark(item, view.log, view.contest, view.mode);
                    let (color, number) = match &m {
                        Mark::Worked => (ui.visuals().text_color(), String::new()),
                        Mark::NewMulti(n) => (zlog_ui::palette::error(ui), n.clone()),
                        Mark::Number(n) => (zlog_ui::palette::good(ui), n.clone()),
                        Mark::Unknown => (zlog_ui::palette::good(ui), String::new()),
                    };
                    let alpha = 0.4 + 0.15 * freshness(item, now, self.expire_minutes) as f32;
                    let text = row_text(item, on, &number);
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
                    if on && moved {
                        row.scroll_to_me(Some(egui::Align::Center));
                    }
                    let tip = hover_text(item, view.language);
                    let row = if tip.is_empty() {
                        row
                    } else {
                        row.on_hover_text(tip)
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
                if let (Some(m), Some(hz)) = (rig, view.hz) {
                    if m.row == count {
                        rig_row(ui, hz, moved);
                    }
                }
            });
        });
        if let Some((call, band)) = remove {
            self.scope
                .items
                .retain(|i| !(i.callsign == call && i.band == band));
        }
        ui.ctx().request_repaint_after(Duration::from_secs(5));
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        if self.pending {
            return Some(std::time::Duration::ZERO);
        }
        (!self.scope.items.is_empty()).then_some(std::time::Duration::from_secs(5))
    }

    fn tick(&mut self, view: &View<'_>) {
        let now = now();
        self.refresh(view, now);
        if std::mem::take(&mut self.pending) {
            self.memorise(view, now);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        let mut answer = None;
        egui::Window::new(t("memorise"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("{} {}", t("frequency"), p.callsign));
                ui.text_edit_singleline(&mut p.text).request_focus();
                let enter = ui.input(|i| i.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    if ui.button(t("ok")).clicked() || enter {
                        answer = Some(true);
                    }
                    if ui.button(t("cancel")).clicked() || ui.input(|i| i.key_pressed(Key::Escape))
                    {
                        answer = Some(false);
                    }
                });
            });
        match answer {
            Some(true) => {
                let text = p.text.clone();
                self.answer(Some(&text), now());
            }
            Some(false) => self.answer(None, now()),
            None => {}
        }
    }

    fn modal(&self) -> bool {
        self.prompt.is_some()
    }

    fn close_dialogs(&mut self) {
        self.prompt = None;
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

fn row_text(item: &scope::Item, on: bool, number: &str) -> String {
    let mode = item.mode.map(|m| m.label()).unwrap_or("");
    let memo = if item.memory { "*" } else { " " };
    format!(
        "{}{:>9.1}{memo} {:<11} {:<4} {:<4} {}",
        if on { ">>" } else { "  " },
        item.hz as f64 / 1000.0,
        item.callsign,
        mode,
        item.source,
        number
    )
}

fn hover_text(item: &scope::Item, language: &str) -> String {
    let t = |k| texts().get(language, k);
    let mut lines = Vec::new();
    if !item.source.is_empty() {
        lines.push(format!("{} {}", t("source"), item.source));
    }
    if !item.spotter.is_empty() {
        lines.push(format!("{} {}", t("spotter"), item.spotter));
    }
    lines.join("\n")
}

fn rig_row(ui: &mut Ui, hz: u64, moved: bool) {
    let text = RichText::new(format!(">>{:>9.1}", hz as f64 / 1000.0))
        .monospace()
        .strong()
        .background_color(ui.visuals().selection.bg_fill);
    let row = ui.label(text);
    if moved {
        row.scroll_to_me(Some(egui::Align::Center));
    }
}

pub struct ScopeOptions;

impl zlog_ui::SettingsPage for ScopeOptions {
    fn id(&self) -> &str {
        "panel.scope"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut Section, lang: &str) {
        let t = |k| texts().get(lang, k);
        let mut minutes = section
            .get("expire_minutes")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_EXPIRE_MINUTES);
        zlog_ui::form(ui, "scope-options", &[t("expire")], |ui, _| {
            ui.label(t("expire"));
            ui.add(
                DragValue::new(&mut minutes)
                    .range(1..=24 * 60)
                    .suffix(" min"),
            );
            ui.end_row();
        });
        section.insert("expire_minutes".into(), minutes.to_string());
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.page(410, |_| ScopeOptions);
    b.panel(480, |_| BandScope::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_show_the_source_tag() {
        let mut i = scope::memorised("JA1ZLO", "", 7_010_100, Mode::Cw, 0).unwrap();
        i.memory = false;
        i.source = "CW4I".into();
        assert_eq!(
            row_text(&i, false, "13M"),
            "     7010.1  JA1ZLO      CW   CW4I 13M"
        );
        i.spotter = "JA1YWX".into();
        assert_eq!(hover_text(&i, "ja"), "情報源 CW4I\nスポット元 JA1YWX");
        i.source.clear();
        assert_eq!(hover_text(&i, "en"), "Spotted by JA1YWX");
    }

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
        with_entry(spots, "", "", None, f)
    }

    fn with_entry<R>(
        spots: &[zlog_ui::Spot],
        callsign: &str,
        number: &str,
        hz: Option<u64>,
        f: impl FnOnce(&View<'_>) -> R,
    ) -> R {
        let log = zlog_core::Log::default();
        let sheet = zlog_core::ScoreSheet::default();
        let station = zlog_core::Station::default();
        let view = View {
            station: &station,
            number,
            memo: "",
            rst_sent: "",
            power: zlog_core::Power::P010,
            log: &log,
            contest: &crate::scope::tests::Test,
            sheet: &sheet,
            callsign,
            band: Band::B7,
            mode: zlog_core::Mode::Cw,
            language: "en",
            spots,
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

    #[test]
    fn tick_ingests_while_closed() {
        let mut b = BandScope::default();
        let spots = [crate::scope::tests::spot("JA1YAA", 7_010_000, now())];
        with_view(&spots, |v| b.tick(v));
        assert_eq!(b.scope.items.len(), 1);
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

    fn cleared() -> Vec<EntryAction> {
        vec![
            EntryAction::Callsign(String::new()),
            EntryAction::Number(String::new()),
            EntryAction::Focus(EntryField::Call),
        ]
    }

    #[test]
    fn ctrl_n_is_the_memorise_command() {
        let c = BandScope::default().commands("en");
        let m = c.iter().find(|c| c.id == MEMORISE).unwrap();
        assert_eq!(
            m.shortcut,
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::N))
        );
    }

    #[test]
    fn ctrl_n_stores_entry_at_rig_frequency_and_clears() {
        let mut b = BandScope::default();
        b.run_command(MEMORISE);
        assert_eq!(b.tick_after(), Some(Duration::ZERO));
        let before = now();
        with_entry(&[], "JA1ZLO", "10", Some(7_012_340), |v| b.tick(v));
        let i = &b.scope.items[0];
        assert_eq!(b.scope.items.len(), 1);
        assert_eq!((i.callsign.as_str(), i.number.as_str()), ("JA1ZLO", "10"));
        assert_eq!(
            (i.hz, i.band, i.mode),
            (7_012_340, Band::B7, Some(zlog_core::Mode::Cw))
        );
        assert!(i.memory && i.time >= before);
        assert_eq!(b.take_actions(), cleared());
        with_entry(&[], "JA1YWX", "11", Some(7_012_340), |v| b.tick(v));
        assert_eq!(b.scope.items.len(), 1);
        assert!(b.take_actions().is_empty());
    }

    #[test]
    fn ctrl_n_without_call_does_nothing() {
        let mut b = BandScope::default();
        b.run_command(MEMORISE);
        with_entry(&[], "", "10", Some(7_012_000), |v| b.tick(v));
        assert!(b.scope.items.is_empty() && b.prompt.is_none());
        assert!(b.take_actions().is_empty());
    }

    #[test]
    fn ctrl_n_without_rig_asks_for_khz() {
        let mut b = BandScope::default();
        b.run_command(MEMORISE);
        with_entry(&[], "JA1YWX", "11", None, |v| b.tick(v));
        assert!(b.scope.items.is_empty());
        assert!(b.take_actions().is_empty());
        b.answer(Some("7023.5"), 100);
        assert!(b.prompt.is_none());
        let i = &b.scope.items[0];
        assert_eq!(
            (i.callsign.as_str(), i.number.as_str(), i.hz),
            ("JA1YWX", "11", 7_023_500)
        );
        assert_eq!(b.take_actions(), cleared());
    }

    #[test]
    fn khz_at_or_below_1000_is_dropped_but_entry_cleared() {
        let mut b = BandScope::default();
        b.run_command(MEMORISE);
        with_entry(&[], "JA1YWX", "11", None, |v| b.tick(v));
        b.answer(Some("1000"), 100);
        assert!(b.scope.items.is_empty());
        assert_eq!(b.take_actions(), cleared());
    }

    #[test]
    fn cancelled_prompt_keeps_entry() {
        let mut b = BandScope::default();
        b.run_command(MEMORISE);
        with_entry(&[], "JA1YWX", "11", None, |v| b.tick(v));
        b.answer(None, 100);
        assert!(b.prompt.is_none() && b.scope.items.is_empty());
        assert!(b.take_actions().is_empty());
    }
}
