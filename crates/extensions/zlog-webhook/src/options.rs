use std::sync::Arc;

use zlog_core::{Band, Mode, Qso};
use zlog_net::Job;
use zlog_ui::{Section, SettingsPage};

use crate::config::Config;
use crate::format::{line, Change};
use crate::outbox::Outcome;
use crate::site::Site;
use crate::texts;
use crate::webhook::{Sender, Webhook};
use crate::Hub;

#[derive(Default)]
struct Test {
    job: Option<Job<Outcome>>,
    result: Option<(String, bool)>,
}

pub struct AutoPostOptions {
    tests: [Test; 2],
    hub: Arc<Hub>,
    seen: Option<[bool; 2]>,
    selected: usize,
}

impl Default for AutoPostOptions {
    fn default() -> Self {
        AutoPostOptions::new(crate::shared())
    }
}

impl AutoPostOptions {
    pub fn new(hub: Arc<Hub>) -> Self {
        AutoPostOptions {
            tests: Default::default(),
            hub,
            seen: None,
            selected: 0,
        }
    }

    fn follow_menu(&mut self, c: &mut Config) {
        let live = {
            let s = self.hub.state.lock().unwrap();
            Site::ALL.map(|site| s.config.target(site).enabled)
        };
        for site in Site::ALL {
            let i = site.index();
            if self.seen.is_none_or(|seen| seen[i] != live[i]) {
                c.target_mut(site).enabled = live[i];
            }
        }
        self.seen = Some(live);
    }
}

impl Test {
    fn start(&mut self, site: Site, url: String, content: String) {
        self.job = Some(Job::spawn(move || Ok(Webhook.send(site, &url, &content))));
        self.result = None;
    }

    fn poll(&mut self, site: Site, lang: &str) {
        let Some(answer) = self.job.as_mut().and_then(|j| j.take()) else {
            return;
        };
        let t = texts();
        self.job = None;
        self.result = Some(match answer.unwrap_or_else(Outcome::Failed) {
            Outcome::Sent => (t.get(lang, "test.sent").into(), false),
            Outcome::Wait(d) => (
                t.format(lang, "test.wait", &[&site.name(), &d.as_secs_f32()]),
                true,
            ),
            Outcome::Failed(e) | Outcome::Refused(e) => {
                (t.format(lang, "test.failed", &[&e]), true)
            }
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, site: Site, url: &str, lang: &str) {
        let t = texts();
        self.poll(site, lang);
        let testing = self.job.is_some();
        ui.horizontal_wrapped(|ui| {
            let can_test = !testing && !url.trim().is_empty();
            if ui
                .add_enabled(can_test, egui::Button::new(t.get(lang, "test")))
                .on_hover_text(t.get(lang, "test.tip"))
                .clicked()
            {
                self.start(
                    site,
                    url.to_string(),
                    t.format(lang, "test.message", &[&zlog_ui::product::NAME]),
                );
            }
            if testing {
                ui.label(t.get(lang, "test.sending"));
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
            } else if let Some((m, failed)) = &self.result {
                let text = egui::RichText::new(m.as_str());
                let text = if *failed {
                    text.color(ui.visuals().warn_fg_color)
                } else {
                    text
                };
                ui.add(egui::Label::new(text).wrap());
            }
        });
    }
}

fn sample() -> Qso {
    Qso {
        callsign: "JA1ZLO".into(),
        band: Band::B7,
        mode: Mode::Cw,
        rst_rcvd: 599,
        nr_rcvd: "13M".into(),
        ..Qso::default()
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong());
}

impl SettingsPage for AutoPostOptions {
    fn id(&self) -> &str {
        "panel.dc"
    }

    fn section_kind(&self) -> zlog_ui::SectionKind {
        zlog_ui::SectionKind::Secret
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, lang: &str) {
        let t = texts();
        let mut c = Config::load(section);
        self.follow_menu(&mut c);
        let height = ui.spacing().interact_size.y;
        for site in Site::ALL {
            let target = c.target(site);
            let state = match (target.url.trim().is_empty(), target.enabled) {
                (true, _) => t.get(lang, "state.no_url"),
                (false, true) => t.get(lang, "state.on"),
                (false, false) => t.get(lang, "state.off"),
            };
            let row = egui::Button::selectable(self.selected == site.index(), site.name())
                .right_text(egui::RichText::new(state).weak())
                .min_size(egui::vec2(ui.available_width(), height));
            if ui.add(row).clicked() {
                self.selected = site.index();
            }
        }
        ui.add_space(6.0);
        let site = Site::ALL[self.selected.min(Site::ALL.len() - 1)];
        let target = c.target_mut(site);
        heading(ui, site.name());
        ui.checkbox(
            &mut target.enabled,
            t.format(lang, "enabled", &[&site.name()]),
        );
        zlog_ui::form(
            ui,
            ("dc_options", site.code()),
            &[t.get(lang, "url")],
            |ui, form| {
                ui.label(t.get(lang, "url"));
                let edit = egui::TextEdit::singleline(&mut target.url)
                    .password(true)
                    .hint_text(site.hint());
                let key = format!("url.tip.{}", site.code());
                form.text(ui, edit).on_hover_text(t.get(lang, &key));
                ui.end_row();
            },
        );
        self.tests[site.index()].ui(ui, site, &target.url, lang);
        ui.separator();
        heading(ui, t.get(lang, "shared"));
        ui.checkbox(&mut c.edits, t.get(lang, "edits"));
        ui.add_space(6.0);
        ui.label(t.get(lang, "preview"));
        let q = sample();
        let after = Qso {
            nr_rcvd: "14M".into(),
            ..sample()
        };
        let mut changes = vec![Change::Logged(&q)];
        if c.edits {
            changes.push(Change::Corrected {
                before: &q,
                after: &after,
            });
        }
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for change in &changes {
                let text = line(change, "JA1YAA", "ALLJA1", lang);
                ui.add(egui::Label::new(egui::RichText::new(text).monospace()).wrap());
            }
        });
        ui.add_space(6.0);
        ui.add(egui::Label::new(egui::RichText::new(t.get(lang, "note")).weak()).wrap());
        c.save(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_follows_the_menu_toggles() {
        let hub = Arc::new(Hub::default());
        let mut page = AutoPostOptions::new(hub.clone());
        let mut c = Config::default();
        hub.state
            .lock()
            .unwrap()
            .config
            .target_mut(Site::Discord)
            .enabled = true;
        page.follow_menu(&mut c);
        assert!(c.target(Site::Discord).enabled);
        c.target_mut(Site::Slack).enabled = true;
        page.follow_menu(&mut c);
        assert!(c.target(Site::Slack).enabled);
        hub.state
            .lock()
            .unwrap()
            .config
            .target_mut(Site::Discord)
            .enabled = false;
        page.follow_menu(&mut c);
        assert!(!c.target(Site::Discord).enabled && c.target(Site::Slack).enabled);
        let mut stale = Config::default();
        hub.state
            .lock()
            .unwrap()
            .config
            .target_mut(Site::Discord)
            .enabled = true;
        page.follow_menu(&mut stale);
        assert!(stale.target(Site::Discord).enabled && !stale.target(Site::Slack).enabled);
    }
}
