//! Parameterized SQL helpers — user values are always bound, never interpolated.

use duckdb::types::Value;

use super::router::{Filter, Source};

/// Accumulates `WHERE` clauses and their bound parameters.
#[derive(Debug, Default, Clone)]
pub struct Where {
    clauses: Vec<String>,
    params: Vec<Value>,
}

impl Where {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, clause: impl Into<String>, params: impl IntoIterator<Item = Value>) {
        self.clauses.push(clause.into());
        self.params.extend(params);
    }

    /// `col IN (?, ?, …)`; no-op when `values` is empty.
    pub fn in_list(&mut self, col: &str, values: Vec<Value>) {
        if values.is_empty() {
            return;
        }
        let marks = vec!["?"; values.len()].join(", ");
        self.push(format!("{col} IN ({marks})"), values);
    }

    pub fn sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.clauses.join(" AND "))
        }
    }

    pub fn params(&self) -> &[Value] {
        &self.params
    }

    /// Standard filter → clauses for a source (time column differs).
    pub fn from_filter(f: &Filter, source: &Source) -> Self {
        let mut w = Where::new();
        let time = source.time_column();
        w.push(
            format!("{time} >= ? AND {time} < ?"),
            [Value::BigInt(f.from_ms), Value::BigInt(f.to_ms)],
        );
        w.in_list(
            "local_ip",
            f.hosts.iter().map(|h| Value::Text(h.clone())).collect(),
        );
        w.in_list(
            "category_id",
            f.categories.iter().map(|c| Value::USmallInt(*c)).collect(),
        );
        w.in_list(
            "proto",
            f.protos.iter().map(|p| Value::UTinyInt(*p)).collect(),
        );
        w.in_list(
            "direction",
            f.directions
                .iter()
                .map(|d| Value::Text(d.as_str().into()))
                .collect(),
        );
        w.in_list(
            "exporter_id",
            f.exporters.iter().map(|e| Value::USmallInt(*e)).collect(),
        );
        w.in_list(
            "remote_ip",
            f.remote_ips
                .iter()
                .map(|r| Value::Text(r.clone()))
                .collect(),
        );
        w.in_list(
            "remote_port",
            f.ports.iter().map(|p| Value::USmallInt(*p)).collect(),
        );
        let country_col = if source.is_raw() {
            "coalesce(country, '')"
        } else {
            "country"
        };
        w.in_list(
            country_col,
            f.countries.iter().map(|c| Value::Text(c.clone())).collect(),
        );
        let asn_col = if source.is_raw() {
            "coalesce(asn, 0)"
        } else {
            "asn"
        };
        w.in_list(asn_col, f.asns.iter().map(|a| Value::UInt(*a)).collect());
        // Only raw sources carry `app`; routing guarantees raw when `apps` is set.
        // An empty name selects unclassified flows (`app` NULL).
        w.in_list(
            "coalesce(lower(app), '')",
            f.apps
                .iter()
                .map(|a| Value::Text(a.to_ascii_lowercase()))
                .collect(),
        );
        w
    }
}
