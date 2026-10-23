//! The Discord tab of Options: a section per destination (Discord, Slack) with
//! whether to post, the webhook URL and a test button, then what is posted to
//! every destination and how a line looks. It edits the section of the status
//! item ([`crate::status`]), which takes the settings after OK. A test button posts
//! one message to the URL being typed.

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

/// The test of one destination.
#[derive(Default)]
struct Test {
    /// The post in flight.
    job: Option<Job<Outcome>>,
    /// How the last one went: the text, and whether it failed.
    result: Option<(String, bool)>,
}

pub struct DiscordOptions {
    /// By [`Site::index`].
    tests: [Test; 2],
    /// The hub, whose toggles (switched from the menu) the tab follows.
    hub: Arc<Hub>,
    /// The hub's toggles when the tab was last drawn.
    seen: Option<[bool; 2]>,
}

impl Default for DiscordOptions {
    fn default() -> Self {
        DiscordOptions::new(crate::shared())
    }
}

impl DiscordOptions {
    pub fn new(hub: Arc<Hub>) -> Self {
        DiscordOptions {
            tests: Default::default(),
            hub,
            seen: None,
        }
    }

    /// Put into `c` the toggles switched from the menu since the tab was last
    /// drawn (all of them the first time), as the section may not have them yet:
    /// the status item writes them only now and then.
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

    /// The test button and how the test went, on one row.
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
                self.start(site, url.to_string(), t.get(lang, "test.message").into());
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

/// The QSO the preview shows.
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

/// A heading of the tab.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong());
}

impl SettingsPage for DiscordOptions {
    fn id(&self) -> &str {
        "panel.dc"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, lang: &str) {
        let t = texts();
        let mut c = Config::load(section);
        self.follow_menu(&mut c);
        let width = zlog_ui::form_field_width(ui, &[t.get(lang, "url")]);
        for site in Site::ALL {
            let target = c.target_mut(site);
            heading(ui, site.name());
            ui.checkbox(
                &mut target.enabled,
                t.format(lang, "enabled", &[&site.name()]),
            );
            egui::Grid::new(("dc_options", site.code()))
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(t.get(lang, "url"));
                    let edit = egui::TextEdit::singleline(&mut target.url)
                        .password(true)
                        .hint_text(site.hint());
                    let key = format!("url.tip.{}", site.code());
                    zlog_ui::sized_field(ui, edit, width).on_hover_text(t.get(lang, &key));
                    ui.end_row();
                });
            self.tests[site.index()].ui(ui, site, &target.url, lang);
            ui.separator();
        }
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

    /// A toggle from the menu is not undone by OK in Options: the tab takes the
    /// hub's toggles first and when they change, the operator's otherwise.
    #[test]
    fn the_tab_follows_the_menu_toggles() {
        let hub = Arc::new(Hub::default());
        let mut page = DiscordOptions::new(hub.clone());
        let mut c = Config::default();
        hub.state
            .lock()
            .unwrap()
            .config
            .target_mut(Site::Discord)
            .enabled = true;
        // Opened: the section, saved before the toggle, says off.
        page.follow_menu(&mut c);
        assert!(c.target(Site::Discord).enabled);
        // The operator ticks Slack in the tab; the hub has not changed.
        c.target_mut(Site::Slack).enabled = true;
        page.follow_menu(&mut c);
        assert!(c.target(Site::Slack).enabled);
        // Discord switched off from the menu while the tab is open.
        hub.state
            .lock()
            .unwrap()
            .config
            .target_mut(Site::Discord)
            .enabled = false;
        page.follow_menu(&mut c);
        assert!(!c.target(Site::Discord).enabled && c.target(Site::Slack).enabled);
        // Switched on from the menu, then the tab is drawn again (reopened).
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
