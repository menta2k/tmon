//! Command line: the keys tmon passes (`DpiSettings::args`) plus test-only
//! sandbox self-tests. Unknown keys are an error (exit 2).

use crate::flows::Limits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfTest {
    /// Call `socket()` after sandboxing (must die with SIGSYS).
    Network,
    /// `open("/etc/passwd")` after sandboxing (must be denied).
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub fd: i32,
    pub max_flows: u32,
    pub max_packets_per_conn: u16,
    pub idle_timeout_ms: u64,
    pub memory_limit_mb: u32,
    pub self_test: Option<SelfTest>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            fd: 3,
            max_flows: 50_000,
            max_packets_per_conn: 32,
            idle_timeout_ms: 15 * 60_000,
            memory_limit_mb: 192,
            self_test: None,
        }
    }
}

impl Args {
    pub fn limits(&self) -> Limits {
        Limits {
            max_flows: self.max_flows as usize,
            max_packets: self.max_packets_per_conn,
            idle_timeout_ms: self.idle_timeout_ms,
            mem_limit_bytes: u64::from(self.memory_limit_mb) << 20,
        }
    }
}

fn num<T: std::str::FromStr>(key: &str, v: Option<String>, range: (u64, u64)) -> Result<T, String> {
    let v = v.ok_or_else(|| format!("{key}: missing value"))?;
    let n: u64 = v.parse().map_err(|_| format!("{key}: not a number: {v}"))?;
    if n < range.0 || n > range.1 {
        return Err(format!(
            "{key}: must be between {} and {}",
            range.0, range.1
        ));
    }
    v.parse().map_err(|_| format!("{key}: out of range"))
}

/// Parse `--key value` pairs (ranges as in contracts/config.md).
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.into_iter();
    while let Some(k) = it.next() {
        match k.as_str() {
            "--fd" => a.fd = num(&k, it.next(), (0, 1_024))?,
            "--max-flows" => a.max_flows = num(&k, it.next(), (1, 1_000_000))?,
            "--max-packets-per-conn" => a.max_packets_per_conn = num(&k, it.next(), (1, 256))?,
            "--idle-timeout-ms" => a.idle_timeout_ms = num(&k, it.next(), (1_000, 3_600_000))?,
            "--memory-limit-mb" => a.memory_limit_mb = num(&k, it.next(), (16, 2_048))?,
            "--self-test-network" => a.self_test = Some(SelfTest::Network),
            "--self-test-open" => a.self_test = Some(SelfTest::Open),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &[&str]) -> Result<Args, String> {
        parse(s.iter().map(|x| x.to_string()))
    }

    #[test]
    fn parses_what_tmon_passes() {
        let a = p(&[
            "--fd",
            "3",
            "--max-flows",
            "1000",
            "--max-packets-per-conn",
            "16",
            "--idle-timeout-ms",
            "60000",
            "--memory-limit-mb",
            "64",
        ])
        .unwrap();
        assert_eq!((a.fd, a.max_flows, a.max_packets_per_conn), (3, 1000, 16));
        assert_eq!(a.limits().mem_limit_bytes, 64 << 20);
        assert_eq!(p(&[]).unwrap(), Args::default());
    }

    #[test]
    fn rejects_unknown_and_out_of_range() {
        assert!(p(&["--bogus"]).is_err());
        assert!(p(&["--max-flows"]).is_err());
        assert!(p(&["--max-flows", "0"]).is_err());
        assert!(p(&["--memory-limit-mb", "x"]).is_err());
        assert_eq!(
            p(&["--self-test-open"]).unwrap().self_test,
            Some(SelfTest::Open)
        );
    }
}
