//! Discord: every QSO the operator logs is posted to a Discord channel, a Slack
//! channel or both through their incoming webhooks, and, when chosen, corrections
//! and deletions too.
//!
//! - [`config`]: the settings (webhook URLs, what to post).
//! - [`site`]: the destinations, Discord and Slack, and what differs between them.
//! - [`format`]: the line posted for a QSO.
//! - [`outbox`]: the bounded queue of a destination, batching and pacing.
//! - [`webhook`]: one post to a webhook, and Discord's answers.
//! - [`slack`]: Slack's body, escaping and answers.
//! - [`worker`]: the thread of a destination that empties its queue.
//! - [`poster`]: the observer that fills the queues; it never changes the log.
//! - [`options`]: the Discord tab of Options.
//! - [`status`]: the status bar item and the menu, which also loads the settings.
//!
//! The observer is recreated with each log, while the tab and the status item live
//! for the whole program, so all reach the one [`Hub`] through [`shared`].

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
pub use options::DiscordOptions;
pub use site::Site;
pub use status::DiscordStatus;

zlog_core::texts!();

/// What the observer, the worker and the windows share.
pub struct State {
    pub config: Config,
    /// A queue per destination, by [`Site::index`].
    pub outboxes: [outbox::Outbox; 2],
    /// The operator's callsign and the contest, added to every post.
    pub station: String,
    pub contest: String,
    /// Language of the user interface, for the words of corrections.
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

/// The state, and the signal that wakes the worker when it changed.
#[derive(Default)]
pub struct Hub {
    pub state: Mutex<State>,
    pub wake: Condvar,
}

/// The process-wide hub; the workers of the destinations start with it.
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

/// The package: one observer on the shared hub.
pub struct Discord;

impl Extension for Discord {
    fn name(&self) -> &str {
        "Discord"
    }

    fn observers(&mut self) -> Vec<Box<dyn Observer>> {
        vec![Box::new(poster::Poster::new(shared()))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
