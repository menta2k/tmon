use std::io::Write;
use std::sync::Mutex;
use tmon::config::{Config, ConfigError};

/// Env vars are process-global: serialize tests that touch them.
static ENV: Mutex<()> = Mutex::new(());

fn toml_file(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let p = dir.join("tmon.toml");
    std::fs::File::create(&p)
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    p
}

fn base(dir: &std::path::Path) -> String {
    format!(
        "data_dir = {:?}\n[http]\npassword_hash = \"$argon2id$v=19$m=19456,t=2,p=1$abc$def\"\n",
        dir.join("data")
    )
}

#[test]
fn empty_password_hash_is_rejected_with_hint() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p = toml_file(dir.path(), &format!("data_dir = {:?}\n", dir.path()));
    let err = Config::load(Some(&p)).unwrap().validate().unwrap_err();
    assert!(err.to_string().contains("tmon hash-password"), "{err}");
}

#[test]
fn invalid_listen_address_is_rejected() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{}listen = \"nope:99999\"\n", base(dir.path()));
    let err = Config::load(Some(&toml_file(dir.path(), &body)))
        .unwrap()
        .validate()
        .unwrap_err();
    assert!(matches!(err, ConfigError::Invalid(m) if m.contains("http.listen")));
}

#[test]
fn non_writable_data_dir_is_rejected() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("file");
    std::fs::write(&blocker, b"x").unwrap();
    let body = base(dir.path()).replacen(
        &format!("{:?}", dir.path().join("data")),
        &format!("{:?}", blocker.join("sub")),
        1,
    );
    let err = Config::load(Some(&toml_file(dir.path(), &body)))
        .unwrap()
        .validate()
        .unwrap_err();
    assert!(err.to_string().contains("not writable"), "{err}");
}

#[test]
fn env_overrides_file() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{}[retention]\nraw_days = 9\n", base(dir.path()));
    let p = toml_file(dir.path(), &body);
    unsafe { std::env::set_var("TMON_RETENTION__RAW_DAYS", "3") };
    let cfg = Config::load(Some(&p));
    unsafe { std::env::remove_var("TMON_RETENTION__RAW_DAYS") };
    assert_eq!(cfg.unwrap().retention.raw_days, 3);
}

#[test]
fn missing_geoip_is_only_a_warning() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let body = format!(
        "{}[geoip]\ncountry_db = \"/nonexistent/c.mmdb\"\n",
        base(dir.path())
    );
    let v = Config::load(Some(&toml_file(dir.path(), &body)))
        .unwrap()
        .validate()
        .unwrap();
    assert!(v.warnings.iter().any(|w| w.contains("geoip-update")));
    assert_eq!(v.netflow_listen.len(), 2);
    assert_eq!(v.config.retention.rollup_1h_days, 365);
}

#[test]
fn unknown_keys_and_missing_file_fail_to_load() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{}bogus = 1\n", base(dir.path()));
    assert!(matches!(
        Config::load(Some(&toml_file(dir.path(), &body))),
        Err(ConfigError::Load(_))
    ));
    assert!(Config::load(Some(&dir.path().join("missing.toml"))).is_err());
}

#[test]
fn bad_exporter_and_retention_values() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let body = format!(
        "{}[collector]\nallowed_exporters = [\"x\"]\n",
        base(dir.path())
    );
    assert!(
        Config::load(Some(&toml_file(dir.path(), &body)))
            .unwrap()
            .validate()
            .is_err()
    );
    let body = format!("{}[retention]\nraw_days = 0\n", base(dir.path()));
    assert!(
        Config::load(Some(&toml_file(dir.path(), &body)))
            .unwrap()
            .validate()
            .is_err()
    );
}

fn capture_err(dir: &std::path::Path, section: &str) -> String {
    let body = format!("{}[capture]\nenabled = true\n{section}", base(dir));
    Config::load(Some(&toml_file(dir, &body)))
        .unwrap()
        .validate()
        .unwrap_err()
        .to_string()
}

#[test]
fn capture_is_off_by_default_with_contract_defaults() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let c = Config::load(Some(&toml_file(dir.path(), &base(dir.path())))).unwrap();
    let cap = &c.capture;
    assert!(!cap.enabled);
    assert_eq!(cap.listen, "0.0.0.0:37008");
    assert!(cap.allowed_sources.is_empty());
    assert_eq!(cap.socket_recv_buffer_bytes, 4_194_304);
    assert_eq!(cap.queue_capacity, 16_384);
    assert_eq!(cap.conn_ttl_secs, 600);
    assert_eq!(cap.max_connections, 100_000);
    assert_eq!(cap.max_reassembly_bytes_per_conn, 16_384);
    assert_eq!(cap.max_reassembly_bytes_total, 67_108_864);
    assert_eq!(cap.max_device_bindings, 200_000);
    assert_eq!(cap.reassembly_timeout_secs, 10);
    assert_eq!(cap.silent_warning_secs, 300);
    assert!(c.validate().unwrap().capture.is_none());
}

#[test]
fn capture_env_override_enables_it() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p = toml_file(dir.path(), &base(dir.path()));
    // SAFETY: serialized by ENV.
    unsafe {
        std::env::set_var("TMON_CAPTURE__ENABLED", "true");
        std::env::set_var("TMON_CAPTURE__LISTEN", "127.0.0.1:47008");
    }
    let v = Config::load(Some(&p)).unwrap().validate();
    unsafe {
        std::env::remove_var("TMON_CAPTURE__ENABLED");
        std::env::remove_var("TMON_CAPTURE__LISTEN");
    }
    let s = v.unwrap().capture.expect("capture enabled");
    assert_eq!(s.listen.to_string(), "127.0.0.1:47008");
    assert_eq!(s.limits.conn_ttl_ms, 600_000);
    assert_eq!(s.limits.reassembly_timeout_ms, 10_000);
}

