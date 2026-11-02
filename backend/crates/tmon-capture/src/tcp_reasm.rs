//! Bounded in-order reassembly of the first client → server bytes (R4).

use std::collections::BTreeMap;

/// Out-of-order segments kept per connection; a ClientHello needs 2–3.
pub const MAX_PENDING_SEGMENTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasmError {
    /// Data beyond the per-connection cap.
    Overflow,
}

/// Contiguous prefix plus out-of-order segments, keyed by offset from `base_seq`.
#[derive(Debug, Clone)]
pub struct TcpReassembly {
    base_seq: u32,
    prefix: Vec<u8>,
    pending: BTreeMap<u32, Vec<u8>>,
    pending_bytes: usize,
    cap: usize,
}

impl TcpReassembly {
    /// `base_seq` = sequence number of the first payload byte.
    pub fn new(base_seq: u32, cap: usize) -> Self {
        Self {
            base_seq,
            prefix: Vec::new(),
            pending: BTreeMap::new(),
            pending_bytes: 0,
            cap,
        }
    }

    /// Bytes held (prefix + out-of-order).
    pub fn bytes(&self) -> usize {
        self.prefix.len() + self.pending_bytes
    }

    /// Sequence number of the first payload byte.
    pub fn base_seq(&self) -> u32 {
        self.base_seq
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn prefix(&self) -> &[u8] {
        &self.prefix
    }

    /// Add one segment; duplicates and overlaps are trimmed. Returns the
    /// change in held bytes.
    pub fn insert(&mut self, seq: u32, data: &[u8]) -> Result<isize, ReasmError> {
        let before = self.bytes() as isize;
        let rel = seq.wrapping_sub(self.base_seq) as i32;
        let (offset, data) = if rel < 0 {
            let skip = rel.unsigned_abs() as usize;
            (0u32, data.get(skip..).unwrap_or_default())
        } else {
            (rel as u32, data)
        };
        if data.is_empty() {
            return Ok(0);
        }
        if offset as usize + data.len() > self.cap {
            return Err(ReasmError::Overflow);
        }
        if offset as usize <= self.prefix.len() {
            self.extend_prefix(offset as usize, data);
            self.absorb_pending();
        } else {
            let old_len = self.pending.get(&offset).map(Vec::len);
            let keep = old_len.is_none_or(|old| old < data.len());
            let grows = data.len() - old_len.unwrap_or(0).min(data.len());
            // Bound what is held, not just the offset: many small overlapping
            // segments must not exceed the per-connection cap.
            if keep
                && (self.bytes() + grows > self.cap
                    || (old_len.is_none() && self.pending.len() >= MAX_PENDING_SEGMENTS))
            {
                return Err(ReasmError::Overflow);
            }
            if keep {
                let old = self
                    .pending
                    .insert(offset, data.to_vec())
                    .map_or(0, |o| o.len());
                self.pending_bytes = self.pending_bytes + data.len() - old;
            }
        }
        Ok(self.bytes() as isize - before)
    }

    fn extend_prefix(&mut self, offset: usize, data: &[u8]) {
        let have = self.prefix.len() - offset;
        if data.len() > have {
            self.prefix.extend_from_slice(&data[have..]);
        }
    }

    fn absorb_pending(&mut self) {
        while let Some(entry) = self.pending.first_entry() {
            if *entry.key() as usize > self.prefix.len() {
                break;
            }
            let (off, seg) = entry.remove_entry();
            self.pending_bytes -= seg.len();
            self.extend_prefix(off as usize, &seg);
        }
    }
}
