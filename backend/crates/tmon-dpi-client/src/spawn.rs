//! Locate and start the classifier with its end of a private socketpair
//! as fd 3 and a clean environment (contracts/dpi-ipc.md "Transport").

use command_fds::{CommandFdExt, FdMapping};
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};
use tokio_seqpacket::UnixSeqpacket;

use crate::settings::DpiSettings;

/// Environment variables passed through to the classifier.
const KEPT_ENV: [&str; 2] = ["RUST_LOG", "TZ"];
/// File descriptor number of the classifier's socket.
pub const CHILD_FD: i32 = 3;

fn executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Absolute path of `binary`: a path is checked as is, a bare name is
/// looked up in `PATH`. `None` when missing or not executable.
pub fn resolve(binary: &str) -> Option<PathBuf> {
    if binary.contains('/') {
        let p = PathBuf::from(binary);
        return executable(&p).then_some(p);
    }
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(binary))
        .find(|p| executable(p))
}

/// A running classifier and tmon's end of its socket.
pub struct Spawned {
    pub child: Child,
    pub sock: UnixSeqpacket,
}

pub fn spawn(path: &Path, settings: &DpiSettings) -> std::io::Result<Spawned> {
    let (ours, theirs) = UnixSeqpacket::pair()?;
    let theirs: OwnedFd = theirs.into();
    let mut cmd = Command::new(path);
    cmd.args(settings.args())
        .env_clear()
        .envs(
            KEPT_ENV
                .iter()
                .filter_map(|k| std::env::var(k).ok().map(|v| (*k, v))),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd.fd_mappings(vec![FdMapping {
        parent_fd: theirs,
        child_fd: CHILD_FD,
    }])
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    let child = cmd.spawn()?;
    Ok(Spawned { child, sock: ours })
}
