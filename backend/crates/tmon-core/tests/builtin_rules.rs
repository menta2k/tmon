mod common;

use common::flow;
use std::collections::HashMap;
use tmon_core::rules::{Origin, RuleSet, USER_PRIORITY_BASE, load_builtin};
use tmon_core::{Categorizer, OTHER_ID};

const REQUIRED: [&str; 15] = [
    "Web",
    "Streaming",
    "Gaming",
    "Messaging/VoIP",
    "Social",
    "Cloud/Storage",
    "Software Updates",
    "VPN/Tunnels",
    "DNS",
    "Email",
    "Network Services",
    "Other",
    "Advertising",
    "AI Services",
    "Remote Access",
];

#[test]
fn builtin_file_parses_and_covers_required_categories() {
    let b = load_builtin().expect("built-in rules must be valid");
    for name in REQUIRED {
        assert!(
            b.categories.iter().any(|c| c.name == name),
            "missing category {name}"
        );
    }
    assert!(
        b.categories
            .iter()
            .any(|c| c.id == OTHER_ID && c.name == "Other")
    );
    assert!(b.rules.len() >= 20);
    for r in &b.rules {
        assert_eq!(r.origin, Origin::Builtin);
        assert!(r.priority < USER_PRIORITY_BASE);
        r.validate().unwrap();
    }
}

#[test]
fn builtin_rules_categorize_common_traffic() {
    let b = load_builtin().unwrap();
    let rs = RuleSet::compile(&b.rules, &HashMap::new());
    let id = |name: &str| b.categories.iter().find(|c| c.name == name).unwrap().id;
    assert_eq!(
        rs.categorize(&flow("10.0.0.2", "1.1.1.1", 53, 17)).0,
        id("DNS")
    );
    assert_eq!(
        rs.categorize(&flow("10.0.0.2", "1.1.1.1", 443, 6)).0,
        id("Web")
    );
    assert_eq!(
        rs.categorize(&flow("10.0.0.2", "1.1.1.1", 27020, 17)).0,
        id("Gaming")
    );
    assert_eq!(
        rs.categorize(&flow("10.0.0.2", "1.1.1.1", 51820, 17)).0,
        id("VPN/Tunnels")
    );
    let yt =
        flow("10.0.0.2", "1.1.1.1", 443, 17).with_remote_host(Some("rr1.googlevideo.com".into()));
    assert_eq!(rs.categorize(&yt).0, id("Streaming"));
    let nf = flow("10.0.0.2", "1.1.1.1", 443, 6).with_geo(None, Some(2906), None);
    assert_eq!(rs.categorize(&nf).0, id("Streaming"));
    assert_eq!(
        rs.categorize(&flow("10.0.0.2", "1.1.1.1", 9999, 6)).0,
        OTHER_ID
    );
}

#[test]
fn invalid_builtin_text_is_rejected() {
    use tmon_core::rules::builtin::parse_builtin;
    assert!(parse_builtin("nonsense = [").is_err());
    let bad_cat = r##"
[[categories]]
id = 0
name = "Other"
color = "#000000"
[[rules]]
id = 1
name = "x"
category = 42
priority = 1
match = { proto = [6] }
"##;
    assert!(parse_builtin(bad_cat).is_err());
}

fn builtin() -> (RuleSet, impl Fn(&str) -> u16) {
    let b = load_builtin().unwrap();
    let rs = RuleSet::compile(&b.rules, &HashMap::new());
    let cats = b.categories;
    (rs, move |name: &str| {
        cats.iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{name}"))
            .id
    })
}

fn https_to(host: &str) -> tmon_core::FlowRecord {
    flow("192.168.88.10", "94.236.166.212", 443, 6).with_remote_host(Some(host.into()))
}

#[test]
fn hostnames_seen_on_real_home_traffic() {
    let (rs, id) = builtin();
    let cases = [
        // ISP-embedded TV/video caches (show up under the ISP's ASN).
        (
            "vivacom-sf-ml1-r2-9-edge.vivacom-af31.cdn.united.cloud",
            "Streaming",
        ),
        ("rr1.sn-oxufvgmcb-nv4z.googlevideo.com", "Streaming"),
        ("in-ftd-705.fr3.vip.prod.criteo.com", "Advertising"),
        ("s0.2mdn.net", "Advertising"),
        ("securepubads.g.doubleclick.net", "Advertising"),
        ("fastlane.rubiconproject.com", "Advertising"),
        ("api.anthropic.com", "AI Services"),
        ("claude.ai", "AI Services"),
        ("chatgpt.com", "AI Services"),
        ("ab.chatgpt.com", "AI Services"),
        ("gemini.google.com", "AI Services"),
        ("my-bucket.s3.amazonaws.com", "Cloud/Storage"),
    ];
    for (host, cat) in cases {
        assert_eq!(rs.categorize(&https_to(host)).0, id(cat), "{host}");
    }
}

