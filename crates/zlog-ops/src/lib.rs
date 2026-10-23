//! The operators of a multi-op station (zLog's operator list: the Operators tab of
//! Options, `TformOperatorEdit`, `TOperatorPowerDialog`, and the operator menu of
//! the entry row, `TMainForm.SelectOperator`).
//!
//! - [`list`]: the operators, their power per band, and how they are saved.
//! - [`Crew`]: the window listing them, where one is chosen for the entry row and
//!   others are added, edited or deleted.
//!
//! Choosing an operator sets the entry row's operator, which every QSO logged
//! after carries and Z-Link sends (`SendOperator`), and its power for the band
//! (`SetOpPower`); with the option on, the power follows the operator's as the band
//! changes (`ApplyPowerCodeOnBandChange`).
//!
//! Not supported: CW messages and voice files per operator, the age per operator in
//! the `$A` macro (`GetAge`), choosing the last QSO's operator when a log is opened
//! (`SelectLastOperator`), the warning while none is chosen (zLog's multi-op
//! categories are not kept here), reading `zlog_oplist.ini` or `ZLOG.OP`, and
//! asking whether the higher bands take a power just chosen.

use egui::{RichText, Ui};
use zlog_core::{Band, Station};
use zlog_ui::{Command, EntryAction, MenuGroup, Panel, Section, View};

pub mod list;

use crate::list::{power_codes, Operator, Operators, POWER_BANDS};

zlog_core::texts!();

/// The operator being added or edited (`TformOperatorEdit`).
#[derive(Debug, Clone, PartialEq, Default)]
struct Form {
    op: Operator,
    /// Editing a listed operator: the callsign cannot change (`SetObject`).
    existing: bool,
    /// OK was pressed without a callsign (`PLEASE_ENTER_OPS_CALLSIGN`).
    no_callsign: bool,
}

pub struct Crew {
    list: Operators,
    /// The row picked in the list.
    picked: Option<usize>,
    form: Option<Form>,
    /// zLog `ApplyPowerCodeOnBandChange`, on by default.
    power_on_band_change: bool,
    /// The entry row's band in the frame before.
    band: Option<Band>,
    actions: Vec<EntryAction>,
}

impl Default for Crew {
    fn default() -> Self {
        Crew {
            list: Operators::default(),
            picked: None,
            form: None,
            power_on_band_change: true,
            band: None,
            actions: Vec::new(),
        }
    }
}

impl Crew {
    /// `SelectOperator`: the operator (empty for `Clear`), then the power for the
    /// band.
    fn choose(&mut self, operator: &str, band: Band, station: &Station) {
        self.actions.push(EntryAction::Operator(operator.into()));
        if let Some(p) = self.list.power_for(operator, band, station) {
            self.actions.push(EntryAction::Power(p));
        }
    }

    /// See [`Panel::tick`].
    fn follow_band(&mut self, band: Band, operator: &str) {
        let before = self.band.replace(band);
        if before.is_none_or(|b| b == band) || !self.power_on_band_change {
            return;
        }
        if let Some(p) = self.list.get(operator).and_then(|o| o.power_for(band)) {
            self.actions.push(EntryAction::Power(p));
        }
    }

    /// `buttonOKClick` then `GetObject`: whether the form may close.
    fn submit(&mut self) -> bool {
        let Some(form) = &mut self.form else {
            return true;
        };
        form.op.callsign = form.op.callsign.trim().to_string();
        if form.op.callsign.is_empty() {
            form.no_callsign = true;
            return false;
        }
        self.list.put(form.op.clone());
        self.form = None;
        true
    }

