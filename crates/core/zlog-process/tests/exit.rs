#![cfg(unix)]

use std::process::Stdio;
use std::time::Duration;

use zlog_process::{exiting, running, shut_down, Process, Spec};

fn sleeper() -> Spec {
    Spec {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "exec sleep 30".into()],
        memory: 0,
        timeout: Duration::from_secs(5),
        greeting: String::new(),
    }
}

#[test]
fn no_helper_starts_or_stays_once_zlog_is_quitting() {
    let (first, _stdin, _stdout) = Process::spawn(&sleeper(), Stdio::null()).unwrap();
    let queued: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| Process::spawn(&sleeper(), Stdio::null()).map(|(p, _, _)| p))
        })
        .collect();
    shut_down();
    assert!(exiting());
    let late = Process::spawn(&sleeper(), Stdio::null());
    assert_eq!(late.err().unwrap().to_string(), "zLog is quitting");
    for q in queued {
        drop(q.join().unwrap());
    }
    assert_eq!(running(), [first.id()]);
}
