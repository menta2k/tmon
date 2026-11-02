//! The seam between the flow table and nDPI: a plain result type and an
//! `Engine` trait, so everything above it is tested without libndpi.

/// `ndpi_confidence_t`, by name (nDPI 6.0 `ndpi_typedefs.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NdpiConfidence {
    Unknown,
    MatchByPort,
    Nbpf,
    DpiPartial,
    DpiPartialCache,
    DpiCache,
    Dpi,
    MatchByIp,
    DpiAggressive,
    CustomRule,
}

impl NdpiConfidence {
    /// Map the numeric enum; unknown future values count as `Unknown`.
    pub fn from_raw(v: u32) -> Self {
        use NdpiConfidence::*;
        match v {
            1 => MatchByPort,
            2 => Nbpf,
            3 => DpiPartial,
            4 => DpiPartialCache,
            5 => DpiCache,
            6 => Dpi,
            7 => MatchByIp,
            8 => DpiAggressive,
            9 => CustomRule,
            _ => Unknown,
        }
    }
}

/// What nDPI says about a flow after a packet (names already resolved).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NdpiResult {
    /// `proto.app_protocol` name ("Unknown" when 0).
    pub app: String,
    /// `proto.master_protocol` name ("Unknown" when 0).
    pub master: String,
    /// `ndpi_category_get_name(proto.category)`.
    pub category: String,
    pub confidence: Option<NdpiConfidence>,
    /// `flow->host_server_name`, empty when none.
    pub host: String,
}

/// A packet classifier with per-flow state.
pub trait Engine {
    type Flow;

    /// A fresh flow, or `None` when the allocation failed.
    fn new_flow(&mut self) -> Option<Self::Flow>;

    /// Feed one L3 packet of the flow.
    fn process(&mut self, flow: &mut Self::Flow, l3: &[u8], ts_ms: u64) -> NdpiResult;

    /// Best guess when no more packets will come.
    fn giveup(&mut self, flow: &mut Self::Flow) -> NdpiResult;

    /// Live bytes allocated by the engine (0 when not tracked).
    fn mem_bytes(&self) -> u64;
}
