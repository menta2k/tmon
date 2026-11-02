//! RFC 9001 Appendix A and RFC 9369 Appendix A: client Initial keys and packet.

use tmon_capture::quic::{self, InitialKeys, Version, crypto_frames, keys};
use tmon_capture::tls;

const DCID: [u8; 8] = [0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08];

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn vector(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/vectors/{name}.hex", env!("CARGO_MANIFEST_DIR"));
    hex(&std::fs::read_to_string(path).unwrap())
}

#[test]
fn v1_client_keys_match_rfc9001_a1() {
    let s = keys::client_secrets(Version::V1, &DCID);
    assert_eq!(
        s.client_secret.to_vec(),
        hex("c00cf151ca5be075ed0ebfb5c80323c42d6b7db67881289af4008f1f6c357aea")
    );
    assert_eq!(s.key.to_vec(), hex("1f369613dd76d5467730efcbe3b1a22d"));
    assert_eq!(s.iv.to_vec(), hex("fa044b2f42a3fd3b46fb255c"));
    assert_eq!(s.hp.to_vec(), hex("9f50449e04a0e810283a1e9933adedd2"));
}

#[test]
fn v2_client_keys_match_rfc9369_a1() {
    let s = keys::client_secrets(Version::V2, &DCID);
    assert_eq!(
        s.client_secret.to_vec(),
        hex("14ec9d6eb9fd7af83bf5a668bc17a7e283766aade7ecd0891f70f9ff7f4bf47b")
    );
    assert_eq!(s.key.to_vec(), hex("8b1a0bc121284290a29e0971b5cd045d"));
    assert_eq!(s.iv.to_vec(), hex("91f73e2351d8fa91660e909f"));
    assert_eq!(s.hp.to_vec(), hex("45b95e15235d6f45a6b19cbcb0294ba9"));
}

fn open_client_initial(packet: &str, crypto: &str, version: Version) {
    let datagram = vector(packet);
    let pkts = quic::packets(&datagram);
    assert_eq!(pkts.len(), 1);
    let p = pkts[0].unwrap();
    assert_eq!(p.header.dcid, DCID);
    assert_eq!(p.initial_version().unwrap(), Some(version));
    let plain = quic::decrypt(&p, &InitialKeys::client(version, p.header.dcid)).unwrap();
    assert_eq!(plain.len(), 1162);
    let expected = vector(crypto);
    assert_eq!(&plain[..expected.len()], &expected[..]);
    assert!(plain[expected.len()..].iter().all(|b| *b == 0), "PADDING");

    let chunks = crypto_frames::crypto_chunks(&plain).unwrap();
    assert_eq!(chunks.len(), 1);
    let (offset, hello) = chunks[0];
    assert_eq!((offset, hello.len()), (0, 241));
    let info = tls::from_handshake(hello).unwrap();
    assert_eq!(info.sni.as_deref(), Some("example.com"));
    assert_eq!(info.alpn.as_deref(), Some("alpn"));
}

#[test]
fn v1_client_initial_decrypts_to_rfc9001_a2_payload() {
    open_client_initial("rfc9001_packet", "rfc9001_crypto", Version::V1);
}

#[test]
fn v2_client_initial_decrypts_to_rfc9369_a2_payload() {
    open_client_initial("rfc9369_packet", "rfc9369_crypto", Version::V2);
}

#[test]
fn tampered_packet_fails_to_decrypt() {
    let mut d = vector("rfc9001_packet");
    let last = d.len() - 1;
    d[last] ^= 1;
    let p = quic::packets(&d)[0].unwrap();
    assert_eq!(
        quic::decrypt(&p, &InitialKeys::client(Version::V1, p.header.dcid)),
        Err(quic::QuicError::Decrypt)
    );
    // wrong version keys also fail
    let good = vector("rfc9001_packet");
    let p = quic::packets(&good)[0].unwrap();
    assert!(quic::decrypt(&p, &InitialKeys::client(Version::V2, p.header.dcid)).is_err());
}

#[test]
fn fixture_encoder_reproduces_the_rfc_packets() {
    use tmon_capture::fixtures::quic::client_initial;
    for (v, pkt, crypto) in [
        (Version::V1, "rfc9001_packet", "rfc9001_crypto"),
        (Version::V2, "rfc9369_packet", "rfc9369_crypto"),
    ] {
        let built = client_initial(v, &DCID, &[], 2, &vector(crypto), 1162);
        assert_eq!(built, vector(pkt), "{v:?}");
    }
}
