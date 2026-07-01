//! The vendored Claude Code settings JSON Schema (SchemaStore), parsed into a
//! lookup of top-level key → `{ type, description }`.
//!
//! This is the **shape source**: the set of valid `settings.json` keys, their
//! JSON types, and human descriptions — acquired deterministically and bundled
//! via `include_str!` (offline, ships with the binary; the lockfile pattern).
//! It powers template scaffolding (`ways settings new`) and the linter's
//! key-set/type checks.
//!
//! It deliberately does **not** encode scope-class (managed-only /
//! managed-overridable) — a generic JSON Schema has no such axis. That stays the
//! hand-curated overlay in [`super::schema`].
//!
//! Provenance: community-maintained SchemaStore, not an official Anthropic
//! artifact (anthropics/claude-code#11795); it may lag the latest CLI release.
//! Refresh with `refresh-settings-schema.sh` and re-commit; project-pulse tracks
//! the drift.

use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The vendored schema, embedded at build time.
const SCHEMA_JSON: &str = include_str!("claude-code-settings.schema.json");

/// A coarse JSON type — enough for template placeholders and type checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropType {
    String,
    Number,
    Boolean,
    Array,
    Object,
    /// Anything we can't reduce to one concrete type (unresolved `$ref`,
    /// mixed `oneOf`, etc.).
    Other,
}

impl PropType {
    /// A syntactically-valid placeholder value of this type, for a scaffolded
    /// template the user then fills in.
    pub fn placeholder(&self) -> Value {
        match self {
            PropType::String => Value::String(String::new()),
            PropType::Number => Value::from(0),
            PropType::Boolean => Value::Bool(false),
            PropType::Array => Value::Array(Vec::new()),
            PropType::Object => Value::Object(serde_json::Map::new()),
            PropType::Other => Value::Null,
        }
    }

    /// Human name, for diagnostics.
    pub fn name(&self) -> &'static str {
        match self {
            PropType::String => "string",
            PropType::Number => "number",
            PropType::Boolean => "boolean",
            PropType::Array => "array",
            PropType::Object => "object",
            PropType::Other => "any",
        }
    }
}

/// What the schema knows about one top-level key.
pub struct PropInfo {
    pub ty: PropType,
    pub description: String,
}

/// The parsed settings schema: top-level key → info.
pub struct SettingsSchema {
    props: BTreeMap<String, PropInfo>,
}

impl SettingsSchema {
    pub fn get(&self, key: &str) -> Option<&PropInfo> {
        self.props.get(key)
    }
    pub fn contains(&self, key: &str) -> bool {
        self.props.contains_key(key)
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.props.keys().map(String::as_str)
    }
    pub fn len(&self) -> usize {
        self.props.len()
    }
    pub fn is_empty(&self) -> bool {
        self.props.is_empty()
    }
}

/// The bundled schema, parsed once. Panics only if the *vendored* file is
/// malformed — a build-time invariant, not a runtime input.
pub fn bundled() -> &'static SettingsSchema {
    static SCHEMA: LazyLock<SettingsSchema> = LazyLock::new(|| {
        parse(SCHEMA_JSON).expect("vendored settings schema must parse")
    });
    &SCHEMA
}

fn parse(json: &str) -> anyhow::Result<SettingsSchema> {
    let root: Value = serde_json::from_str(json)?;
    let props_obj = root
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("settings schema has no top-level `properties`"))?;
    let mut props = BTreeMap::new();
    for (key, spec) in props_obj {
        let ty = derive_type(&root, spec);
        let description = spec
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        props.insert(key.clone(), PropInfo { ty, description });
    }
    Ok(SettingsSchema { props })
}

