//! Categories (data-model.md §4) and the `Categorizer` abstraction.

use serde::{Deserialize, Serialize};

use crate::error::ValidationErrors;
use crate::flow::FlowRecord;

/// Reserved "Other" category id; cannot be deleted.
pub const OTHER_ID: u16 = 0;
pub const OTHER_NAME: &str = "Other";
const NAME_MAX: usize = 40;
const DESCRIPTION_MAX: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: u16,
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub builtin: bool,
}

impl Category {
    /// Validate name ("Unique, 1–40 chars" — uniqueness checked by the store),
    /// color `#RRGGBB` and description "≤ 200 chars".
    pub fn validate(&self) -> Result<(), ValidationErrors> {
        validate_fields(&self.name, &self.color, &self.description)
    }
}

pub fn validate_fields(name: &str, color: &str, description: &str) -> Result<(), ValidationErrors> {
    let mut errors = ValidationErrors::new();
    let len = name.trim().chars().count();
    if len == 0 || len > NAME_MAX {
        errors.add("name", format!("must be 1–{NAME_MAX} characters"));
    }
    if !is_hex_color(color) {
        errors.add("color", "must be #RRGGBB");
    }
    if description.chars().count() > DESCRIPTION_MAX {
        errors.add(
            "description",
            format!("must be at most {DESCRIPTION_MAX} characters"),
        );
    }
    errors.into_result(())
}

pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Assigns `(category_id, rule_id)` to a flow.
pub trait Categorizer: Send + Sync {
    fn categorize(&self, flow: &FlowRecord) -> (u16, Option<u32>);
}

/// Always returns "Other" — used until a rule set is loaded.
#[derive(Debug, Default, Clone, Copy)]
pub struct FallbackCategorizer;

impl Categorizer for FallbackCategorizer {
    fn categorize(&self, _flow: &FlowRecord) -> (u16, Option<u32>) {
        (OTHER_ID, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_fields() {
        assert!(validate_fields("Gaming", "#AABBcc", "").is_ok());
        let err = validate_fields("", "red", &"x".repeat(201)).unwrap_err();
        assert_eq!(err.fields().len(), 3);
        assert!(validate_fields(&"n".repeat(41), "#000000", "").is_err());
    }
}
