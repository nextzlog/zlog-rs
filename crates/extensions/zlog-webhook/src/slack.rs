use std::time::Duration;

use zlog_net::Reply;

use crate::outbox::Outcome;
use crate::webhook::MAX_WAIT;

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn body(content: &str) -> String {
    serde_json::json!({ "text": escape(content) }).to_string()
}

const DEFAULT_WAIT: Duration = Duration::from_secs(5);

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
                .map_or(DEFAULT_WAIT, |s| Duration::from_secs_f64(s.min(MAX_WAIT))),
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
        assert_eq!(
            outcome(posted(429, "", Some(1e20))),
            Outcome::Wait(Duration::from_secs(3600))
        );
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
