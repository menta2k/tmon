//! Shared error types.

use std::collections::BTreeMap;
use std::fmt;

/// Field-indexed validation errors, e.g. `remote_cidr[0]` → `invalid CIDR`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationErrors {
    fields: BTreeMap<String, String>,
}

impl ValidationErrors {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn single(field: impl Into<String>, message: impl Into<String>) -> Self {
        let mut errors = Self::new();
        errors.add(field, message);
        errors
    }

    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.fields
            .entry(field.into())
            .or_insert_with(|| message.into());
    }

    pub fn merge_prefixed(&mut self, prefix: &str, other: ValidationErrors) {
        for (k, v) in other.fields {
            self.add(format!("{prefix}.{k}"), v);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn fields(&self) -> &BTreeMap<String, String> {
        &self.fields
    }

    /// `Ok(value)` when no errors were collected.
    pub fn into_result<T>(self, value: T) -> Result<T, ValidationErrors> {
        if self.is_empty() {
            Ok(value)
        } else {
            Err(self)
        }
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .fields
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        write!(f, "{}", parts.join("; "))
    }
}

impl std::error::Error for ValidationErrors {}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("validation failed: {0}")]
    Validation(#[from] ValidationErrors),
    #[error("built-in rules are invalid: {0}")]
    BuiltinRules(String),
}
