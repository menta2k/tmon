//! TCP: SYN tracking, reassembly, TLS ClientHello and HTTP Host.

use tmon_core::naming::{ConnKey, ConnName, NameSource};

use super::make_name;
use super::{CaptureProcessor, Pending, TcpKind};
use crate::frame::TcpInfo;
use crate::health::{Health, inc};
use crate::http::{self, HttpResult};
use crate::tcp_reasm::TcpReassembly;
use crate::tls;

/// Bytes needed to classify a stream (TLS record header + handshake type).
const CLASSIFY_LEN: usize = 6;

enum Step {
    Wait,
    Done(Option<ConnName>),
    Fail,
}

impl CaptureProcessor {
    pub(super) fn tcp(&mut self, key: ConnKey, t: TcpInfo, payload: &[u8], now_ms: i64) {
        if t.syn && !t.ack {
            if !self.reopen_on_syn(&key, t.seq) {
                return;
            }
            inc(&self.out.health.counters.tcp_syn_seen);
            self.out.health.record(now_ms, |b| b.tcp_syn += 1);
            self.syns.put(key, t.seq);
            return;
        }
        if payload.is_empty() || self.is_done(&key, now_ms) {
            return;
        }
        let isn = self.syns.pop(&key);
        if isn.is_some() {
            inc(&self.out.health.counters.tcp_payload_first_seen);
            self.out.health.record(now_ms, |b| b.tcp_payload_first += 1);
        }
        if !self.pending.contains(&key) {
            let base = match isn {
                Some(isn) => isn.wrapping_add(1),
                None if tls::looks_like_client_hello(payload)
                    || http::looks_like_request(payload) =>
                {
                    t.seq
                }
                // Mid-connection without its start: nothing to learn here.
                None => return self.finish(key, None, now_ms),
            };
            let reasm = TcpReassembly::new(base, self.limits.max_reassembly_bytes_per_conn);
            self.start(
                key,
                Pending::Tcp {
                    reasm,
                    kind: TcpKind::Unknown,
                    started_ms: now_ms,
                },
            );
        }
        let inserted = match self.pending.get_mut(&key) {
            Some(Pending::Tcp { reasm, .. }) => reasm.insert(t.seq, payload),
            _ => return,
        };
        match inserted {
            Ok(delta) => self.account(&key, delta),
            Err(_) => return self.fail_tcp(key, now_ms),
        }
        self.advance_tcp(key, now_ms);
    }

    fn advance_tcp(&mut self, key: ConnKey, now_ms: i64) {
        let health = &self.out.health;
        let cap = self.limits.max_reassembly_bytes_per_conn;
        let Some(Pending::Tcp { reasm, kind, .. }) = self.pending.get_mut(&key) else {
            return;
        };
        let prefix = reasm.prefix();
        if *kind == TcpKind::Unknown {
            if prefix.len() < CLASSIFY_LEN {
                return;
            }
            *kind = if tls::looks_like_client_hello(prefix) {
                TcpKind::Tls
            } else if http::looks_like_request(prefix) {
                TcpKind::Http
            } else {
                return self.finish(key, None, now_ms);
            };
            if *kind == TcpKind::Tls {
                inc(&health.counters.tls_started);
                health.record(now_ms, |b| b.tls_started += 1);
            }
        }
        let step = match kind {
            TcpKind::Tls => tls_step(health, cap, prefix),
            _ => http_step(health, prefix),
        };
        match step {
            Step::Wait => {}
            Step::Done(name) => self.finish(key, name, now_ms),
            Step::Fail => self.fail_tcp(key, now_ms),
        }
    }

    /// Reassembly overflow, gap or oversized record.
    fn fail_tcp(&mut self, key: ConnKey, now_ms: i64) {
        self.incomplete(&key, now_ms);
        self.finish(key, None, now_ms);
    }
}

fn tls_step(health: &Health, cap: usize, prefix: &[u8]) -> Step {
    let Some(need) = tls::record_len(prefix) else {
        return Step::Wait;
    };
    if need > cap {
        return Step::Fail;
    }
    if prefix.len() < need {
        return Step::Wait;
    }
    let c = &health.counters;
    match tls::from_record(&prefix[..need]) {
        Ok(info) => {
            let Some(sni) = info.sni.as_deref() else {
                return Step::Done(None);
            };
            let name = make_name(health, sni, NameSource::Tls).map(|n| {
                inc(&c.tls_named);
                if info.ech {
                    inc(&c.tls_ech);
                }
                n.with_alpn(info.alpn.clone()).with_ech_outer(info.ech)
            });
            Step::Done(name)
        }
        Err(_) => {
            inc(&c.tls_parse_error);
            Step::Done(None)
        }
    }
}

fn http_step(health: &Health, prefix: &[u8]) -> Step {
    match http::host(prefix) {
        HttpResult::Partial => Step::Wait,
        HttpResult::Invalid | HttpResult::Done(None) => Step::Done(None),
        HttpResult::Done(Some(h)) => {
            let name = make_name(health, &h, NameSource::Http);
            if name.is_some() {
                inc(&health.counters.http_named);
            }
            Step::Done(name.map(|n| n.with_alpn(Some("http/1.1".into()))))
        }
    }
}
