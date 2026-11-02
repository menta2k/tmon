//! TZSP framing (R1): version 1, type 0 ("received tag list"), Ethernet.
//!
//! ```text
//! ver(1) type(1) encap(2, BE) | tags… END(0x01) | Ethernet frame
//! ```
//! PADDING (0x00) is one byte, END (0x01) ends the tags, any other tag is
//! TLV: tag(1) len(1) value[len].

pub const VERSION: u8 = 1;
pub const TYPE_RECEIVED: u8 = 0;
pub const ENCAP_ETHERNET: u16 = 1;
const TAG_PADDING: u8 = 0x00;
const TAG_END: u8 = 0x01;
const HEADER_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TzspError {
    /// Too short, missing END, or a tag running past the end.
    Malformed,
    /// Version or type we do not handle.
    Unsupported,
    /// Encapsulation other than Ethernet.
    Encap,
}

/// The encapsulated Ethernet frame of one TZSP datagram.
pub fn decode(d: &[u8]) -> Result<&[u8], TzspError> {
    let hdr = d.get(..HEADER_LEN).ok_or(TzspError::Malformed)?;
    if hdr[0] != VERSION || hdr[1] != TYPE_RECEIVED {
        return Err(TzspError::Unsupported);
    }
    if u16::from_be_bytes([hdr[2], hdr[3]]) != ENCAP_ETHERNET {
        return Err(TzspError::Encap);
    }
    let mut i = HEADER_LEN;
    loop {
        match *d.get(i).ok_or(TzspError::Malformed)? {
            TAG_END => return Ok(&d[i + 1..]),
            TAG_PADDING => i += 1,
            _ => {
                let len = *d.get(i + 1).ok_or(TzspError::Malformed)? as usize;
                i += 2 + len;
                if i > d.len() {
                    return Err(TzspError::Malformed);
                }
            }
        }
    }
}

/// Wrap an Ethernet frame the way RouterOS does (no tags, just END).
pub fn encode(frame: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(frame.len() + HEADER_LEN + 1);
    out.extend_from_slice(&[VERSION, TYPE_RECEIVED]);
    out.extend_from_slice(&ENCAP_ETHERNET.to_be_bytes());
    out.push(TAG_END);
    out.extend_from_slice(frame);
    out
}
