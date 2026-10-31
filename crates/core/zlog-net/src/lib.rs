mod exchange;

pub use exchange::{exchange, multipart, urlencoded, Exchange, Reply, Request, Validators};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::Duration;

static OFFLINE: AtomicBool = AtomicBool::new(false);

const OFFLINE_ERROR: &str = "offline";

#[cfg(test)]
static OFFLINE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn set_offline(offline: bool) {
    OFFLINE.store(offline, Ordering::Relaxed);
}

pub fn offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

fn online() -> Result<(), String> {
    match OFFLINE.load(Ordering::Relaxed) {
        true => Err(OFFLINE_ERROR.into()),
        false => Ok(()),
    }
}

static REMOTE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn remote_connects() -> usize {
    REMOTE.load(Ordering::SeqCst)
}

fn loopback_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.eq_ignore_ascii_case("localhost")
        || bare
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<std::net::TcpStream, String> {
    let offline = OFFLINE.load(Ordering::Relaxed);
    if offline && !loopback_host(host) {
        return Err(OFFLINE_ERROR.into());
    }
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let addrs =
        std::net::ToSocketAddrs::to_socket_addrs(&(bare, port)).map_err(|e| e.to_string())?;
    let mut last = String::from("no address");
    for a in addrs {
        if !a.ip().is_loopback() {
            if offline {
                last = OFFLINE_ERROR.into();
                continue;
            }
            REMOTE.fetch_add(1, Ordering::SeqCst);
        }
        match std::net::TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

pub fn connect_address(address: &str, timeout: Duration) -> Result<std::net::TcpStream, String> {
    let (host, port) = address
        .rsplit_once(':')
        .and_then(|(h, p)| Some((h, p.trim().parse().ok()?)))
        .ok_or_else(|| format!("invalid address {address:?}"))?;
    connect(host.trim(), port, timeout)
}

pub const USER_AGENT: &str = concat!("zmax/", env!("CARGO_PKG_VERSION"));

pub const BROWSER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub struct Job<T> {
    rx: Receiver<Result<T, String>>,
    done: Option<Result<T, String>>,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            zlog_core::wake::wake();
        });
        Job { rx, done: None }
    }
}

impl<T> Job<T> {
    pub fn ready(result: Result<T, String>) -> Self {
        let (_, rx) = channel();
        Job {
            rx,
            done: Some(result),
        }
    }

    pub fn poll(&mut self) -> Option<&Result<T, String>> {
        if self.done.is_none() {
            match self.rx.try_recv() {
                Ok(r) => self.done = Some(r),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.done = Some(Err("request thread ended".into()))
                }
            }
        }
        self.done.as_ref()
    }

    pub fn take(&mut self) -> Option<Result<T, String>> {
        self.poll();
        self.done.take()
    }
}

pub type Fetch = Job<String>;

impl Job<String> {
    pub fn get(url: impl Into<String>) -> Self {
        Fetch::get_as(url, USER_AGENT)
    }

    pub fn get_as(url: impl Into<String>, user_agent: &'static str) -> Self {
        let url = url.into();
        Job::spawn(move || fetch(&url, user_agent))
    }
}

fn fetch(url: &str, user_agent: &str) -> Result<String, String> {
    online()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();
    agent
        .get(url)
        .header("User-Agent", user_agent)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
}

pub fn download(
    url: &str,
    out: &mut impl std::io::Write,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    download_within(url, out, progress, DOWNLOAD_TIMEOUT)
}

pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn download_within(
    url: &str,
    out: &mut impl std::io::Write,
    mut progress: impl FnMut(u64, Option<u64>),
    timeout: Duration,
) -> Result<(), String> {
    online()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        let n = std::io::Read::read(&mut reader, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        progress(received, total);
    }
    out.flush().map_err(|e| e.to_string())
}

pub use html5gum;

