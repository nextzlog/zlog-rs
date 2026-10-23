//! The Options dialog and the rig window.

use egui::{RichText, Ui};
use zlog_core::qso::QslState;
use zlog_core::{Band, Power, Station};
use zlog_rig::RigMode;
use zlog_rig_cat::MODELS;
use zlog_ui::SettingsPage;

use crate::dialog::{dialog_with_buttons, dialog_with_pages, ok_cancel};
use crate::i18n::{self, tr};
use crate::rig::{RigConfig, RigControl, SIMULATOR};
use crate::rigs::Rigs;
use crate::settings::{Preferences, Settings};

/// The Options dialog: one tab a [`SettingsPage`], the application's own first
/// (station, preferences, rig) and then the extensions', as the menus and the
/// toolbar are built from one list of commands. Every tab edits a copy of the
/// settings, taken when the dialog opens; OK writes the tabs' sections back and
/// Cancel drops them.
pub struct Options {
    pub open: bool,
    page: usize,
    pages: Vec<Box<dyn SettingsPage>>,
    draft: Settings,
    /// The settings as they were when the dialog opened: OK writes only the values
    /// the tabs changed from these, so a value changed elsewhere meanwhile stays.
    base: Settings,
    /// Each tab's section as its first drawing left it, before any widget was used:
    /// a tab writes defaults and normalized values (a migrated PC name) back, which
    /// are not edits, so OK compares with this instead of [`Options::base`].
    drawn: std::collections::BTreeMap<String, zlog_ui::Section>,
}

impl Options {
    pub fn new(pages: Vec<Box<dyn SettingsPage>>) -> Self {
        Options {
            open: false,
            page: 0,
            pages,
            draft: Settings::default(),
            base: Settings::default(),
            drawn: Default::default(),
        }
    }

    /// Open with the current settings.
    pub fn show_with(&mut self, settings: &mut Settings) {
        if self.open {
            // Already open: keep what the tabs were editing.
            return;
        }
        self.draft = settings.clone();
        self.base = settings.clone();
        self.drawn.clear();
        self.open = true;
    }

    /// Tell the tabs the commands of the menus and the keys kept for the entry
    /// row and the grid ([`SettingsPage::set_commands`]).
    pub fn set_commands(
        &mut self,
        commands: &[zlog_ui::keys::CommandInfo],
        reserved: &[egui::KeyboardShortcut],
    ) {
        for p in &mut self.pages {
            p.set_commands(commands, reserved);
        }
    }

    /// The shortcuts the tabs chose in `settings` ([`SettingsPage::shortcuts`]).
    pub fn shortcuts(&self, settings: &mut Settings) -> zlog_ui::keys::Overrides {
        let mut all = zlog_ui::keys::Overrides::new();
        for p in &self.pages {
            if let Some(o) = p.shortcuts(settings.section(p.id())) {
                all.extend(o);
            }
        }
        all
    }

    /// Whether a tab waits for a key, so no shortcut runs meanwhile.
    pub fn wants_keys(&self) -> bool {
        self.open && self.pages.iter().any(|p| p.wants_keys())
    }

    /// Draw the dialog; when OK was pressed (the tabs' sections were written to
    /// `settings` and saved), returns the sections that changed.
    pub fn ui(&mut self, ctx: &egui::Context, settings: &mut Settings) -> Option<Vec<String>> {
        let mut open = self.open;
        let lang = i18n::current();
        let titles: Vec<String> = self.pages.iter().map(|p| p.title(lang)).collect();
        let page = &mut self.page;
        let (pages, draft, drawn) = (&mut self.pages, &mut self.draft, &mut self.drawn);
        let shown = dialog_with_pages(
            ctx,
            "options",
            tr("options.title"),
            [640.0, 560.0],
            &mut open,
            |ui| {
                for (i, title) in titles.iter().enumerate() {
                    ui.selectable_value(page, i, title);
                }
                *page
            },
            |ui, page| {
                if let Some(p) = pages.get_mut(page) {
                    let section = draft.section(p.id());
                    p.ui(ui, section, lang);
                    // No widget can have been used in the frame a tab first shows.
                    drawn
                        .entry(p.id().to_string())
                        .or_insert_with(|| section.clone());
                }
            },
            |ui| ok_cancel(ui, crate::i18n::current(), true, false),
        );
        let pressed = shown.and_then(|(_, b)| b);
        let ok = pressed == Some(true);
        if pressed == Some(false) || !open {
            self.open = false;
        }
        if !ok {
            return None;
        }
        // Only what the tabs changed: the rest may have changed meanwhile.
        let mut changed = Vec::new();
        for p in &self.pages {
            let id = p.id().to_string();
            let edited = self.draft.section(&id).clone();
            let base = match self.drawn.get(&id) {
                Some(drawn) => drawn.clone(),
                None => self.base.section(&id).clone(),
            };
            if merge_edits(&base, &edited, settings.section(&id)) {
                changed.push(id);
            }
        }
        let _ = settings.save();
        self.open = false;
        Some(changed)
    }
}

