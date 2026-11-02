//! UDP: QUIC client Initial packets and DNS responses.

use std::sync::Arc;
use tmon_core::naming::{ConnKey, ConnName, NameSource};

use super::{CaptureProcessor, Pending};
use crate::dns;
use crate::frame::Packet;
use crate::health::inc;
use crate::quic::{self, InitialKeys, QuicError, crypto_frames};
use crate::tcp_reasm::TcpReassembly;
use crate::tls;

pub(super) const DNS_PORT: u16 = 53;
const HTTPS_PORT: u16 = 443;

impl CaptureProcessor {
    pub(super) fn quic(&mut self, key: ConnKey, payload: &[u8], now_ms: i64) {
        if !quic::header::is_long(payload) || self.is_done(&key, now_ms) {
            return;
        }
        for pkt in quic::packets(payload) {
            let Ok(pkt) = pkt else { return };
            match pkt.initial_version() {
                Ok(Some(_)) => {
                    if self.quic_initial(key, &pkt, now_ms) {
                        return;
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    // Unknown version: only worth counting on the HTTPS port.
                    if key.remote_port == HTTPS_PORT {
                        inc(&self.out.health.counters.quic_unsupported_version);
                    }
                    return self.finish(key, None, now_ms);
                }
            }
        }
    }

    /// Returns true when the connection is finished.
    fn quic_initial(&mut self, key: ConnKey, pkt: &quic::InitialPacket<'_>, now_ms: i64) -> bool {
        let Some(plain) = self.quic_plaintext(key, pkt, now_ms) else {
            return false;
        };
        let Some(Pending::Quic { reasm, .. }) = self.pending.get_mut(&key) else {
            return false;
        };
        let (delta, overflow) = ingest_crypto(reasm, &plain);
        let outcome = (!overflow)
            .then(|| hello_from(reasm.prefix(), reasm.cap()))
            .flatten();
        self.account(&key, delta);
        if overflow {
            self.incomplete(&key, now_ms);
            self.finish(key, None, now_ms);
            return true;
        }
        self.quic_conclude(key, outcome, now_ms)
    }

    /// Decrypt one Initial, deriving keys (once) for a new connection.
    fn quic_plaintext(
        &mut self,
        key: ConnKey,
        pkt: &quic::InitialPacket<'_>,
        now_ms: i64,
    ) -> Option<Vec<u8>> {
        let counters = &self.out.health.counters;
        let plain = match self.pending.get(&key) {
            Some(Pending::Quic { keys, .. }) => quic::decrypt(pkt, keys),
            Some(Pending::Tcp { .. }) => return None,
            None => {
                let version = pkt.initial_version().ok().flatten()?;
                let keys = InitialKeys::client(version, pkt.header.dcid);
                let plain = quic::decrypt(pkt, &keys);
                if plain.is_ok() {
                    inc(&counters.quic_started);
                    let reasm = TcpReassembly::new(0, self.limits.max_reassembly_bytes_per_conn);
                    self.start(
                        key,
                        Pending::Quic {
                            keys: Box::new(keys),
                            reasm,
                            started_ms: now_ms,
                        },
                    );
                }
                plain
            }
        };
        plain
            .map_err(|_| inc(&self.out.health.counters.quic_decrypt_error))
            .ok()
    }

    fn quic_conclude(
        &mut self,
        key: ConnKey,
        outcome: Option<Result<tls::HelloInfo, QuicError>>,
        now_ms: i64,
    ) -> bool {
        let c = Arc::clone(&self.out.health);
        match outcome {
            None | Some(Err(QuicError::Truncated)) => false,
            Some(Err(_)) => {
                inc(&c.counters.tls_parse_error);
                self.finish(key, None, now_ms);
                true
            }
            Some(Ok(info)) => {
                let name = info
                    .sni
                    .as_deref()
                    .and_then(|s| super::make_name(&c, s, NameSource::Quic))
                    .map(|n| {
                        inc(&c.counters.quic_named);
                        if info.ech {
                            inc(&c.counters.tls_ech);
                        }
                        n.with_alpn(info.alpn.clone()).with_ech_outer(info.ech)
                    });
                self.finish(key, name, now_ms);
                true
            }
        }
    }

    pub(super) fn dns(&mut self, p: &Packet<'_>, now_ms: i64) {
        let c = &self.out.health.counters;
        inc(&c.dns_responses);
        let bindings = match dns::bindings(p.payload) {
            Ok(b) => b,
            Err(_) => return inc(&c.dns_parse_error),
        };
        for b in bindings.into_iter().filter(|b| routable(b.addr)) {
            let Ok(name) = ConnName::new(&b.qname, NameSource::Dns) else {
                inc(&c.name_invalid);
                continue;
            };
            let name = Arc::new(name.with_aliases(&b.aliases));
            let expires = now_ms + i64::from(b.ttl_secs) * 1000;
            self.out
                .table
                .bind(p.dst_ip, b.addr, Arc::clone(&name), expires);
            self.out.names.push(b.addr, Arc::clone(&name.host));
            inc(&c.dns_bindings);
        }
    }
}

/// Answer addresses worth naming: never loopback, unspecified or multicast
/// (forged or junk answers must not label them).
fn routable(ip: std::net::IpAddr) -> bool {
    !(ip.is_loopback() || ip.is_unspecified() || ip.is_multicast())
        && !matches!(ip, std::net::IpAddr::V4(v4) if v4.is_broadcast())
}

/// Insert the CRYPTO frames of one Initial; returns (byte delta, overflow).
fn ingest_crypto(reasm: &mut TcpReassembly, plain: &[u8]) -> (isize, bool) {
    let Ok(chunks) = crypto_frames::crypto_chunks(plain) else {
        return (0, true);
    };
    let mut delta = 0isize;
    for (offset, data) in chunks {
        let fits = u32::try_from(offset)
            .ok()
            .filter(|o| (*o as usize) <= reasm.cap());
        match fits.map(|o| reasm.insert(o, data)) {
            Some(Ok(d)) => delta += d,
            _ => return (delta, true),
        }
    }
    (delta, false)
}

/// `None` = need more CRYPTO bytes; `Some(Err(Malformed))` = not a ClientHello.
fn hello_from(prefix: &[u8], cap: usize) -> Option<Result<tls::HelloInfo, QuicError>> {
    let need = tls::handshake_len(prefix)?;
    if prefix[0] != 1 || need > cap {
        return Some(Err(QuicError::Malformed));
    }
    if prefix.len() < need {
        return None;
    }
    Some(tls::from_handshake(&prefix[..need]).map_err(|_| QuicError::Malformed))
}
