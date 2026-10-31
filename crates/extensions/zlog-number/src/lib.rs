use egui::Ui;
use zlog_core::{Band, Qso, QsoId, Station};
use zlog_ui::{Command, EntryAction, MenuGroup, Panel, QsoMenuItem, View};

zlog_core::texts!();

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Correction {
    pub low: String,
    pub high: String,
    pub power_code: bool,
    pub band: Option<Band>,
}

impl Correction {
    pub fn number(&self, band: Band, station: &Station) -> String {
        let base = if band < Band::B2400 {
            &self.low
        } else {
            &self.high
        };
        let code = self
            .power_code
            .then(|| station.power_for(band).code())
            .flatten();
        match code {
            Some(c) => format!("{base}{c}"),
            None => base.clone(),
        }
    }

    pub fn apply(&self, qsos: &[Qso], station: &Station) -> Vec<(QsoId, Qso)> {
        qsos.iter()
            .filter(|q| self.band.is_none_or(|b| q.band == b))
            .filter_map(|q| {
                let nr = self.number(q.band, station);
                (nr != q.nr_sent).then(|| {
                    let mut new = q.clone();
                    new.nr_sent = nr;
                    (QsoId::of(q), new)
                })
            })
            .collect()
    }
}

#[derive(Default)]
pub struct CorrectNr {
    form: Correction,
    fill: Option<Qso>,
    changes: Vec<(QsoId, Qso)>,
    actions: Vec<EntryAction>,
}

impl CorrectNr {
    fn fill_from(&mut self, q: &Qso) {
        self.form.low = q.nr_sent.clone();
        self.form.high = q.nr_sent.clone();
        self.form.band = self.form.band.map(|_| q.band);
    }
}

impl Panel for CorrectNr {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

    fn id(&self) -> &str {
        "correct_nr"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [520.0, 360.0]
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Edit
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Bulk, 0)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        vec![Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "menu.tip"),
        )
        .menu_label(texts().get(language, "menu"))
        .icon(zlog_ui::icon::HASH)]
    }

    fn qso_menu(&self, language: &str, _qso: &Qso) -> Vec<QsoMenuItem> {
        vec![QsoMenuItem::new("correct", texts().get(language, "menu")).dialog()]
    }

    fn on_qso_menu(&mut self, _id: &str, qso: &Qso) -> bool {
        self.fill = Some(qso.clone());
        true
    }

    fn opened(&mut self, view: &View<'_>) {
        if self.fill.is_none() {
            if let Some(q) = view.log.qsos.first() {
                self.fill_from(&q.clone());
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = texts();
        let lang = view.language;
        let qsos = &view.log.qsos;
        if let Some(q) = self.fill.take() {
            self.fill_from(&q);
        }
        let labels = [t.get(lang, "low"), t.get(lang, "high"), t.get(lang, "band")];
        zlog_ui::form(ui, "correct_nr", &labels, |ui, form| {
            ui.label(labels[0]);
            form.text(ui, egui::TextEdit::singleline(&mut self.form.low));
            ui.end_row();
            ui.label(labels[1]);
            form.text(ui, egui::TextEdit::singleline(&mut self.form.high));
            ui.end_row();
            ui.label(labels[2]);
            let shown = self
                .form
                .band
                .map_or_else(|| t.get(lang, "all").to_string(), |b| b.label().to_string());
            form.combo("correct_nr_band")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.form.band, None, t.get(lang, "all"));
                    let (lo, hi) = view.contest.band_range();
                    for b in Band::range(lo, hi).filter(|b| qsos.iter().any(|q| q.band == *b)) {
                        ui.selectable_value(&mut self.form.band, Some(b), b.label());
                    }
                });
            ui.end_row();
        });
        ui.checkbox(&mut self.form.power_code, t.get(lang, "power_code"));
        ui.add_space(6.0);
        self.changes = self.form.apply(qsos, view.station);
        if let Some((_, q)) = self.changes.first() {
            ui.weak(t.format(lang, "example", &[&q.callsign, &q.nr_sent]));
        }
        ui.label(t.format(lang, "count", &[&self.changes.len()]));
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn capability_mut(&mut self, want: zlog_ui::WantMut) -> Option<zlog_ui::CapabilityMut<'_>> {
        match want {
            zlog_ui::WantMut::Buttons => Some(zlog_ui::CapabilityMut::Buttons(self)),
            _ => None,
        }
    }
}

impl zlog_ui::ButtonRow for CorrectNr {
    fn buttons(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let t = texts();
        let changes = self.form.apply(&view.log.qsos, view.station);
        zlog_ui::form::buttons(ui, |ui| {
            let apply = ui
                .add_enabled(
                    !changes.is_empty(),
                    egui::Button::new(t.get(view.language, "apply")),
                )
                .clicked();
            if apply {
                self.actions.push(EntryAction::update_qsos(changes));
            }
            let cancel = ui.button(t.get(view.language, "cancel")).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape));
            apply || cancel
        })
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(500, |_| CorrectNr::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Power;

    fn qso(call: &str, band: Band, nr: &str) -> Qso {
        Qso {
            callsign: call.into(),
            band,
            nr_sent: nr.into(),
            reserve3: 100 * (call.len() as i32 + band as i32 + 1),
            ..Qso::default()
        }
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn bands_from_2400_take_the_second_number_and_power_codes_follow() {
        let mut st = Station::default();
        st.power.insert(Band::B430, Power::P005);
        st.power.insert(Band::B2400, Power::P010);
        let c = Correction {
            low: "10".into(),
            high: "1010".into(),
            power_code: true,
            band: None,
        };
        assert_eq!(c.number(Band::B430, &st), "10M");
        assert_eq!(c.number(Band::B2400, &st), "1010H");
        let plain = Correction {
            power_code: false,
            ..c
        };
        assert_eq!(plain.number(Band::B1200, &st), "10");
    }

    #[test]
    fn only_qsos_that_change_and_of_the_band_chosen() {
        let st = Station::default();
        let log = [
            qso("JA1A", Band::B7, "10H"),
            qso("JA1B", Band::B7, "11H"),
            qso("JA1C", Band::B14, "11H"),
        ];
        let c = Correction {
            low: "10H".into(),
            high: "10H".into(),
            power_code: false,
            band: Some(Band::B7),
        };
        let changed: Vec<String> = c
            .apply(&log, &st)
            .into_iter()
            .map(|(_, q)| q.callsign)
            .collect();
        assert_eq!(changed, ["JA1B"]);
        let all = Correction { band: None, ..c };
        assert_eq!(all.apply(&log, &st).len(), 2);
    }
}
