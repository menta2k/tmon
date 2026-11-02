//! QUIC client Initial encoder (same key schedule as the decoder).

use ring::aead::{Aad, Nonce};

use crate::quic::InitialKeys;
use crate::quic::keys::Version;
use crate::quic::varint::write_varint;

const PN_LEN: usize = 4;
const TAG_LEN: usize = 16;

/// A CRYPTO frame at `offset`.
pub fn crypto_frame(offset: u64, data: &[u8]) -> Vec<u8> {
    let mut f = vec![0x06];
    write_varint(&mut f, offset);
    write_varint(&mut f, data.len() as u64);
    f.extend_from_slice(data);
    f
}

/// Protected client Initial packet carrying `frames`, PADDING up to
/// `payload_len` plaintext bytes.
pub fn client_initial(
    version: Version,
    dcid: &[u8],
    scid: &[u8],
    pn: u32,
    frames: &[u8],
    payload_len: usize,
) -> Vec<u8> {
    let keys = InitialKeys::client(version, dcid);
    let mut payload = frames.to_vec();
    payload.resize(payload_len.max(frames.len()).max(20), 0);
    let mut hdr = vec![0xc0 | (version.initial_type() << 4) | (PN_LEN as u8 - 1)];
    hdr.extend_from_slice(&version.wire().to_be_bytes());
    hdr.push(dcid.len() as u8);
    hdr.extend_from_slice(dcid);
    hdr.push(scid.len() as u8);
    hdr.extend_from_slice(scid);
    write_varint(&mut hdr, 0); // token length
    let len = (PN_LEN + payload.len() + TAG_LEN) as u64;
    // Always the 2-byte varint form, like the RFC sample.
    hdr.extend_from_slice(&((len as u16) | 0x4000).to_be_bytes());
    let pn_offset = hdr.len();
    hdr.extend_from_slice(&pn.to_be_bytes());

    let mut nonce = keys.iv;
    for (n, b) in nonce
        .iter_mut()
        .rev()
        .zip(u64::from(pn).to_be_bytes().iter().rev())
    {
        *n ^= b;
    }
    keys.key
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(&hdr[..]),
            &mut payload,
        )
        .expect("seal");
    // RFC 9001 §5.4.2: the sample starts 4 bytes after the packet number start.
    let sample = 4usize.saturating_sub(PN_LEN);
    let mask = keys
        .hp
        .new_mask(&payload[sample..sample + 16])
        .expect("mask");
    hdr[0] ^= mask[0] & 0x0f;
    for i in 0..PN_LEN {
        hdr[pn_offset + i] ^= mask[1 + i];
    }
    hdr.extend_from_slice(&payload);
    hdr
}
