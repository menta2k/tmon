//! Built-in categories and rules shipped in `assets/builtin-rules.toml`.

use serde::Deserialize;

use super::model::{Origin, Rule, RuleMatch, USER_PRIORITY_BASE};
use crate::category::Category;
use crate::error::CoreError;

pub const BUILTIN_TOML: &str = include_str!("../../../../assets/builtin-rules.toml");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinRules {
    pub categories: Vec<Category>,
    pub rules: Vec<Rule>,
}

#[derive(Deserialize)]
struct FileFormat {
    categories: Vec<CategoryEntry>,
    rules: Vec<RuleEntry>,
}

#[derive(Deserialize)]
struct CategoryEntry {
    id: u16,
    name: String,
    color: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct RuleEntry {
    id: u32,
    name: String,
    category: u16,
    priority: i64,
    #[serde(rename = "match")]
    match_: toml::Value,
}

/// Parse and validate the embedded built-in rule file.
pub fn load_builtin() -> Result<BuiltinRules, CoreError> {
    parse_builtin(BUILTIN_TOML)
}

pub fn parse_builtin(text: &str) -> Result<BuiltinRules, CoreError> {
    let file: FileFormat =
        toml::from_str(text).map_err(|e| CoreError::BuiltinRules(e.to_string()))?;
    let categories: Vec<Category> = file
        .categories
        .into_iter()
        .map(|c| Category {
            id: c.id,
            name: c.name,
            color: c.color,
            description: c.description,
            builtin: true,
        })
        .collect();
    for c in &categories {
        c.validate()
            .map_err(|e| CoreError::BuiltinRules(format!("category {}: {e}", c.id)))?;
    }
    let rules = file
        .rules
        .into_iter()
        .map(|r| to_rule(r, &categories))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BuiltinRules { categories, rules })
}

fn to_rule(r: RuleEntry, categories: &[Category]) -> Result<Rule, CoreError> {
    let err = |m: String| CoreError::BuiltinRules(format!("rule {} ({}): {m}", r.id, r.name));
    let json = serde_json::to_value(&r.match_).map_err(|e| err(e.to_string()))?;
    let match_ = RuleMatch::from_json(&json).map_err(|e| err(e.to_string()))?;
    if !categories.iter().any(|c| c.id == r.category) {
        return Err(err(format!("unknown category {}", r.category)));
    }
    if r.priority >= USER_PRIORITY_BASE {
        return Err(err("priority must be below the user band".into()));
    }
    let rule = Rule {
        id: r.id,
        category_id: r.category,
        priority: r.priority,
        enabled: true,
        origin: Origin::Builtin,
        name: r.name.clone(),
        match_,
    };
    rule.validate().map_err(|e| err(e.to_string()))?;
    Ok(rule)
}
