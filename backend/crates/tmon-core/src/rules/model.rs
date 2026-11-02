//! Rule and RuleMatch models (data-model.md §5) with field-indexed validation.

use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::fmt;

use crate::cidr::Cidr;
use crate::error::ValidationErrors;
use crate::flow::Direction;

/// User rules live in the band 1,000,000+; built-in rules below it.
pub const USER_PRIORITY_BASE: i64 = 1_000_000;
const NAME_MAX: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Builtin,
    User,
}

impl Origin {
    pub fn as_str(&self) -> &'static str {
        match self {
            Origin::Builtin => "builtin",
            Origin::User => "user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: u32,
    pub category_id: u16,
    /// Higher wins.
    pub priority: i64,
    pub enabled: bool,
    pub origin: Origin,
    pub name: String,
    #[serde(rename = "match")]
    pub match_: RuleMatch,
}

impl Rule {
    pub fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        let len = self.name.trim().chars().count();
        if len == 0 || len > NAME_MAX {
            errors.add("name", format!("must be 1–{NAME_MAX} characters"));
        }
        match self.origin {
            Origin::User if self.priority < USER_PRIORITY_BASE => errors.add(
                "priority",
                format!("user rules must be ≥ {USER_PRIORITY_BASE}"),
            ),
            Origin::Builtin if self.priority >= USER_PRIORITY_BASE => errors.add(
                "priority",
                format!("built-in rules must be < {USER_PRIORITY_BASE}"),
            ),
            _ => {}
        }
        if self.match_.is_empty() {
            errors.add("match", "at least one criterion must be set");
        }
        errors.into_result(())
    }
}

/// Port or inclusive port range (`443` or `"27015-27030"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortSpec {
    pub lo: u16,
    pub hi: u16,
}

impl PortSpec {
    pub fn contains(&self, port: u16) -> bool {
        (self.lo..=self.hi).contains(&port)
    }

    pub fn is_single(&self) -> bool {
        self.lo == self.hi
    }

    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Number(n) => {
                let p = u16::try_from(n.as_u64()?).ok()?;
                Some(Self { lo: p, hi: p })
            }
            Value::String(s) => Self::parse(s),
            _ => None,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (lo, hi) = match s.split_once('-') {
            Some((a, b)) => (a.trim().parse().ok()?, b.trim().parse().ok()?),
            None => {
                let p = s.parse().ok()?;
                (p, p)
            }
        };
        (lo <= hi).then_some(Self { lo, hi })
    }
}

impl Serialize for PortSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.is_single() {
            s.serialize_u16(self.lo)
        } else {
            s.serialize_str(&format!("{}-{}", self.lo, self.hi))
        }
    }
}

/// Local host selector: IP, CIDR, or MAC address (resolved via local hosts).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostSpec {
    Net(Cidr),
    Mac(String),
}

impl HostSpec {
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(mac) = normalize_mac(s) {
            return Some(HostSpec::Mac(mac));
        }
        s.parse::<Cidr>().ok().map(HostSpec::Net)
    }
}

impl fmt::Display for HostSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostSpec::Net(c) if c.prefix() == 32 || c.prefix() == 128 => {
                write!(f, "{}", c.network())
            }
            HostSpec::Net(c) => write!(f, "{c}"),
            HostSpec::Mac(m) => write!(f, "{m}"),
        }
    }
}

impl Serialize for HostSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

/// Normalize `aa:bb:cc:dd:ee:ff` (also accepts `-` separators / upper case).
pub fn normalize_mac(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.trim().split([':', '-']).collect();
    let valid = parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()));
    valid.then(|| parts.join(":").to_ascii_lowercase())
}

/// All set fields must match (AND); lists inside a field are OR.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RuleMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proto: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_port: Option<Vec<PortSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_port: Option<Vec<PortSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_cidr: Option<Vec<Cidr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_host: Option<Vec<HostSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<Vec<Direction>>,
    /// Application names from the classifier (feature 003), case-insensitive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<Vec<String>>,
    /// Classifier category names (e.g. `VPN`), case-insensitive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_category: Option<Vec<String>>,
}

impl RuleMatch {
    pub fn is_empty(&self) -> bool {
        self == &RuleMatch::default()
    }

