use cw4i_ui::{Contact, CutTable};
use regex::Regex;
use zlog_keyer::Abbrev;
use zlog_ui::device::{AudioDrv, SettingKind};
use zlog_ui::spot::{heard_hz, Sideband};
use zlog_ui::{EntryAction, Section, Sections, SettingsPage, Spot};

use crate::texts;

pub const SECTION: &str = "panel.cw-decoder";

pub const DEFAULT_CALLSIGN: &str = "[A-Z0-9]{0,2}[A-Z][0-9]+[A-Z]{1,4}";

const JA_PREFIX: &str = "(J[A-S]|7[J-N]|8[J-N])";

const ENABLED: &str = "extension";
const CALLSIGN: &str = "extension.callsign";
const PORTABLE: &str = "extension.portable";
const JA_ONLY: &str = "extension.ja_only";
const NUMBER: &str = "extension.number";
const NUMBER_PATTERN: &str = "extension.number.pattern";
const CUT: &str = "extension.cut";
const CUT_TABLE: &str = "extension.cut.table";
const SCOPE: &str = "extension.scope";
const PITCH: &str = "extension.pitch";
const CW_REVERSE: &str = "extension.cw_reverse";

pub const SOURCE: &str = "CW4I";

pub const DEFAULT_PITCH: u32 = 600;

pub const KEYER_SECTION: &str = "panel.keyer";

pub fn keyer_abbrev(others: &Sections) -> Abbrev {
    others
        .get(KEYER_SECTION)
        .map_or_else(Abbrev::default, |s| zlog_keyer::Settings::load(s).abbrev)
}

pub fn default_table(abbrev: &Abbrev) -> CutTable {
    let mut pairs = CutTable::default().pairs();
    pairs.extend([(abbrev.zero, '0'), (abbrev.one, '1'), (abbrev.nine, '9')]);
    CutTable::new(pairs)
}

