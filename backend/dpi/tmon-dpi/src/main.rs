//! `tmon-dpi`: the application classifier sidecar. Started by tmon with the
//! protocol socket as fd 3 (contracts/dpi-ipc.md); not meant to be run by hand.

use std::process::ExitCode;
use tmon_dpi::args::{self, SelfTest};
use tmon_dpi::flows::Flows;
use tmon_dpi::ipc::{self, Seqpacket};
use tmon_dpi::ndpi::Module;
use tmon_dpi::{alloc, sandbox};
use tmon_dpi_proto::{Hello, PROTO_VERSION};

fn log(msg: impl std::fmt::Display) {
    eprintln!("tmon-dpi: {msg}");
}

/// Test-only: misbehave after sandboxing; the sandbox must stop it.
fn self_test(t: Option<SelfTest>) {
    match t {
        Some(SelfTest::Network) => {
            // SAFETY: plain socket(2) call; expected to be killed by seccomp.
            let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            log(format!("self-test network: socket returned {fd}"));
        }
        Some(SelfTest::Open) => {
            let r = std::fs::read("/etc/passwd");
            log(format!(
                "self-test open: {}",
                if r.is_ok() { "ALLOWED" } else { "denied" }
            ));
        }
        None => {}
    }
}

fn main() -> ExitCode {
    if !sandbox::die_with_parent() {
        log("cannot follow tmon's lifetime (PR_SET_PDEATHSIG failed)");
        return ExitCode::from(1);
    }
    let args = match args::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            log(e);
            return ExitCode::from(2);
        }
    };
    alloc::install();
    let module = match Module::new() {
        Ok(m) => m,
        Err(e) => {
            log(e);
            return ExitCode::from(1);
        }
    };
    let hello = Hello {
        proto_version: PROTO_VERSION,
        impl_name: "tmon-dpi".into(),
        impl_version: env!("CARGO_PKG_VERSION").into(),
        engine: Module::version(),
        max_flows: args.max_flows,
        catalog: module.catalog(),
    };
    // SAFETY: tmon passes the protocol socket as `--fd` (3) and nothing else
    // in this process owns it.
    let mut sock = match unsafe { Seqpacket::from_fd(args.fd) } {
        Ok(s) => s,
        Err(e) => {
            log(format!("fd {}: {e}", args.fd));
            return ExitCode::from(1);
        }
    };
    let mut flows = Flows::new(module, args.limits());
    for step in sandbox::apply(args.memory_limit_mb) {
        match &step.outcome {
            Ok(how) => log(format!("sandbox {}: {how}", step.name)),
            Err(e) => log(format!("sandbox {}: unavailable ({e})", step.name)),
        }
        if step.failed() {
            log("refusing to classify without the sandbox");
            return ExitCode::from(1);
        }
    }
    self_test(args.self_test);
    log(format!(
        "{} ready, {} applications",
        hello.engine,
        hello.catalog.len()
    ));
    let served = ipc::hello(&mut sock, &hello).and_then(|_| ipc::run(&mut sock, &mut flows));
    match served {
        Ok(end) => {
            log(format!("exiting: {end:?}"));
            ExitCode::SUCCESS
        }
        Err(e) => {
            log(format!("socket error: {e}"));
            ExitCode::from(1)
        }
    }
}
