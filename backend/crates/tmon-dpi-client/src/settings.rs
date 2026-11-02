//! Validated classifier settings (from `[dpi]`, contracts/config.md).

/// Everything the supervisor and tap need. Built only from a validated
/// configuration; see `tmon::config_dpi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiSettings {
    /// Executable name looked up in `PATH`, or an absolute path.
    pub binary: String,
    pub max_flows: u32,
    pub max_packets_per_conn: u16,
    pub idle_timeout_ms: u64,
    pub memory_limit_mb: u32,
    pub queue_capacity: usize,
    pub queue_max_bytes: usize,
    /// Extra classifier arguments (tests only; not settable from config).
    pub extra_args: Vec<String>,
}

impl Default for DpiSettings {
    fn default() -> Self {
        Self {
            binary: "tmon-dpi".into(),
            max_flows: 50_000,
            max_packets_per_conn: 32,
            idle_timeout_ms: 900_000,
            memory_limit_mb: 192,
            queue_capacity: 16_384,
            queue_max_bytes: 16 << 20,
            extra_args: Vec::new(),
        }
    }
}

impl DpiSettings {
    /// Command-line arguments passed to the classifier (contracts/dpi-ipc.md).
    pub fn args(&self) -> Vec<String> {
        let mut args = vec![
            "--fd".into(),
            "3".into(),
            "--max-flows".into(),
            self.max_flows.to_string(),
            "--max-packets-per-conn".into(),
            self.max_packets_per_conn.to_string(),
            "--idle-timeout-ms".into(),
            self.idle_timeout_ms.to_string(),
            "--memory-limit-mb".into(),
            self.memory_limit_mb.to_string(),
        ];
        args.extend(self.extra_args.iter().cloned());
        args
    }

    /// `AppTable` capacity: twice the classifier's flow table.
    pub fn app_table_capacity(&self) -> usize {
        (self.max_flows as usize).saturating_mul(2)
    }
}