/// The station tab (section `station`).
pub struct StationPage;

impl SettingsPage for StationPage {
    fn id(&self) -> &str {
        "station"
    }

    fn title(&self, _language: &str) -> String {
        tr("options.station").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut zlog_ui::Section, _language: &str) {
        let mut s = Settings::only("station", section);
        let mut st = s.station();
        station_page(ui, &mut st);
        s.set_station(&st);
        *section = s.take("station");
    }
}

/// The preferences tab (section `preferences`): the contest period, the QSY
/// rules, the default QSL state, the PC name and, when more than one is linked in,
/// the style of the entry row.
pub struct PreferencesPage {
    /// The entry styles linked in, to name them.
    styles: Vec<Box<dyn zlog_ui::EntryStyle>>,
}

impl PreferencesPage {
    pub fn new(styles: Vec<Box<dyn zlog_ui::EntryStyle>>) -> Self {
        PreferencesPage { styles }
    }
}

impl SettingsPage for PreferencesPage {
    fn id(&self) -> &str {
        "preferences"
    }

    fn title(&self, _language: &str) -> String {
        tr("options.preferences").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut zlog_ui::Section, language: &str) {
        let mut s = Settings::only("preferences", section);
        let mut period = s.use_contest_period();
        ui.checkbox(&mut period, tr("options.use_contest_period"));
        ui.add_space(6.0);
        let mut p = s.preferences();
        preferences_page(ui, &mut p);
        s.set_use_contest_period(period);
        s.set_preferences(&p);
        if self.styles.len() > 1 {
            ui.add_space(6.0);
            let saved = s.entry_style();
            let mut style = saved.clone();
            let first = self.styles[0].id().to_string();
            let chosen = self
                .styles
                .iter()
                .find(|e| e.id() == style)
                .map_or(first, |e| e.id().to_string());
            ui.horizontal(|ui| {
                ui.label(tr("options.entry_style"));
                let name = |id: &str| {
                    let e = self.styles.iter().find(|e| e.id() == id);
                    e.map(|e| e.title(language)).unwrap_or_default()
                };
                egui::ComboBox::from_id_salt("entry_style")
                    .selected_text(name(&chosen))
                    .show_ui(ui, |ui| {
                        for e in &self.styles {
                            ui.selectable_value(&mut style, e.id().to_string(), e.title(language));
                        }
                    });
            });
            if style != saved {
                s.set_entry_style(&style);
            }
        }
        *section = s.take("preferences");
    }
}

/// The tab of rig 1 or rig 2 (section `rig` or `rig2`, [`crate::rigs::SECTIONS`]).
pub struct RigPage {
    /// 1 or 2.
    number: u8,
    /// The serial ports, listed when the tab is first shown.
    ports: Option<Vec<String>>,
}

impl RigPage {
    pub fn new(number: u8) -> Self {
        RigPage {
            number,
            ports: None,
        }
    }
}

impl SettingsPage for RigPage {
    fn id(&self) -> &str {
        crate::rigs::SECTIONS[usize::from(self.number == 2)]
    }

    fn title(&self, _language: &str) -> String {
        let key = if self.number == 2 {
            "options.rig2"
        } else {
            "options.rig"
        };
        tr(key).into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut zlog_ui::Section, _language: &str) {
        let ports = self.ports.get_or_insert_with(|| {
            zlog_rig::serial::available_ports()
                .map(|ps| ps.into_iter().map(|p| p.port_name).collect())
                .unwrap_or_default()
        });
        let mut rig = RigConfig::load(section);
        rig_page(ui, &mut rig, ports);
        rig.save(section);
    }
}

/// A one-line field `width` wide.
fn sized_edit(ui: &mut Ui, text: &mut String, width: f32) {
    zlog_ui::sized_field(ui, egui::TextEdit::singleline(text), width);
}

/// [`zlog_ui::form_field_width`] for labels given by their keys.
fn field_width(ui: &Ui, keys: &[&str]) -> f32 {
    let labels: Vec<&str> = keys.iter().map(|k| tr(k)).collect();
    zlog_ui::form_field_width(ui, &labels)
}

