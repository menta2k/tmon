//! DuckDB schema, versioned migrations and seeds (data-model.md).

use duckdb::Connection;

use crate::StoreResult;

/// Columns of a raw flow row, in storage order (shared by hot table + Parquet).
pub const FLOW_COLUMNS: &str = "ts, ts_start, exporter_id, in_if, out_if, ip_ver, local_ip, \
    remote_ip, local_port, remote_port, proto, tcp_flags, bytes, packets, sampled, direction, \
    nat_ip, nat_port, category_id, rule_id, country, asn, as_name, remote_host, remote_host_source, \
    app, app_confident";

/// Flow table DDL as shipped in migration 2 (frozen; later columns come from
/// `ALTER TABLE` migrations, see [`ADDED_FLOW_COLUMNS`]).
const FLOW_TABLE_BODY: &str = "(
    ts BIGINT NOT NULL,
    ts_start BIGINT NOT NULL,
    exporter_id USMALLINT NOT NULL,
    in_if UINTEGER NOT NULL,
    out_if UINTEGER NOT NULL,
    ip_ver UTINYINT NOT NULL,
    local_ip VARCHAR NOT NULL,
    remote_ip VARCHAR NOT NULL,
    local_port USMALLINT NOT NULL,
    remote_port USMALLINT NOT NULL,
    proto UTINYINT NOT NULL,
    tcp_flags UTINYINT NOT NULL,
    bytes UBIGINT NOT NULL,
    packets UBIGINT NOT NULL,
    sampled BOOLEAN NOT NULL,
    direction VARCHAR NOT NULL,
    nat_ip VARCHAR,
    nat_port USMALLINT,
    category_id USMALLINT NOT NULL,
    rule_id UINTEGER,
    country VARCHAR,
    asn UINTEGER,
    as_name VARCHAR,
    remote_host VARCHAR
)";

/// Flow columns added after migration 2, in `FLOW_COLUMNS` order.
const ADDED_FLOW_COLUMNS: [&str; 3] = [
    "remote_host_source UTINYINT",
    "app VARCHAR",
    "app_confident BOOLEAN",
];

