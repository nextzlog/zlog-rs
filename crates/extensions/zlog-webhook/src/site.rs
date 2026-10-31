use std::time::Duration;

use zlog_net::Reply;

use crate::outbox::Outcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    Discord,
    Slack,
}

impl Site {
    pub const ALL: [Site; 2] = [Site::Discord, Site::Slack];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Site::Discord => "Discord",
            Site::Slack => "Slack",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Site::Discord => "discord",
            Site::Slack => "slack",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Site::Discord => "https://discord.com/api/webhooks/...",
            Site::Slack => "https://hooks.slack.com/services/...",
        }
    }

    pub fn pace(self) -> Duration {
        match self {
            Site::Discord => Duration::from_secs(2),
            Site::Slack => Duration::from_secs(1),
        }
    }

    pub fn body(self, content: &str) -> String {
        match self {
            Site::Discord => crate::webhook::body(content),
            Site::Slack => crate::slack::body(content),
        }
    }

    pub fn outcome(self, answer: Result<Reply, String>) -> Outcome {
        match self {
            Site::Discord => crate::webhook::outcome(answer),
            Site::Slack => crate::slack::outcome(answer),
        }
    }
}
