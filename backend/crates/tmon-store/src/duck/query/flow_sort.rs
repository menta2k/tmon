//! Sort order for raw flow search and export. Keys map to a fixed set of
//! column expressions (never user text), with a deterministic tie-break so
//! paging is stable.

/// Sortable flow columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Time,
    Bytes,
    Packets,
}

impl SortKey {
    /// `time` | `bytes` | `packets`; anything else is rejected.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "time" => Some(Self::Time),
            "bytes" => Some(Self::Bytes),
            "packets" => Some(Self::Packets),
            _ => None,
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Time => "ts",
            Self::Bytes => "bytes",
            Self::Packets => "packets",
        }
    }
}

/// Column and direction; the default is newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub desc: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            key: SortKey::Time,
            desc: true,
        }
    }
}

impl Sort {
    /// `ORDER BY` body: the key, then newest first and the other counters
    /// as tie-breaks.
    pub fn order_by(self) -> String {
        let dir = if self.desc { "DESC" } else { "ASC" };
        format!(
            "{} {dir}, ts DESC, bytes DESC, packets DESC",
            self.key.column()
        )
    }
}

/// One page request: 1-based page, page size and order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageReq {
    pub page: u64,
    pub limit: u64,
    pub sort: Sort,
}

impl PageReq {
    /// The first page, newest first.
    pub fn first(limit: u64) -> Self {
        Self {
            page: 1,
            limit,
            sort: Sort::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_by_uses_fixed_columns_and_tie_breaks() {
        let s = Sort {
            key: SortKey::Bytes,
            desc: false,
        };
        assert_eq!(s.order_by(), "bytes ASC, ts DESC, bytes DESC, packets DESC");
        assert!(Sort::default().order_by().starts_with("ts DESC"));
        assert_eq!(SortKey::parse("Bytes"), None);
    }
}