pub fn table_text(table: &CutTable) -> String {
    let pairs = table.pairs().into_iter();
    pairs
        .map(|(l, d)| format!("{l}{d}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_table(text: &str) -> CutTable {
    CutTable::new(text.split_whitespace().filter_map(|w| {
        let mut c = w.chars();
        match (c.next(), c.next(), c.next()) {
            (Some(l), Some(d), None) => Some((l, d)),
            _ => None,
        }
    }))
}

pub fn letters_for(table: &CutTable, digit: char) -> String {
    let pairs = table.pairs().into_iter();
    pairs.filter(|(_, d)| *d == digit).map(|(l, _)| l).collect()
}

pub fn with_letters(table: &CutTable, digit: char, letters: &str) -> CutTable {
    let mut pairs: Vec<(char, char)> = table
        .pairs()
        .into_iter()
        .filter(|(_, d)| *d != digit)
        .collect();
    for l in letters.chars().map(|c| c.to_ascii_uppercase()) {
        pairs.retain(|(k, _)| *k != l);
        pairs.push((l, digit));
    }
    CutTable::new(pairs)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberKind {
    Contest,
    Serial,
    CqZone,
    ItuZone,
    Free,
    Custom,
}

impl NumberKind {
    pub const ALL: [NumberKind; 6] = [
        NumberKind::Contest,
        NumberKind::Serial,
        NumberKind::CqZone,
        NumberKind::ItuZone,
        NumberKind::Free,
        NumberKind::Custom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            NumberKind::Contest => "contest",
            NumberKind::Serial => "serial",
            NumberKind::CqZone => "cq_zone",
            NumberKind::ItuZone => "itu_zone",
            NumberKind::Free => "free",
            NumberKind::Custom => "custom",
        }
    }

    fn preset(self) -> Option<&'static str> {
        match self {
            NumberKind::Serial => Some("[0-9]{1,5}"),
            NumberKind::CqZone => Some("0?[1-9]|[1-3][0-9]|40"),
            NumberKind::ItuZone => Some("0?[1-9]|[1-8][0-9]|90"),
            NumberKind::Free => Some(".+"),
            NumberKind::Contest | NumberKind::Custom => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formats {
    pub enabled: bool,
    pub callsign: String,
    pub portable: bool,
    pub ja_only: bool,
    pub number: NumberKind,
    pub number_pattern: String,
    pub cut: bool,
    pub table: CutTable,
    pub scope: bool,
    pub pitch: u32,
    pub cw_reverse: bool,
    pub keyer: Abbrev,
}

impl Default for Formats {
    fn default() -> Self {
        Formats {
            enabled: false,
            callsign: DEFAULT_CALLSIGN.into(),
            portable: true,
            ja_only: false,
            number: NumberKind::Contest,
            number_pattern: String::new(),
            cut: true,
            table: default_table(&Abbrev::default()),
            scope: false,
            pitch: DEFAULT_PITCH,
            cw_reverse: false,
            keyer: Abbrev::default(),
        }
    }
}

fn anchored(pattern: &str) -> Result<Regex, String> {
    Regex::new(&format!("^(?:{})$", pattern.trim())).map_err(|e| e.to_string())
}

pub struct Matcher {
    callsign: Regex,
    ja: Option<Regex>,
    number: Option<Regex>,
    formats: Formats,
}

impl Formats {
    pub fn load(section: &Section) -> Self {
        Formats::load_with(section, Abbrev::default())
    }

    pub fn load_with(section: &Section, keyer: Abbrev) -> Self {
        let d = Formats {
            table: default_table(&keyer),
            keyer,
            ..Formats::default()
        };
        let factory = default_table(&Abbrev::default());
        let get = |k: &str| section.get(k).map(String::as_str);
        let flag = |k: &str, d: bool| get(k).map_or(d, |v| v == "true");
        Formats {
            enabled: flag(ENABLED, d.enabled),
            callsign: get(CALLSIGN).unwrap_or(&d.callsign).into(),
            portable: flag(PORTABLE, d.portable),
            ja_only: flag(JA_ONLY, d.ja_only),
            number: get(NUMBER)
                .and_then(|v| NumberKind::ALL.into_iter().find(|k| k.name() == v))
                .unwrap_or(d.number),
            number_pattern: get(NUMBER_PATTERN).unwrap_or_default().into(),
            cut: flag(CUT, d.cut),
            table: get(CUT_TABLE)
                .map(parse_table)
                .filter(|t| *t != factory)
                .unwrap_or(d.table),
            scope: flag(SCOPE, d.scope),
            pitch: get(PITCH)
                .and_then(|v| v.parse().ok())
                .map_or(d.pitch, |p: u32| p.clamp(100, 3000)),
            cw_reverse: flag(CW_REVERSE, d.cw_reverse),
            keyer,
        }
    }

    pub fn reset_table(&mut self) {
        self.table = default_table(&self.keyer);
    }

    pub fn customised(&self) -> bool {
        self.table != default_table(&self.keyer)
    }

    pub fn save(&self, section: &mut Section) {
        section.insert(ENABLED.into(), self.enabled.to_string());
        section.insert(CALLSIGN.into(), self.callsign.trim().into());
        section.insert(PORTABLE.into(), self.portable.to_string());
        section.insert(JA_ONLY.into(), self.ja_only.to_string());
        section.insert(NUMBER.into(), self.number.name().into());
        section.insert(NUMBER_PATTERN.into(), self.number_pattern.trim().into());
        section.insert(CUT.into(), self.cut.to_string());
        if self.customised() {
            section.insert(CUT_TABLE.into(), table_text(&self.table));
        } else {
            section.remove(CUT_TABLE);
        }
        section.insert(SCOPE.into(), self.scope.to_string());
        section.insert(PITCH.into(), self.pitch.to_string());
        section.insert(CW_REVERSE.into(), self.cw_reverse.to_string());
    }

    fn callsign_pattern(&self) -> &str {
        match self.callsign.trim() {
            "" => DEFAULT_CALLSIGN,
            p => p,
        }
    }

    pub fn callsign_error(&self) -> Option<String> {
        anchored(self.callsign_pattern()).err()
    }

    pub fn number_error(&self) -> Option<String> {
        match self.number {
            NumberKind::Custom if self.number_pattern.trim().is_empty() => Some(String::new()),
            NumberKind::Custom => anchored(&self.number_pattern).err(),
            _ => None,
        }
    }

    pub fn matcher(&self) -> Result<Matcher, String> {
        if let Some(e) = self.number_error() {
            return Err(e);
        }
        let callsign = anchored(self.callsign_pattern())?;
        let number = match (self.number, self.number.preset()) {
            (_, Some(p)) => Some(anchored(p)?),
            (NumberKind::Custom, None) => Some(anchored(&self.number_pattern)?),
            _ => None,
        };
        let ja = self
            .ja_only
            .then(|| anchored(&format!("{JA_PREFIX}.*")))
            .transpose()?;
        Ok(Matcher {
            callsign,
            ja,
            number,
            formats: self.clone(),
        })
    }
}

fn cut(word: &str, table: &CutTable) -> String {
    let mapped = table.cut(word);
    let body = mapped.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let tail = mapped.len() - body.len();
    if !body.is_empty() && body.chars().all(|c| c.is_ascii_digit()) && tail <= 1 {
        let mut out = body.to_string();
        out.push_str(&word[body.len()..]);
        out
    } else {
        word.to_string()
    }
}

impl Matcher {
    pub fn callsign(&self, call: &str) -> Option<String> {
        let call = call.trim().to_uppercase();
        let parts: Vec<&str> = call.split('/').collect();
        if parts.len() > 1 && !self.formats.portable {
            return None;
        }
        let base = parts.iter().find(|p| self.callsign.is_match(p))?;
        if self.ja.as_ref().is_some_and(|ja| !ja.is_match(base)) {
            return None;
        }
        Some(call.clone())
    }

    pub fn number(&self, words: &[String], contest: &dyn Fn(&str) -> bool) -> Option<String> {
        let words: Vec<String> = words
            .iter()
            .map(|w| match self.formats.cut {
                true => cut(w, &self.formats.table),
                false => w.clone(),
            })
            .collect();
        if words.is_empty() {
            return None;
        }
        let ok = |s: &str| match &self.number {
            Some(re) => re.is_match(s),
            None => contest(s),
        };
        let joined = words.join(" ");
        if ok(&joined) {
            return Some(joined);
        }
        words.iter().rev().find(|w| ok(w)).cloned()
    }
}

#[derive(Default)]
pub struct Extension {
    pub formats: Formats,
    filled: Option<String>,
}

impl Extension {
    pub fn fill(
        &mut self,
        contact: &Contact,
        entry_call: &str,
        contest: &dyn Fn(&str) -> bool,
    ) -> Vec<EntryAction> {
        if !self.formats.enabled {
            return Vec::new();
        }
        let Ok(m) = self.formats.matcher() else {
            return Vec::new();
        };
        let Some(call) = contact.callsign.as_deref().and_then(|c| m.callsign(c)) else {
            return Vec::new();
        };
        let entry_call = entry_call.trim();
        let ours = entry_call.is_empty() || self.filled.as_deref() == Some(entry_call);
        let mut out = Vec::new();
        if !entry_call.eq_ignore_ascii_case(&call) {
            if !ours {
                return out;
            }
            out.push(EntryAction::Callsign(call.clone()));
            self.filled = Some(call);
        }
        if ours {
            if let Some(rst) = contact.report.as_deref().and_then(|r| r.parse().ok()) {
                out.push(EntryAction::RstRcvd(rst));
            }
        }
        if let Some(n) = m.number(&contact.raw_exchange, contest) {
            out.push(EntryAction::Number(n));
        }
        out
    }

    pub fn spot(
        &self,
        contact: &Contact,
        dial: u64,
        contest: &dyn Fn(&str) -> bool,
        now: i64,
    ) -> Option<Spot> {
        if !self.formats.scope {
            return None;
        }
        let m = self.formats.matcher().ok()?;
        let callsign = m.callsign(contact.callsign.as_deref()?)?;
        let number = match contact.raw_exchange.is_empty() {
            true => String::new(),
            false => m.number(&contact.raw_exchange, contest)?,
        };
        let sideband = match self.formats.cw_reverse {
            true => Sideband::Lower,
            false => Sideband::Upper,
        };
        let pitch = self.formats.pitch as f64;
        Some(Spot {
            callsign,
            number,
            hz: heard_hz(dial, contact.freq, pitch, sideband),
            mode: Some(zlog_core::Mode::Cw),
            time: now,
            source: SOURCE.into(),
            ..Spot::default()
        })
    }
}

#[derive(Default)]
pub struct Options {
    drivers: Vec<Box<dyn AudioDrv>>,
}

impl Options {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>) -> Self {
        Options { drivers }
    }

    fn inputs(&self, lang: &str) -> Vec<(String, String)> {
        let field = self
            .drivers
            .iter()
            .find(|d| d.id() == crate::SOUND)
            .and_then(|d| d.settings(lang).into_iter().find(|f| f.key == "input"));
        let mut list = match field.map(|f| f.kind) {
            Some(SettingKind::Choice(list)) => list,
            _ => vec![(String::new(), texts().get(lang, "default").into())],
        };
        list.retain(|(v, _)| v != crate::SIMULATOR);
        list
    }

    fn input_ui(&self, ui: &mut egui::Ui, section: &mut Section, lang: &str, width: f32) {
        let list = self.inputs(lang);
        let mut value = crate::input_of(section).unwrap_or_default();
        section.remove("input");
        for k in crate::LEGACY_SIM {
            section.remove(k);
        }
        let shown = list
            .iter()
            .find(|(v, _)| *v == value)
            .map_or_else(|| value.clone(), |(_, l)| l.clone());
        ui.label(texts().get(lang, "device"));
        egui::ComboBox::from_id_salt("cw4isr_input")
            .width(width)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (v, l) in &list {
                    ui.selectable_value(&mut value, v.clone(), l);
                }
            });
        ui.end_row();
        section.insert(crate::INPUT.into(), value);
    }
}

impl SettingsPage for Options {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "ext").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, lang: &str) {
        self.ui_with(ui, section, &Sections::new(), lang);
    }

    fn ui_with(&mut self, ui: &mut egui::Ui, section: &mut Section, others: &Sections, lang: &str) {
        let t = |k| texts().get(lang, k);
        let labels = [
            t("device"),
            t("ext.callsign"),
            t("ext.number"),
            t("ext.pitch"),
        ];
        let width = zlog_ui::form_field_width(ui, &labels);
        let mut f = Formats::load_with(section, keyer_abbrev(others));
        let mut candidates = section.get("cw4i.candidates").is_some_and(|v| v == "true");
        egui::Grid::new("cw4isr_extension")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                self.input_ui(ui, section, lang, width);
                ui.label("");
                ui.vertical(|ui| {
                    ui.checkbox(&mut candidates, t("candidates"))
                        .on_hover_text(t("candidates.tip"));
                    ui.checkbox(&mut f.enabled, t("ext.enabled"))
                        .on_hover_text(t("ext.enabled.tip"));
                    ui.checkbox(&mut f.scope, t("ext.scope"))
                        .on_hover_text(t("ext.scope.tip"));
                });
                ui.end_row();
                ui.add_enabled_ui(f.scope, |ui| ui.label(t("ext.pitch")));
                ui.add_enabled_ui(f.scope, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let check = ui.spacing().interact_size.x * 2.0;
                        let drag = egui::DragValue::new(&mut f.pitch)
                            .range(100..=3000)
                            .suffix(" Hz");
                        let height = ui.spacing().interact_size.y;
                        ui.add_sized([(width - check).max(60.0), height], drag);
                        ui.checkbox(&mut f.cw_reverse, t("ext.cw_reverse"));
                    });
                });
                ui.end_row();
                let on = f.enabled || f.scope;
                ui.add_enabled_ui(on, |ui| ui.label(t("ext.callsign")));
                ui.add_enabled_ui(on, |ui| {
                    ui.vertical(|ui| {
                        let edit = egui::TextEdit::singleline(&mut f.callsign)
                            .hint_text(DEFAULT_CALLSIGN)
                            .font(egui::TextStyle::Monospace);
                        zlog_ui::sized_field(ui, edit, width).on_hover_text(t("ext.pattern.tip"));
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut f.portable, t("ext.portable"));
                            ui.checkbox(&mut f.ja_only, t("ext.ja_only"));
                        });
                    });
                });
                ui.end_row();
                ui.add_enabled_ui(on, |ui| ui.label(t("ext.number")));
                ui.add_enabled_ui(on, |ui| {
                    ui.vertical(|ui| {
                        egui::ComboBox::from_id_salt("cw4isr_number")
                            .width(width)
                            .selected_text(t(number_key(f.number)))
                            .show_ui(ui, |ui| {
                                for k in NumberKind::ALL {
                                    ui.selectable_value(&mut f.number, k, t(number_key(k)));
                                }
                            });
                        if f.number == NumberKind::Custom {
                            let edit = egui::TextEdit::singleline(&mut f.number_pattern)
                                .hint_text("[0-9]{1,4}")
                                .font(egui::TextStyle::Monospace);
                            zlog_ui::sized_field(ui, edit, width)
                                .on_hover_text(t("ext.pattern.tip"));
                        }
                        ui.checkbox(&mut f.cut, t("ext.cut"));
                    });
                });
                ui.end_row();
            });
        section.insert("cw4i.candidates".into(), candidates.to_string());
        ui.add_enabled_ui(f.enabled || f.scope, |ui| {
            ui.add_enabled_ui(f.cut, |ui| cut_table_ui(ui, &mut f, lang));
            let error = zlog_ui::palette::error(ui);
            if let Some(e) = f.callsign_error() {
                ui.colored_label(error, format!("{}: {e}", t("ext.invalid.callsign")));
            }
            if let Some(e) = f.number_error() {
                let label = t("ext.invalid.number");
                let text = if e.is_empty() {
                    label.into()
                } else {
                    format!("{label}: {e}")
                };
                ui.colored_label(error, text);
            }
        });
        f.save(section);
    }
}

