use egui::{RichText, Ui};
use zlog_ui::SettingsPage;

use crate::i18n::tr;

pub const SECTION: &str = "wizard";

pub const USES: [&str; 5] = ["local", "remote", "share", "join", "try"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Usage,
    Page(usize),
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    TestRig,
    TestKeyer,
}

#[derive(Debug, Clone, Default)]
pub struct Wizard {
    pub uses: [bool; 5],
    pub at: usize,
    pub status: Option<(String, bool)>,
}

pub enum Move {
    Back,
    Next,
    Finish,
}

pub fn first_run(station: &zlog_ui::Section, wizard: &zlog_ui::Section) -> bool {
    let call = zlog_ui::read_station(station).callsign;
    call.trim().is_empty() && wizard.get("done").is_none_or(|v| v != "true")
}

fn page_index(pages: &[Box<dyn SettingsPage>], id: &str, nth: usize) -> Option<usize> {
    pages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.id() == id)
        .nth(nth)
        .map(|(i, _)| i)
}

impl Wizard {
    pub fn on(&self, name: &str) -> bool {
        USES.iter()
            .position(|u| *u == name)
            .is_some_and(|i| self.uses[i])
    }

    pub fn steps(&self, pages: &[Box<dyn SettingsPage>]) -> Vec<Step> {
        let mut want: Vec<(&str, usize)> = vec![("station", 0)];
        if self.on("local") || self.on("try") {
            want.extend([("rig", 0), ("panel.keyer", 0), ("sound", 0)]);
        }
        if self.on("remote") {
            want.push(("panel.remote", 0));
        }
        if self.on("share") {
            want.push(("panel.remote", 1));
        }
        if self.on("join") {
            want.push(("panel.link", 0));
        }
        let mut steps = vec![Step::Usage];
        steps.extend(
            want.into_iter()
                .filter_map(|(id, n)| page_index(pages, id, n))
                .map(Step::Page),
        );
        steps.push(Step::Done);
        steps
    }

    pub fn usage_ui(&mut self, ui: &mut Ui) {
        ui.label(tr("wizard.usage"));
        for (i, u) in USES.iter().enumerate() {
            ui.checkbox(&mut self.uses[i], tr(&format!("wizard.use.{u}")))
                .on_hover_text(tr(&format!("wizard.use.{u}.tip")));
        }
    }

    pub fn try_without_rig(&self, draft: &mut crate::settings::Settings) {
        if !self.on("try") {
            return;
        }
        let rig = draft.section("rig");
        if rig.get("port_chosen").is_none_or(|v| v != "true") {
            rig.insert("port".into(), "simulator".into());
            rig.insert("port_chosen".into(), "true".into());
        }
        let keyer = draft.section("panel.keyer");
        if keyer.get("device").is_none_or(|d| d == "none") {
            keyer.insert("device".into(), "simulated".into());
        }
    }

    pub fn extras_ui(&mut self, ui: &mut Ui, id: &str) -> Option<Ask> {
        let mut ask = None;
        if let Some((text, ok)) = &self.status {
            let color = match ok {
                true => ui.visuals().text_color(),
                false => ui.visuals().weak_text_color(),
            };
            ui.label(RichText::new(text).color(color));
        }
        match id {
            "rig" => {
                if ui
                    .button(tr("wizard.test_rig"))
                    .on_hover_text(tr("wizard.test_rig.tip"))
                    .clicked()
                {
                    self.status = Some((tr("wizard.testing").into(), false));
                    ask = Some(Ask::TestRig);
                }
            }
            "panel.keyer" => {
                ui.colored_label(ui.visuals().warn_fg_color, tr("wizard.test_keyer.warn"));
                if ui
                    .button(tr("wizard.test_keyer"))
                    .on_hover_text(tr("wizard.test_keyer.tip"))
                    .clicked()
                {
                    ask = Some(Ask::TestKeyer);
                }
            }
            _ => {}
        }
        ask
    }

    pub fn done_ui(ui: &mut Ui) {
        ui.heading(tr("wizard.done"));
        for k in ["rig", "keyer", "sound", "remote", "link", "again"] {
            ui.label(tr(&format!("wizard.where.{k}")));
        }
    }

    pub fn buttons(ui: &mut Ui, at: usize, last: bool, can_next: bool, skip: bool) -> Option<Move> {
        let mut m = None;
        zlog_ui::form::buttons(ui, |ui| {
            if at > 0 && ui.button(tr("wizard.back")).clicked() {
                m = Some(Move::Back);
            }
            if skip && ui.button(tr("wizard.skip")).clicked() {
                m = Some(Move::Next);
            }
            let (key, mv) = match last {
                true => ("wizard.finish", Move::Finish),
                false => ("wizard.next", Move::Next),
            };
            if ui
                .add_enabled(can_next, egui::Button::new(tr(key)))
                .clicked()
            {
                m = Some(mv);
            }
        });
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct P(&'static str, &'static str);

    impl SettingsPage for P {
        fn id(&self) -> &str {
            self.0
        }
        fn title(&self, _l: &str) -> String {
            self.1.into()
        }
        fn ui(&mut self, _ui: &mut Ui, _s: &mut zlog_ui::Section, _l: &str) {}
    }

    fn pages() -> Vec<Box<dyn SettingsPage>> {
        [
            ("station", "Station"),
            ("rig", "Rig 1"),
            ("panel.keyer", "CW keyer"),
            ("sound", "Sound"),
            ("panel.remote", "Connect"),
            ("panel.remote", "Allow"),
            ("panel.link", "Z-Link"),
        ]
        .into_iter()
        .map(|(i, t)| Box::new(P(i, t)) as Box<dyn SettingsPage>)
        .collect()
    }

    #[test]
    fn the_steps_follow_the_usage_choice() {
        let p = pages();
        let mut w = Wizard::default();
        assert_eq!(w.steps(&p), [Step::Usage, Step::Page(0), Step::Done]);
        w.uses = [true, false, true, true, false];
        assert_eq!(
            w.steps(&p),
            [
                Step::Usage,
                Step::Page(0),
                Step::Page(1),
                Step::Page(2),
                Step::Page(3),
                Step::Page(5),
                Step::Page(6),
                Step::Done
            ]
        );
    }

    #[test]
    fn trying_without_a_rig_picks_the_simulators() {
        let mut d = crate::settings::Settings::default();
        let w = Wizard {
            uses: [false, false, false, false, true],
            ..Wizard::default()
        };
        w.try_without_rig(&mut d);
        assert_eq!(d.section("rig")["port"], "simulator");
        assert_eq!(d.section("panel.keyer")["device"], "simulated");
    }

    #[test]
    fn the_wizard_runs_first_until_done_or_a_callsign_is_set() {
        let mut station = zlog_ui::Section::new();
        let mut done = zlog_ui::Section::new();
        assert!(first_run(&station, &done));
        done.insert("done".into(), "true".into());
        assert!(!first_run(&station, &done));
        station.insert("callsign".into(), "JA1ZLO".into());
        assert!(!first_run(&station, &zlog_ui::Section::new()));
    }
}
