//! Classifier → tmon messages.

use crate::limits::{MAX_CATALOG, MAX_MESSAGE, MAX_NAME, MAX_STR, MAX_TOP, kind};
use crate::msg::{
    CatalogEntry, Classification, FromClassifier, Hello, Key, Stats, TopApp, non_empty, read_entry,
    read_host, write_entry,
};
use crate::wire::{DecodeError, Reader, Writer};

/// Hello header: type + version + 3 × str8 + max_flows + count.
const HELLO_HEADER_MAX: usize = 1 + 2 + 3 * (1 + MAX_STR) + 4 + 2;
/// Upper bound of one encoded catalog entry.
const ENTRY_MAX: usize = 2 * (1 + MAX_NAME);

impl Hello {
    /// Hello followed by as many HelloCatalog messages as needed to keep
    /// each message within [`MAX_MESSAGE`]. At most [`MAX_CATALOG`] entries.
    pub fn to_messages(&self) -> Vec<Vec<u8>> {
        let catalog = &self.catalog[..self.catalog.len().min(MAX_CATALOG)];
        let first = (MAX_MESSAGE - HELLO_HEADER_MAX) / ENTRY_MAX;
        let rest = (MAX_MESSAGE - 3) / ENTRY_MAX;
        let (head, tail) = catalog.split_at(catalog.len().min(first));
        let mut w = Writer::with_kind(kind::HELLO);
        w.u16(self.proto_version);
        w.str8(&self.impl_name, MAX_STR);
        w.str8(&self.impl_version, MAX_STR);
        w.str8(&self.engine, MAX_STR);
        w.u32(self.max_flows);
        write_entries(&mut w, head);
        let mut out = vec![w.buf];
        out.extend(tail.chunks(rest).map(|c| {
            let mut w = Writer::with_kind(kind::HELLO_CATALOG);
            write_entries(&mut w, c);
            w.buf
        }));
        out
    }
}

fn write_entries(w: &mut Writer, entries: &[CatalogEntry]) {
    w.u16(entries.len() as u16);
    entries.iter().for_each(|e| write_entry(w, e));
}

fn read_entries(r: &mut Reader<'_>) -> Result<Vec<CatalogEntry>, DecodeError> {
    let n = usize::from(r.u16()?);
    if n > MAX_CATALOG {
        return Err(DecodeError::Invalid("catalog count"));
    }
    (0..n).map(|_| read_entry(r)).collect()
}

impl FromClassifier {
    /// Encode one message. A Hello with a large catalog must be sent with
    /// [`Hello::to_messages`] instead (this encodes it whole).
    pub fn encode(&self) -> Vec<u8> {
        match self {
            FromClassifier::Hello(h) => h.to_messages().swap_remove(0),
            FromClassifier::Classification(c) => encode_classification(c),
            FromClassifier::Stats(s) => encode_stats(s),
            FromClassifier::HelloCatalog(c) => {
                let mut w = Writer::with_kind(kind::HELLO_CATALOG);
                write_entries(&mut w, c);
                w.buf
            }
            FromClassifier::Pong(n) => {
                let mut w = Writer::with_kind(kind::PONG);
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
            kind::HELLO => FromClassifier::Hello(read_hello(&mut r)?),
            kind::CLASSIFICATION => FromClassifier::Classification(read_classification(&mut r)?),
            kind::STATS => FromClassifier::Stats(read_stats(&mut r)?),
            kind::HELLO_CATALOG => FromClassifier::HelloCatalog(read_entries(&mut r)?),
            kind::PONG => FromClassifier::Pong(r.u64()?),
            other => return Err(DecodeError::UnknownType(other)),
        };
        r.finish()?;
        Ok(msg)
    }
}

fn read_hello(r: &mut Reader<'_>) -> Result<Hello, DecodeError> {
    Ok(Hello {
        proto_version: r.u16()?,
        impl_name: r.str8(MAX_STR, "impl")?,
        impl_version: r.str8(MAX_STR, "impl_version")?,
        engine: r.str8(MAX_STR, "engine")?,
        max_flows: r.u32()?,
        catalog: read_entries(r)?,
    })
}

fn encode_classification(c: &Classification) -> Vec<u8> {
    let mut w = Writer::with_kind(kind::CLASSIFICATION);
    c.key.write(&mut w);
    w.str8(&c.app, MAX_NAME);
    w.str8(&c.category, MAX_NAME);
    w.u8(u8::from(c.confident));
    w.u8(u8::from(c.final_));
    w.str8(c.host.as_deref().unwrap_or(""), crate::limits::MAX_HOST);
    w.buf
}

fn read_classification(r: &mut Reader<'_>) -> Result<Classification, DecodeError> {
    Ok(Classification {
        key: Key::read(r)?,
        app: non_empty(r.str8(MAX_NAME, "app")?, "app")?,
        category: r.str8(MAX_NAME, "category")?,
        confident: r.flag("confidence")?,
        final_: r.flag("final")?,
        host: read_host(r)?,
    })
}

fn encode_stats(s: &Stats) -> Vec<u8> {
    let mut w = Writer::with_kind(kind::STATS);
    for v in [s.uptime_ms, s.packets_in, s.packets_invalid] {
        w.u64(v);
    }
    w.u32(s.flows_active);
    for v in [
        s.flows_created,
        s.evicted_lru,
        s.evicted_idle,
        s.evicted_memory,
        s.classified_confident,
        s.classified_guess,
        s.giveups,
        s.mem_bytes,
        s.mem_limit_bytes,
    ] {
        w.u64(v);
    }
    let top = &s.top[..s.top.len().min(MAX_TOP)];
    w.u8(top.len() as u8);
    for t in top {
        w.str8(&t.app, MAX_NAME);
        w.u32(t.flows);
    }
    w.buf
}

fn read_stats(r: &mut Reader<'_>) -> Result<Stats, DecodeError> {
    let mut s = Stats {
        uptime_ms: r.u64()?,
        packets_in: r.u64()?,
        packets_invalid: r.u64()?,
        flows_active: r.u32()?,
        flows_created: r.u64()?,
        evicted_lru: r.u64()?,
        evicted_idle: r.u64()?,
        evicted_memory: r.u64()?,
        classified_confident: r.u64()?,
        classified_guess: r.u64()?,
        giveups: r.u64()?,
        mem_bytes: r.u64()?,
        mem_limit_bytes: r.u64()?,
        top: Vec::new(),
    };
    let n = usize::from(r.u8()?);
    if n > MAX_TOP {
        return Err(DecodeError::Invalid("top count"));
    }
    s.top = (0..n)
        .map(|_| {
            Ok(TopApp {
                app: non_empty(r.str8(MAX_NAME, "top app")?, "top app")?,
                flows: r.u32()?,
            })
        })
        .collect::<Result<_, DecodeError>>()?;
    Ok(s)
}
