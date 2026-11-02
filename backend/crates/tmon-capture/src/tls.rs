//! TLS ClientHello → server name, first ALPN, ECH flag (R4, FR-004, FR-008).

use std::sync::Arc;
use tls_parser::{
    TlsExtension, TlsMessage, TlsMessageHandshake, parse_tls_extensions,
    parse_tls_message_handshake, parse_tls_plaintext,
};
use tmon_core::naming::normalize_alpn;

const CONTENT_HANDSHAKE: u8 = 0x16;
const HANDSHAKE_CLIENT_HELLO: u8 = 1;
const RECORD_HEADER: usize = 5;
const HANDSHAKE_HEADER: usize = 4;
const EXT_ECH: u16 = 0xfe0d;
const SNI_HOST_NAME: u8 = 0;

/// Names visible in a ClientHello. `sni` is raw (not yet normalized).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelloInfo {
    pub sni: Option<String>,
    pub alpn: Option<Arc<str>>,
    pub ech: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsError {
    /// Bytes do not form a valid ClientHello.
    Parse,
}

/// First bytes of a TLS handshake record carrying a ClientHello.
pub fn looks_like_client_hello(p: &[u8]) -> bool {
    p.len() > RECORD_HEADER
        && p[0] == CONTENT_HANDSHAKE
        && p[1] == 3
        && p[2] <= 4
        && p[RECORD_HEADER] == HANDSHAKE_CLIENT_HELLO
}

/// Length of the first record including its header, once the header is known.
pub fn record_len(p: &[u8]) -> Option<usize> {
    let h = p.get(..RECORD_HEADER)?;
    Some(RECORD_HEADER + usize::from(u16::from_be_bytes([h[3], h[4]])))
}

/// Length of a bare handshake message including its header.
pub fn handshake_len(p: &[u8]) -> Option<usize> {
    let h = p.get(..HANDSHAKE_HEADER)?;
    Some(HANDSHAKE_HEADER + (usize::from(h[1]) << 16 | usize::from(h[2]) << 8 | usize::from(h[3])))
}

/// Parse one complete TLS record holding a ClientHello.
pub fn from_record(p: &[u8]) -> Result<HelloInfo, TlsError> {
    let (_, rec) = parse_tls_plaintext(p).map_err(|_| TlsError::Parse)?;
    rec.msg.iter().find_map(hello).ok_or(TlsError::Parse)?
}

/// Parse a bare ClientHello handshake message (QUIC CRYPTO data).
pub fn from_handshake(p: &[u8]) -> Result<HelloInfo, TlsError> {
    let (_, msg) = parse_tls_message_handshake(p).map_err(|_| TlsError::Parse)?;
    hello(&msg).ok_or(TlsError::Parse)?
}

fn hello(m: &TlsMessage<'_>) -> Option<Result<HelloInfo, TlsError>> {
    let TlsMessage::Handshake(TlsMessageHandshake::ClientHello(ch)) = m else {
        return None;
    };
    Some(extensions(ch.ext.unwrap_or_default()))
}

fn extensions(raw: &[u8]) -> Result<HelloInfo, TlsError> {
    let (_, exts) = parse_tls_extensions(raw).map_err(|_| TlsError::Parse)?;
    let mut info = HelloInfo::default();
    for e in &exts {
        match e {
            TlsExtension::SNI(names) => {
                info.sni = names
                    .iter()
                    .find(|(t, _)| t.0 == SNI_HOST_NAME)
                    .and_then(|(_, n)| std::str::from_utf8(n).ok())
                    .map(str::to_owned);
            }
            TlsExtension::ALPN(protos) => {
                info.alpn = protos.first().and_then(|p| normalize_alpn(p));
            }
            TlsExtension::Unknown(t, _) if t.0 == EXT_ECH => info.ech = true,
            _ => {}
        }
    }
    Ok(info)
}