    fn list_ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = texts();
        let lang = view.language;
        let picked = self.picked.and_then(|i| self.list.0.get(i)).cloned();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(picked.is_some(), egui::Button::new(t.get(lang, "choose")))
                .clicked()
            {
                if let Some(op) = &picked {
                    self.choose(&op.callsign.clone(), view.band, view.station);
                }
            }
            if ui.button(t.get(lang, "none")).clicked() {
                self.choose("", view.band, view.station);
            }
            ui.separator();
            if ui.button(t.get(lang, "add")).clicked() {
                self.form = Some(Form::default());
            }
            if ui
                .add_enabled(picked.is_some(), egui::Button::new(t.get(lang, "edit")))
                .clicked()
            {
                self.form = picked.clone().map(|op| Form {
                    op,
                    existing: true,
                    no_callsign: false,
                });
            }
            if ui
                .add_enabled(picked.is_some(), egui::Button::new(t.get(lang, "delete")))
                .clicked()
            {
                if let Some(i) = self.picked.take() {
                    self.list.0.remove(i);
                }
            }
        });
        ui.checkbox(
            &mut self.power_on_band_change,
            t.get(lang, "power_on_band_change"),
        );
        ui.label(t.format(lang, "current", &[&shown_operator(view.operator, lang)]));
        ui.separator();
        if self.list.0.is_empty() {
            ui.weak(t.get(lang, "empty"));
            return;
        }
        let mut choose = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Grid::new("ops-list")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong(t.get(lang, "callsign"));
                        ui.strong(t.get(lang, "power"));
                        ui.strong(t.get(lang, "qsos"));
                        ui.end_row();
                        for (i, op) in self.list.0.iter().enumerate() {
                            let mut text = RichText::new(&op.callsign).monospace();
                            if op.callsign == view.operator {
                                text = text.strong();
                            }
                            let row = ui.selectable_label(self.picked == Some(i), text);
                            if row.clicked() {
                                self.picked = Some(i);
                            }
                            if row.double_clicked() {
                                choose = Some(op.callsign.clone());
                            }
                            ui.label(RichText::new(op.power()).monospace());
                            // The console's `OP` command (`Log.OpQSO`).
                            let n = view
                                .log
                                .qsos
                                .iter()
                                .filter(|q| q.operator == op.callsign)
                                .count();
                            ui.label(n.to_string());
                            ui.end_row();
                        }
                    });
            });
        if let Some(op) = choose {
            self.choose(&op, view.band, view.station);
        }
    }

    /// `TformOperatorEdit` with the codes of `TOperatorPowerDialog`, one per band.
    fn form_ui(ui: &mut Ui, form: &mut Form, lang: &str) {
        let t = texts();
        let labels = [t.get(lang, "callsign"), t.get(lang, "age")];
        let width = zlog_ui::form_field_width(ui, &labels);
        egui::Grid::new("ops-form").num_columns(2).show(ui, |ui| {
            ui.label(labels[0]);
            let edit =
                egui::TextEdit::singleline(&mut form.op.callsign).interactive(!form.existing);
            zlog_ui::sized_field(ui, edit, width);
            ui.end_row();
            ui.label(labels[1]);
            zlog_ui::sized_field(ui, egui::TextEdit::singleline(&mut form.op.age), width);
            ui.end_row();
        });
        form.op.callsign = form.op.callsign.to_uppercase();
        if form.no_callsign {
            ui.colored_label(ui.visuals().error_fg_color, t.get(lang, "no_callsign"));
        }
        ui.add_space(6.0);
        ui.label(RichText::new(t.get(lang, "power")).strong());
        ui.weak(t.get(lang, "power_hint"));
        let mut codes: Vec<char> = form.op.power().chars().collect();
        egui::Grid::new("ops-power").num_columns(8).show(ui, |ui| {
            for (i, band) in POWER_BANDS.iter().enumerate() {
                ui.label(band.label());
                egui::ComboBox::from_id_salt(("ops-power", i))
                    .selected_text(codes[i].to_string())
                    .width(40.0)
                    .show_ui(ui, |ui| {
                        for c in power_codes(i) {
                            ui.selectable_value(&mut codes[i], *c, c.to_string());
                        }
                    });
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
        form.op.set_power(&codes.into_iter().collect::<String>());
    }
}

/// The operator as shown: `(none)` when empty.
fn shown_operator(operator: &str, lang: &str) -> String {
    if operator.is_empty() {
        texts().get(lang, "nobody").into()
    } else {
        operator.into()
    }
}

impl Panel for Crew {
    fn id(&self) -> &str {
        "ops"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [460.0, 380.0]
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Settings
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Station, 0)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        // A window of the Settings menu, without a toolbar button.
        let mut c = Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "menu.tip"),
        );
        c.toolbar = false;
        vec![c]
    }

    /// The power follows the operator's when the band changes, as `UpdateBand` does
    /// in zLog's multi-op categories with `ApplyPowerCodeOnBandChange`; only for a
    /// listed operator, since the entry row already takes the station's power.
    fn tick(&mut self, view: &View<'_>) {
        self.follow_band(view.band, view.operator);
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        match &mut self.form {
            Some(form) => Self::form_ui(ui, form, view.language),
            None => self.list_ui(ui, view),
        }
    }

    /// The form has OK and Cancel; the list is a tool window with its own buttons.
    fn has_buttons(&self) -> bool {
        self.form.is_some()
    }

    /// Returns false: closing the form goes back to the list, not out of the window.
    fn buttons(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        match zlog_ui::dialog::ok_cancel(ui, view.language, true, false) {
            Some(true) => {
                self.submit();
            }
            Some(false) => self.form = None,
            None if escape => self.form = None,
            None => {}
        }
        false
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &Section) {
        self.list = Operators::load(section);
        self.power_on_band_change = section
            .get("power_on_band_change")
            .is_none_or(|v| v != "false");
    }

    fn save(&self, section: &mut Section) {
        self.list.save(section);
        section.insert(
            "power_on_band_change".into(),
            self.power_on_band_change.to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Power;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    fn crew() -> Crew {
        let mut c = Crew::default();
        c.list.put(Operator::new("JA1ZLO", "PPPPPPPLL", ""));
        c
    }

    #[test]
    fn choosing_sets_operator_and_power() {
        let mut c = crew();
        let st = Station::default();
        c.choose("JA1ZLO", Band::B7, &st);
        assert_eq!(
            c.take_actions(),
            [
                EntryAction::Operator("JA1ZLO".into()),
                EntryAction::Power(Power::P001)
            ]
        );
        // None chosen: the station's power for the band.
        c.choose("", Band::B7, &st);
        assert_eq!(
            c.take_actions(),
            [
                EntryAction::Operator("".into()),
                EntryAction::Power(Station::default_power(Band::B7))
            ]
        );
    }

    #[test]
    fn power_follows_the_band_when_asked() {
        let mut c = crew();
        c.follow_band(Band::B7, "JA1ZLO");
        assert!(c.take_actions().is_empty());
        c.follow_band(Band::B144, "JA1ZLO");
        assert_eq!(c.take_actions(), [EntryAction::Power(Power::P002)]);
        // Same band: nothing.
        c.follow_band(Band::B144, "JA1ZLO");
        assert!(c.take_actions().is_empty());
        // An unlisted operator, or the option off: nothing.
        c.follow_band(Band::B7, "JR8PPG");
        assert!(c.take_actions().is_empty());
        c.power_on_band_change = false;
        c.follow_band(Band::B144, "JA1ZLO");
        assert!(c.take_actions().is_empty());
    }

    #[test]
    fn form_needs_a_callsign() {
        let mut c = Crew {
            form: Some(Form::default()),
            ..Crew::default()
        };
        assert!(!c.submit());
        assert!(c.form.as_ref().unwrap().no_callsign);
        c.form.as_mut().unwrap().op = Operator::new(" JR8PPG ", "M", "30");
        assert!(c.submit());
        assert!(c.form.is_none());
        assert_eq!(c.list.get("JR8PPG").unwrap().age, "30");
    }

    #[test]
    fn settings_round_trip() {
        let mut c = crew();
        c.power_on_band_change = false;
        let mut section = Section::new();
        c.save(&mut section);
        let mut d = Crew::default();
        d.load(&section);
        assert_eq!(d.list, c.list);
        assert!(!d.power_on_band_change);
    }
}
