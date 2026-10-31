use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

type Hook = Arc<dyn Fn() + Send + Sync>;

static HOOK: RwLock<Option<Hook>> = RwLock::new(None);

pub fn set(hook: impl Fn() + Send + Sync + 'static) {
    *HOOK.write().unwrap() = Some(Arc::new(hook));
}

pub fn wake() {
    let hook = HOOK.read().unwrap().clone();
    if let Some(h) = hook {
        h();
    }
}

pub const ALONE_LIMIT: Duration = Duration::from_secs(120);

pub fn alone(test: &str) -> bool {
    alone_within(test, ALONE_LIMIT)
}

pub fn alone_within(test: &str, limit: Duration) -> bool {
    const ALONE: &str = "ZLOG_WAKE_ALONE";
    if std::env::var_os(ALONE).is_some() {
        return true;
    }
    let name = test.split_once("::").map_or(test, |(_, t)| t);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1"])
        .env(ALONE, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let drain = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = r.read_to_string(&mut s);
            s
        })
    };
    let stdout = drain(Box::new(child.stdout.take().unwrap()));
    let stderr = drain(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (stdout, stderr) = (stdout.join().unwrap(), stderr.join().unwrap());
    let Some(status) = status else {
        panic!("{test} did not finish within {limit:?}\n{stdout}{stderr}");
    };
    assert!(
        status.success() && stdout.contains("test result: ok. 1 passed"),
        "{stdout}{stderr}"
    );
    false
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn a_wake_calls_the_hook_of_the_host() {
        if !alone(concat!(
            module_path!(),
            "::a_wake_calls_the_hook_of_the_host"
        )) {
            return;
        }
        wake();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        set(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        wake();
        std::thread::spawn(wake).join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_stuck_test_is_killed_after_the_limit() {
        if std::env::var_os("ZLOG_WAKE_ALONE").is_some() {
            std::thread::sleep(Duration::from_secs(60));
            return;
        }
        let start = Instant::now();
        let stuck = std::panic::catch_unwind(|| {
            alone_within(
                concat!(module_path!(), "::a_stuck_test_is_killed_after_the_limit"),
                Duration::from_millis(500),
            )
        });
        assert!(stuck.is_err());
        assert!(start.elapsed() < Duration::from_secs(30));
    }
}
