use std::sync::Arc;

use zlog_ui::{Command, Panel, Section, StatusItem, View};

use crate::config::Config;
use crate::site::Site;
use crate::{texts, Hub, State};

const POST: &str = "post";

const REPAINT: std::time::Duration = std::time::Duration::from_millis(300);

pub struct AutoPostStatus {
    hub: Arc<Hub>,
}

impl Default for AutoPostStatus {
    fn default() -> Self {
        AutoPostStatus::new(crate::shared())
    }
}

impl AutoPostStatus {
    pub fn new(hub: Arc<Hub>) -> Self {
        AutoPostStatus { hub }
    }
}

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

impl Panel for AutoPostStatus {
    fn id(&self) -> &str {
        "dc"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn section_kind(&self) -> zlog_ui::SectionKind {
        zlog_ui::SectionKind::Secret
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Network
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Post, 0)
    }

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
        let post = Command::new(
            POST,
            t.get(language, "toolbar.post"),
            t.get(language, "toolbar.post.tip"),
        )
        .menu_label(t.get(language, "menu.post"))
        .icon(zlog_ui::icon::CHAT_CIRCLE)
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

    fn tick_after(&self) -> Option<std::time::Duration> {
        let s = self.hub.state.lock().unwrap();
        Site::ALL
            .into_iter()
            .any(|site| s.config.ready(site) && s.outbox(site).busy())
            .then_some(REPAINT)
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
        s.config.target_mut(Site::Discord).enabled = false;
        assert!(!item(&s, "en").unwrap().warning);
    }

    #[test]
    fn the_status_bar_repaints_while_a_post_waits_or_is_sent() {
        let hub = Arc::new(Hub::default());
        let p = AutoPostStatus::new(hub.clone());
        assert_eq!(zlog_ui::Panel::tick_after(&p), None);
        ready(&mut hub.state.lock().unwrap(), Site::Discord);
        assert_eq!(zlog_ui::Panel::tick_after(&p), None);
        hub.state
            .lock()
            .unwrap()
            .outbox_mut(Site::Discord)
            .push("a".into());
        let waiting = zlog_ui::Panel::tick_after(&p).expect("waiting");
        assert!(
            waiting <= std::time::Duration::from_millis(500),
            "{waiting:?}"
        );
        let now = std::time::Instant::now();
        let b = hub
            .state
            .lock()
            .unwrap()
            .outbox_mut(Site::Discord)
            .take(now)
            .unwrap();
        assert!(zlog_ui::Panel::tick_after(&p).is_some(), "while sending");
        hub.state
            .lock()
            .unwrap()
            .outbox_mut(Site::Discord)
            .done(b, Outcome::Sent, now);
        assert_eq!(zlog_ui::Panel::tick_after(&p), None);
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
        let mut p = AutoPostStatus::new(hub.clone());
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
        assert_eq!(menu[0].menu_text(), "SNS に自動投稿");
        let ticks: Vec<(&str, bool, bool)> = menu[0]
            .children
            .iter()
            .map(|c| (c.label.as_str(), c.checkable, c.selected))
            .collect();
        assert_eq!(ticks, [("Discord", true, true), ("Slack", true, false)]);
        assert_eq!(p.commands("en")[0].menu_text(), "Auto-post to social media");
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
