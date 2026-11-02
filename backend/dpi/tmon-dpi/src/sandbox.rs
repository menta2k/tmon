//! Self-restriction after nDPI initialization (research R8, FR-023):
//! no new privileges, drop root, Landlock deny-all, a seccomp allow-list
//! (no network, no files, no executable memory), and an address-space
//! limit. Every step reports whether it was applied. The sandbox fails
//! closed: only Landlock (kernel-dependent, and redundant with seccomp's
//! ban on `open*`) may be unavailable; any other failure stops the sidecar
//! before it reads a packet.

use std::collections::BTreeMap;

/// Kill this process when tmon dies (first thing in `main`). A parent that
/// died even earlier is caught by the socket: tmon's end is then closed and
/// the first read sees EOF. (No `getppid() == 1` check: in a container tmon
/// itself is PID 1.)
pub fn die_with_parent() -> bool {
    // SAFETY: prctl with integer arguments only.
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) == 0 }
}

/// Syscalls the I/O loop needs after initialization. Deliberately absent:
/// `sendmsg`/`recvmsg` (fd passing), `kill`/`tgkill` (a compromised sidecar
/// running as tmon's uid could signal tmon; `abort` still ends the process
/// via the default action), and everything that opens files or sockets.
const ALLOWED: &[libc::c_long] = &[
    libc::SYS_recvfrom,
    libc::SYS_sendto,
    libc::SYS_read,
    libc::SYS_write,
    libc::SYS_close,
    libc::SYS_munmap,
    libc::SYS_mremap,
    libc::SYS_brk,
    libc::SYS_madvise,
    libc::SYS_futex,
    libc::SYS_clock_gettime,
    libc::SYS_getrandom,
    libc::SYS_rt_sigreturn,
    libc::SYS_rt_sigprocmask,
    libc::SYS_sigaltstack,
    libc::SYS_exit,
    libc::SYS_exit_group,
];

/// File opens fail with EACCES instead of killing the process: the C
/// library may lazily open files while nDPI runs (e.g. the timezone database
/// on the first time conversion) and copes with the error, so killing would
/// crash-loop the sidecar on real traffic. Access stays denied (Landlock
/// denies it too); only the failure mode differs.
#[cfg(target_arch = "x86_64")]
const OPEN_DENIED: &[libc::c_long] = &[libc::SYS_open, libc::SYS_openat, libc::SYS_openat2];
#[cfg(not(target_arch = "x86_64"))]
const OPEN_DENIED: &[libc::c_long] = &[libc::SYS_openat, libc::SYS_openat2];

/// `mmap`/`mprotect`, allowed only without `PROT_EXEC` (W^X: nDPI does not
/// generate code, so shellcode cannot be staged).
const NO_EXEC: &[libc::c_long] = &[libc::SYS_mmap, libc::SYS_mprotect];

/// One sandbox step and its outcome, for the startup log.
pub struct Step {
    pub name: &'static str,
    /// A failure must stop the sidecar.
    pub required: bool,
    pub outcome: Result<&'static str, String>,
}

impl Step {
    /// A required step that did not apply.
    pub fn failed(&self) -> bool {
        self.required && self.outcome.is_err()
    }
}

fn os_err(rc: libc::c_int) -> Result<(), String> {
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

fn no_new_privs() -> Result<&'static str, String> {
    // SAFETY: prctl with integer arguments only.
    os_err(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) }).map(|_| "applied")
}

/// Drop to `nobody` (65534) when started as root.
fn drop_root() -> Result<&'static str, String> {
    // SAFETY: plain getter.
    if unsafe { libc::geteuid() } != 0 {
        return Ok("not root");
    }
    const NOBODY: libc::uid_t = 65_534;
    // SAFETY: setgroups with an empty list; setgid/setuid with plain ids.
    unsafe {
        os_err(libc::setgroups(0, std::ptr::null()))?;
        os_err(libc::setgid(NOBODY))?;
        os_err(libc::setuid(NOBODY))?;
        if libc::setuid(0) == 0 || libc::geteuid() == 0 || libc::getegid() == 0 {
            return Err("root could be regained".into());
        }
    }
    Ok("applied")
}

fn landlock_deny_all() -> Result<&'static str, String> {
    use landlock::{ABI, Access, AccessFs, Ruleset, RulesetAttr, RulesetStatus};
    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(ABI::V1))
        .and_then(|r| r.create())
        .and_then(|r| r.restrict_self())
        .map_err(|e| e.to_string())?;
    Ok(match status.ruleset {
        RulesetStatus::FullyEnforced => "applied",
        RulesetStatus::PartiallyEnforced => "partially applied",
        RulesetStatus::NotEnforced => "unavailable (kernel)",
    })
}

fn no_exec_rule() -> Result<Vec<seccompiler::SeccompRule>, String> {
    use seccompiler::{SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompRule};
    let prot_exec = u64::try_from(libc::PROT_EXEC).map_err(|e| e.to_string())?;
    let cond = SeccompCondition::new(
        2,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::MaskedEq(prot_exec),
        0,
    )
    .map_err(|e| e.to_string())?;
    Ok(vec![
        SeccompRule::new(vec![cond]).map_err(|e| e.to_string())?,
    ])
}

