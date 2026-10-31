use std::time::Duration;

use zlog_net::{Reply, Request};

use crate::outbox::Outcome;
use crate::site::Site;

pub trait Sender: Send {
    fn send(&mut self, site: Site, url: &str, content: &str) -> Outcome;
}

const TIMEOUT: Duration = Duration::from_secs(20);

pub struct Webhook;

impl Sender for Webhook {
    fn send(&mut self, site: Site, url: &str, content: &str) -> Outcome {
        let body = site.body(content).into_bytes();
        let request = Request::post(url.trim(), "application/json", body).timeout(TIMEOUT);
        let answer = zlog_net::exchange(&request);
        match site.outcome(answer) {
            Outcome::Failed(e) => Outcome::Failed(redact(&e, url)),
            Outcome::Refused(e) => Outcome::Refused(redact(&e, url)),
            o => o,
        }
    }
}

pub fn redact(text: &str, url: &str) -> String {
    let url = url.trim();
    let mut secrets: Vec<String> = vec![url.to_string()];
    if let Ok(u) = url::Url::parse(url) {
        let segments: Vec<&str> = u.path_segments().into_iter().flatten().collect();
        let known = match segments.as_slice() {
            ["api", "webhooks", rest @ ..] | ["services", rest @ ..] => Some(rest),
            _ => None,
        };
        match known {
            Some(rest) => secrets.extend(rest.iter().map(|s| s.to_string())),
            None => secrets.extend(
                segments
                    .iter()
                    .filter(|s| looks_like_token(s))
                    .map(|s| s.to_string()),
            ),
        }
        secrets.extend(
            u.query_pairs()
                .filter(|(k, _)| secret_key(k))
                .map(|(_, v)| v.into_owned()),
        );
        secrets.extend(Some(u.password().unwrap_or_default().to_string()));
    } else {
        secrets.extend(url.rsplit('/').next().map(String::from));
    }
    secrets.retain(|s| !s.is_empty());
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = text.to_string();
    for s in &secrets {
        out = out.replace(s.as_str(), if s == url { "(webhook)" } else { "(secret)" });
    }
    out
}

fn looks_like_token(s: &str) -> bool {
    s.len() >= 16
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && (s.chars().any(|c| c.is_ascii_digit())
            || s.chars().any(|c| c.is_ascii_uppercase())
                && s.chars().any(|c| c.is_ascii_lowercase()))
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["token", "key", "secret", "sig", "auth", "password"]
        .iter()
        .any(|k| key.contains(k))
}

pub fn markdown_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '*'
                | '_'
                | '~'
                | '`'
                | '|'
                | '>'
                | '#'
                | '-'
                | '['
                | ']'
                | '('
                | ')'
                | ':'
                | '<'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub fn body(content: &str) -> String {
    serde_json::json!({ "content": markdown_escape(content), "allowed_mentions": { "parse": [] } })
        .to_string()
}

pub(crate) const MAX_WAIT: f64 = 3600.0;

const DEFAULT_WAIT: Duration = Duration::from_secs(5);

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
            Outcome::Wait(secs.map_or(DEFAULT_WAIT, |s| Duration::from_secs_f64(s.min(MAX_WAIT))))
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
        assert_eq!(
            outcome(posted(429, "", Some(1e20))),
            Outcome::Wait(Duration::from_secs(3600))
        );
        assert_eq!(
            outcome(posted(429, r#"{"retry_after": 1e300}"#, None)),
            Outcome::Wait(Duration::from_secs(3600))
        );
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
        let e = "error for https://discord.com:443/api/webhooks/123456/SeCrEt: id 123456";
        let r = redact(e, "https://discord.com/api/webhooks/123456/SeCrEt");
        assert!(!r.contains("123456") && !r.contains("SeCrEt"), "{r}");
        let slack = "https://hooks.slack.com/services/T0AAA/B0BBB/xoxTOKEN";
        let r = redact(
            "T0AAA B0BBB xoxTOKEN https://hooks.slack.com/services/T0AAA/B0BBB/xoxTOKEN",
            slack,
        );
        assert!(
            !r.contains("T0AAA") && !r.contains("B0BBB") && !r.contains("xoxTOKEN"),
            "{r}"
        );
        assert!(r.contains("(webhook)"));
        let r = redact(
            "bad key=k3y in https://x.example/h",
            "https://x.example/h?key=k3y",
        );
        assert!(!r.contains("k3y"), "{r}");
        let r = redact(
            "HTTP 404: the webhook refused the post, true",
            "https://example.com/webhook/AbCdEf1234567890xyz?wait=true",
        );
        assert_eq!(r, "HTTP 404: the webhook refused the post, true");
        let r = redact(
            "at /webhook/AbCdEf1234567890xyz with pa55word",
            "https://u:pa55word@example.com/webhook/AbCdEf1234567890xyz",
        );
        assert_eq!(r, "at /webhook/(secret) with (secret)");
    }

    #[test]
    fn markdown_in_posts_is_shown_as_typed() {
        assert_eq!(
            markdown_escape("JA1ZLO/1 *599* _13M_ ~x~ `a` ||s|| > # - [l](u) <@1> :e:"),
            "JA1ZLO/1 \\*599\\* \\_13M\\_ \\~x\\~ \\`a\\` \\|\\|s\\|\\| \\> \\# \\- \\[l\\]\\(u\\) \\<@1\\> \\:e\\:"
        );
        let v: serde_json::Value = serde_json::from_str(&body("a_b")).unwrap();
        assert_eq!(v["content"], "a\\_b");
    }
}