fn station_page(ui: &mut Ui, st: &mut Station) {
    let width = field_width(
        ui,
        &[
            "menu_form.callsign",
            "menu_form.operator",
            "menu_form.prov",
            "menu_form.city",
            "menu_form.age",
            "menu_form.cq_zone",
            "menu_form.iaru_zone",
            "menu_form.itu_region",
            "menu_form.iota",
            "menu_form.handle_cw",
            "menu_form.handle_ph",
            "menu_form.tx_nr",
        ],
    );
    egui::Grid::new("station").num_columns(2).show(ui, |ui| {
        for (key, value) in [
            ("menu_form.callsign", &mut st.callsign),
            ("menu_form.operator", &mut st.operator),
            ("menu_form.prov", &mut st.prov),
            ("menu_form.city", &mut st.city),
            ("menu_form.age", &mut st.age),
            ("menu_form.cq_zone", &mut st.cq_zone),
            ("menu_form.iaru_zone", &mut st.iaru_zone),
            ("menu_form.itu_region", &mut st.itu_region),
            ("menu_form.iota", &mut st.iota),
        ] {
            ui.label(tr(key));
            sized_edit(ui, value, width);
            *value = value.to_uppercase();
            ui.end_row();
        }
        // Handles (`_myhandle_cw`, `_myhandle_ph`, macro `$H`), kept as typed.
        for (key, value) in [
            ("menu_form.handle_cw", &mut st.handle_cw),
            ("menu_form.handle_ph", &mut st.handle_ph),
        ] {
            ui.label(tr(key));
            sized_edit(ui, value, width);
            ui.end_row();
        }
        ui.label(tr("menu_form.tx_nr"));
        ui.add(egui::DragValue::new(&mut st.tx_nr).range(0..=15));
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label(RichText::new(tr("options.power_per_band")).strong());
    egui::ScrollArea::vertical()
        .max_height(ui.available_height().max(120.0))
        .show(ui, |ui| {
            egui::Grid::new("power").striped(true).show(ui, |ui| {
                for band in Band::range(Band::B1_9, Band::HIGHEST) {
                    ui.monospace(band.label());
                    let mut power = st.power_for(band);
                    for code in ['H', 'M', 'L', 'P'] {
                        let p = Power::from_code(code).unwrap();
                        ui.radio_value(&mut power, p, code.to_string());
                    }
                    if power != st.power_for(band) {
                        st.power.insert(band, power);
                    }
                    ui.end_row();
                }
            });
        });
}

/// The QSY rules (UOptions2.pas `groupQsyAssist`: `CountDown`, `QSYCount`, 1 to 99),
/// the default QSL state (`groupMyQslDefault`) and the PC name (`[Z-Link] PCName`).
fn preferences_page(ui: &mut Ui, p: &mut Preferences) {
    let width = field_width(
        ui,
        &[
            "options.countdown",
            "options.qsy_count",
            "options.qsl_default",
            "options.pc_name",
        ],
    );
    egui::Grid::new("preferences")
        .num_columns(2)
        .show(ui, |ui| {
            let mut on = p.qsy_rule.countdown_minutes.is_some();
            ui.checkbox(&mut on, tr("options.countdown"));
            ui.add_enabled(
                on,
                egui::DragValue::new(&mut p.countdown_minutes).range(1..=99),
            );
            p.set_countdown(on);
            ui.end_row();
            let mut on = p.qsy_rule.max_qsy_per_hour.is_some();
            ui.checkbox(&mut on, tr("options.qsy_count"));
            ui.add_enabled(on, egui::DragValue::new(&mut p.qsy_per_hour).range(1..=99));
            p.set_qsy_count(on);
            ui.end_row();
            ui.label(tr("options.qsl_default"));
            ui.horizontal(|ui| {
                for (state, key) in [
                    (QslState::None, "options.qsl_none"),
                    (QslState::PseQsl, "options.qsl_pse"),
                    (QslState::NoQsl, "options.qsl_no"),
                ] {
                    ui.radio_value(&mut p.qsl_default, state, tr(key));
                }
            });
            ui.end_row();
            ui.label(tr("options.pc_name"));
            sized_edit(ui, &mut p.pc_name, width);
            ui.end_row();
        });
}

fn rig_page(ui: &mut Ui, rig: &mut RigConfig, ports: &[String]) {
    egui::Grid::new("rig").num_columns(2).show(ui, |ui| {
        ui.label(tr("rig.model"));
        egui::ComboBox::from_id_salt("rig_model")
            .selected_text(MODELS[rig.model].name)
            .show_ui(ui, |ui| {
                for (i, m) in MODELS.iter().enumerate() {
                    if ui.selectable_value(&mut rig.model, i, m.name).clicked() {
                        rig.baud = m.baud;
                    }
                }
            });
        ui.end_row();
        ui.label(tr("rig.port"));
        let shown = if rig.port == SIMULATOR {
            tr("rig.simulator").to_string()
        } else {
            rig.port.clone()
        };
        egui::ComboBox::from_id_salt("rig_port")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut rig.port, SIMULATOR.to_string(), tr("rig.simulator"));
                for p in ports {
                    ui.selectable_value(&mut rig.port, p.clone(), p);
                }
            });
        ui.end_row();
        ui.label(tr("rig.baud"));
        egui::ComboBox::from_id_salt("rig_baud")
            .selected_text(rig.baud.to_string())
            .show_ui(ui, |ui| {
                for b in [4800, 9600, 19200, 38400, 57600, 115200] {
                    ui.selectable_value(&mut rig.baud, b, b.to_string());
                }
            });
        ui.end_row();
        ui.label(tr("rig.transverter"));
        ui.add(egui::DragValue::new(&mut rig.transverter_khz).range(0..=300_000_000));
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut rig.init_freq_on_mode, tr("rig.init_freq_on_mode"));
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut rig.ignore_rig_mode, tr("rig.ignore_rig_mode"));
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut rig.rtty_keeps_rig_mode, tr("rig.rtty_keeps_rig_mode"));
        ui.end_row();
    });
}

