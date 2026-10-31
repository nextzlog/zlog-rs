use std::sync::{Arc, Condvar, Mutex, OnceLock};

use zlog_core::{Extension, Observer};

pub mod config;
pub mod format;
pub mod options;
pub mod outbox;
pub mod poster;
pub mod site;
pub mod slack;
pub mod status;
pub mod webhook;
pub mod worker;

pub use config::Config;
pub use options::AutoPostOptions;
pub use site::Site;
pub use status::AutoPostStatus;

zlog_core::texts!();

pub struct State {
    pub config: Config,
    pub outboxes: [outbox::Outbox; 2],
    pub station: String,
    pub contest: String,
    pub language: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            config: Config::default(),
            outboxes: Site::ALL.map(|s| outbox::Outbox::new(s.pace())),
            station: String::new(),
            contest: String::new(),
            language: String::new(),
        }
    }
}

impl State {
    pub fn outbox(&self, site: Site) -> &outbox::Outbox {
        &self.outboxes[site.index()]
    }

    pub fn outbox_mut(&mut self, site: Site) -> &mut outbox::Outbox {
        &mut self.outboxes[site.index()]
    }
}

#[derive(Default)]
pub struct Hub {
    pub state: Mutex<State>,
    pub wake: Condvar,
}

pub fn shared() -> Arc<Hub> {
    static HUB: OnceLock<Arc<Hub>> = OnceLock::new();
    HUB.get_or_init(|| {
        let hub = Arc::new(Hub::default());
        for site in Site::ALL {
            worker::start(hub.clone(), site, Box::new(webhook::Webhook));
        }
        hub
    })
    .clone()
}

pub const NAME: &str = "Auto-post to social media";

pub struct AutoPost;

impl Extension for AutoPost {
    fn name(&self) -> &str {
        NAME
    }

    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        vec![Box::new(poster::Poster::new(shared()))]
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.extension(zlog_ui::builder::order::OBSERVERS, || AutoPost);
    b.panel(610, |_| AutoPostStatus::default());
    b.page(510, |_| AutoPostOptions::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn the_names_shown_are_not_discord_only() {
        assert_eq!(AutoPost.name(), NAME);
        assert_eq!(poster::Poster::new(shared()).name(), NAME);
        assert_eq!(texts().get("ja", "title"), "SNS に自動投稿");
        assert_eq!(texts().get("en", "title"), "Auto-post to social media");
        assert_eq!(texts().get("ja", "menu.post"), "SNS に自動投稿");
    }
}
