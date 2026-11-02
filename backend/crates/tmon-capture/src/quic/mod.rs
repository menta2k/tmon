//! QUIC Initial packets → ClientHello bytes (R5, FR-005). Only client
//! Initial packets are opened; nothing else is decrypted.

pub mod crypto_frames;
pub mod header;
pub mod keys;
pub mod varint;

use ring::aead::{Aad, Nonce};

pub use header::LongHeader;
pub use keys::{InitialKeys, Version};

const SAMPLE_LEN: usize = 16;
const SAMPLE_OFFSET: usize = 4;
const TAG_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicError {
    Truncated,
    Malformed,
    VersionNegotiation,
    UnsupportedVersion,
    /// Header protection or AEAD failed (wrong keys or corrupt packet).
    Decrypt,
}

/// One long-header packet inside a (possibly coalesced) datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitialPacket<'a> {
    pub header: LongHeader<'a>,
    pub bytes: &'a [u8],
}

/// Long-header packets coalesced in one UDP payload (RFC 9000 §12.2), up to
/// the first short-header packet or parse error.
pub fn packets(d: &[u8]) -> Vec<Result<InitialPacket<'_>, QuicError>> {
    let mut out = Vec::new();
    let mut rest = d;
    while header::is_long(rest) {
        match header::parse(rest) {
            Ok(h) => {
                out.push(Ok(InitialPacket {
                    header: h,
                    bytes: &rest[..h.packet_len],
                }));
                rest = &rest[h.packet_len..];
            }
            Err(e) => {
                out.push(Err(e));
                break;
            }
        }
    }
    out
}

impl InitialPacket<'_> {
    /// Initial packet of a version we can open.
    pub fn initial_version(&self) -> Result<Option<Version>, QuicError> {
        let v = Version::from_wire(self.header.version).ok_or(QuicError::UnsupportedVersion)?;
        Ok((v.initial_type() == self.header.packet_type).then_some(v))
    }
}

/// Remove header protection and decrypt; returns the plaintext frames.
pub fn decrypt(p: &InitialPacket<'_>, keys: &InitialKeys) -> Result<Vec<u8>, QuicError> {
    let pn_off = p.header.pn_offset;
    let sample_at = pn_off + SAMPLE_OFFSET;
    let sample = p
        .bytes
        .get(sample_at..sample_at + SAMPLE_LEN)
        .ok_or(QuicError::Truncated)?;
    let mask = keys.hp.new_mask(sample).map_err(|_| QuicError::Decrypt)?;
    let first = p.bytes[0] ^ (mask[0] & 0x0f);
    let pn_len = usize::from(first & 0x03) + 1;
    let body_at = pn_off + pn_len;
    if body_at + TAG_LEN > p.bytes.len() {
        return Err(QuicError::Truncated);
    }
    let mut header = p.bytes[..body_at].to_vec();
    header[0] = first;
    let mut pn: u64 = 0;
    for (i, m) in mask[1..=pn_len].iter().enumerate() {
        header[pn_off + i] ^= m;
        pn = (pn << 8) | u64::from(header[pn_off + i]);
    }
    let mut nonce = keys.iv;
    for (n, b) in nonce.iter_mut().rev().zip(pn.to_be_bytes().iter().rev()) {
        *n ^= b;
    }
    let mut body = p.bytes[body_at..].to_vec();
    let plain = keys
        .key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(&header[..]),
            &mut body,
        )
        .map_err(|_| QuicError::Decrypt)?;
    Ok(plain.to_vec())
}