fn khz(hz: Option<u64>) -> String {
    hz.map(|h| format!("{:.1}", h as f64 / 1000.0))
        .unwrap_or_else(|| "-".into())
}

/// The rig window: RIG1 and RIG2 to choose the rig in use, what the selected rig
/// reports, connect and disconnect, and the controls of the simulated radio when it
/// is in use. Returns the rig chosen, which the caller selects.
pub fn rig_window(
    ctx: &egui::Context,
    rigs: &mut Rigs,
    open: &mut bool,
    config: &RigConfig,
) -> Option<u8> {
    let mut chosen = None;
    let current = rigs.number();
    let rig: &mut RigControl = rigs;
    let connected = rig.connected();
    let shown = dialog_with_buttons(
        ctx,
        "rig",
        tr("window.rig"),
        [380.0, 330.0],
        open,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("rig.select"));
                for n in 1..=2u8 {
                    if ui
                        .selectable_label(n == current, format!("RIG{n}"))
                        .clicked()
                    {
                        chosen = Some(n);
                    }
                }
            });
            ui.separator();
            rig_state(ui, rig, config)
        },
        |ui| {
            let key = if connected {
                "rig.disconnect"
            } else {
                "rig.connect"
            };
            ui.button(tr(key)).clicked()
        },
    );
    if shown.is_some_and(|(_, clicked)| clicked) {
        if connected {
            rig.disconnect();
        } else {
            rig.connect(config.clone());
        }
    }
    chosen.filter(|n| *n != current)
}

/// What the rig reports, and the controls of the simulated radio when it is in use.
fn rig_state(ui: &mut Ui, rig: &mut RigControl, config: &RigConfig) {
    {
        if config.port == crate::rig::SIMULATOR {
            ui.label(RichText::new(tr("rig.simulator")).strong());
        } else {
            ui.label(RichText::new(MODELS[config.model].name).strong());
            // The settings Connect uses; they are changed in Options.
            ui.label(format!(
                "{}: {}   {}: {}",
                tr("rig.port"),
                config.port,
                tr("rig.baud"),
                config.baud
            ));
        }
        if rig.connected() {
            let st = rig.state().unwrap_or_default();
            egui::Grid::new("rig_state").num_columns(2).show(ui, |ui| {
                ui.label(tr("rig.vfo_a"));
                ui.monospace(khz(st.freq_a));
                ui.end_row();
                ui.label(tr("rig.vfo_b"));
                ui.monospace(khz(st.freq_b));
                ui.end_row();
                ui.label(tr("rig.mode"));
                ui.monospace(
                    st.mode
                        .map(|m| m.label().to_string())
                        .unwrap_or_else(|| "-".into()),
                );
                ui.end_row();
            });
            if let Some(e) = rig.error() {
                ui.colored_label(zlog_ui::palette::error(ui), e);
            }
        } else {
            ui.weak(tr("rig.not_connected"));
            if let Some(m) = &rig.message {
                ui.colored_label(zlog_ui::palette::error(ui), m);
            }
        }
        if let Some(sim) = rig.sim.clone() {
            ui.separator();
            ui.label(RichText::new(tr("rig.simulator")).strong());
            let mut khz = sim.with(|r| match r.vfo {
                zlog_rig::Vfo::A => r.freq_a,
                zlog_rig::Vfo::B => r.freq_b,
            }) as f64
                / 1000.0;
            ui.horizontal(|ui| {
                ui.label(tr("rig.sim.dial"));
                let dial = egui::DragValue::new(&mut khz)
                    .speed(0.1)
                    .range(1800.0..=10_250_000.0)
                    .fixed_decimals(1);
                if ui.add(dial).changed() {
                    sim.tune((khz * 1000.0).round() as u64);
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("rig.sim.mode"));
                for m in [
                    RigMode::Cw,
                    RigMode::Lsb,
                    RigMode::Usb,
                    RigMode::Fm,
                    RigMode::Am,
                    RigMode::Rtty,
                ] {
                    if ui.button(m.label()).clicked() {
                        sim.press_mode(m);
                    }
                }
            });
            let mut on = sim.with(|r| r.power_on);
            if ui.checkbox(&mut on, tr("rig.sim.power")).changed() {
                sim.with(|r| r.power_on = on);
            }
        }
    }
}

