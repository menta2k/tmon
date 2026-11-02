//! QUIC variable-length integers (RFC 9000 §16).

use super::QuicError;

pub fn read_varint(d: &[u8], at: &mut usize) -> Result<u64, QuicError> {
    let first = *d.get(*at).ok_or(QuicError::Truncated)?;
    let len = 1usize << (first >> 6);
    let bytes = d.get(*at..*at + len).ok_or(QuicError::Truncated)?;
    let v = bytes[1..]
        .iter()
        .fold(u64::from(first & 0x3f), |acc, b| (acc << 8) | u64::from(*b));
    *at += len;
    Ok(v)
}

/// Encode (used by fixtures).
pub fn write_varint(out: &mut Vec<u8>, v: u64) {
    match v {
        0..=0x3f => out.push(v as u8),
        0x40..=0x3fff => out.extend_from_slice(&((v as u16) | 0x4000).to_be_bytes()),
        0x4000..=0x3fff_ffff => out.extend_from_slice(&((v as u32) | 0x8000_0000).to_be_bytes()),
        _ => out.extend_from_slice(&(v | 0xc000_0000_0000_0000).to_be_bytes()),
    }
}
