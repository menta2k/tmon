use std::net::IpAddr;
use tmon_capture::fixtures::{self, ACK, SYN, addr};
use tmon_capture::frame::{self, Reject, Transport};
use tmon_capture::tzsp;

#[test]
fn routeros_style_tzsp_decodes_to_tcp() {
    let d = fixtures::tcp(
        addr("192.168.88.10:51000"),
        addr("142.250.1.1:443"),
        7,
        ACK,
        b"hello",
    );
    let p = frame::decode(&d).unwrap();
    assert_eq!(p.src_ip, "192.168.88.10".parse::<IpAddr>().unwrap());
    assert_eq!(p.dst_ip, "142.250.1.1".parse::<IpAddr>().unwrap());
    assert_eq!((p.src_port, p.dst_port, p.proto()), (51000, 443, 6));
    assert_eq!(p.payload, b"hello");
    let Transport::Tcp(t) = p.transport else {
        panic!("tcp")
    };
    assert_eq!(t.seq, 7);
    assert!(t.ack && !t.syn);
}

#[test]
fn padding_and_tlv_tags_are_skipped() {
    let eth = fixtures::ethernet(
        &fixtures::udp_ip(addr("10.0.0.2:5353"), addr("10.0.0.1:53"), b"q"),
        &[],
    );
    // header, PADDING, tag 0x0a len 2, PADDING, END, frame
    let mut d = vec![1, 0, 0, 1, 0x00, 0x0a, 2, 0xaa, 0xbb, 0x00, 0x01];
    d.extend_from_slice(&eth);
    let p = frame::decode(&d).unwrap();
    assert_eq!((p.proto(), p.payload), (17, &b"q"[..]));
}

#[test]
fn bad_tzsp_is_classified() {
    let eth = fixtures::ethernet(
        &fixtures::udp_ip(addr("10.0.0.2:1"), addr("10.0.0.1:2"), b""),
        &[],
    );
    let with = |hdr: &[u8]| [hdr, &eth].concat();
    assert_eq!(frame::decode(&[1, 0]), Err(Reject::MalformedTzsp));
    assert_eq!(
        frame::decode(&[1, 0, 0, 1, 0x00, 0x00]),
        Err(Reject::MalformedTzsp),
        "no END"
    );
    assert_eq!(
        frame::decode(&[1, 0, 0, 1, 0x0a, 200, 1, 2]),
        Err(Reject::MalformedTzsp),
        "tag past end"
    );
    assert_eq!(
        frame::decode(&with(&[2, 0, 0, 1, 1])),
        Err(Reject::UnsupportedTzsp)
    );
    assert_eq!(
        frame::decode(&with(&[1, 1, 0, 1, 1])),
        Err(Reject::UnsupportedTzsp)
    );
    assert_eq!(
        frame::decode(&with(&[1, 0, 0, 18, 1])),
        Err(Reject::UnsupportedEncap)
    );
    assert_eq!(tzsp::decode(&tzsp::encode(&eth)), Ok(&eth[..]));
}

#[test]
fn vlan_tags_and_ipv6_are_supported() {
    for vlans in [&[][..], &[10], &[100, 200]] {
        let ip = fixtures::tcp_ip(
            addr("[fd00::10]:50000"),
            addr("[2a00:1450::1]:443"),
            1,
            SYN,
            b"",
        );
        let d = tzsp::encode(&fixtures::ethernet(&ip, vlans));
        let p = frame::decode(&d).unwrap();
        assert_eq!(
            p.dst_ip,
            "2a00:1450::1".parse::<IpAddr>().unwrap(),
            "{vlans:?}"
        );
        let Transport::Tcp(t) = p.transport else {
            panic!()
        };
        assert!(t.syn);
    }
}

#[test]
fn ipv6_extension_headers_are_walked() {
    let mut ip = fixtures::udp_ip(addr("[fd00::10]:5000"), addr("[fd00::1]:53"), b"dns");
    // Insert a hop-by-hop header (8 bytes: next=UDP, len=0, PadN) after the fixed header.
    let hbh = [17u8, 0, 1, 4, 0, 0, 0, 0];
    ip[6] = 0; // next header = hop-by-hop
    let plen = u16::from_be_bytes([ip[4], ip[5]]) + 8;
    ip[4..6].copy_from_slice(&plen.to_be_bytes());
    ip.splice(40..40, hbh);
    let d = fixtures::tzsp_ip(&ip);
    let p = frame::decode(&d).unwrap();
    assert_eq!((p.proto(), p.dst_port, p.payload), (17, 53, &b"dns"[..]));
}

#[test]
fn fragments_non_ip_and_truncation_are_rejected() {
    let mut ip = fixtures::udp_ip(addr("10.0.0.2:5000"), addr("10.0.0.1:53"), b"xxxxxxxx");
    ip[6] |= 0x20; // more fragments
    assert_eq!(
        frame::decode(&fixtures::tzsp_ip(&ip)),
        Err(Reject::Fragment)
    );

    let mut arp = fixtures::ethernet(&[0u8; 28], &[]);
    arp[12..14].copy_from_slice(&0x0806u16.to_be_bytes());
    assert_eq!(
        frame::decode(&tzsp::encode(&arp)),
        Err(Reject::UnsupportedL3)
    );

    let mut icmp = fixtures::udp_ip(addr("10.0.0.2:1"), addr("10.0.0.1:2"), b"");
    icmp[9] = 1; // protocol ICMP
    let d = fixtures::tzsp_ip(&icmp);
    let r = frame::decode(&d);
    assert!(
        matches!(r, Err(Reject::UnsupportedL3 | Reject::MalformedFrame)),
        "{r:?}"
    );

    let full = fixtures::tcp(addr("10.0.0.2:1"), addr("10.0.0.1:2"), 1, ACK, b"payload");
    for cut in [5, 14, 20, 38, 50] {
        assert_eq!(
            frame::decode(&full[..cut]),
            Err(Reject::MalformedFrame),
            "cut {cut}"
        );
    }
}