fn cut_table_ui(ui: &mut egui::Ui, f: &mut Formats, lang: &str) {
    let t = |k| texts().get(lang, k);
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label(t("ext.cut.table"));
        if ui.small_button(t("ext.cut.reset")).clicked() {
            f.reset_table();
        }
    });
    let gap = ui.spacing().item_spacing.x;
    let width = ((ui.available_width() - 9.0 * gap - 2.0) / 10.0).max(16.0);
    egui::Grid::new("cw4isr_cut_table")
        .num_columns(10)
        .min_col_width(width)
        .show(ui, |ui| {
            for d in '0'..='9' {
                ui.vertical_centered(|ui| ui.monospace(d.to_string()));
            }
            ui.end_row();
            for d in '0'..='9' {
                let mut letters = letters_for(&f.table, d);
                let edit = egui::TextEdit::singleline(&mut letters)
                    .font(egui::TextStyle::Monospace)
                    .char_limit(8);
                if zlog_ui::sized_field(ui, edit, width).changed() {
                    f.table = with_letters(&f.table, d, &letters);
                }
            }
            ui.end_row();
        });
}

fn number_key(k: NumberKind) -> &'static str {
    match k {
        NumberKind::Contest => "ext.number.contest",
        NumberKind::Serial => "ext.number.serial",
        NumberKind::CqZone => "ext.number.cq_zone",
        NumberKind::ItuZone => "ext.number.itu_zone",
        NumberKind::Free => "ext.number.free",
        NumberKind::Custom => "ext.number.custom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(call: &str, raw: &[&str]) -> Contact {
        Contact {
            station: 1,
            callsign: Some(call.into()),
            report: None,
            exchange: Vec::new(),
            raw_exchange: raw.iter().map(|s| s.to_string()).collect(),
            freq: 700.0,
            wpm: 25.0,
            p: 0.9,
            time: std::time::SystemTime::now(),
        }
    }

    fn on() -> Extension {
        let mut e = Extension::default();
        e.formats.enabled = true;
        e
    }

    fn acag(n: &str) -> bool {
        n.len() >= 4 && n[..n.len() - 1].chars().all(|c| c.is_ascii_digit())
    }

    #[test]
    fn off_by_default_fills_nothing() {
        let mut e = Extension::default();
        assert!(!e.formats.enabled);
        assert!(e.fill(&contact("JA1ZLO", &["1317M"]), "", &acag).is_empty());
    }

    #[test]
    fn default_formats_fill_callsign_and_number() {
        let mut e = on();
        assert_eq!(
            e.fill(&contact("JA1ZLO", &["1317M"]), "", &acag),
            [
                EntryAction::Callsign("JA1ZLO".into()),
                EntryAction::Number("1317M".into())
            ]
        );
        assert_eq!(
            e.fill(&contact("JA1YWX", &["13NTM"]), "JA1ZLO", &acag),
            [
                EntryAction::Callsign("JA1YWX".into()),
                EntryAction::Number("1390M".into())
            ]
        );
        assert_eq!(e.fill(&contact("JA1YWX", &["TU"]), "JA1YWX", &acag), []);
    }

    #[test]
    fn a_report_fills_the_rst_between_callsign_and_number() {
        let mut e = on();
        let mut c = contact("JA1ZLO", &["1317M"]);
        c.report = cw4i_ui::contact::report_of("5NN");
        assert_eq!(
            e.fill(&c, "", &acag),
            [
                EntryAction::Callsign("JA1ZLO".into()),
                EntryAction::RstRcvd(599),
                EntryAction::Number("1317M".into())
            ]
        );
        assert_eq!(
            e.fill(&contact("JA1YWX", &["1390M"]), "JA1ZLO", &acag),
            [
                EntryAction::Callsign("JA1YWX".into()),
                EntryAction::Number("1390M".into())
            ]
        );
    }

    #[test]
    fn a_report_never_overwrites_a_typed_entry() {
        let mut e = on();
        let mut c = contact("JA1ZLO", &["1317M"]);
        c.report = Some("599".into());
        assert!(e.fill(&c, "JA1Y", &acag).is_empty());
        assert_eq!(
            e.fill(&c, "ja1zlo", &acag),
            [EntryAction::Number("1317M".into())]
        );
    }

    #[test]
    fn a_typed_callsign_is_never_overwritten() {
        let mut e = on();
        assert!(e
            .fill(&contact("JA1ZLO", &["1317M"]), "JA1Y", &acag)
            .is_empty());
        assert_eq!(
            e.fill(&contact("JA1ZLO", &["1317M"]), "ja1zlo", &acag),
            [EntryAction::Number("1317M".into())]
        );
    }

    #[test]
    fn cut_numbers_can_be_turned_off() {
        let mut e = on();
        e.formats.cut = false;
        assert_eq!(
            e.fill(&contact("JA1ZLO", &["1A1TM"]), "", &acag),
            [EntryAction::Callsign("JA1ZLO".into())]
        );
    }

    #[test]
    fn a_custom_serial_pattern() {
        let mut e = on();
        e.formats.number = NumberKind::Custom;
        e.formats.number_pattern = "[0-9]{3}".into();
        assert_eq!(
            e.fill(&contact("W1AW", &["1317M", "TNN"]), "", &|_| false),
            [
                EntryAction::Callsign("W1AW".into()),
                EntryAction::Number("099".into())
            ]
        );
        e.formats.number = NumberKind::Serial;
        assert_eq!(
            e.fill(&contact("W1AW", &["15"]), "", &|_| false)[1],
            EntryAction::Number("15".into())
        );
    }

    #[test]
    fn ja_only_and_portable_restrictions() {
        let mut e = on();
        e.formats.ja_only = true;
        assert!(e.fill(&contact("W1AW", &[]), "", &acag).is_empty());
        assert_eq!(
            e.fill(&contact("7K1ABC/1", &[]), "", &acag),
            [EntryAction::Callsign("7K1ABC/1".into())]
        );
        e.formats.portable = false;
        assert!(e.fill(&contact("JA1ZLO/1", &[]), "", &acag).is_empty());
    }

    #[test]
    fn invalid_patterns_are_rejected() {
        let mut f = Formats {
            callsign: "[A-Z".into(),
            ..Formats::default()
        };
        assert!(f.callsign_error().is_some() && f.matcher().is_err());
        f.callsign = DEFAULT_CALLSIGN.into();
        f.number = NumberKind::Custom;
        f.number_pattern = "(12".into();
        assert!(f.number_error().is_some() && f.matcher().is_err());
        f.number_pattern = String::new();
        assert!(f.number_error().is_some());
        let mut e = Extension {
            formats: f,
            ..Extension::default()
        };
        e.formats.enabled = true;
        assert!(e.fill(&contact("JA1ZLO", &["15"]), "", &acag).is_empty());
    }

    #[test]
    fn an_emptied_callsign_pattern_stays_empty_and_matches_by_default() {
        let f = Formats {
            enabled: true,
            callsign: String::new(),
            ..Formats::default()
        };
        let mut s = Section::new();
        f.save(&mut s);
        assert_eq!(Formats::load(&s), f, "the field can be cleared to retype");
        assert!(f.callsign_error().is_none());
        let mut e = Extension {
            formats: f,
            ..Extension::default()
        };
        assert_eq!(
            e.fill(&contact("JA1ZLO", &[]), "", &acag),
            [EntryAction::Callsign("JA1ZLO".into())]
        );
    }

    #[test]
    fn the_default_cut_table_comes_from_the_abbreviations() {
        let f = Formats::default();
        assert_eq!(table_text(&f.table), "A1 E5 N9 O0 T0 U2 V3");
        assert_eq!(f.table, CutTable::default(), "O, A and N as ALLJA1");
        let keyer = Abbrev {
            zero: 'D',
            one: 'A',
            nine: 'R',
        };
        let t = default_table(&keyer);
        assert_eq!(letters_for(&t, '0'), "DOT");
        assert_eq!(letters_for(&t, '9'), "NR");
        let digits = Abbrev {
            zero: '0',
            one: '1',
            nine: '9',
        };
        assert_eq!(default_table(&digits), CutTable::default());
    }

    fn keyer(zero: &str, one: &str, nine: &str) -> Sections {
        let mut k = Section::new();
        k.insert("zero".into(), zero.into());
        k.insert("one".into(), one.into());
        k.insert("nine".into(), nine.into());
        Sections::from([(KEYER_SECTION.to_string(), k)])
    }

    #[test]
    fn the_keyer_abbreviations_give_the_default_cut_table() {
        let f = Formats::load_with(&Section::new(), keyer_abbrev(&keyer("T", "A", "N")));
        assert_eq!(letters_for(&f.table, '0'), "OT");
        assert_eq!(letters_for(&f.table, '1'), "A");
        assert_eq!(letters_for(&f.table, '9'), "N");
        assert!(!f.customised());
        let mut s = Section::new();
        f.save(&mut s);
        assert!(!s.contains_key(CUT_TABLE));
        let g = Formats::load_with(&s, keyer_abbrev(&keyer("D", "B", "R")));
        assert_eq!(letters_for(&g.table, '0'), "DOT");
        assert_eq!(letters_for(&g.table, '1'), "AB");
        assert_eq!(letters_for(&g.table, '9'), "NR");
    }

    #[test]
    fn a_custom_cut_table_survives_keyer_changes() {
        let mut f = Formats::load_with(&Section::new(), keyer_abbrev(&keyer("T", "A", "N")));
        f.table = with_letters(&f.table, '0', "X");
        let mut s = Section::new();
        f.save(&mut s);
        let g = Formats::load_with(&s, keyer_abbrev(&keyer("D", "B", "R")));
        assert_eq!(g.table, f.table);
        assert!(g.customised());
    }

    #[test]
    fn reset_uses_the_keyer_abbreviations() {
        let mut f = Formats::load_with(&Section::new(), keyer_abbrev(&keyer("D", "B", "R")));
        f.table = CutTable::new([('X', '0')]);
        f.reset_table();
        assert_eq!(f.table, default_table(&f.keyer));
        assert_eq!(letters_for(&f.table, '9'), "NR");
        assert!(!f.customised());
    }

    #[test]
    fn a_saved_factory_table_follows_the_keyer() {
        let mut s = Section::new();
        s.insert(CUT_TABLE.into(), table_text(&CutTable::default()));
        let f = Formats::load_with(&s, keyer_abbrev(&keyer("D", "B", "R")));
        assert_eq!(letters_for(&f.table, '0'), "DOT");
    }

    #[test]
    fn the_cut_table_is_edited_per_digit() {
        let t = with_letters(&CutTable::default(), '0', "ox");
        assert_eq!(letters_for(&t, '0'), "OX");
        assert_eq!(letters_for(&t, '1'), "A");
        let t = with_letters(&t, '1', "X");
        assert_eq!(
            (letters_for(&t, '0'), letters_for(&t, '1')),
            ("O".into(), "X".into())
        );
        assert_eq!(
            parse_table("A1 N9 bad 99 T0"),
            CutTable::new([('A', '1'), ('N', '9'), ('T', '0')])
        );
    }

    #[test]
    fn a_custom_cut_table_changes_the_fill() {
        let mut e = on();
        assert_eq!(
            e.fill(&contact("JA1ZLO", &["1V1EM"]), "", &acag)[1],
            EntryAction::Number("1315M".into())
        );
        e.formats.table = CutTable::new([('N', '9'), ('T', '0'), ('A', '1')]);
        assert_eq!(
            e.fill(&contact("JA1ZLO", &["1V1EM"]), "", &acag),
            [EntryAction::Callsign("JA1ZLO".into())]
        );
        assert_eq!(
            e.fill(&contact("JA1ZLO", &["1A1TM"]), "", &acag)[1],
            EntryAction::Number("1110M".into())
        );
    }

    fn heard(call: &str, raw: &[&str], freq: f64) -> Contact {
        Contact {
            freq,
            ..contact(call, raw)
        }
    }

    #[test]
    fn scope_spots_are_off_by_default_and_filtered() {
        let mut e = Extension::default();
        let c = heard("JA1ZLO", &["1317M"], 700.0);
        assert_eq!(e.spot(&c, 7_010_000, &acag, 5), None);
        e.formats.scope = true;
        let s = e.spot(&c, 7_010_000, &acag, 5).unwrap();
        assert_eq!(
            (s.callsign.as_str(), s.number.as_str(), s.hz, s.time),
            ("JA1ZLO", "1317M", 7_010_100, 5)
        );
        assert_eq!(
            (s.mode, s.source.as_str()),
            (Some(zlog_core::Mode::Cw), "CW4I")
        );
        assert_eq!(
            e.spot(&heard("JA1YWX", &[], 500.0), 7_010_000, &acag, 5)
                .unwrap()
                .number,
            ""
        );
        assert_eq!(
            e.spot(&heard("JA1YWX", &["X"], 500.0), 7_010_000, &acag, 5),
            None
        );
        e.formats.cw_reverse = true;
        e.formats.pitch = 700;
        assert_eq!(
            e.spot(&heard("JA1YWX", &[], 500.0), 7_010_000, &acag, 5)
                .unwrap()
                .hz,
            7_010_200
        );
        e.formats.ja_only = true;
        assert_eq!(
            e.spot(&heard("W1AW", &[], 500.0), 7_010_000, &acag, 5),
            None
        );
        assert!(e.fill(&c, "", &acag).is_empty(), "the fill stays off");
    }

    #[test]
    fn the_cut_table_reaches_the_scope_filter() {
        let mut e = Extension::default();
        e.formats.scope = true;
        let c = heard("JA1ZLO", &["1V1EM"], 700.0);
        assert_eq!(e.spot(&c, 7_010_000, &acag, 5).unwrap().number, "1315M");
        e.formats.table = CutTable::new([('N', '9')]);
        assert_eq!(e.spot(&c, 7_010_000, &acag, 5), None);
        e.formats.cut = false;
        assert_eq!(
            e.spot(&heard("JA1ZLO", &["1317M"], 700.0), 7_010_000, &acag, 5)
                .unwrap()
                .number,
            "1317M"
        );
    }

    #[test]
    fn formats_round_trip() {
        let f = Formats {
            enabled: true,
            callsign: "JA[0-9][A-Z]{3}".into(),
            portable: false,
            ja_only: true,
            number: NumberKind::Custom,
            number_pattern: "[0-9]{2}".into(),
            cut: false,
            table: CutTable::new([('O', '0'), ('X', '1')]),
            scope: true,
            pitch: 700,
            cw_reverse: true,
            keyer: Abbrev::default(),
        };
        let mut s = Section::new();
        f.save(&mut s);
        assert_eq!(Formats::load(&s), f);
        assert_eq!(Formats::load(&Section::new()), Formats::default());
    }
}
