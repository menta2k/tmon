//! Bounded little-endian reader/writer primitives.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::limits::MAX_MESSAGE;

/// Why a message was rejected. Every variant maps to `ipc_invalid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("empty message")]
    Empty,
    #[error("message longer than {MAX_MESSAGE} bytes")]
    TooLong,
    #[error("truncated message")]
    Truncated,
    #[error("trailing bytes after message body")]
    Trailing,
    #[error("invalid {0}")]
    Invalid(&'static str),
    /// Not an error on the wire (unknown types are ignored), but callers
    /// need to know nothing was decoded.
    #[error("unknown message type {0:#04x}")]
    UnknownType(u8),
}

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.buf.len() < n {
            return Err(DecodeError::Truncated);
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub fn flag(&mut self, what: &'static str) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::Invalid(what)),
        }
    }

    /// `str8` of printable ASCII (0x20–0x7E), at most `max` bytes.
    pub fn str8(&mut self, max: usize, what: &'static str) -> Result<String, DecodeError> {
        let len = usize::from(self.u8()?);
        let raw = self.take(len)?;
        if len > max || !raw.iter().all(|b| (0x20..=0x7e).contains(b)) {
            return Err(DecodeError::Invalid(what));
        }
        // Printable ASCII is valid UTF-8.
        Ok(raw.iter().map(|&b| char::from(b)).collect())
    }

    pub fn ip(&mut self) -> Result<IpAddr, DecodeError> {
        match self.u8()? {
            4 => Ok(IpAddr::V4(Ipv4Addr::from(self.array::<4>()?))),
            6 => Ok(IpAddr::V6(Ipv6Addr::from(self.array::<16>()?))),
            _ => Err(DecodeError::Invalid("ip family")),
        }
    }

    pub fn finish(self) -> Result<(), DecodeError> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::Trailing)
        }
    }
}

/// Append-only writer. Callers keep values within the limits; `str8`
/// truncates defensively so an encoder can never emit an invalid string.
#[derive(Default)]
pub(crate) struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn with_kind(kind: u8) -> Self {
        Self { buf: vec![kind] }
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    pub fn str8(&mut self, s: &str, max: usize) {
        let clean: Vec<u8> = s
            .bytes()
            .filter(|b| (0x20..=0x7e).contains(b))
            .take(max.min(255))
            .collect();
        self.u8(clean.len() as u8);
        self.bytes(&clean);
    }

    pub fn ip(&mut self, ip: &IpAddr) {
        match ip {
            IpAddr::V4(a) => {
                self.u8(4);
                self.bytes(&a.octets());
            }
            IpAddr::V6(a) => {
                self.u8(6);
                self.bytes(&a.octets());
            }
        }
    }
}
