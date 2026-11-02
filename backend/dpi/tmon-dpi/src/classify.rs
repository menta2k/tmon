//! nDPI result → protocol `Classification` fields (research R6):
//! app protocol preferred, generic carriers never reported, and the
//! confidence map to "confident" / "guess".

use crate::engine::{NdpiConfidence, NdpiResult};

/// Carriers that say nothing about the application. Mirrors
/// `tmon_core::app::GENERIC_CARRIERS` (tmon re-checks them on receipt).
pub const GENERIC_CARRIERS: [&str; 6] = ["TLS", "QUIC", "HTTP", "DNS", "Unknown", "HTTP_Proxy"];

/// Longest app / category name on the wire (`tmon_dpi_proto::MAX_NAME`).
const MAX_NAME: usize = tmon_dpi_proto::MAX_NAME;
const MAX_HOST: usize = tmon_dpi_proto::MAX_HOST;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub app: String,
    pub category: String,
    pub confident: bool,
    pub host: Option<String>,
}

fn generic(name: &str) -> bool {
    name.is_empty()
        || GENERIC_CARRIERS
            .iter()
            .any(|g| g.eq_ignore_ascii_case(name))
}

/// `Some(true)` confident, `Some(false)` guess, `None` not reportable.
pub fn confident(c: NdpiConfidence) -> Option<bool> {
    use NdpiConfidence::*;
    match c {
        Dpi | DpiCache | CustomRule => Some(true),
        DpiPartial | DpiPartialCache | MatchByIp | MatchByPort | DpiAggressive | Nbpf => {
            Some(false)
        }
        Unknown => None,
    }
}

fn wire_name(s: &str) -> Option<String> {
    let ok = !s.is_empty() && s.len() <= MAX_NAME && s.bytes().all(|b| (0x20..=0x7e).contains(&b));
    ok.then(|| s.to_string())
}

fn wire_host(s: &str) -> Option<String> {
    let s = s.trim_end_matches('.');
    let ok = !s.is_empty()
        && s.len() <= MAX_HOST
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_');
    ok.then(|| s.to_ascii_lowercase())
}

/// The reportable verdict for a result, if any.
pub fn classify(r: &NdpiResult) -> Option<Verdict> {
    let confident = confident(r.confidence?)?;
    let name = if !generic(&r.app) {
        &r.app
    } else if !generic(&r.master) {
        &r.master
    } else {
        return None;
    };
    Some(Verdict {
        app: wire_name(name)?,
        category: wire_name(&r.category).unwrap_or_default(),
        confident,
        host: wire_host(&r.host),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use NdpiConfidence::*;

    fn r(app: &str, master: &str, c: NdpiConfidence) -> NdpiResult {
        NdpiResult {
            app: app.into(),
            master: master.into(),
            category: "Download".into(),
            confidence: Some(c),
            host: String::new(),
        }
    }

    #[test]
    fn confidence_map_follows_research_r6() {
        for c in [Dpi, DpiCache, CustomRule] {
            assert_eq!(confident(c), Some(true), "{c:?}");
        }
        for c in [
            DpiPartial,
            DpiPartialCache,
            MatchByIp,
            MatchByPort,
            DpiAggressive,
        ] {
            assert_eq!(confident(c), Some(false), "{c:?}");
        }
        assert_eq!(confident(Unknown), None);
        assert_eq!(classify(&r("BitTorrent", "Unknown", Unknown)), None);
        assert_eq!(
            classify(&NdpiResult {
                confidence: None,
                ..r("BitTorrent", "", Dpi)
            }),
            None
        );
    }

    #[test]
    fn app_protocol_is_preferred_over_master() {
        let v = classify(&r("Zoom", "TLS", Dpi)).unwrap();
        assert_eq!((v.app.as_str(), v.confident), ("Zoom", true));
        let v = classify(&r("Unknown", "WireGuard", MatchByPort)).unwrap();
        assert_eq!((v.app.as_str(), v.confident), ("WireGuard", false));
    }

    #[test]
    fn generic_carriers_are_never_reported() {
        for (app, master) in [
            ("TLS", "Unknown"),
            ("Unknown", "QUIC"),
            ("HTTP", "DNS"),
            ("", ""),
        ] {
            assert_eq!(classify(&r(app, master, Dpi)), None, "{app}/{master}");
        }
        assert_eq!(classify(&r("tls", "", Dpi)), None);
    }

    #[test]
    fn names_and_hosts_are_wire_safe() {
        assert_eq!(classify(&r(&"x".repeat(49), "", Dpi)), None);
        assert_eq!(classify(&r("Bad\u{7f}", "", Dpi)), None);
        let mut res = r("RDP", "", Dpi);
        res.host = "Desktop.Example.NET.".into();
        assert_eq!(
            classify(&res).unwrap().host.as_deref(),
            Some("desktop.example.net")
        );
        res.host = "evil\nhost".into();
        assert_eq!(classify(&res).unwrap().host, None);
        res.category = "\u{1}".into();
        assert_eq!(classify(&res).unwrap().category, "");
    }

    #[test]
    fn raw_confidence_values_map_by_position() {
        assert_eq!(NdpiConfidence::from_raw(6), Dpi);
        assert_eq!(NdpiConfidence::from_raw(1), MatchByPort);
        assert_eq!(NdpiConfidence::from_raw(99), Unknown);
    }
}
