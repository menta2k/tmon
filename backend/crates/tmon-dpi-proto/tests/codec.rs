//! Contract tests for protocol v1 (contracts/dpi-ipc.md).

use proptest::prelude::*;
use std::net::IpAddr;
use tmon_dpi_proto::*;

fn key4() -> Key {
    Key {
        lan_ip: "192.168.1.10".parse().unwrap(),
        lan_port: 50_000,
        remote_ip: "203.0.113.5".parse().unwrap(),
        remote_port: 6881,
        proto: 6,
    }
}

fn key6() -> Key {
    Key {
        lan_ip: "fd00::10".parse().unwrap(),
        lan_port: 1,
        remote_ip: "2001:db8::1".parse().unwrap(),
        remote_port: 443,
        proto: 17,
    }
}

fn l3_v4(len: usize) -> Vec<u8> {
    let mut v = vec![0u8; len];
    v[0] = 0x45;
    v
}

fn packet(l3: Vec<u8>) -> ToClassifier {
    ToClassifier::Packet(Packet {
        key: key4(),
        dir: Dir::RemoteToLan,
        ts_ms: 1_767_225_600_000,
        l3,
    })
}

fn entry(app: &str, cat: &str) -> CatalogEntry {
    CatalogEntry {
        app: app.into(),
        category: cat.into(),
    }
}

fn classification() -> Classification {
    Classification {
        key: key6(),
        app: "WireGuard".into(),
        category: "VPN".into(),
        confident: true,
        final_: true,
        host: Some("vpn.example.net".into()),
    }
}

fn stats() -> Stats {
    Stats {
        uptime_ms: 1,
        packets_in: 2,
        packets_invalid: 3,
        flows_active: 4,
        flows_created: 5,
        evicted_lru: 6,
        evicted_idle: 7,
        evicted_memory: 8,
        classified_confident: 9,
        classified_guess: 10,
        giveups: 11,
        mem_bytes: 12,
        mem_limit_bytes: 13,
        top: vec![TopApp {
            app: "BitTorrent".into(),
            flows: 12,
        }],
    }
}

#[test]
fn to_classifier_round_trips() {
    for m in [
        packet(l3_v4(60)),
        ToClassifier::Forget(key6()),
        ToClassifier::Ping(42),
    ] {
        assert_eq!(ToClassifier::decode(&m.encode()), Ok(m));
    }
}

#[test]
fn from_classifier_round_trips() {
    let hello = Hello {
        proto_version: PROTO_VERSION,
        impl_name: "tmon-dpi".into(),
        impl_version: "0.1.0".into(),
        engine: "nDPI 6.0".into(),
        max_flows: 50_000,
        catalog: vec![entry("BitTorrent", "Download")],
    };
    for m in [
        FromClassifier::Hello(hello),
        FromClassifier::Classification(classification()),
        FromClassifier::Classification(Classification {
            host: None,
            category: String::new(),
            confident: false,
            final_: false,
            ..classification()
        }),
        FromClassifier::Stats(stats()),
        FromClassifier::HelloCatalog(vec![entry("Steam", "Game")]),
        FromClassifier::Pong(7),
    ] {
        assert_eq!(FromClassifier::decode(&m.encode()), Ok(m));
    }
}

