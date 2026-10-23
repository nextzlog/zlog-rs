//! One post to a webhook (Discord's body and answers here, Slack's in
//! [`crate::slack`]), and what its answer means for the queue.

use std::time::Duration;

use zlog_net::{Reply, Request};

use crate::outbox::Outcome;
use crate::site::Site;

/// Posts a message to a destination; each worker owns one, tests a fake.
pub trait Sender: Send {
    fn send(&mut self, site: Site, url: &str, content: &str) -> Outcome;
}

/// Longest one post may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The real sender, through [`zlog_net::exchange`].
pub struct Webhook;

impl Sender for Webhook {
    fn send(&mut self, site: Site, url: &str, content: &str) -> Outcome {
        let body = site.body(content).into_bytes();
        let request = Request::post(url.trim(), "application/json", body).timeout(TIMEOUT);
        let answer = zlog_net::exchange(&request);
        // A network error may quote the address, which holds the token.
        site.outcome(answer.map_err(|e| redact(&e, url)))
    }
}

/// `text` without the webhook address or its token, however it is quoted.
pub fn redact(text: &str, url: &str) -> String {
    let url = url.trim();
    let mut out = text.replace(url, "(webhook)");
    // The token, the last part of the path, also when the address is quoted
    // otherwise (with a port, in part).
    let token = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if token.len() >= 8 {
        out = out.replace(token, "(token)");
    }
    out
}

/// Discord's JSON body: the text, with mentions (`@everyone`) disabled.
pub fn body(content: &str) -> String {
    serde_json::json!({ "content": content, "allowed_mentions": { "parse": [] } }).to_string()
}

/// Wait asked for when a 429 gives none.
const DEFAULT_WAIT: Duration = Duration::from_secs(5);

/// The meaning of Discord's answer: 2xx sent, 429 wait as long as asked (the header, or
/// `retry_after` of the body), anything else failed.
pub fn outcome(answer: Result<Reply, String>) -> Outcome {
    match answer {
        Err(e) => Outcome::Failed(e),
        Ok(p) if (200..300).contains(&p.status) => Outcome::Sent,
        Ok(p) if p.status == 429 => {
            let body = serde_json::from_slice::<serde_json::Value>(&p.body).ok();
            let secs = p
                .retry_after
                .or_else(|| body?.get("retry_after")?.as_f64())
                .filter(|s| s.is_finite() && *s >= 0.0);
            Outcome::Wait(secs.map_or(DEFAULT_WAIT, Duration::from_secs_f64))
        }
        Ok(p) if matches!(p.status, 400 | 401 | 403 | 404) => Outcome::Refused(format!(
            "HTTP {}: the webhook refused the post; posting stopped, the lines kept \
(check the URL in Options, then switch posting off and on)",
            p.status
        )),
        Ok(p) => Outcome::Failed(format!("HTTP {}", p.status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posted(status: u16, body: &str, retry_after: Option<f64>) -> Result<Reply, String> {
        Ok(Reply {
            status,
            body: body.into(),
            retry_after,
            ..Reply::default()
        })
    }

    #[test]
    fn the_body_is_json_without_mentions() {
        let v: serde_json::Value = serde_json::from_str(&body("a \"b\"\n@everyone")).unwrap();
        assert_eq!(v["content"], "a \"b\"\n@everyone");
        assert_eq!(v["allowed_mentions"]["parse"], serde_json::json!([]));
    }

    #[test]
    fn answers_are_read() {
        assert_eq!(outcome(posted(204, "", None)), Outcome::Sent);
        assert_eq!(
            outcome(posted(429, r#"{"retry_after": 1.5}"#, None)),
            Outcome::Wait(Duration::from_millis(1500))
        );
        assert_eq!(
            outcome(posted(429, "", Some(3.0))),
            Outcome::Wait(Duration::from_secs(3))
        );
        assert_eq!(outcome(posted(429, "", None)), Outcome::Wait(DEFAULT_WAIT));
        assert!(
            matches!(outcome(posted(404, "", None)), Outcome::Refused(e) if e.starts_with("HTTP 404"))
        );
        assert_eq!(
            outcome(posted(500, "", None)),
            Outcome::Failed("HTTP 500".into())
        );
        assert_eq!(
            outcome(Err("offline".into())),
            Outcome::Failed("offline".into())
        );
    }

    #[test]
    fn the_token_never_shows() {
        let url = "https://discord.com/api/webhooks/123/SeCrEtToKeN ";
        let e = "io error: https://discord.com:443/api/webhooks/123/SeCrEtToKeN: refused";
        assert!(!redact(e, url).contains("SeCrEtToKeN"));
    }
}