/// `fcntl(fd, F_GETFD)` only: Rust's std checks a descriptor is still open
/// before closing it in debug builds; any other fcntl command stays denied.
fn fcntl_getfd_rule() -> Result<Vec<seccompiler::SeccompRule>, String> {
    use seccompiler::{SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompRule};
    let getfd = u64::try_from(libc::F_GETFD).map_err(|e| e.to_string())?;
    let cond = SeccompCondition::new(1, SeccompCmpArgLen::Dword, SeccompCmpOp::Eq, getfd)
        .map_err(|e| e.to_string())?;
    Ok(vec![
        SeccompRule::new(vec![cond]).map_err(|e| e.to_string())?,
    ])
}

fn install(
    rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>>,
    mismatch: seccompiler::SeccompAction,
    matched: seccompiler::SeccompAction,
) -> Result<(), String> {
    use seccompiler::{BpfProgram, SeccompFilter};
    let arch = std::env::consts::ARCH
        .try_into()
        .map_err(|e| format!("{e:?}"))?;
    let filter = SeccompFilter::new(rules, mismatch, matched, arch).map_err(|e| e.to_string())?;
    let prog: BpfProgram = filter
        .try_into()
        .map_err(|e: seccompiler::BackendError| e.to_string())?;
    seccompiler::apply_filter(&prog).map_err(|e| e.to_string())
}

/// Two stacked filters (the kernel applies the strictest verdict): first
/// "file opens → EACCES", then the allow-list, which lets the open calls
/// through to that verdict and kills on everything else not listed.
fn seccomp_allow_list() -> Result<&'static str, String> {
    use seccompiler::SeccompAction;
    let eacces = u32::try_from(libc::EACCES).map_err(|e| e.to_string())?;
    let opens = || OPEN_DENIED.iter().map(|s| (*s, Vec::new()));
    install(
        opens().collect(),
        SeccompAction::Allow,
        SeccompAction::Errno(eacces),
    )?;
    let mut rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = ALLOWED
        .iter()
        .map(|s| (*s, Vec::new()))
        .chain(opens())
        .collect();
    for s in NO_EXEC {
        rules.insert(*s, no_exec_rule()?);
    }
    rules.insert(libc::SYS_fcntl, fcntl_getfd_rule()?);
    install(rules, SeccompAction::KillProcess, SeccompAction::Allow)?;
    Ok("applied")
}

/// Address space beyond what is mapped at sandbox time plus the flow budget
/// (malloc arenas, stacks, nDPI's per-flow scratch).
const AS_HEADROOM_MB: u64 = 128;
/// Allowance on top of the budget when the current size is unknown.
const AS_FALLBACK_MB: u64 = 1024;

/// `RLIMIT_AS` value: everything mapped now (binary, libndpi, nDPI's tables)
/// plus the flow budget and headroom. A cap of only `limit + 64 MB` made
/// malloc fail long before the flow table's budget could evict anything.
fn address_space_bytes(current: Option<u64>, memory_limit_mb: u32) -> u64 {
    let limit = u64::from(memory_limit_mb) << 20;
    match current {
        Some(now) => now + limit + (AS_HEADROOM_MB << 20),
        None => limit + (AS_FALLBACK_MB << 20),
    }
}

/// First field of `/proc/self/statm` (total program size in pages) → bytes.
fn parse_statm(statm: &str, page: u64) -> Option<u64> {
    let pages: u64 = statm.split_whitespace().next()?.parse().ok()?;
    pages.checked_mul(page)
}

/// Current address-space size; read before landlock hides `/proc`.
fn current_address_space() -> Option<u64> {
    // SAFETY: sysconf has no preconditions.
    let page = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
    parse_statm(&std::fs::read_to_string("/proc/self/statm").ok()?, page)
}

fn limit_address_space(bytes: u64) -> Result<&'static str, String> {
    let lim = libc::rlimit {
        rlim_cur: bytes,
        rlim_max: bytes,
    };
    // SAFETY: `lim` is a valid rlimit.
    os_err(unsafe { libc::setrlimit(libc::RLIMIT_AS, &lim) }).map(|_| "applied")
}

type StepFn<'a> = dyn Fn() -> Result<&'static str, String> + 'a;

/// Apply the steps in order, stopping at the first required one that
/// fails (the caller must then exit without serving).
pub fn apply(memory_limit_mb: u32) -> Vec<Step> {
    let as_bytes = address_space_bytes(current_address_space(), memory_limit_mb);
    let steps: [(&str, bool, &StepFn); 5] = [
        ("no_new_privs", true, &no_new_privs),
        ("drop_root", true, &drop_root),
        ("landlock", false, &landlock_deny_all),
        ("rlimit_as", true, &|| limit_address_space(as_bytes)),
        // Last: after it, only the I/O loop's syscalls are possible.
        ("seccomp", true, &seccomp_allow_list),
    ];
    let mut out = Vec::with_capacity(steps.len());
    for (name, required, run) in steps {
        let step = Step {
            name,
            required,
            outcome: run(),
        };
        let stop = step.failed();
        out.push(step);
        if stop {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_space_allows_what_is_mapped_plus_limit_and_headroom() {
        const MB: u64 = 1 << 20;
        // nDPI's tables and the libraries are already mapped at sandbox time.
        assert_eq!(
            address_space_bytes(Some(300 * MB), 32),
            (300 + 32 + 128) * MB
        );
        // Unknown current size: a generous fixed allowance instead.
        assert_eq!(address_space_bytes(None, 32), (32 + 1024) * MB);
    }

    #[test]
    fn statm_size_is_parsed_in_pages() {
        assert_eq!(parse_statm("1000 200 30 4 0 50 0\n", 4096), Some(4_096_000));
        assert_eq!(parse_statm("garbage", 4096), None);
    }
}
