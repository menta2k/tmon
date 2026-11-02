//! Wire encoders and the deterministic fixture corpus used by tests,
//! `tmon gen-fixtures` and benchmarks. Mirrors what RouterOS exports.

pub mod corpus;
pub mod encode;

pub use encode::{FlowSpec, encode_ipfix, encode_sflow, encode_v5, encode_v9};
