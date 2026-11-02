//! Strict query-string parsing: repeatable keys, unknown keys rejected (400).

use std::collections::BTreeMap;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tmon_core::Direction;
use tmon_store::duck::query::router::Filter;

use crate::envelope::ApiError;

pub const DEFAULT_FROM: &str = "now-15m";
pub const MAX_SPAN_MS: i64 = 400 * 86_400_000;
pub const DEFAULT_LIMIT: u64 = 20;
pub const MAX_LIMIT: u64 = 1000;
const COMMON: [&str; 11] = [
    "from",
    "to",
    "step",
    "host",
    "category",
    "proto",
    "direction",
    "country",
    "asn",
    "exporter",
    "limit",
];

/// Multimap of query parameters.
#[derive(Debug, Clone, Default)]
pub struct Params {
    map: BTreeMap<String, Vec<String>>,
}

impl Params {
    pub fn parse(raw: Option<&str>) -> Self {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (k, v) in form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
            map.entry(k.into_owned()).or_default().push(v.into_owned());
        }
        Self { map }
    }

    /// Reject keys outside `COMMON ∪ extra`.
    pub fn allow(&self, extra: &[&str], common: bool) -> Result<(), ApiError> {
        match self
            .map
            .keys()
            .find(|k| !(extra.contains(&k.as_str()) || (common && COMMON.contains(&k.as_str()))))
        {
            Some(k) => Err(ApiError::field(k, "unknown query parameter")),
            None => Ok(()),
        }
    }

    pub fn all(&self, key: &str) -> &[String] {
        self.map.get(key).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn one(&self, key: &str) -> Option<&str> {
        self.all(key).last().map(String::as_str)
    }

    fn parsed<T: std::str::FromStr>(&self, key: &str, msg: &str) -> Result<Vec<T>, ApiError> {
        self.all(key)
            .iter()
            .map(|v| v.trim().parse().map_err(|_| ApiError::field(key, msg)))
            .collect()
    }

    pub fn u64_or(&self, key: &str, default: u64, min: u64, max: u64) -> Result<u64, ApiError> {
        match self.one(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<u64>()
                .ok()
                .filter(|n| (min..=max).contains(n))
                .ok_or_else(|| ApiError::field(key, &format!("must be an integer {min}–{max}"))),
        }
    }

    pub fn limit(&self) -> Result<u64, ApiError> {
        self.u64_or("limit", DEFAULT_LIMIT, 1, MAX_LIMIT)
    }

    /// Common analytics filter (`from`/`to`, repeatable dimensions).
    pub fn filter(&self, now_ms: i64) -> Result<Filter, ApiError> {
        let from_ms = parse_time(self.one("from").unwrap_or(DEFAULT_FROM), now_ms)
            .ok_or_else(|| ApiError::field("from", "must be RFC3339 or now[±N(s|m|h|d)]"))?;
        let to_ms = parse_time(self.one("to").unwrap_or("now"), now_ms)
            .ok_or_else(|| ApiError::field("to", "must be RFC3339 or now[±N(s|m|h|d)]"))?;
        if from_ms >= to_ms {
            return Err(ApiError::field("from", "must be before `to`"));
        }
        if to_ms - from_ms > MAX_SPAN_MS {
            return Err(ApiError::field("from", "range must not exceed 400 days"));
        }
        let directions = self
            .all("direction")
            .iter()
            .map(|d| {
                Direction::parse(d)
                    .ok_or_else(|| ApiError::field("direction", "must be down|up|internal|transit"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Filter {
            from_ms,
            to_ms,
            hosts: self.ips("host")?,
            categories: self.parsed("category", "must be a category id")?,
            protos: self.parsed("proto", "must be 0–255")?,
            directions,
            countries: self.countries()?,
            asns: self.parsed("asn", "must be an ASN")?,
            exporters: self.parsed("exporter", "must be an exporter id")?,
            remote_ips: Vec::new(),
            ports: Vec::new(),
            apps: Vec::new(),
        })
    }

    pub fn ips(&self, key: &str) -> Result<Vec<String>, ApiError> {
        self.all(key)
            .iter()
            .map(|h| {
                h.trim()
                    .parse::<std::net::IpAddr>()
                    .map(|ip| ip.to_string())
                    .map_err(|_| ApiError::field(key, "must be an IP address"))
            })
            .collect()
    }

    /// `app` values (feature 003): classifier application names; an empty
    /// value selects unclassified flows.
    pub fn apps(&self) -> Result<Vec<String>, ApiError> {
        self.all("app")
            .iter()
            .map(|a| {
                let a = a.trim();
                (a.is_empty() || tmon_core::app::valid_app_name(a))
                    .then(|| a.to_string())
                    .ok_or_else(|| {
                        ApiError::field(
                            "app",
                            "must be empty (unclassified) or a 1–48 character application name",
                        )
                    })
            })
            .collect()
    }

    fn countries(&self) -> Result<Vec<String>, ApiError> {
        self.all("country")
            .iter()
            .map(|c| {
                let ok = c.len() == 2 && c.chars().all(|x| x.is_ascii_alphabetic());
                ok.then(|| c.to_ascii_uppercase())
                    .ok_or_else(|| ApiError::field("country", "must be ISO-3166 alpha-2"))
            })
            .collect()
    }

    /// `step` as a duration in ms, if given.
    pub fn step_ms(&self) -> Result<Option<i64>, ApiError> {
        self.one("step")
            .map(|s| {
                parse_duration(s)
                    .ok_or_else(|| ApiError::field("step", "must be a duration like 60s, 5m, 1h"))
            })
            .transpose()
    }
}

/// `30s`, `15m`, `24h`, `7d` or plain seconds → ms.
pub fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num, unit) = s
        .find(|c: char| !c.is_ascii_digit())
        .map_or((s, "s"), |i| s.split_at(i));
    let n: i64 = num.parse().ok().filter(|n| *n > 0)?;
    let mult = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return None,
    };
    n.checked_mul(mult)
}