#[test]
fn ports_seen_on_real_home_traffic() {
    let (rs, id) = builtin();
    let cases = [
        (21116, 6, "Remote Access"),   // RustDesk
        (21116, 17, "Remote Access"),  // RustDesk
        (3389, 6, "Remote Access"),    // RDP
        (5938, 6, "Remote Access"),    // TeamViewer
        (5223, 6, "Network Services"), // Apple push
        (5228, 6, "Network Services"), // Google FCM push
        (7680, 6, "Software Updates"), // Windows Delivery Optimization
        (13231, 17, "VPN/Tunnels"),    // RouterOS WireGuard default
        (8883, 6, "Network Services"), // MQTT over TLS
    ];
    for (port, proto, cat) in cases {
        assert_eq!(
            rs.categorize(&flow("192.168.88.10", "1.1.1.1", port, proto))
                .0,
            id(cat),
            "{proto}/{port}"
        );
    }
}

#[test]
fn websites_hosted_on_cloud_providers_are_web() {
    let (rs, id) = builtin();
    // www.ntop.org, Datadog, Amplitude… are hosted on AWS (AS16509) but are not "cloud storage".
    let aws_https =
        flow("192.168.88.10", "52.1.2.3", 443, 6).with_geo(Some("US".into()), Some(16509), None);
    assert_eq!(rs.categorize(&aws_https).0, id("Web"));
    let aws_other =
        flow("192.168.88.10", "52.1.2.3", 9000, 6).with_geo(Some("US".into()), Some(16509), None);
    assert_eq!(rs.categorize(&aws_other).0, id("Cloud/Storage"));
}

#[test]
fn services_with_their_own_networks_are_recognized_by_asn() {
    let (rs, id) = builtin();
    let via =
        |asn: u32| flow("192.168.88.10", "149.154.167.50", 443, 6).with_geo(None, Some(asn), None);
    for (asn, cat) in [
        (62041, "Messaging/VoIP"), // Telegram
        (62014, "Messaging/VoIP"), // Telegram
        (30103, "Messaging/VoIP"), // Zoom
        (6507, "Gaming"),          // Riot Games
        (57976, "Gaming"),         // Blizzard
    ] {
        assert_eq!(rs.categorize(&via(asn)).0, id(cat), "AS{asn}");
    }
}

#[test]
fn hostname_rules_also_match_cname_aliases() {
    use std::sync::Arc;
    use tmon_core::rules::Confidence;
    let (rs, id) = builtin();
    let aliases: Arc<[Arc<str>]> = Arc::from(vec![
        Arc::<str>::from("edge.example.net"),
        Arc::<str>::from("rr3.googlevideo.com"),
    ]);
    // Neither the queried name nor port 8443 identifies the service: only the alias does.
    let f = flow("192.168.88.10", "94.236.166.212", 8443, 6)
        .with_remote_host(Some("media.unknown-brand.example".into()))
        .with_aliases(aliases.clone());
    let (cat, rule) = rs.categorize(&f);
    assert_eq!(cat, id("Streaming"));
    let b = load_builtin().unwrap();
    let r = b.rules.iter().find(|r| Some(r.id) == rule).unwrap();
    assert_eq!(r.match_.confidence(), Confidence::Detected);

    // Without the alias it is not Streaming.
    let plain = flow("192.168.88.10", "94.236.166.212", 8443, 6)
        .with_remote_host(Some("media.unknown-brand.example".into()));
    assert_ne!(rs.categorize(&plain).0, id("Streaming"));

    // Indexed lookup agrees with the reference evaluator on aliased flows.
    let ai = https_to("chatgpt.com").with_aliases(aliases);
    assert_eq!(
        rs.categorize(&ai),
        tmon_core::rules::categorize_linear(&rs, &ai)
    );
    assert!(
        f.with_aliases(Arc::from(Vec::new()))
            .remote_aliases
            .is_none()
    );
}