/// Write into `live` the values `edited` changed from `base`; whether `live` changed.
fn merge_edits(
    base: &zlog_ui::Section,
    edited: &zlog_ui::Section,
    live: &mut zlog_ui::Section,
) -> bool {
    let before = live.clone();
    for k in base.keys().chain(edited.keys()) {
        match (base.get(k), edited.get(k)) {
            (b, e) if b == e => {}
            (_, Some(e)) => {
                live.insert(k.clone(), e.clone());
            }
            (_, None) => {
                live.remove(k);
            }
        }
    }
    *live != before
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression G1: OK wrote back whole sections as they were when the dialog
    /// opened, undoing what changed elsewhere meanwhile (the operator, a panel).
    #[test]
    fn ok_keeps_values_changed_elsewhere_while_open() {
        let mut o = Options::new(vec![Box::new(StationPage)]);
        let mut settings = Settings::default();
        settings
            .section("station")
            .insert("operator".into(), "JA1AAA".into());
        o.show_with(&mut settings);
        o.draft
            .section("station")
            .insert("callsign".into(), "JA1ZLO".into());
        // Changed outside the dialog while it is open.
        settings
            .section("station")
            .insert("operator".into(), "JA1BBB".into());
        let mut live = settings.section("station").clone();
        let base = o.base.section("station").clone();
        assert!(merge_edits(&base, o.draft.section("station"), &mut live));
        assert_eq!(live["callsign"], "JA1ZLO");
        assert_eq!(live["operator"], "JA1BBB");
        assert!(!merge_edits(&base, &base, &mut live));
    }

    /// U9: drawing a tab wrote its defaults into the draft, and OK counted them as
    /// edits, overwriting values changed elsewhere meanwhile.
    #[test]
    fn drawing_a_tab_is_not_an_edit() {
        let mut o = Options::new(vec![Box::new(StationPage)]);
        let mut settings = Settings::default();
        o.show_with(&mut settings);
        let ctx = egui::Context::default();
        let frame = |o: &mut Options, settings: &mut Settings, events| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut ok = None;
            let mut out = ctx.run_ui(input, |ui| ok = o.ui(ui.ctx(), settings));
            out.textures_delta.clear();
            ok
        };
        assert!(frame(&mut o, &mut settings, vec![]).is_none());
        // Changed elsewhere while the dialog is open.
        let changed: Vec<(String, String)> = o
            .draft
            .section("station")
            .keys()
            .map(|k| (k.clone(), "elsewhere".to_string()))
            .collect();
        assert!(!changed.is_empty(), "the tab wrote its values");
        for (k, v) in &changed {
            settings.section("station").insert(k.clone(), v.clone());
        }
        // OK merges against the section as first drawn: nothing to write.
        let mut live = settings.section("station").clone();
        let drawn = o.drawn["station"].clone();
        assert!(!merge_edits(&drawn, o.draft.section("station"), &mut live));
        for (k, v) in &changed {
            assert_eq!(&live[k], v);
        }
    }

    #[test]
    fn opening_again_keeps_the_tabs_edits() {
        let mut o = Options::new(vec![Box::new(StationPage)]);
        let mut settings = Settings::default();
        o.show_with(&mut settings);
        o.draft
            .section("station")
            .insert("callsign".into(), "JA1ZLO".into());
        o.show_with(&mut settings);
        assert_eq!(o.draft.section("station")["callsign"], "JA1ZLO");
    }
}