#[test]
fn large_catalog_is_split_within_the_size_limit() {
    let catalog: Vec<CatalogEntry> = (0..600)
        .map(|i| entry(&format!("{i:0>48}"), &"c".repeat(48)))
        .collect();
    let hello = Hello {
        proto_version: 1,
        impl_name: "x".repeat(64),
        impl_version: "y".repeat(64),
        engine: "z".repeat(64),
        max_flows: 1,
        catalog: catalog.clone(),
    };
    let msgs = hello.to_messages();
    assert!(msgs.len() > 1);
    let mut got = Vec::new();
    for (i, m) in msgs.iter().enumerate() {
        assert!(m.len() <= MAX_MESSAGE, "message {i} is {} bytes", m.len());
        match FromClassifier::decode(m).unwrap() {
            FromClassifier::Hello(h) => got.extend(h.catalog),
            FromClassifier::HelloCatalog(c) => got.extend(c),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(got, catalog);
}

#[test]
fn rejects_malformed_input() {
    let good = packet(l3_v4(60)).encode();
    assert_eq!(ToClassifier::decode(&[]), Err(DecodeError::Empty));
    assert_eq!(
        ToClassifier::decode(&good[..good.len() - 1]),
        Err(DecodeError::Truncated)
    );
    let mut trailing = good.clone();
    trailing.push(0);
    assert_eq!(ToClassifier::decode(&trailing), Err(DecodeError::Trailing));
    assert_eq!(
        ToClassifier::decode(&[0x7f, 1, 2]),
        Err(DecodeError::UnknownType(0x7f))
    );
    assert_eq!(
        FromClassifier::decode(&[0x01]),
        Err(DecodeError::UnknownType(0x01))
    );
    let mut big = vec![kind::PING];
    big.resize(MAX_MESSAGE + 1, 0);
    assert_eq!(ToClassifier::decode(&big), Err(DecodeError::TooLong));
}

#[test]
fn rejects_l3_out_of_limits_and_wrong_family() {
    for len in [MIN_L3 - 1, MAX_L3 + 1] {
        let m = packet(l3_v4(len)).encode();
        assert!(matches!(
            ToClassifier::decode(&m),
            Err(DecodeError::Invalid(_)) | Err(DecodeError::TooLong)
        ));
    }
    let mut v6 = l3_v4(60);
    v6[0] = 0x60;
    assert_eq!(
        ToClassifier::decode(&packet(v6).encode()),
        Err(DecodeError::Invalid("l3 version"))
    );
}

#[test]
fn rejects_bad_enums_strings_and_families() {
    // dir = 2
    let mut m = packet(l3_v4(60)).encode();
    let dir_at = 1 + 5 + 2 + 5 + 2 + 1;
    m[dir_at] = 2;
    assert_eq!(ToClassifier::decode(&m), Err(DecodeError::Invalid("dir")));

    // mixed key families
    let mixed = Key {
        remote_ip: "2001:db8::1".parse::<IpAddr>().unwrap(),
        ..key4()
    };
    let m = ToClassifier::Forget(mixed).encode();
    assert_eq!(
        ToClassifier::decode(&m),
        Err(DecodeError::Invalid("key family"))
    );

    // confidence = 2
    let mut m = FromClassifier::Classification(classification()).encode();
    let host_len = "vpn.example.net".len();
    let conf_at = m.len() - 1 - host_len - 2;
    m[conf_at] = 2;
    assert_eq!(
        FromClassifier::decode(&m),
        Err(DecodeError::Invalid("confidence"))
    );
}

#[test]
fn rejects_non_printable_and_long_names() {
    let mut m = FromClassifier::Classification(Classification {
        app: "Bit".into(),
        ..classification()
    })
    .encode();
    let app_at = m.iter().position(|&b| b == b'B').unwrap();
    m[app_at] = 0x07;
    assert_eq!(FromClassifier::decode(&m), Err(DecodeError::Invalid("app")));

    // A hand-built 49-byte app name.
    let mut raw = vec![kind::CLASSIFICATION];
    raw.extend([4, 10, 0, 0, 1, 1, 0, 4, 1, 1, 1, 1, 2, 0, 6]);
    raw.push(49);
    raw.extend(std::iter::repeat_n(b'a', 49));
    raw.extend([0, 1, 0, 0]);
    assert_eq!(
        FromClassifier::decode(&raw),
        Err(DecodeError::Invalid("app"))
    );

    // Empty app name.
    let mut raw = vec![kind::CLASSIFICATION];
    raw.extend([4, 10, 0, 0, 1, 1, 0, 4, 1, 1, 1, 1, 2, 0, 6, 0, 0, 1, 0, 0]);
    assert_eq!(
        FromClassifier::decode(&raw),
        Err(DecodeError::Invalid("app"))
    );
}

#[test]
fn rejects_counts_above_limits() {
    let mut s = FromClassifier::Stats(Stats {
        top: vec![],
        ..stats()
    })
    .encode();
    *s.last_mut().unwrap() = (MAX_TOP + 1) as u8;
    assert_eq!(
        FromClassifier::decode(&s),
        Err(DecodeError::Invalid("top count"))
    );
    let mut c = vec![kind::HELLO_CATALOG];
    c.extend(((MAX_CATALOG + 1) as u16).to_le_bytes());
    assert_eq!(
        FromClassifier::decode(&c),
        Err(DecodeError::Invalid("catalog count"))
    );
}

#[test]
fn encoder_never_emits_invalid_strings() {
    let m = FromClassifier::Classification(Classification {
        app: format!("{}\u{7}é", "a".repeat(60)),
        host: Some("h".repeat(300)),
        ..classification()
    });
    match FromClassifier::decode(&m.encode()).unwrap() {
        FromClassifier::Classification(c) => {
            assert_eq!(c.app.len(), MAX_NAME);
            assert_eq!(c.host.unwrap().len(), MAX_HOST);
        }
        other => panic!("unexpected {other:?}"),
    }
}

proptest! {
    #[test]
    fn decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = ToClassifier::decode(&bytes);
        let _ = FromClassifier::decode(&bytes);
    }

    #[test]
    fn decode_of_typed_prefix_never_panics(
        ty in prop::sample::select(vec![0x01u8, 0x02, 0x03, 0x81, 0x82, 0x83, 0x84, 0x85]),
        body in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let mut m = vec![ty];
        m.extend(body);
        let _ = ToClassifier::decode(&m);
        let _ = FromClassifier::decode(&m);
    }

    #[test]
    fn packets_round_trip(
        len in MIN_L3..=1500usize,
        ts in any::<u64>(),
        lp in any::<u16>(),
        rp in any::<u16>(),
        reply in any::<bool>(),
    ) {
        let m = ToClassifier::Packet(Packet {
            key: Key { lan_port: lp, remote_port: rp, ..key4() },
            dir: if reply { Dir::RemoteToLan } else { Dir::LanToRemote },
            ts_ms: ts,
            l3: l3_v4(len),
        });
        prop_assert_eq!(ToClassifier::decode(&m.encode()), Ok(m));
    }

    #[test]
    fn classifications_round_trip(app in "[ -~]{1,48}", cat in "[ -~]{0,48}", c in any::<bool>(), f in any::<bool>()) {
        let m = FromClassifier::Classification(Classification {
            app, category: cat, confident: c, final_: f, ..classification()
        });
        prop_assert_eq!(FromClassifier::decode(&m.encode()), Ok(m));
    }
}