#[test]
fn capture_validation_names_the_key() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let cases = [
        ("listen = \"nope\"\n", "capture.listen"),
        (
            "allowed_sources = [\"router\"]\n",
            "capture.allowed_sources",
        ),
        ("max_connections = 0\n", "capture.max_connections"),
        (
            "max_connections = 50000000\n",
            "capture.max_connections: must be <= 5000000",
        ),
        (
            "max_device_bindings = 99999999\n",
            "capture.max_device_bindings",
        ),
        ("queue_capacity = 0\n", "capture.queue_capacity"),
        (
            "max_reassembly_bytes_per_conn = 2000\nmax_reassembly_bytes_total = 1000\n",
            "capture.max_reassembly_bytes_per_conn",
        ),
        ("conn_ttl_secs = 59\n", "capture.conn_ttl_secs"),
        ("conn_ttl_secs = 3601\n", "capture.conn_ttl_secs"),
    ];
    for (section, key) in cases {
        let err = capture_err(d, section);
        assert!(err.contains(key), "{section:?} → {err}");
    }
    let ok = format!(
        "{}[capture]\nenabled = true\nallowed_sources = [\"192.168.88.1\"]\nconn_ttl_secs = 60\n",
        base(d)
    );
    let s = Config::load(Some(&toml_file(d, &ok)))
        .unwrap()
        .validate()
        .unwrap()
        .capture
        .unwrap();
    assert_eq!(s.allowed_sources[0].to_string(), "192.168.88.1");
    assert_eq!(s.limits.conn_ttl_ms, 60_000);
}

fn dpi_err(dir: &std::path::Path, capture_on: bool, section: &str) -> String {
    let capture = if capture_on {
        "[capture]\nenabled = true\n"
    } else {
        ""
    };
    let body = format!("{}{capture}[dpi]\nenabled = true\n{section}", base(dir));
    Config::load(Some(&toml_file(dir, &body)))
        .unwrap()
        .validate()
        .unwrap_err()
        .to_string()
}

#[test]
fn dpi_is_off_by_default_with_contract_defaults() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let c = Config::load(Some(&toml_file(dir.path(), &base(dir.path())))).unwrap();
    let d = &c.dpi;
    assert!(!d.enabled);
    assert_eq!(d.binary, "tmon-dpi");
    assert_eq!(d.max_flows, 50_000);
    assert_eq!(d.max_packets_per_conn, 32);
    assert_eq!(d.idle_timeout_secs, 900);
    assert_eq!(d.memory_limit_mb, 192);
    assert_eq!(d.queue_capacity, 16_384);
    assert_eq!(d.queue_max_bytes, 16_777_216);
    assert!(c.validate().unwrap().dpi.is_none());
}

#[test]
fn dpi_env_override_enables_it() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p = toml_file(dir.path(), &base(dir.path()));
    // SAFETY: serialized by ENV.
    unsafe {
        std::env::set_var("TMON_CAPTURE__ENABLED", "true");
        std::env::set_var("TMON_DPI__ENABLED", "true");
        std::env::set_var("TMON_DPI__BINARY", "/nonexistent/tmon-dpi");
    }
    let v = Config::load(Some(&p)).unwrap().validate();
    unsafe {
        std::env::remove_var("TMON_CAPTURE__ENABLED");
        std::env::remove_var("TMON_DPI__ENABLED");
        std::env::remove_var("TMON_DPI__BINARY");
    }
    // A missing binary is not a configuration error.
    let s = v.unwrap().dpi.expect("dpi enabled");
    assert_eq!(s.binary, "/nonexistent/tmon-dpi");
    assert_eq!(s.idle_timeout_ms, 900_000);
}

#[test]
fn dpi_requires_capture() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(
        dpi_err(dir.path(), false, "").contains("dpi.enabled: requires capture.enabled = true")
    );
}

#[test]
fn dpi_validation_names_the_key() {
    let _g = ENV.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (section, want) in [
        ("binary = \"  \"\n", "dpi.binary"),
        (
            "max_flows = 999\n",
            "dpi.max_flows: must be between 1000 and 1000000",
        ),
        ("max_flows = 1000001\n", "dpi.max_flows"),
        ("max_packets_per_conn = 3\n", "dpi.max_packets_per_conn"),
        ("max_packets_per_conn = 257\n", "dpi.max_packets_per_conn"),
        ("idle_timeout_secs = 59\n", "dpi.idle_timeout_secs"),
        ("idle_timeout_secs = 3601\n", "dpi.idle_timeout_secs"),
        ("memory_limit_mb = 31\n", "dpi.memory_limit_mb"),
        ("memory_limit_mb = 2049\n", "dpi.memory_limit_mb"),
        ("queue_capacity = 63\n", "dpi.queue_capacity"),
        ("queue_capacity = 1000001\n", "dpi.queue_capacity"),
        ("queue_max_bytes = 1048575\n", "dpi.queue_max_bytes"),
        ("queue_max_bytes = 268435457\n", "dpi.queue_max_bytes"),
    ] {
        let err = dpi_err(dir.path(), true, section);
        assert!(err.contains(want), "{section} → {err}");
    }
}
