//! The status bar item: per destination, posts waiting, and why the last one
//! failed or what was dropped, so nothing is lost unnoticed. It has no window,
//! only the submenu "Auto-post QSOs" of the Network menu, ticking each
//! destination; its settings section is the one the Discord tab edits, so it
//! loads the settings into the hub (and saves the toggles), and it tells the hub
//! the station, contest and language every frame.

use std::sync::Arc;

use zlog_ui::{Command, Panel, Section, StatusItem, View};

use crate::config::Config;
use crate::site::Site;
use crate::{texts, Hub, State};

/// The submenu of the destinations' toggles; each toggle's id is
/// [`Site::code`].
const POST: &str = "post";

pub struct DiscordStatus {
    hub: Arc<Hub>,
}

impl Default for DiscordStatus {
    fn default() -> Self {
        DiscordStatus::new(crate::shared())
    }
}

impl DiscordStatus {
    pub fn new(hub: Arc<Hub>) -> Self {
        DiscordStatus { hub }
    }
}

/// What `site` has to say in `language`: the text, the error and whether it is a
/// warning; nothing while all is well and idle.
fn part(state: &State, site: Site, language: &str) -> Option<(String, Option<String>, bool)> {
    let t = texts();
    let o = state.outbox(site);
    let waiting = o.len();
    let name = site.name();
    if !state.config.ready(site) {
        return (waiting > 0).then(|| {
            (
                t.format(language, "status.off", &[&name, &waiting]),
                None,
                false,
            )
        });
    }
    if let Some(e) = &o.error {
        let tip = t.format(language, "status.failed.tip", &[&name, e]);
        Some((
            t.format(language, "status.failed", &[&name, &waiting]),
            Some(tip),
            true,
        ))
    } else if o.dropped > 0 {
        Some((
            t.format(language, "status.dropped", &[&name, &o.dropped]),
            None,
            true,
        ))
    } else if waiting == 0 {
        None
    } else {
        Some((
            t.format(language, "status.waiting", &[&name, &waiting]),
            None,
            false,
        ))
    }
}

/// The item for `state` in `language`, the destinations side by side; nothing
/// while all is well and idle.
pub fn item(state: &State, language: &str) -> Option<StatusItem> {
    let parts: Vec<_> = Site::ALL
        .into_iter()
        .filter_map(|site| part(state, site, language))
        .collect();
    if parts.is_empty() {
        return None;
    }
    let text: Vec<&str> = parts.iter().map(|p| p.0.as_str()).collect();
    let tips: Vec<&str> = parts.iter().filter_map(|p| p.1.as_deref()).collect();
    let mut item = StatusItem::new(text.join("  ")).warning(parts.iter().any(|p| p.2));
    if !tips.is_empty() {
        item = item.tooltip(tips.join("\n"));
    }
    Some(item)
}

impl Panel for DiscordStatus {
    fn id(&self) -> &str {
        "dc"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn has_window(&self) -> bool {
        false
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Network
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Post, 0)
    }

    /// Posting to each destination on and off, without opening Options.
    fn commands(&self, language: &str) -> Vec<Command> {
        let s = self.hub.state.lock().unwrap();
        let t = texts();
        let toggles = Site::ALL
            .into_iter()
            .map(|site| {
                let tip = t.format(language, "menu.post.tip", &[&site.name()]);
                let mut c = Command::new(site.code(), site.name(), tip)
                    .selected(s.config.target(site).enabled);
                c.checkable = true;
                c
            })
            .collect();
        let post = Command::new(POST, t.get(language, "menu.post"), t.get(language, "tip"))
            .children(toggles);
        vec![post]
    }

    fn run_command(&mut self, id: &str) {
        let Some(site) = Site::ALL.into_iter().find(|s| s.code() == id) else {
            return;
        };
        let mut s = self.hub.state.lock().unwrap();
        let t = s.config.target_mut(site);
        t.enabled = !t.enabled;
        s.outbox_mut(site).resume();
        self.hub.wake.notify_all();
    }

    fn status(&self, view: &View<'_>) -> Vec<StatusItem> {
        item(&self.hub.state.lock().unwrap(), view.language)
            .into_iter()
            .collect()
    }

