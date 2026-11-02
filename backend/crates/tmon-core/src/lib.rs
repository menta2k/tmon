//! tmon-core: pure domain logic (no I/O) — flow records, direction
//! classification, categories and the categorization rule engine.

pub mod app;
pub mod category;
pub mod cidr;
pub mod direction;
pub mod error;
pub mod flow;
pub mod naming;
pub mod rules;

pub use app::{AppInfo, ConnectionClassifier};
pub use category::{Categorizer, Category, FallbackCategorizer, OTHER_ID};
pub use cidr::Cidr;
pub use direction::LocalNetworks;
pub use error::{CoreError, ValidationErrors};
pub use flow::{Direction, FlowRecord, RawFlow};
pub use naming::{ConnKey, ConnName, ConnectionNamer, NameLookup, NameSource};