    /// Parse from JSON with field-indexed errors such as `remote_cidr[0]: invalid CIDR`.
    pub fn from_json(v: &Value) -> Result<Self, ValidationErrors> {
        let Some(obj) = v.as_object() else {
            return Err(ValidationErrors::single("match", "must be an object"));
        };
        let mut e = ValidationErrors::new();
        for key in obj.keys() {
            if !KNOWN_FIELDS.contains(&key.as_str()) {
                e.add(key.clone(), "unknown match field");
            }
        }
        let m = RuleMatch {
            proto: list(obj.get("proto"), "proto", &mut e, |v| {
                v.as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or("must be 0–255")
            }),
            remote_port: list(obj.get("remote_port"), "remote_port", &mut e, port),
            local_port: list(obj.get("local_port"), "local_port", &mut e, port),
            remote_cidr: list(obj.get("remote_cidr"), "remote_cidr", &mut e, |v| {
                v.as_str()
                    .and_then(|s| s.parse::<Cidr>().ok())
                    .ok_or("invalid CIDR")
            }),
            local_host: list(obj.get("local_host"), "local_host", &mut e, |v| {
                v.as_str()
                    .and_then(HostSpec::parse)
                    .ok_or("must be an IP, CIDR or MAC")
            }),
            asn: list(obj.get("asn"), "asn", &mut e, |v| {
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("invalid ASN")
            }),
            country: list(obj.get("country"), "country", &mut e, country),
            interface: list(obj.get("interface"), "interface", &mut e, |v| {
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("invalid ifIndex")
            }),
            hostname: list(obj.get("hostname"), "hostname", &mut e, hostname),
            direction: list(obj.get("direction"), "direction", &mut e, |v| {
                v.as_str()
                    .and_then(Direction::parse)
                    .ok_or("must be down|up|internal|transit")
            }),
            app: app_list(obj.get("app"), "app", &mut e),
            app_category: app_list(obj.get("app_category"), "app_category", &mut e),
        };
        e.into_result(m)
    }
}

const KNOWN_FIELDS: [&str; 12] = [
    "proto",
    "remote_port",
    "local_port",
    "remote_cidr",
    "local_host",
    "asn",
    "country",
    "interface",
    "hostname",
    "direction",
    "app",
    "app_category",
];

/// Most entries in an `app` / `app_category` list.
pub const MAX_APP_ENTRIES: usize = 64;

/// `app` / `app_category`: 1–64 entries, each 1–48 printable ASCII.
fn app_list(v: Option<&Value>, field: &str, errors: &mut ValidationErrors) -> Option<Vec<String>> {
    let out = list(v, field, errors, |v| {
        v.as_str()
            .filter(|s| crate::app::valid_app_name(s))
            .map(|s| s.trim().to_string())
            .ok_or("must be 1–48 printable ASCII characters")
    })?;
    if out.len() > MAX_APP_ENTRIES {
        errors.add(field, format!("at most {MAX_APP_ENTRIES} entries"));
        return None;
    }
    Some(out)
}

fn port(v: &Value) -> Result<PortSpec, &'static str> {
    PortSpec::from_value(v).ok_or("must be a port or range a-b with a ≤ b")
}

fn country(v: &Value) -> Result<String, &'static str> {
    v.as_str()
        .filter(|s| s.len() == 2 && s.chars().all(|c| c.is_ascii_alphabetic()))
        .map(|s| s.to_ascii_uppercase())
        .ok_or("must be an ISO-3166 alpha-2 code")
}

fn hostname(v: &Value) -> Result<String, &'static str> {
    let s = v.as_str().map(str::trim).unwrap_or_default();
    let ok = !s.is_empty() && s.len() <= 253 && globset::Glob::new(s).is_ok();
    if ok {
        Ok(s.to_ascii_lowercase())
    } else {
        Err("invalid hostname pattern")
    }
}

/// Parse an optional list field, collecting `field[i]` errors.
fn list<T>(
    v: Option<&Value>,
    field: &str,
    errors: &mut ValidationErrors,
    parse: impl Fn(&Value) -> Result<T, &'static str>,
) -> Option<Vec<T>> {
    let v = v?;
    if v.is_null() {
        return None;
    }
    let Some(items) = v.as_array() else {
        errors.add(field, "must be a list");
        return None;
    };
    if items.is_empty() {
        errors.add(field, "must not be empty");
        return None;
    }
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        match parse(item) {
            Ok(x) => out.push(x),
            Err(msg) => errors.add(format!("{field}[{i}]"), msg),
        }
    }
    Some(out)
}

impl<'de> Deserialize<'de> for RuleMatch {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        RuleMatch::from_json(&v).map_err(serde::de::Error::custom)
    }
}