    fn tick(&mut self, view: &View<'_>) {
        let mut s = self.hub.state.lock().unwrap();
        if s.station != view.station.callsign {
            s.station = view.station.callsign.clone();
        }
        if s.contest != view.contest.name() {
            s.contest = view.contest.name().to_string();
        }
        if s.language != view.language {
            s.language = view.language.to_string();
        }
    }

    /// The settings of the Discord tab, at start-up and after OK in Options.
    fn load(&mut self, section: &Section) {
        let mut s = self.hub.state.lock().unwrap();
        s.config = Config::load(section);
        for o in &mut s.outboxes {
            o.resume();
        }
        drop(s);
        self.hub.wake.notify_all();
    }

    fn save(&self, section: &mut Section) {
        self.hub.state.lock().unwrap().config.save(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outbox::Outcome;

    fn ready(s: &mut State, site: Site) {
        let t = s.config.target_mut(site);
        t.enabled = true;
        t.url = "https://example.invalid/hook".into();
    }

    #[test]
    fn says_what_waits_and_what_went_wrong() {
        let mut s = State::default();
        assert!(item(&s, "en").is_none());
        s.outbox_mut(Site::Discord).push("a".into());
        assert_eq!(item(&s, "ja").unwrap().text, "Discord: 送信オフ (1件)");
        ready(&mut s, Site::Discord);
        assert_eq!(item(&s, "en").unwrap().text, "Discord: 1 waiting");
        let now = std::time::Instant::now();
        let o = s.outbox_mut(Site::Discord);
        let b = o.take(now).unwrap();
        o.done(b, Outcome::Failed("offline".into()), now);
        let i = item(&s, "en").unwrap();
        assert!(i.warning);
        assert_eq!(i.tooltip.as_deref(), Some("Discord: last error: offline"));
        // Switched off, the error is no longer shown, and switching on resumes.
        s.config.target_mut(Site::Discord).enabled = false;
        assert!(!item(&s, "en").unwrap().warning);
    }

    #[test]
    fn both_destinations_are_shown_side_by_side() {
        let mut s = State::default();
        ready(&mut s, Site::Discord);
        ready(&mut s, Site::Slack);
        s.outbox_mut(Site::Discord).push("a".into());
        s.outbox_mut(Site::Slack).push("a".into());
        s.outbox_mut(Site::Slack).push("b".into());
        let now = std::time::Instant::now();
        let o = s.outbox_mut(Site::Slack);
        let b = o.take(now).unwrap();
        o.done(b, Outcome::Refused("HTTP 404 no_service".into()), now);
        let i = item(&s, "en").unwrap();
        assert_eq!(i.text, "Discord: 1 waiting  Slack: not sent (2 waiting)");
        assert!(i.warning);
        assert_eq!(
            i.tooltip.as_deref(),
            Some("Slack: last error: HTTP 404 no_service")
        );
    }

    #[test]
    fn loading_the_section_sets_the_hub() {
        let hub = Arc::new(Hub::default());
        let mut p = DiscordStatus::new(hub.clone());
        let mut section = Section::new();
        let mut c = Config {
            edits: true,
            ..Config::default()
        };
        c.target_mut(Site::Discord).url = "u".into();
        c.target_mut(Site::Discord).enabled = true;
        c.save(&mut section);
        p.load(&section);
        assert!(hub.state.lock().unwrap().config.edits);
        let menu = p.commands("ja");
        assert_eq!(menu.len(), 1);
        assert_eq!(menu[0].label, "QSO を自動投稿");
        let ticks: Vec<(&str, bool, bool)> = menu[0]
            .children
            .iter()
            .map(|c| (c.label.as_str(), c.checkable, c.selected))
            .collect();
        assert_eq!(ticks, [("Discord", true, true), ("Slack", true, false)]);
        assert_eq!(p.commands("en")[0].label, "Auto-post QSOs");
        // Each item toggles its destination only.
        p.run_command("discord");
        p.run_command("slack");
        let kids = &p.commands("en")[0].children;
        assert!(!kids[0].selected && kids[1].selected);
        p.run_command("post");
        let mut saved = Section::new();
        p.save(&mut saved);
        assert_eq!(saved["discord.enabled"], "false");
        assert_eq!(saved["slack.enabled"], "true");
        assert_eq!(saved["discord.url"], "u");
    }
}
