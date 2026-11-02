//! Categorization rule engine (FR-009..FR-011).

pub mod builtin;
pub mod compile;
pub mod confidence;
pub mod matcher;
pub mod model;

pub use builtin::{BuiltinRules, load_builtin};
pub use compile::{CompiledRule, RuleSet};
pub use confidence::Confidence;
pub use matcher::categorize_linear;
pub use model::{HostSpec, Origin, PortSpec, Rule, RuleMatch, USER_PRIORITY_BASE, normalize_mac};
