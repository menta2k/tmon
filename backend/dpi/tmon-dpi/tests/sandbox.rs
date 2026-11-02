//! The sidecar's self-restriction (feature 003, T069 / FR-023).
#![cfg(feature = "ndpi")]

mod real;

use real::Sidecar;
use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::time::{Duration, Instant};

fn proc_status(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap()
}

fn wait_exit(s: &mut Sidecar) -> std::process::ExitStatus {
    wait_child(&mut s.child)
}

fn wait_child(child: &mut std::process::Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st;
        }
        assert!(Instant::now() < deadline, "sidecar still running");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn stderr(s: &mut Sidecar) -> String {
    let mut out = String::new();
    if let Some(e) = s.child.stderr.as_mut() {
        let _ = e.read_to_string(&mut out);
    }
    out
}

#[test]
fn running_sidecar_has_no_new_privs_and_seccomp() {
    let s = Sidecar::spawn(&[]);
    let st = proc_status(s.child.id());
    assert!(st.contains("NoNewPrivs:\t1"), "{st}");
    assert!(st.contains("Seccomp:\t2"), "{st}");
}

#[test]
fn network_access_kills_the_sidecar() {
    // The self-test runs before the Hello, so do not wait for one.
    let (mut child, _sock) = Sidecar::start(&["--self-test-network"]);
    let st = wait_child(&mut child);
    assert_eq!(st.signal(), Some(libc::SIGSYS), "{st:?}");
}

#[test]
fn file_access_is_denied_without_killing() {
    // seccomp answers openat with EACCES (and Landlock denies it too): the
    // sidecar must survive the denied open and keep serving.
    let mut s = Sidecar::spawn(&["--self-test-open"]);
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert!(
        s.child.try_wait().unwrap().is_none(),
        "sidecar died on a denied open"
    );
    let _ = s.child.kill();
    let _ = s.child.wait();
    let log = stderr(&mut s);
    assert!(log.contains("self-test open: denied"), "{log}");
    assert!(!log.contains("ALLOWED"));
}

#[test]
fn startup_log_lists_every_restriction() {
    let mut s = Sidecar::spawn(&[]);
    // EOF on the protocol socket makes the sidecar exit and close stderr.
    // SAFETY: shutdown(2) on a socket we own; the descriptor stays valid.
    unsafe { libc::shutdown(std::os::fd::AsRawFd::as_raw_fd(&s.sock), libc::SHUT_RDWR) };
    let st = wait_exit(&mut s);
    let log = stderr(&mut s);
    assert!(st.success(), "{st:?}\n{log}");
    for step in [
        "no_new_privs",
        "drop_root",
        "landlock",
        "rlimit_as",
        "seccomp",
    ] {
        assert!(
            log.contains(&format!("sandbox {step}:")),
            "{step} not logged:\n{log}"
        );
    }
}