/// `ALTER TABLE <table> ADD COLUMN IF NOT EXISTS …` for each column.
fn add_columns(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|c| format!("ALTER TABLE {table} ADD COLUMN IF NOT EXISTS {c};"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rollup families: (name, extra dimension DDL, extra key columns).
pub const ROLLUPS: [(&str, &str, &str); 5] = [
    (
        "rollup_category",
        "category_id USMALLINT NOT NULL, proto UTINYINT NOT NULL",
        "category_id, proto",
    ),
    (
        "rollup_host",
        "local_ip VARCHAR NOT NULL, category_id USMALLINT NOT NULL",
        "local_ip, category_id",
    ),
    (
        "rollup_geo",
        "country VARCHAR NOT NULL, asn UINTEGER NOT NULL, category_id USMALLINT NOT NULL",
        "country, asn, category_id",
    ),
    (
        "rollup_port",
        "proto UTINYINT NOT NULL, remote_port USMALLINT NOT NULL",
        "proto, remote_port",
    ),
    // rule_id 0 = no rule matched (rule ids start at 1). New table: history before it existed is empty.
    (
        "rollup_rule",
        "rule_id UINTEGER NOT NULL, category_id USMALLINT NOT NULL",
        "rule_id, category_id",
    ),
];

pub const GRAINS: [(&str, i64); 2] = [("1m", 60_000), ("1h", 3_600_000)];

fn rollup_ddl() -> String {
    let mut sql = String::new();
    for (name, dims, keys) in ROLLUPS {
        for (grain, _) in GRAINS {
            sql.push_str(&format!(
                "CREATE TABLE IF NOT EXISTS {name}_{grain} (bucket BIGINT NOT NULL, \
                 exporter_id USMALLINT NOT NULL, direction VARCHAR NOT NULL, {dims}, \
                 bytes UBIGINT NOT NULL, packets UBIGINT NOT NULL, flows UBIGINT NOT NULL, \
                 PRIMARY KEY (bucket, exporter_id, direction, {keys}));\n"
            ));
        }
    }
    sql.push_str(
        "CREATE TABLE IF NOT EXISTS rollup_remote_1h (bucket BIGINT NOT NULL, \
         exporter_id USMALLINT NOT NULL, direction VARCHAR NOT NULL, local_ip VARCHAR NOT NULL, \
         remote_ip VARCHAR NOT NULL, remote_host VARCHAR, country VARCHAR, asn UINTEGER, \
         as_name VARCHAR, bytes UBIGINT NOT NULL, packets UBIGINT NOT NULL, flows UBIGINT NOT NULL, \
         PRIMARY KEY (bucket, exporter_id, direction, local_ip, remote_ip));\n",
    );
    sql
}

const CONFIG_DDL: &str = "
CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at BIGINT NOT NULL);
CREATE TABLE IF NOT EXISTS exporter (
    id USMALLINT PRIMARY KEY, address VARCHAR NOT NULL, domain_id UINTEGER NOT NULL,
    protocol VARCHAR NOT NULL, name VARCHAR, sampling_override UINTEGER,
    count_direction VARCHAR NOT NULL DEFAULT 'both', first_seen BIGINT NOT NULL,
    last_seen BIGINT NOT NULL, UNIQUE (address, domain_id));
CREATE TABLE IF NOT EXISTS category (
    id USMALLINT PRIMARY KEY, name VARCHAR NOT NULL UNIQUE, color VARCHAR NOT NULL,
    description VARCHAR NOT NULL DEFAULT '', builtin BOOLEAN NOT NULL DEFAULT false);
CREATE TABLE IF NOT EXISTS rule (
    id UINTEGER PRIMARY KEY, category_id USMALLINT NOT NULL, priority BIGINT NOT NULL,
    enabled BOOLEAN NOT NULL, origin VARCHAR NOT NULL, name VARCHAR NOT NULL,
    match_json VARCHAR NOT NULL);
CREATE TABLE IF NOT EXISTS local_host (
    ip VARCHAR PRIMARY KEY, mac VARCHAR, friendly_name VARCHAR, dhcp_hostname VARCHAR,
    first_seen BIGINT NOT NULL, last_seen BIGINT NOT NULL);
CREATE TABLE IF NOT EXISTS local_network (cidr VARCHAR PRIMARY KEY, label VARCHAR NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS remote_hostname (
    ip VARCHAR PRIMARY KEY, hostname VARCHAR NOT NULL, source VARCHAR NOT NULL, last_seen BIGINT NOT NULL);
CREATE TABLE IF NOT EXISTS setting (key VARCHAR PRIMARY KEY, value VARCHAR NOT NULL);
";

/// Ordered migrations; each runs once.
fn migrations() -> Vec<(i32, String)> {
    vec![
        (1, CONFIG_DDL.to_string()),
        (2, format!("CREATE TABLE IF NOT EXISTS flows_hot {FLOW_TABLE_BODY};")),
        (3, rollup_ddl()),
        (4, "CREATE TABLE IF NOT EXISTS asn_name (asn UINTEGER PRIMARY KEY, name VARCHAR NOT NULL);".to_string()),
        // Rollup families added after v3 (rollup_rule). Never edit a shipped migration:
        // databases that already ran it would not get the new tables.
        (5, rollup_ddl()),
        // Feature 002: where `remote_host` came from (1 tls … 5 router, NULL unknown).
        (
            6,
            "ALTER TABLE flows_hot ADD COLUMN IF NOT EXISTS remote_host_source UTINYINT;".to_string(),
        ),
        // Feature 003: application from the classifier (NULL = none).
        (7, add_columns("flows_hot", &ADDED_FLOW_COLUMNS[1..])),
    ]
}

/// Temporary staging table with the flow schema (per connection).
pub fn create_stage(conn: &Connection) -> StoreResult<()> {
    conn.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS batch_stage {FLOW_TABLE_BODY}; {}",
        add_columns("batch_stage", &ADDED_FLOW_COLUMNS)
    ))?;
    Ok(())
}

pub fn migrate(conn: &Connection, now_ms: i64) -> StoreResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at BIGINT NOT NULL);",
    )?;
    let current: i32 = conn.query_row(
        "SELECT coalesce(max(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    for (version, sql) in migrations().into_iter().filter(|(v, _)| *v > current) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(&sql)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES (?, ?)",
            duckdb::params![version, now_ms],
        )?;
        tx.commit()?;
    }
    seed(conn)
}

fn seed(conn: &Connection) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO category (id, name, color, description, builtin) VALUES (0, 'Other', '#9E9E9E', \
         'Traffic not matched by any rule', true) ON CONFLICT DO NOTHING",
        [],
    )?;
    let networks: i64 = conn.query_row("SELECT count(*) FROM local_network", [], |r| r.get(0))?;
    let seeded: i64 = conn.query_row(
        "SELECT count(*) FROM setting WHERE key = 'networks_seeded'",
        [],
        |r| r.get(0),
    )?;
    if networks == 0 && seeded == 0 {
        for cidr in tmon_core::direction::DEFAULT_LOCAL_NETWORKS {
            conn.execute("INSERT INTO local_network VALUES (?, 'default')", [cidr])?;
        }
        conn.execute("INSERT INTO setting VALUES ('networks_seeded', 'true')", [])?;
    }
    Ok(())
}