pub fn html_tokens(html: &str) -> impl Iterator<Item = html5gum::Token> + '_ {
    let mut emitter = html5gum::DefaultEmitter::default();
    emitter.naively_switch_states(true);
    html5gum::Tokenizer::new_with_emitter(html, emitter).flatten()
}

pub fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut hidden = false;
    for token in html_tokens(html) {
        match token {
            html5gum::Token::StartTag(t) => hidden = matches!(&t.name[..], b"script" | b"style"),
            html5gum::Token::EndTag(_) => hidden = false,
            html5gum::Token::String(s) if !hidden => out.push_str(&String::from_utf8_lossy(&s)),
            _ => {}
        }
    }
    out.replace('\u{a0}', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_job_wakes_the_window() {
        if !zlog_core::wake::alone(concat!(module_path!(), "::a_finished_job_wakes_the_window")) {
            return;
        }
        let woken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let w = woken.clone();
        zlog_core::wake::set(move || w.store(true, Ordering::SeqCst));
        let (tx, rx) = channel::<()>();
        let mut job = Job::spawn(move || rx.recv().map_err(|e| e.to_string()));
        assert!(job.poll().is_none());
        tx.send(()).unwrap();
        let end = std::time::Instant::now() + Duration::from_secs(3);
        while !woken.load(Ordering::SeqCst) && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(woken.load(Ordering::SeqCst));
        assert!(matches!(job.take(), Some(Ok(()))));
    }

    #[test]
    fn offline_connects_refuse_remote_hosts_but_reach_loopback() {
        let _guard = OFFLINE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_offline(true);
        let before = remote_connects();
        let far = connect("192.0.2.1", 23, Duration::from_secs(5));
        let named = connect_address("cluster.example.org:7300", Duration::from_secs(5));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let near = connect("127.0.0.1", port, Duration::from_secs(5));
        let local = connect_address(&format!("localhost:{port}"), Duration::from_secs(5));
        set_offline(false);
        assert_eq!(far.unwrap_err(), OFFLINE_ERROR);
        assert_eq!(named.unwrap_err(), OFFLINE_ERROR);
        assert!(near.is_ok());
        assert!(local.is_ok());
        assert_eq!(remote_connects(), before);
    }

    #[test]
    fn offline_requests_fail_at_once() {
        let _lock = OFFLINE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_offline(true);
        let mut f = Fetch::get("http://192.0.2.1/");
        let started = std::time::Instant::now();
        let r = loop {
            if let Some(r) = f.poll() {
                break r.clone();
            }
            std::thread::yield_now();
        };
        let posted = exchange(&Request::post(
            "http://192.0.2.1/",
            "application/json",
            vec![],
        ));
        set_offline(false);
        assert_eq!(r, Err(OFFLINE_ERROR.to_string()));
        assert_eq!(posted, Err(OFFLINE_ERROR.to_string()));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn tags_are_removed() {
        assert_eq!(html_text("<td><P align=center> 11.5</td>"), " 11.5");
        assert_eq!(html_text("a&amp;b"), "a&b");
        assert_eq!(html_text("a&amp;lt;b&amp;gt;"), "a&lt;b&gt;");
        assert_eq!(html_text("&lt;&#x41;&#66;&amp;amp; & x"), "<AB&amp; & x");
        assert_eq!(html_text("<b>a&amp;lt;b</b>&nbsp;c"), "a&lt;b c");
        assert_eq!(html_text("a<script>x</script >b<STYLE>y</STYLE\n>c"), "abc");
    }

    #[test]
    fn a_stalled_download_times_out() {
        let _lock = OFFLINE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut s, &mut buf);
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nabc")
                .unwrap();
            std::thread::sleep(Duration::from_secs(10));
        });
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let r = download_within(
                &format!("http://{addr}/"),
                &mut out,
                |_, _| {},
                Duration::from_secs(1),
            );
            let _ = tx.send(r);
        });
        let r = rx
            .recv_timeout(Duration::from_secs(8))
            .expect("download hung");
        assert!(r.is_err());
        drop(server);
    }
}
