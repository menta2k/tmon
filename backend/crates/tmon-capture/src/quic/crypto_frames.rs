//! Frames inside a decrypted Initial payload: collect CRYPTO data (R5).

use super::QuicError;
use super::varint::read_varint;

const PADDING: u64 = 0x00;
const PING: u64 = 0x01;
const ACK: u64 = 0x02;
const ACK_ECN: u64 = 0x03;
const CRYPTO: u64 = 0x06;
const CLOSE_TRANSPORT: u64 = 0x1c;
const CLOSE_APP: u64 = 0x1d;

/// One CRYPTO frame: stream offset + data.
pub type CryptoChunk<'a> = (u64, &'a [u8]);

fn skip_ack(p: &[u8], at: &mut usize, ecn: bool) -> Result<(), QuicError> {
    read_varint(p, at)?; // largest acknowledged
    read_varint(p, at)?; // delay
    let ranges = read_varint(p, at)?;
    read_varint(p, at)?; // first range
    for _ in 0..ranges.min(p.len() as u64) {
        read_varint(p, at)?;
        read_varint(p, at)?;
    }
    if ecn {
        for _ in 0..3 {
            read_varint(p, at)?;
        }
    }
    Ok(())
}

/// All CRYPTO chunks in a plaintext Initial payload. Stops at
/// CONNECTION_CLOSE; any frame type not allowed in Initial packets is an error.
pub fn crypto_chunks(p: &[u8]) -> Result<Vec<CryptoChunk<'_>>, QuicError> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < p.len() {
        match read_varint(p, &mut at)? {
            PADDING | PING => {}
            ACK => skip_ack(p, &mut at, false)?,
            ACK_ECN => skip_ack(p, &mut at, true)?,
            CRYPTO => {
                let offset = read_varint(p, &mut at)?;
                let len = read_varint(p, &mut at)? as usize;
                let data = p
                    .get(at..at.saturating_add(len))
                    .ok_or(QuicError::Truncated)?;
                at += len;
                out.push((offset, data));
            }
            CLOSE_TRANSPORT | CLOSE_APP => break,
            _ => return Err(QuicError::Malformed),
        }
    }
    Ok(out)
}
