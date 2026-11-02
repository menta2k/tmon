//! Protocol v1 on the inherited SOCK_SEQPACKET socket (contracts/dpi-ipc.md):
//! Hello first, then packets in and Classifications out, Stats every 2 s as
//! the heartbeat, Pong for Ping, and exit on EOF. Blocking and
//! single-threaded, so the seccomp allow-list stays small.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tmon_dpi_proto::{FromClassifier, Hello, MAX_MESSAGE, ToClassifier};

use crate::engine::Engine;
use crate::flows::Flows;

/// Heartbeat period (tmon restarts the classifier after 6 s of silence).
pub const STATS_EVERY: Duration = Duration::from_secs(2);
/// How often idle flows are swept.
pub const SWEEP_EVERY: Duration = Duration::from_secs(10);
/// Receive timeout, so Stats go out while no packets arrive.
pub const RECV_TIMEOUT: Duration = Duration::from_millis(200);

/// A message-preserving duplex channel.
pub trait Transport {
    /// `Ok(Some(0))` is EOF, `Ok(None)` a timeout.
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>>;
    fn send(&mut self, msg: &[u8]) -> io::Result<()>;
}

/// The socket tmon passes as fd 3.
pub struct Seqpacket {
    fd: OwnedFd,
}

impl Seqpacket {
    /// Take ownership of `fd`, make it blocking (tmon's end of the pair is
    /// created non-blocking and the flag is shared by the child's copy) and
    /// set the receive timeout the loop's ticks rely on.
    ///
    /// # Safety
    /// `fd` must be an open socket that nothing else in this process owns.
    pub unsafe fn from_fd(fd: RawFd) -> io::Result<Self> {
        // SAFETY: the caller guarantees `fd` is an open, unowned socket.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        // SAFETY: F_GETFL on an open descriptor.
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: F_SETFL on an open descriptor with integer flags.
        let rc = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        let tv = libc::timeval {
            tv_sec: RECV_TIMEOUT.as_secs() as libc::time_t,
            tv_usec: RECV_TIMEOUT.subsec_micros() as libc::suseconds_t,
        };
        // SAFETY: `tv` is a valid timeval and the length matches its type.
        let rc = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                (&raw const tv).cast(),
                size_of::<libc::timeval>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }
}

impl AsRawFd for Seqpacket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl Transport for Seqpacket {
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        // SAFETY: `buf` is valid for writes of `buf.len()` bytes.
        let n = unsafe { libc::recv(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n >= 0 {
            return Ok(Some(n as usize));
        }
        let e = io::Error::last_os_error();
        match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted => {
                Ok(None)
            }
            _ => Err(e),
        }
    }

    fn send(&mut self, msg: &[u8]) -> io::Result<()> {
        loop {
            // SAFETY: `msg` is valid for reads of `msg.len()` bytes;
            // MSG_NOSIGNAL turns a closed peer into EPIPE instead of SIGPIPE.
            let n = unsafe {
                libc::send(
                    self.fd.as_raw_fd(),
                    msg.as_ptr().cast(),
                    msg.len(),
                    libc::MSG_NOSIGNAL,
                )
            };
            if n >= 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}

/// Send Hello (and HelloCatalog continuations).
pub fn hello(t: &mut impl Transport, h: &Hello) -> io::Result<()> {
    h.to_messages().iter().try_for_each(|m| t.send(m))
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Why the loop ended.
#[derive(Debug, PartialEq, Eq)]
pub enum End {
    /// tmon closed the socket.
    Eof,
    /// tmon went away while we were sending.
    PeerGone,
}

/// Serve until EOF. Invalid messages are counted, never fatal (FR-024: no
/// payload is ever logged).
pub fn run<E: Engine>(t: &mut impl Transport, flows: &mut Flows<E>) -> io::Result<End> {
    let started = Instant::now();
    let (mut next_stats, mut next_sweep) = (started, started + SWEEP_EVERY);
    let mut buf = vec![0u8; MAX_MESSAGE + 1];
    loop {
        let now = Instant::now();
        if now >= next_stats {
            let s = flows.stats(started.elapsed().as_millis() as u64);
            if send(t, &FromClassifier::Stats(s).encode())?.is_some() {
                return Ok(End::PeerGone);
            }
            next_stats = now + STATS_EVERY;
        }
        if now >= next_sweep {
            flows.sweep(wall_ms());
            next_sweep = now + SWEEP_EVERY;
        }
        let n = match t.recv(&mut buf)? {
            None => continue,
            Some(0) => return Ok(End::Eof),
            Some(n) => n,
        };
        let reply = match ToClassifier::decode(&buf[..n]) {
            Ok(ToClassifier::Packet(p)) => flows.packet(&p).map(FromClassifier::Classification),
            Ok(ToClassifier::Forget(k)) => {
                flows.forget(&k);
                None
            }
            Ok(ToClassifier::Ping(x)) => Some(FromClassifier::Pong(x)),
            Err(_) => {
                flows.invalid();
                None
            }
        };
        if let Some(m) = reply {
            if send(t, &m.encode())?.is_some() {
                return Ok(End::PeerGone);
            }
        }
    }
}

/// `Ok(Some(()))` when the peer is gone (EPIPE / ECONNRESET).
fn send(t: &mut impl Transport, msg: &[u8]) -> io::Result<Option<()>> {
    match t.send(msg) {
        Ok(()) => Ok(None),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
            ) =>
        {
            Ok(Some(()))
        }
        Err(e) => Err(e),
    }
}
