//! Schema loading for way frontmatter + locale stubs.
//!
//! The single source of truth is `hooks/ways/frontmatter-schema.yaml`.
//! This module parses it into a `Schema` struct the rest of the linter
//! reads. Adding a new field means: add it to the yaml, extend the
//! extractor here if it's a new category, and the validators in
//! `per_file.rs` / `locale_stubs.rs` pick it up automatically.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;

pub(super) struct Schema {
    pub(super) way_fields: HashSet<String>,
    pub(super) check_fields: HashSet<String>,
    pub(super) when_subfields: HashSet<String>,
    pub(super) locale_stub_fields: HashSet<String>,
    pub(super) valid_scopes: Vec<String>,
    pub(super) valid_macros: Vec<String>,
    pub(super) valid_triggers: Vec<String>,
    pub(super) valid_tool_phases: Vec<String>,
    pub(super) excluded_path_segments: Vec<String>,
}

pub(super) fn load(path: &Path) -> Result<Schema> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading schema {}", path.display()))?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&content)?;

    Ok(Schema {
        way_fields: extract_fields(&doc, "way"),
        check_fields: extract_fields(&doc, "check"),
        when_subfields: extract_when_subfields(&doc),
        locale_stub_fields: extract_locale_stub_fields(&doc),
        valid_scopes: extract_enum_values(&doc, "way", "scope"),
        valid_macros: extract_enum_values(&doc, "way", "macro"),
        valid_triggers: extract_enum_values(&doc, "way", "trigger"),
        valid_tool_phases: extract_enum_values(&doc, "way", "tool_phase"),
        excluded_path_segments: extract_string_list(&doc, &["lint", "excluded_path_segments"]),
    })
}

/// Schema-escape hatch: fields whose name starts with `x-` are intentionally
/// foreign and the linter leaves them alone. Follows the JSON Schema / OpenAPI
/// / package.json convention. Useful when you want a field that's "mostly
/// correct" to survive --fix, or when prototyping a schema extension.
pub(super) fn is_reserved_field(name: &str) -> bool {
    name.starts_with("x-")
}

fn extract_fields(doc: &serde_yaml::Value, type_name: &str) -> HashSet<String> {
    let mut fields = HashSet::new();
    if let Some(type_block) = doc.get(type_name).and_then(|v| v.as_mapping()) {
        for (_category, category_block) in type_block {
            if let Some(mapping) = category_block.as_mapping() {
                for (field_name, _) in mapping {
                    if let Some(name) = field_name.as_str() {
                        fields.insert(name.to_string());
                    }
                }
            }
        }
    }
    fields
}

fn extract_when_subfields(doc: &serde_yaml::Value) -> HashSet<String> {
    let mut fields = HashSet::new();
    for type_name in &["way", "check"] {
        if let Some(subfields) = doc
            .get(*type_name)
            .and_then(|v| v.get("preconditions"))
            .and_then(|v| v.get("when"))
            .and_then(|v| v.get("subfields"))
            .and_then(|v| v.as_mapping())
        {
            for (name, _) in subfields {
                if let Some(n) = name.as_str() {
                    fields.insert(n.to_string());
                }
            }
        }
    }
    fields
}

fn extract_locale_stub_fields(doc: &serde_yaml::Value) -> HashSet<String> {
    let mut fields = HashSet::new();
    if let Some(block) = doc.get("locale_stub").and_then(|v| v.as_mapping()) {
        for (name, _) in block {
            if let Some(n) = name.as_str() {
                fields.insert(n.to_string());
            }
        }
    }
    fields
}

fn extract_enum_values(doc: &serde_yaml::Value, type_name: &str, field_name: &str) -> Vec<String> {
    let type_block = match doc.get(type_name).and_then(|v| v.as_mapping()) {
        Some(m) => m,
        None => return Vec::new(),
    };

    for (_category, category_block) in type_block {
        if let Some(field) = category_block.get(field_name) {
            if let Some(values) = field.get("values").and_then(|v| v.as_sequence()) {
                return values
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
            }
        }
    }
    Vec::new()
}

fn extract_string_list(doc: &serde_yaml::Value, keys: &[&str]) -> Vec<String> {
    let mut val = doc;
    for key in keys {
        match val.get(*key) {
            Some(v) => val = v,
            None => return Vec::new(),
        }
    }
    val.as_sequence()
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}
