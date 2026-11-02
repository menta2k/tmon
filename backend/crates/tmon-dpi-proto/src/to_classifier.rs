//! tmon → classifier messages.

use crate::limits::{MAX_L3, MAX_MESSAGE, MIN_L3, kind};
use crate::msg::{Dir, Key, Packet, ToClassifier};
use crate::wire::{DecodeError, Reader, Writer};

impl ToClassifier {
    /// Encode one message. Packets with `l3` outside the limits are the
    /// caller's bug; they encode but the peer will reject them.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            ToClassifier::Packet(p) => {
                let mut w = Writer::with_kind(kind::PACKET);
                p.key.write(&mut w);
                w.u8(p.dir as u8);
                w.u64(p.ts_ms);
                w.u16(p.l3.len().min(usize::from(u16::MAX)) as u16);
                w.bytes(&p.l3);
                w.buf
            }
            ToClassifier::Forget(k) => {
                let mut w = Writer::with_kind(kind::FORGET);
                k.write(&mut w);
                w.buf
            }
            ToClassifier::Ping(n) => {
                let mut w = Writer::with_kind(kind::PING);
                w.u64(*n);
                w.buf
            }
        }
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let (&ty, body) = buf.split_first().ok_or(DecodeError::Empty)?;
        if buf.len() > MAX_MESSAGE {
            return Err(DecodeError::TooLong);
        }
        let mut r = Reader::new(body);
        let msg = match ty {
            kind::PACKET => ToClassifier::Packet(read_packet(&mut r)?),
            kind::FORGET => ToClassifier::Forget(Key::read(&mut r)?),
            kind::PING => ToClassifier::Ping(r.u64()?),
            other => return Err(DecodeError::UnknownType(other)),
        };
        r.finish()?;
        Ok(msg)
    }
}

fn read_packet(r: &mut Reader<'_>) -> Result<Packet, DecodeError> {
    let key = Key::read(r)?;
    let dir = Dir::from_u8(r.u8()?)?;
    let ts_ms = r.u64()?;
    let len = usize::from(r.u16()?);
    if !(MIN_L3..=MAX_L3).contains(&len) {
        return Err(DecodeError::Invalid("l3 length"));
    }
    let l3 = r.take(len)?;
    let version = l3[0] >> 4;
    let family_ok = match version {
        4 => key.lan_ip.is_ipv4(),
        6 => key.lan_ip.is_ipv6(),
        _ => false,
    };
    if !family_ok {
        return Err(DecodeError::Invalid("l3 version"));
    }
    Ok(Packet {
        key,
        dir,
        ts_ms,
        l3: l3.to_vec(),
    })
}
