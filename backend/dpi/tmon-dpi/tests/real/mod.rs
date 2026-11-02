#![allow(dead_code)]
//! Drive the built `tmon-dpi` binary over a socketpair as fd 3 (nDPI only).

use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tmon_capture::fixtures::apps::{self, AppExpect};
use tmon_dpi::ipc::{Seqpacket, Transport};
use tmon_dpi_proto::{
    Classification, Dir, FromClassifier, Hello, Key, MAX_MESSAGE, Packet, Stats, ToClassifier,
};

pub struct Sidecar {
    pub child: Child,
    pub sock: Seqpacket,
    pub hello: Hello,
}

fn pair() -> (OwnedFd, OwnedFd) {
    let mut fds = [0; 2];
    // SAFETY: `fds` has room for the two descriptors socketpair writes.
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, fds.as_mut_ptr()) };
    assert_eq!(rc, 0);
    // SAFETY: both descriptors were just created and are owned here only.
    unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
}

impl Sidecar {
    /// Start the sidecar without waiting for its Hello (for runs that are
    /// expected to die during startup).
    pub fn start(extra: &[&str]) -> (Child, Seqpacket) {
        let (ours, theirs) = pair();
        let fd = theirs.as_raw_fd();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tmon-dpi"));
        cmd.args(["--fd", "3"]).args(extra).stderr(Stdio::piped());
        // SAFETY: only async-signal-safe dup2 between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                if libc::dup2(fd, 3) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn().expect("spawn tmon-dpi");
        drop(theirs);
        // SAFETY: `ours` is an open socket handed to the transport.
        let sock = unsafe { Seqpacket::from_fd(ours.into_raw_fd()) }.unwrap();
        (child, sock)
    }

    pub fn spawn(extra: &[&str]) -> Self {
        let (child, mut sock) = Self::start(extra);
        let mut hello = match recv(&mut sock, Duration::from_secs(10)) {
            Some(FromClassifier::Hello(h)) => h,
            m => panic!("expected Hello, got {m:?}"),
        };
        // A large catalog continues in HelloCatalog messages sent right after
        // the Hello (contracts/dpi-ipc.md); nothing else is sent before them.
        while let Some(m) = recv(&mut sock, Duration::from_millis(300)) {
            match m {
                FromClassifier::HelloCatalog(more) => hello.catalog.extend(more),
                // A Stats heartbeat ends the catalog; later calls wait for the next one.
                FromClassifier::Stats(_) => break,
                other => panic!("unexpected message after Hello: {other:?}"),
            }
        }
        Self { child, sock, hello }
    }

    pub fn send(&mut self, m: &ToClassifier) {
        self.sock.send(&m.encode()).expect("send to tmon-dpi");
    }

    /// Discard whatever the sidecar has already sent (non-blocking). Bulk
    /// senders call it so neither side blocks on a full socket buffer.
    pub fn drain(&mut self) -> usize {
        let mut buf = vec![0; MAX_MESSAGE + 1];
        let mut n = 0;
        while let Ok(Some(len)) = self.sock.recv(&mut buf) {
            if len == 0 {
                break;
            }
            n += 1;
        }
        n
    }

    /// Classifications until `quiet` passes without one.
    pub fn classifications(&mut self, quiet: Duration) -> Vec<Classification> {
        let mut out = Vec::new();
        while let Some(m) = recv(&mut self.sock, quiet) {
            if let FromClassifier::Classification(c) = m {
                out.push(c);
            }
        }
        out
    }

    pub fn stats(&mut self) -> Stats {
        loop {
            match recv(&mut self.sock, Duration::from_secs(5)) {
                Some(FromClassifier::Stats(s)) => return s,
                Some(_) => {}
                None => panic!("no Stats heartbeat"),
            }
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn recv(sock: &mut Seqpacket, within: Duration) -> Option<FromClassifier> {
    let deadline = Instant::now() + within;
    let mut buf = vec![0; MAX_MESSAGE + 1];
    while Instant::now() < deadline {
        match sock.recv(&mut buf) {
            Ok(Some(0)) | Err(_) => return None,
            Ok(Some(n)) => return Some(FromClassifier::decode(&buf[..n]).expect("valid message")),
            Ok(None) => {}
        }
    }
    None
}

/// The protocol packets of one fixture scenario, oriented on its client.
pub fn packets(e: &AppExpect) -> Vec<ToClassifier> {
    let client: std::net::SocketAddr = e.client.parse().unwrap();
    apps::datagrams(e)
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let p = tmon_capture::frame::decode(d).expect("fixture decodes");
            let out = p.src_ip == client.ip() && p.src_port == client.port();
            let (lan, remote) = if out {
                ((p.src_ip, p.src_port), (p.dst_ip, p.dst_port))
            } else {
                ((p.dst_ip, p.dst_port), (p.src_ip, p.src_port))
            };
            ToClassifier::Packet(Packet {
                key: Key {
                    lan_ip: lan.0,
                    lan_port: lan.1,
                    remote_ip: remote.0,
                    remote_port: remote.1,
                    proto: p.proto(),
                },
                dir: if out {
                    Dir::LanToRemote
                } else {
                    Dir::RemoteToLan
                },
                ts_ms: 1_767_225_600_000 + i as u64,
                l3: p.l3.to_vec(),
            })
        })
        .collect()
}
