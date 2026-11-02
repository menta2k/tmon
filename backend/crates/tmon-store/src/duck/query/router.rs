//! Choose the cheapest source covering a query (data-model.md "Query routing").

use std::collections::BTreeSet;
use std::path::Path;
use tmon_core::Direction;

use crate::duck::files::{raw_files, union_sql};
use crate::{StoreError, StoreResult};

/// Ranges up to this span use 1-minute rollups.
pub const MINUTE_GRAIN_MAX_SPAN_MS: i64 = 2 * 24 * 3_600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dim {
    Exporter,
    Direction,
    Category,
    Proto,
    Host,
    Country,
    Asn,
    Port,
    RemoteIp,
    /// The rule that categorized the flow (an "application"); key `0` = no rule.
    Rule,
    /// Classifier application (feature 003); raw data only, key `""` = unclassified.
    App,
}

impl Dim {
    pub fn as_str(&self) -> &'static str {
        match self {
            Dim::Exporter => "exporter",
            Dim::Direction => "direction",
            Dim::Category => "category",
            Dim::Proto => "proto",
            Dim::Host => "host",
            Dim::Country => "country",
            Dim::Asn => "asn",
            Dim::Port => "port",
            Dim::RemoteIp => "remote_ip",
            Dim::Rule => "rule",
            Dim::App => "app",
        }
    }
}

/// Common analytics filter (contracts/http-api.md "Common query parameters").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub from_ms: i64,
    pub to_ms: i64,
    pub hosts: Vec<String>,
    pub categories: Vec<u16>,
    pub protos: Vec<u8>,
    pub directions: Vec<Direction>,
    pub countries: Vec<String>,
    pub asns: Vec<u32>,
    pub exporters: Vec<u16>,
    pub remote_ips: Vec<String>,
    pub ports: Vec<u16>,
    /// Classifier application names (matched case-insensitively; raw only).
    pub apps: Vec<String>,
}

impl Filter {
    pub fn range(from_ms: i64, to_ms: i64) -> Self {
        Self {
            from_ms,
            to_ms,
            ..Self::default()
        }
    }

    /// Dimensions constrained by this filter.
    pub fn dims(&self) -> BTreeSet<Dim> {
        let pairs = [
            (Dim::Host, !self.hosts.is_empty()),
            (Dim::Category, !self.categories.is_empty()),
            (Dim::Proto, !self.protos.is_empty()),
            (Dim::Direction, !self.directions.is_empty()),
            (Dim::Country, !self.countries.is_empty()),
            (Dim::Asn, !self.asns.is_empty()),
            (Dim::Exporter, !self.exporters.is_empty()),
            (Dim::RemoteIp, !self.remote_ips.is_empty()),
            (Dim::Port, !self.ports.is_empty()),
            (Dim::App, !self.apps.is_empty()),
        ];
        pairs
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(d, _)| d)
            .collect()
    }
}

/// Rollup families and the dimensions each one carries.
/// The first family covering a query wins, so broader families come later.
pub const FAMILIES: [(&str, &[Dim]); 5] = [
    (
        "rollup_category",
        &[Dim::Exporter, Dim::Direction, Dim::Category, Dim::Proto],
    ),
    (
        "rollup_host",
        &[Dim::Exporter, Dim::Direction, Dim::Host, Dim::Category],
    ),
    (
        "rollup_geo",
        &[
            Dim::Exporter,
            Dim::Direction,
            Dim::Country,
            Dim::Asn,
            Dim::Category,
        ],
    ),
    (
        "rollup_port",
        &[Dim::Exporter, Dim::Direction, Dim::Proto, Dim::Port],
    ),
    (
        "rollup_rule",
        &[Dim::Exporter, Dim::Direction, Dim::Rule, Dim::Category],
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grain {
    Minute,
    Hour,
}

impl Grain {
    pub fn ms(&self) -> i64 {
        match self {
            Grain::Minute => 60_000,
            Grain::Hour => 3_600_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Rollup { family: &'static str, grain: Grain },
    Raw,
}

impl Source {
    pub fn is_raw(&self) -> bool {
        matches!(self, Source::Raw)
    }

    pub fn time_column(&self) -> &'static str {
        if self.is_raw() { "ts" } else { "bucket" }
    }

    /// `meta.source` label.
    pub fn label(&self) -> &'static str {
        match self {
            Source::Rollup {
                grain: Grain::Minute,
                ..
            } => "rollup_1m",
            Source::Rollup {
                grain: Grain::Hour, ..
            } => "rollup_1h",
            Source::Raw => "raw",
        }
    }

    /// Granularity of the source in ms (raw → 1 s).
    pub fn grain_ms(&self) -> i64 {
        match self {
            Source::Rollup { grain, .. } => grain.ms(),
            Source::Raw => 1000,
        }
    }

    /// FROM-clause SQL (a table name or a parenthesized union for raw).
    pub fn from_sql(&self, raw_dir: &Path, f: &Filter) -> StoreResult<String> {
        Ok(match self {
            Source::Rollup { family, grain } => {
                format!(
                    "{family}_{}",
                    if *grain == Grain::Minute { "1m" } else { "1h" }
                )
            }
            Source::Raw => format!("({})", union_sql(&raw_files(raw_dir, f.from_ms, f.to_ms)?)),
        })
    }
}

/// Retention windows that influence routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Windows {
    pub raw_ms: i64,
    pub rollup_1m_ms: i64,
}

impl Default for Windows {
    fn default() -> Self {
        Self {
            raw_ms: 7 * 86_400_000,
            rollup_1m_ms: 30 * 86_400_000,
        }
    }
}

/// Route a query needing `filter` dims plus `group` dims.
pub fn route(f: &Filter, group: &[Dim], w: Windows, now_ms: i64) -> StoreResult<Source> {
    let mut need = f.dims();
    need.extend(group.iter().copied());
    let family = FAMILIES
        .iter()
        .find(|(_, dims)| need.iter().all(|d| dims.contains(d)));
    if let Some((family, _)) = family {
        let minute =
            f.to_ms - f.from_ms <= MINUTE_GRAIN_MAX_SPAN_MS && f.from_ms >= now_ms - w.rollup_1m_ms;
        let grain = if minute { Grain::Minute } else { Grain::Hour };
        return Ok(Source::Rollup { family, grain });
    }
    if f.from_ms >= now_ms - w.raw_ms {
        return Ok(Source::Raw);
    }
    Err(StoreError::UnsupportedFilterRange {
        supported_dims: supported(group),
    })
}

/// Dimensions combinable with `group` beyond raw retention.
fn supported(group: &[Dim]) -> Vec<String> {
    let set: BTreeSet<Dim> = FAMILIES
        .iter()
        .filter(|(_, dims)| group.iter().all(|g| dims.contains(g)))
        .flat_map(|(_, dims)| dims.iter().copied())
        .collect();
    set.into_iter().map(|d| d.as_str().to_string()).collect()
}
