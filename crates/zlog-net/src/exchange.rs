//! Requests that send something: a GET or POST with the cookies of the session,
//! answered with the status, the cookies set and the body as sent, for filling in
//! and submitting a Web form as a browser does, or for posting JSON to a service.
//!
//! An [`Exchange`] runs on its own thread like [`crate::Fetch`]; both fail at once
//! while offline ([`crate::set_offline`]). The form encoders ([`urlencoded`],
//! [`multipart`]) take the values already encoded in the charset of the page.

use std::time::Duration;

use crate::Job;

/// One request to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    /// `None` for a GET; the content type and body of a POST.
    pub post: Option<(String, Vec<u8>)>,
    /// The `Cookie` header, empty for none.
    pub cookies: String,
    pub user_agent: &'static str,
    /// Longest the whole exchange may take.
    pub timeout: Duration,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Request {
            url: url.into(),
            post: None,
            cookies: String::new(),
            user_agent: crate::USER_AGENT,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn post(url: impl Into<String>, content_type: impl Into<String>, body: Vec<u8>) -> Self {
        Request {
            post: Some((content_type.into(), body)),
            ..Request::get(url)
        }
    }

    pub fn cookies(mut self, cookies: impl Into<String>) -> Self {
        self.cookies = cookies.into();
        self
    }

    pub fn user_agent(mut self, user_agent: &'static str) -> Self {
        self.user_agent = user_agent;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// The answer of the server, whatever its status.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub status: u16,
    /// The `Content-Type` header, empty without one.
    pub content_type: String,
    /// The cookies set (`name=value` of each `Set-Cookie`), joined as a `Cookie`
    /// header.
    pub cookies: String,
    pub body: Vec<u8>,
    /// The `Retry-After` header in seconds, when the server sent one.
    pub retry_after: Option<f64>,
    /// The URL answered, after any redirects (empty when not from [`exchange`]).
    pub url: String,
}

impl Reply {
    /// The charset of the body: the `Content-Type` header's, else a `<meta>`
    /// tag's, else UTF-8.
    pub fn encoding(&self) -> &'static encoding_rs::Encoding {
        let head = String::from_utf8_lossy(&self.body[..self.body.len().min(2048)]).to_lowercase();
        [self.content_type.to_lowercase(), head]
            .iter()
            .find_map(|s| {
                let rest = &s[s.find("charset=")? + 8..];
                let name: String = rest
                    .trim_start_matches(['"', '\''])
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                encoding_rs::Encoding::for_label(name.as_bytes())
            })
            .unwrap_or(encoding_rs::UTF_8)
    }

    /// The body decoded in its charset.
    pub fn text(&self) -> String {
        self.encoding().decode(&self.body).0.into_owned()
    }
}

/// One [`Request`] in flight, or its answer, whatever its status.
pub type Exchange = Job<Reply>;

impl Job<Reply> {
    /// Start sending `request`.
    pub fn send(request: Request) -> Self {
        Job::spawn(move || exchange(&request))
    }
}

