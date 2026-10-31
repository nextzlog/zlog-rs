use egui::{RichText, Ui};
use zlog_core::{Band, Station};
use zlog_ui::{Command, EntryAction, MenuGroup, Panel, Section, View};

pub mod list;

use crate::list::{power_codes, Operator, Operators, POWER_BANDS};

zlog_core::texts!();

#[derive(Debug, Clone, PartialEq, Default)]
struct Form {
    op: Operator,
    existing: bool,
    no_callsign: bool,
}

pub struct Crew {
    list: Operators,
    picked: Option<usize>,
    form: Option<Form>,
    power_on_band_change: bool,
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
    fn choose(&mut self, operator: &str, band: Band, station: &Station) {
        self.actions.push(EntryAction::Operator(operator.into()));
        if let Some(p) = self.list.power_for(operator, band, station) {
            self.actions.push(EntryAction::Power(p));
        }
    }

    fn follow_band(&mut self, band: Band, operator: &str) {
        let before = self.band.replace(band);
        if before.is_none_or(|b| b == band) || !self.power_on_band_change {
            return;
        }
        if let Some(p) = self.list.get(operator).and_then(|o| o.power_for(band)) {
            self.actions.push(EntryAction::Power(p));
        }
    }

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
        zlog_ui::layout::toolbar(ui, &[], |ui| {
            use zlog_ui::{icon, layout};
            let on = picked.is_some();
            if ui
                .add_enabled_ui(on, |ui| {
                    layout::tool(ui, icon::USER_CHECK, t.get(lang, "choose"))
                })
                .inner
                .clicked()
            {
                if let Some(op) = &picked {
                    self.choose(&op.callsign.clone(), view.band, view.station);
                }
            }
            if layout::tool(ui, icon::ERASER, t.get(lang, "none")).clicked() {
                self.choose("", view.band, view.station);
            }
            ui.separator();
            if layout::tool(ui, icon::PLUS, t.get(lang, "add")).clicked() {
                self.form = Some(Form::default());
            }
            if ui
                .add_enabled_ui(on, |ui| {
                    layout::tool(ui, icon::PENCIL_SIMPLE, t.get(lang, "edit"))
                })
                .inner
                .clicked()
            {
                self.form = picked.clone().map(|op| Form {
                    op,
                    existing: true,
                    no_callsign: false,
                });
            }
            if ui
                .add_enabled_ui(on, |ui| {
                    layout::tool(ui, icon::TRASH, t.get(lang, "delete"))
                })
                .inner
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

    fn form_ui(ui: &mut Ui, form: &mut Form, lang: &str) {
        let t = texts();
        let labels = [t.get(lang, "callsign"), t.get(lang, "age")];
        zlog_ui::form(ui, "ops-form", &labels, |ui, fields| {
            ui.label(labels[0]);
            let edit =
                egui::TextEdit::singleline(&mut form.op.callsign).interactive(!form.existing);
            fields.text(ui, edit);
            ui.end_row();
            ui.label(labels[1]);
            fields.text(ui, egui::TextEdit::singleline(&mut form.op.age));
            ui.end_row();
        });
        form.op.callsign = form.op.callsign.to_ascii_uppercase();
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

fn shown_operator(operator: &str, lang: &str) -> String {
    if operator.is_empty() {
        texts().get(lang, "nobody").into()
    } else {
        operator.into()
    }
}

impl Panel for Crew {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

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
        vec![Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "menu.tip"),
        )
        .icon(zlog_ui::icon::USER_LIST)
        .role("operator")]
    }

    fn tick(&mut self, view: &View<'_>) {
        self.follow_band(view.band, view.operator);
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        match &mut self.form {
            Some(form) => Self::form_ui(ui, form, view.language),
            None => self.list_ui(ui, view),
        }
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

    fn capability_mut(&mut self, want: zlog_ui::WantMut) -> Option<zlog_ui::CapabilityMut<'_>> {
        match want {
            zlog_ui::WantMut::Buttons => Some(zlog_ui::CapabilityMut::Buttons(self)),
            _ => None,
        }
    }
}

impl zlog_ui::ButtonRow for Crew {
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
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(450, |_| Crew::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Power;

    #[test]
    fn the_window_command_answers_the_operator_role_of_alt_o() {
        let c = &Crew::default().commands("en")[0];
        assert!(c.is_window());
        assert_eq!(c.role, Some("operator"));
    }

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
        c.follow_band(Band::B144, "JA1ZLO");
        assert!(c.take_actions().is_empty());
        c.follow_band(Band::B7, "JA1YWX");
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
        c.form.as_mut().unwrap().op = Operator::new(" JA1ZLO ", "M", "30");
        assert!(c.submit());
        assert!(c.form.is_none());
        assert_eq!(c.list.get("JA1ZLO").unwrap().age, "30");
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