/// `now`, `now-24h` or RFC3339 → epoch ms.
pub fn parse_time(s: &str, now_ms: i64) -> Option<i64> {
    let s = s.trim();
    if s == "now" {
        return Some(now_ms);
    }
    if let Some(rel) = s.strip_prefix("now-") {
        return parse_duration(rel).map(|d| now_ms - d);
    }
    if let Some(rel) = s.strip_prefix("now+") {
        return parse_duration(rel).map(|d| now_ms + d);
    }
    OffsetDateTime::parse(s, &Rfc3339)
        .ok()
        .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times_and_durations() {
        assert_eq!(parse_time("now-15m", 1_000_000), Some(100_000));
        assert_eq!(parse_time("now+1m", 0), Some(60_000));
        assert_eq!(
            parse_time("2026-01-01T00:00:00Z", 0),
            Some(1_767_225_600_000)
        );
        assert_eq!(parse_time("yesterday", 0), None);
        assert_eq!(parse_duration("90"), Some(90_000));
        assert_eq!(parse_duration("2d"), Some(172_800_000));
        assert_eq!(parse_duration("0m"), None);
        assert_eq!(parse_duration("5w"), None);
    }

    #[test]
    fn filter_parsing_and_unknown_keys() {
        let p = Params::parse(Some(
            "from=now-1h&host=10.0.0.1&host=10.0.0.2&direction=up&country=de&limit=5",
        ));
        p.allow(&[], true).unwrap();
        let f = p.filter(10_000_000).unwrap();
        assert_eq!(f.hosts.len(), 2);
        assert_eq!(f.countries, vec!["DE"]);
        assert_eq!(p.limit().unwrap(), 5);
        assert!(Params::parse(Some("bogus=1")).allow(&[], true).is_err());
        assert!(Params::parse(Some("limit=0")).limit().is_err());
        assert!(
            Params::parse(Some("from=now&to=now-1h"))
                .filter(10_000_000)
                .is_err()
        );
        assert!(
            Params::parse(Some("from=now-500d"))
                .filter(100_000_000_000)
                .is_err()
        );
        assert!(Params::parse(Some("host=x")).filter(10_000_000).is_err());
        assert!(
            Params::parse(Some("direction=sideways"))
                .filter(10_000_000)
                .is_err()
        );
    }
}
