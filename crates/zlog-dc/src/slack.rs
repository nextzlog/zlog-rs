//! What a post to a Slack incoming webhook looks like and what its answer means.
//!
//! Slack answers `ok` with 200, and otherwise a short error string with the
//! status: `invalid_payload` (400), `invalid_token` or `action_prohibited` (403),
//! `no_service` or `channel_not_found` (404), `channel_is_archived` (410); 429 with
//! `Retry-After` when posts come faster than about one a second.

use std::time::Duration;

use zlog_net::Reply;

use crate::outbox::Outcome;

/// `text` with `&`, `<` and `>` escaped, so Slack reads no mention (`<!here>`,
/// `<@U123>`) and no link (`<https://...|x>`) in it.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The JSON body: the escaped text.
pub fn body(content: &str) -> String {
    serde_json::json!({ "text": escape(content) }).to_string()
}

/// Wait asked for when a 429 gives none.
const DEFAULT_WAIT: Duration = Duration::from_secs(5);

/// Slack's error string of a body, kept short and plain, as it is shown.
fn error_of(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let word: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(40)
        .collect();
    word
}

/// The meaning of an answer: 2xx sent, 429 wait as long as asked, a refusal of
/// the webhook (400, 403, 404, 410) stops posting, anything else failed.
pub fn outcome(answer: Result<Reply, String>) -> Outcome {
    let p = match answer {
        Err(e) => return Outcome::Failed(e),
        Ok(p) => p,
    };
    let error = error_of(&p.body);
    let said = |status: u16| {
        if error.is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status} {error}")
        }
    };
    match p.status {
        200..=299 => Outcome::Sent,
        429 => Outcome::Wait(
            p.retry_after
                .filter(|s| s.is_finite() && *s >= 0.0)
                .map_or(DEFAULT_WAIT, Duration::from_secs_f64),
        ),
        400 | 403 | 404 | 410 => Outcome::Refused(format!(
            "{}: the webhook refused the post; posting stopped, the lines kept \
(check the URL in Options, then switch posting off and on)",
            said(p.status)
        )),
        s => Outcome::Failed(said(s)),
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
    fn the_body_is_escaped_text() {
        let v: serde_json::Value =
            serde_json::from_str(&body("<!here> A&B <@U1> <https://x|y>\n\"q\"")).unwrap();
        assert_eq!(
            v["text"],
            "&lt;!here&gt; A&amp;B &lt;@U1&gt; &lt;https://x|y&gt;\n\"q\""
        );
        assert_eq!(v.as_object().unwrap().len(), 1);
        assert_eq!(escape("&lt;"), "&amp;lt;");
    }

    #[test]
    fn answers_are_read() {
        assert_eq!(outcome(posted(200, "ok", None)), Outcome::Sent);
        assert_eq!(
            outcome(posted(429, "rate_limited", Some(2.0))),
            Outcome::Wait(Duration::from_secs(2))
        );
        assert_eq!(outcome(posted(429, "", None)), Outcome::Wait(DEFAULT_WAIT));
        for (status, error) in [
            (403, "invalid_token"),
            (404, "no_service"),
            (404, "channel_not_found"),
            (410, "channel_is_archived"),
            (400, "invalid_payload"),
        ] {
            let o = outcome(posted(status, error, None));
            let want = format!("HTTP {status} {error}:");
            assert!(
                matches!(&o, Outcome::Refused(e) if e.starts_with(&want)),
                "{o:?}"
            );
        }
        assert!(
            matches!(outcome(posted(404, "", None)), Outcome::Refused(e) if e.starts_with("HTTP 404:"))
        );
        assert_eq!(
            outcome(posted(500, "<html>oops</html>", None)),
            Outcome::Failed("HTTP 500".into())
        );
        assert_eq!(
            outcome(posted(503, "service_unavailable", None)),
            Outcome::Failed("HTTP 503 service_unavailable".into())
        );
        assert_eq!(
            outcome(Err("offline".into())),
            Outcome::Failed("offline".into())
        );
    }
}
