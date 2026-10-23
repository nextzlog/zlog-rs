//! The destinations a line is posted to: Discord and Slack, each an incoming
//! webhook of its own with its own body, answers and pace. Everything else (the
//! queue, the worker, the status) is the same for both.

use std::time::Duration;

use zlog_net::Reply;

use crate::outbox::Outcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    Discord,
    Slack,
}

impl Site {
    /// Every destination, in the order of the tab, the menu and the status bar.
    pub const ALL: [Site; 2] = [Site::Discord, Site::Slack];

    /// Where it stands in [`Site::ALL`], to index what is kept per destination.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Its name, the same in every language.
    pub fn name(self) -> &'static str {
        match self {
            Site::Discord => "Discord",
            Site::Slack => "Slack",
        }
    }

    /// The prefix of its keys in the settings and its command id.
    pub fn code(self) -> &'static str {
        match self {
            Site::Discord => "discord",
            Site::Slack => "slack",
        }
    }

    /// What its webhook addresses look like, as the hint of the URL field.
    pub fn hint(self) -> &'static str {
        match self {
            Site::Discord => "https://discord.com/api/webhooks/...",
            Site::Slack => "https://hooks.slack.com/services/...",
        }
    }

    /// Least time between two posts: Discord allows a webhook about 30 messages
    /// a minute, Slack about one a second.
    pub fn pace(self) -> Duration {
        match self {
            Site::Discord => Duration::from_secs(2),
            Site::Slack => Duration::from_secs(1),
        }
    }

    /// The JSON body posting `content`.
    pub fn body(self, content: &str) -> String {
        match self {
            Site::Discord => crate::webhook::body(content),
            Site::Slack => crate::slack::body(content),
        }
    }

    /// The meaning of its answer for the queue.
    pub fn outcome(self, answer: Result<Reply, String>) -> Outcome {
        match self {
            Site::Discord => crate::webhook::outcome(answer),
            Site::Slack => crate::slack::outcome(answer),
        }
    }
}
