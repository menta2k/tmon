//! QUIC long header parsing (RFC 9000 §17.2) for Initial packets.

use super::QuicError;
use super::varint::read_varint;

const FORM_LONG: u8 = 0x80;
const MAX_CID_LEN: usize = 20;

/// A long-header packet within a datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LongHeader<'a> {
    pub version: u32,
    /// Bits 4–5 of the first byte (still protected bits excluded).
    pub packet_type: u8,
    pub dcid: &'a [u8],
    /// Offset of the (protected) packet number.
    pub pn_offset: usize,
    /// Total length of this packet in the datagram.
    pub packet_len: usize,
}

pub fn is_long(d: &[u8]) -> bool {
    d.first().is_some_and(|b| b & FORM_LONG != 0)
}

fn cid(d: &[u8], at: &mut usize) -> Result<std::ops::Range<usize>, QuicError> {
    let len = *d.get(*at).ok_or(QuicError::Truncated)? as usize;
    if len > MAX_CID_LEN {
        return Err(QuicError::Malformed);
    }
    let start = *at + 1;
    let end = start + len;
    if end > d.len() {
        return Err(QuicError::Truncated);
    }
    *at = end;
    Ok(start..end)
}

/// Parse the long header at the start of `d` (an Initial is expected for
/// `packet_len`; other types still report their length when they carry one).
pub fn parse(d: &[u8]) -> Result<LongHeader<'_>, QuicError> {
    if !is_long(d) || d.len() < 7 {
        return Err(QuicError::Malformed);
    }
    let version = u32::from_be_bytes([d[1], d[2], d[3], d[4]]);
    let packet_type = (d[0] >> 4) & 0x03;
    let mut at = 5;
    let dcid = cid(d, &mut at)?;
    cid(d, &mut at)?;
    if version == 0 {
        return Err(QuicError::VersionNegotiation);
    }
    let initial =
        super::keys::Version::from_wire(version).is_some_and(|v| v.initial_type() == packet_type);
    if initial {
        let token_len = read_varint(d, &mut at)? as usize;
        at = at
            .checked_add(token_len)
            .filter(|e| *e <= d.len())
            .ok_or(QuicError::Truncated)?;
    }
    let length = read_varint(d, &mut at)? as usize;
    let packet_len = at
        .checked_add(length)
        .filter(|e| *e <= d.len())
        .ok_or(QuicError::Truncated)?;
    Ok(LongHeader {
        version,
        packet_type,
        dcid: &d[dcid],
        pn_offset: at,
        packet_len,
    })
}