/// Send `request` on the calling thread (run it on a thread of your own). Every
/// status is an answer, so the caller can tell a refusal (4xx, 429) from a network
/// failure, which is the error.
pub fn exchange(request: &Request) -> Result<Reply, String> {
    crate::online()?;
    // Redirects are followed here, not by ureq, so that the cookies set on every hop
    // are kept and sent on, and the final URL is known (forms resolve against it).
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(request.timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let started = std::time::Instant::now();
    let mut url = request.url.clone();
    let mut post = request.post.clone();
    // The cookies set along the way, by name, in the order first set.
    let mut set: Vec<(String, String)> = Vec::new();
    for _ in 0..=MAX_REDIRECTS {
        let left = request.timeout.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err("timeout".into());
        }
        let cookies = merge(&request.cookies, &set);
        let mut response = match &post {
            None => {
                let mut r = agent
                    .get(&url)
                    .config()
                    .timeout_global(Some(left))
                    .build()
                    .header("User-Agent", request.user_agent);
                if !cookies.is_empty() {
                    r = r.header("Cookie", &cookies);
                }
                r.call()
            }
            Some((content_type, body)) => {
                let mut r = agent
                    .post(&url)
                    .config()
                    .timeout_global(Some(left))
                    .build()
                    .header("User-Agent", request.user_agent)
                    .header("Content-Type", content_type);
                if !cookies.is_empty() {
                    r = r.header("Cookie", &cookies);
                }
                r.send(&body[..])
            }
        }
        .map_err(|e| e.to_string())?;
        let header = |v: &ureq::http::HeaderValue| v.to_str().unwrap_or_default().to_string();
        for c in response.headers().get_all("set-cookie") {
            let Some((name, value)) = header(c)
                .split(';')
                .next()
                .and_then(|c| c.trim().split_once('='))
                .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
            else {
                continue;
            };
            match set.iter_mut().find(|(n, _)| *n == name) {
                Some(old) => old.1 = value,
                None => set.push((name, value)),
            }
        }
        let status = response.status().as_u16();
        let location = response.headers().get("location").map(header);
        if let (301 | 302 | 303 | 307 | 308, Some(location)) = (status, location) {
            url = join(&url, &location);
            // As browsers do: 303, and 301/302 after a POST, continue as a GET.
            if !matches!(status, 307 | 308) {
                post = None;
            }
            continue;
        }
        return Ok(Reply {
            status,
            content_type: response
                .headers()
                .get("content-type")
                .map(header)
                .unwrap_or_default(),
            cookies: set
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join("; "),
            retry_after: response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse().ok()),
            body: response
                .body_mut()
                .read_to_vec()
                .map_err(|e| e.to_string())?,
            url,
        });
    }
    Err("too many redirects".into())
}

/// Most redirects followed, as ureq's default.
const MAX_REDIRECTS: usize = 10;

/// The `Cookie` header `base` with the cookies `set` since (replacing same names).
fn merge(base: &str, set: &[(String, String)]) -> String {
    let mut all: Vec<(String, String)> = base
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .collect();
    for (name, value) in set {
        match all.iter_mut().find(|(n, _)| n == name) {
            Some(old) => old.1 = value.clone(),
            None => all.push((name.clone(), value.clone())),
        }
    }
    all.iter()
        .map(|(n, v)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The absolute URL of a `Location` sent by the page `base`.
fn join(base: &str, location: &str) -> String {
    let location = location.trim();
    if location.contains("://") {
        return location.to_string();
    }
    let scheme_end = base.find("://").map_or(0, |p| p + 3);
    if let Some(rest) = location.strip_prefix("//") {
        return format!("{}{rest}", &base[..scheme_end]);
    }
    let origin_end = base[scheme_end..]
        .find('/')
        .map_or(base.len(), |e| scheme_end + e);
    let origin = &base[..origin_end];
    if location.starts_with('/') {
        return format!("{origin}{location}");
    }
    let path = base[origin_end..].split(['?', '#']).next().unwrap_or("");
    let dir = &path[..path.rfind('/').map_or(0, |p| p + 1)];
    let dir = if dir.is_empty() { "/" } else { dir };
    format!("{origin}{dir}{location}")
}

/// The fields as `application/x-www-form-urlencoded`.
pub fn urlencoded(fields: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, (name, value)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(b'&');
        }
        percent(&mut out, name.as_bytes());
        out.push(b'=');
        percent(&mut out, value);
    }
    out
}