/// Best-effort concrete type: reads `type`, else resolves one level of `$ref`
/// into `$defs`, else picks a usable type out of `oneOf`/`anyOf`/`allOf`.
fn derive_type(root: &Value, spec: &Value) -> PropType {
    if let Some(t) = spec.get("type") {
        return type_from_type_field(t);
    }
    if let Some(r) = spec.get("$ref").and_then(Value::as_str) {
        if let Some(def) = resolve_ref(root, r) {
            return derive_type(root, def);
        }
    }
    for combiner in ["oneOf", "anyOf", "allOf"] {
        if let Some(arr) = spec.get(combiner).and_then(Value::as_array) {
            let mut fallback = PropType::Other;
            for branch in arr {
                let t = derive_type(root, branch);
                // Prefer a structured type; it makes the best template skeleton.
                if matches!(t, PropType::Object | PropType::Array) {
                    return t;
                }
                if fallback == PropType::Other && t != PropType::Other {
                    fallback = t;
                }
            }
            return fallback;
        }
    }
    PropType::Other
}

fn type_from_type_field(t: &Value) -> PropType {
    match t {
        Value::String(s) => from_type_str(s),
        // A union like ["string","null"] — take the first non-null concrete type.
        Value::Array(arr) => arr
            .iter()
            .filter_map(Value::as_str)
            .find(|s| *s != "null")
            .map(from_type_str)
            .unwrap_or(PropType::Other),
        _ => PropType::Other,
    }
}

fn from_type_str(s: &str) -> PropType {
    match s {
        "string" => PropType::String,
        "number" | "integer" => PropType::Number,
        "boolean" => PropType::Boolean,
        "array" => PropType::Array,
        "object" => PropType::Object,
        _ => PropType::Other,
    }
}

/// Resolve a local `$ref` like `#/$defs/hookMatcher` against the schema root.
fn resolve_ref<'a>(root: &'a Value, r: &str) -> Option<&'a Value> {
    let ptr = r.strip_prefix('#')?; // "#/$defs/foo" -> "/$defs/foo"
    root.pointer(ptr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_schema_parses_and_is_broad() {
        let s = bundled();
        // The vendored SchemaStore schema carried 84 top-level keys when pinned.
        assert!(s.len() >= 80, "expected a broad schema, got {}", s.len());
    }

    #[test]
    fn known_keys_have_expected_types_and_descriptions() {
        let s = bundled();
        assert_eq!(s.get("cleanupPeriodDays").unwrap().ty, PropType::Number);
        assert_eq!(s.get("model").unwrap().ty, PropType::String);
        assert_eq!(s.get("permissions").unwrap().ty, PropType::Object);
        assert_eq!(s.get("hooks").unwrap().ty, PropType::Object);
        assert_eq!(s.get("enabledMcpjsonServers").unwrap().ty, PropType::Array);
        assert_eq!(s.get("includeCoAuthoredBy").unwrap().ty, PropType::Boolean);
        // Every property carries a description — the "what it is" for the body.
        for key in ["cleanupPeriodDays", "permissions", "model", "hooks"] {
            assert!(
                !s.get(key).unwrap().description.is_empty(),
                "`{key}` should have a description"
            );
        }
    }

    #[test]
    fn schema_is_broader_than_the_curated_overlay() {
        // A key present in the vendored schema but NOT in our hand-curated
        // scope-class overlay — the whole point of acquiring the schema.
        let s = bundled();
        assert!(s.contains("autoMemoryEnabled"));
        assert!(super::super::schema::lookup("autoMemoryEnabled").is_none());
    }

    #[test]
    fn placeholders_are_type_correct() {
        assert_eq!(PropType::String.placeholder(), Value::String(String::new()));
        assert_eq!(PropType::Number.placeholder(), Value::from(0));
        assert_eq!(PropType::Boolean.placeholder(), Value::Bool(false));
        assert!(PropType::Array.placeholder().is_array());
        assert!(PropType::Object.placeholder().is_object());
        assert!(PropType::Other.placeholder().is_null());
    }

    #[test]
    fn unknown_key_absent() {
        assert!(bundled().get("totallyMadeUpKey").is_none());
    }
}
