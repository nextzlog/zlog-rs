use std::time::Duration;

use crate::Job;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub post: Option<(String, Vec<u8>)>,
    pub cookies: String,
    pub user_agent: &'static str,
    pub timeout: Duration,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Request {
            url: url.into(),
            post: None,
            cookies: String::new(),
            user_agent: crate::USER_AGENT,
            timeout: Duration::from_secs(30),
            headers: Vec::new(),
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

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn conditional(self, validators: &Validators) -> Self {
        let mut r = self;
        if let Some(etag) = &validators.etag {
            r = r.header("If-None-Match", etag);
        }
        if let Some(date) = &validators.last_modified {
            r = r.header("If-Modified-Since", date);
        }
        r
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub cookies: String,
    pub body: Vec<u8>,
    pub retry_after: Option<f64>,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl Reply {
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

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn not_modified(&self) -> bool {
        self.status == 304
    }

    pub fn validators(&self) -> Validators {
        Validators {
            etag: self.header("etag").map(String::from),
            last_modified: self.header("last-modified").map(String::from),
        }
    }

    pub fn text(&self) -> String {
        self.encoding().decode(&self.body).0.into_owned()
    }
}

pub type Exchange = Job<Reply>;

impl Job<Reply> {
    pub fn send(request: Request) -> Self {
        Job::spawn(move || exchange(&request))
    }
}

pub fn exchange(request: &Request) -> Result<Reply, String> {
    crate::online()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(request.timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let started = std::time::Instant::now();
    let mut url = url::Url::parse(&request.url).map_err(|e| e.to_string())?;
    let origin = url.clone();
    let mut post = request.post.clone();
    let mut jar = cookie_store::CookieStore::default();
    let mut base = parse_cookies(&request.cookies);
    for _ in 0..=MAX_REDIRECTS {
        let left = request.timeout.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err("timeout".into());
        }
        let own: &[(String, String)] = if same_host(&origin, &url) { &base } else { &[] };
        let mut sent: Vec<(&str, &str)> = jar.get_request_values(&url).collect();
        sent.sort_by(|a, b| a.0.cmp(b.0));
        let cookies = merge(own, sent.into_iter());
        let headers: &[(String, String)] = if url.origin() == origin.origin() {
            &request.headers
        } else {
            &[]
        };
        let mut response = match &post {
            None => {
                let mut r = agent
                    .get(url.as_str())
                    .config()
                    .timeout_global(Some(left))
                    .build()
                    .header("User-Agent", request.user_agent);
                if !cookies.is_empty() {
                    r = r.header("Cookie", &cookies);
                }
                for (name, value) in headers {
                    r = r.header(name, value);
                }
                r.call()
            }
            Some((content_type, body)) => {
                let mut r = agent
                    .post(url.as_str())
                    .config()
                    .timeout_global(Some(left))
                    .build()
                    .header("User-Agent", request.user_agent)
                    .header("Content-Type", content_type);
                if !cookies.is_empty() {
                    r = r.header("Cookie", &cookies);
                }
                for (name, value) in headers {
                    r = r.header(name, value);
                }
                r.send(&body[..])
            }
        }
        .map_err(|e| e.to_string())?;
        let header = |v: &ureq::http::HeaderValue| v.to_str().unwrap_or_default().to_string();
        for c in response.headers().get_all("set-cookie") {
            let Ok(cookie) = cookie_store::Cookie::parse(header(c), &url) else {
                continue;
            };
            if same_host(&origin, &url) && cookie.domain.matches(&url) {
                base.retain(|(n, _)| n != cookie.name());
            }
            let _ = jar.insert(cookie, &url);
        }
        let status = response.status().as_u16();
        let location = response.headers().get("location").map(header);
        if let (301 | 302 | 303 | 307 | 308, Some(location)) = (status, location) {
            url = url.join(location.trim()).map_err(|e| e.to_string())?;
            if !matches!(status, 307 | 308) {
                post = None;
            }
            continue;
        }
        let headers = response
            .headers()
            .iter()
            .map(|(n, v)| (n.as_str().to_string(), header(v)))
            .collect();
        let own: &[(String, String)] = if same_host(&origin, &url) { &base } else { &[] };
        let mut set: Vec<(&str, &str)> = jar.get_request_values(&url).collect();
        for c in jar.iter_unexpired().filter(|c| c.domain.matches(&url)) {
            if !set.iter().any(|(n, _)| *n == c.name()) {
                set.push((c.name(), c.value()));
            }
        }
        set.sort_by(|a, b| a.0.cmp(b.0));
        return Ok(Reply {
            headers,
            status,
            content_type: response
                .headers()
                .get("content-type")
                .map(header)
                .unwrap_or_default(),
            cookies: merge(own, set.into_iter()),
            retry_after: response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(retry_after),
            body: response
                .body_mut()
                .read_to_vec()
                .map_err(|e| e.to_string())?,
            url: url.into(),
        });
    }
    Err("too many redirects".into())
}

const MAX_REDIRECTS: usize = 10;

fn same_host(origin: &url::Url, url: &url::Url) -> bool {
    let downgrade = origin.scheme() == "https" && url.scheme() != "https";
    !downgrade && origin.host_str().is_some_and(|h| url.host_str() == Some(h))
}

const RETRY_AFTER_MAX: f64 = 86400.0;

fn parse_cookies(cookies: &str) -> Vec<(String, String)> {
    cookies
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .collect()
}

fn merge<'a>(
    base: &'a [(String, String)],
    set: impl Iterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut all: Vec<(&str, &str)> = base.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
    for (name, value) in set {
        match all.iter_mut().find(|(n, _)| *n == name) {
            Some(old) => old.1 = value,
            None => all.push((name, value)),
        }
    }
    all.iter()
        .map(|(n, v)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn retry_after(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<f64>() {
        return (seconds.is_finite() && seconds >= 0.0).then(|| seconds.min(RETRY_AFTER_MAX));
    }
    let at = httpdate::parse_http_date(value).ok()?;
    let left = at
        .duration_since(std::time::SystemTime::now())
        .unwrap_or_default();
    Some(left.as_secs_f64().min(RETRY_AFTER_MAX))
}

pub fn urlencoded(fields: &[(String, Vec<u8>)]) -> Vec<u8> {
    let percent = |bytes: &[u8]| {
        let mut crlf = Vec::with_capacity(bytes.len());
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' && (i == 0 || bytes[i - 1] != b'\r') {
                crlf.push(b'\r');
            }
            crlf.push(b);
        }
        form_urlencoded::byte_serialize(&crlf).collect::<String>()
    };
    fields
        .iter()
        .map(|(name, value)| format!("{}={}", percent(name.as_bytes()), percent(value)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

const BOUNDARY: &str = "----zmax-form-7b3f9c2e1d";

pub fn multipart(fields: &[(String, Vec<u8>)]) -> (String, Vec<u8>) {
    let mut out = Vec::new();
    for (name, value) in fields {
        out.extend(format!("--{BOUNDARY}\r\n").bytes());
        let name = name
            .replace('"', "%22")
            .replace('\r', "%0D")
            .replace('\n', "%0A");
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
        let body = urlencoded(&[field("*-._~", "\n\u{3042}/")]);
        assert_eq!(body, b"*-._%7E=%0D%0A%E3%81%82%2F");
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
        let _lock = crate::OFFLINE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        let _lock = crate::OFFLINE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
        let _lock = crate::OFFLINE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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
    }

    #[test]
    fn a_conditional_get_sends_the_validators_and_reads_304() {
        let _lock = crate::OFFLINE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for answer in [
                "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nLast-Modified: Sat, 24 Oct 2026 00:00:00 GMT\r\n\
Content-Length: 2\r\nConnection: close\r\n\r\n{}",
                "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n",
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
        let url = format!("http://{addr}/market.json");
        let first = exchange(&Request::get(&url)).unwrap();
        let v = first.validators();
        assert_eq!(v.etag.as_deref(), Some("\"v1\""));
        assert_eq!(first.header("LAST-MODIFIED"), v.last_modified.as_deref());
        assert!(!first.not_modified());
        let second = exchange(&Request::get(&url).conditional(&v)).unwrap();
        let seen = server.join().unwrap();
        assert!(!seen[0].contains("if-none-match"));
        assert!(seen[1].contains("if-none-match: \"v1\""));
        assert!(seen[1].contains("if-modified-since: sat, 24 oct 2026 00:00:00 gmt"));
        assert!(second.not_modified() && second.body.is_empty());
    }

    fn answer(listener: std::net::TcpListener, answer: String) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
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
            String::from_utf8_lossy(&got).to_lowercase()
        })
    }

    #[test]
    fn cookies_and_headers_of_the_caller_stay_with_the_first_origin() {
        let _lock = crate::OFFLINE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let b = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = b.local_addr().unwrap().port();
        let a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = a.local_addr().unwrap();
        let first = answer(
            a,
            format!(
                "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/\r\n\
Content-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );
        let second = answer(
            b,
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        );
        let request = Request::get(format!("http://{addr}/"))
            .cookies("SESSION=secret")
            .header("Authorization", "Bearer tok");
        let reply = exchange(&request).unwrap();
        assert_eq!(reply.status, 200);
        let first = first.join().unwrap();
        let second = second.join().unwrap();
        assert!(first.contains("session=secret") && first.contains("bearer tok"));
        assert!(
            !second.contains("secret") && !second.contains("bearer"),
            "{second}"
        );
        assert_eq!(reply.cookies, "");
    }

    #[test]
    fn a_deleted_cookie_is_left_out_of_the_reply() {
        let _lock = crate::OFFLINE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = answer(
            listener,
            "HTTP/1.1 200 OK\r\nSet-Cookie: S=; Max-Age=0; path=/\r\nSet-Cookie: T=2; path=/\r\n\
Retry-After: Thu, 01 Jan 1970 00:00:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .into(),
        );
        let reply = exchange(&Request::get(format!("http://{addr}/")).cookies("S=1; K=3")).unwrap();
        server.join().unwrap();
        assert_eq!(reply.cookies, "K=3; T=2");
        assert_eq!(reply.retry_after, Some(0.0));
    }

    #[test]
    fn retry_after_in_seconds_or_as_a_date() {
        assert_eq!(retry_after(" 120 "), Some(120.0));
        let later = std::time::SystemTime::now() + Duration::from_secs(3600);
        let left = retry_after(&httpdate::fmt_http_date(later)).unwrap();
        assert!((3590.0..=3600.0).contains(&left), "{left}");
        assert_eq!(retry_after("soon"), None);
    }

    #[test]
    fn retry_after_ignores_odd_values_and_is_clamped() {
        for odd in ["inf", "NaN", "-5", "-inf", "1e400"] {
            assert_eq!(retry_after(odd), None, "{odd}");
        }
        assert_eq!(retry_after("1e9"), Some(RETRY_AFTER_MAX));
        assert_eq!(retry_after("Sun, 06 Nov 1994 08:49:37 GMT"), Some(0.0));
    }

    #[test]
    fn caller_cookies_follow_the_host_not_the_scheme_or_port() {
        let u = |s: &str| url::Url::parse(s).unwrap();
        assert!(same_host(&u("http://site/"), &u("https://site/login")));
        assert!(same_host(&u("http://site/"), &u("http://site:8080/")));
        assert!(!same_host(&u("https://site/"), &u("http://site/")));
        assert!(!same_host(&u("http://site/"), &u("http://other/")));
        assert!(!same_host(&u("http://site/"), &u("http://www.site/")));
    }

    #[test]
    fn a_port_change_on_the_same_host_keeps_cookies_but_not_headers() {
        let _lock = crate::OFFLINE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let b = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = b.local_addr().unwrap().port();
        let a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = a.local_addr().unwrap();
        let first = answer(
            a,
            format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/\r\n\
Content-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );
        let second = answer(
            b,
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        );
        let request = Request::get(format!("http://{addr}/"))
            .cookies("SESSION=s")
            .header("Authorization", "Bearer tok");
        let reply = exchange(&request).unwrap();
        first.join().unwrap();
        let second = second.join().unwrap();
        assert!(second.contains("cookie: session=s"), "{second}");
        assert!(!second.contains("bearer"), "{second}");
        assert_eq!(reply.cookies, "SESSION=s");
    }

    #[test]
    fn multipart_names_are_escaped() {
        let (_, body) = multipart(&[field("a\"b\r\nc", "v")]);
        let body = String::from_utf8(body).unwrap();
        assert!(
            body.contains("name=\"a%22b%0D%0Ac\"\r\n\r\nv\r\n"),
            "{body}"
        );
    }
}