/// Percent-encoding of a form (a space as `+`), line breaks as CRLF.
fn percent(out: &mut Vec<u8>, bytes: &[u8]) {
    let mut prev = 0u8;
    for &b in bytes {
        match b {
            b'\n' if prev != b'\r' => out.extend(b"%0D%0A"),
            b' ' => out.push(b'+'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => out.push(b),
            _ => out.extend(format!("%{b:02X}").bytes()),
        }
        prev = b;
    }
}

/// Boundary of [`multipart`]: never in an E-Log or a form value.
const BOUNDARY: &str = "----zlog-rs-form-7b3f9c2e1d";

/// The fields as `multipart/form-data`: the content type and the body.
pub fn multipart(fields: &[(String, Vec<u8>)]) -> (String, Vec<u8>) {
    let mut out = Vec::new();
    for (name, value) in fields {
        out.extend(format!("--{BOUNDARY}\r\n").bytes());
        out.extend(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").bytes());
        out.extend(value);
        out.extend(b"\r\n");
    }
    out.extend(format!("--{BOUNDARY}--\r\n").bytes());
    (format!("multipart/form-data; boundary={BOUNDARY}"), out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn field(name: &str, value: &str) -> (String, Vec<u8>) {
        (name.into(), value.as_bytes().to_vec())
    }

    #[test]
    fn urlencoded_as_browsers_send_it() {
        let body = urlencoded(&[field("a b", "x&y=z"), field("t", "1\n2\r\n3")]);
        assert_eq!(body, b"a+b=x%26y%3Dz&t=1%0D%0A2%0D%0A3");
    }

    #[test]
    fn multipart_parts() {
        let (ct, body) = multipart(&[field("contest", "allja")]);
        assert!(ct.ends_with(BOUNDARY));
        let body = String::from_utf8(body).unwrap();
        assert!(body.contains("name=\"contest\"\r\n\r\nallja\r\n"));
        assert!(body.ends_with(&format!("--{BOUNDARY}--\r\n")));
    }

    #[test]
    fn charset_from_header_or_meta() {
        let mut r = Reply {
            content_type: "text/html; charset=Shift_JIS".into(),
            body: vec![0x83, 0x65],
            ..Reply::default()
        };
        assert_eq!(r.text(), "テ");
        r.content_type = "text/html".into();
        r.body = b"<meta charset=\"euc-jp\">".to_vec();
        assert_eq!(r.encoding(), encoding_rs::EUC_JP);
        r.body.clear();
        assert_eq!(r.encoding(), encoding_rs::UTF_8);
    }

    #[test]
    fn offline_posts_fail_at_once() {
        let _lock = crate::OFFLINE_LOCK.lock().unwrap();
        crate::set_offline(true);
        let mut e = Exchange::send(Request::post("http://192.0.2.1/", "text/plain", vec![]));
        let r = loop {
            if let Some(r) = e.poll() {
                break r.clone();
            }
            std::thread::yield_now();
        };
        crate::set_offline(false);
        assert_eq!(r, Err(crate::OFFLINE_ERROR.to_string()));
    }

    #[test]
    fn a_post_sends_its_body_and_cookies_and_gets_any_status() {
        let _lock = crate::OFFLINE_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            while !String::from_utf8_lossy(&got).ends_with("a=1") {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                got.extend(&buf[..n]);
            }
            s.write_all(
                b"HTTP/1.1 400 Bad Request\r\nSet-Cookie: S=2; path=/\r\n\
Content-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nno",
            )
            .unwrap();
            String::from_utf8_lossy(&got).into_owned()
        });
        let request = Request::post(
            format!("http://{addr}/"),
            "application/x-www-form-urlencoded",
            b"a=1".to_vec(),
        )
        .cookies("S=1");
        let reply = exchange(&request).unwrap();
        let got = server.join().unwrap();
        assert!(got.starts_with("POST / "));
        assert!(got.to_lowercase().contains("cookie: s=1"));
        assert_eq!((reply.status, reply.cookies.as_str()), (400, "S=2"));
        assert_eq!(reply.text(), "no");
    }

    #[test]
    fn redirects_keep_their_cookies_and_report_the_final_url() {
        let _lock = crate::OFFLINE_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for answer in [
                "HTTP/1.1 302 Found\r\nSet-Cookie: A=1; path=/\r\nLocation: /next/page\r\n\
Content-Length: 0\r\nConnection: close\r\n\r\n",
                "HTTP/1.1 200 OK\r\nSet-Cookie: B=2\r\nContent-Length: 2\r\n\
Connection: close\r\n\r\nok",
            ] {
                let (mut s, _) = listener.accept().unwrap();
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                while !String::from_utf8_lossy(&got).contains("\r\n\r\n") {
                    let n = s.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    got.extend(&buf[..n]);
                }
                s.write_all(answer.as_bytes()).unwrap();
                seen.push(String::from_utf8_lossy(&got).to_lowercase());
            }
            seen
        });
        let reply = exchange(&Request::get(format!("http://{addr}/start"))).unwrap();
        let seen = server.join().unwrap();
        assert!(seen[1].starts_with("get /next/page "));
        assert!(seen[1].contains("cookie: a=1"));
        assert_eq!(reply.cookies, "A=1; B=2");
        assert_eq!(reply.url, format!("http://{addr}/next/page"));
        assert_eq!(join("https://h/a/b?q", "c"), "https://h/a/c");
    }
}
