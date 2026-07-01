//! The Claude Code settings JSON Schema (SchemaStore), read at runtime and parsed
//! into a lookup of top-level key → `{ type, description, placeholder }`.
//!
//! This is the **shape source**: the set of valid `settings.json` keys, their
//! JSON types, and human descriptions. It powers template scaffolding
//! (`ways settings new`) and the linter's key-set/type checks.
//!
//! **Externalized, not compiled in** (ADR-147): the schema is a data file read
//! from [`crate::paths::settings_schema_file`] the first time a `ways settings`
//! command needs it — never on the every-turn hook path. Claude Code's settings
//! surface changes often, so this can be refreshed without rebuilding the binary.
//! If the file is absent or unparseable, [`active`] returns `None` and callers
//! degrade (the linter skips schema checks; scaffolding errors).
//!
//! It deliberately does **not** encode scope-class (managed-only /
//! managed-overridable) — a generic JSON Schema has no such axis. That stays the
//! hand-curated overlay in [`super::schema`].
//!
//! Provenance: community-maintained SchemaStore, not an official Anthropic
//! artifact (anthropics/claude-code#11795); it may lag the latest CLI release.
//! Refresh with `ways settings schema --refresh`; project-pulse tracks the drift.

use anyhow::Context;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

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

    /// Whether `v` satisfies this type. `Other` matches anything (we couldn't
    /// pin a single type, so we don't second-guess the value).
    pub fn matches(&self, v: &Value) -> bool {
        match self {
            PropType::String => v.is_string(),
            PropType::Number => v.is_number(),
            PropType::Boolean => v.is_boolean(),
            PropType::Array => v.is_array(),
            PropType::Object => v.is_object(),
            PropType::Other => true,
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
    /// The type to *validate* against. Concrete only when the schema gives a
    /// single unambiguous `type`; unions (`oneOf`/`anyOf`), multi-type arrays,
    /// nullable types, and unresolved `$ref`s are [`PropType::Other`] so the
    /// linter accepts any branch instead of false-erroring on a valid one.
    pub ty: PropType,
    pub description: String,
    /// A concrete, type-correct placeholder for scaffolding — prefers a real
    /// branch even for unions (where `ty` is `Other`), so templates stay useful.
    pub placeholder: Value,
}

/// The parsed settings schema: top-level key → info.
pub struct SettingsSchema {
    props: BTreeMap<String, PropInfo>,
}

impl SettingsSchema {
    pub fn get(&self, key: &str) -> Option<&PropInfo> {
        self.props.get(key)
    }
    pub fn len(&self) -> usize {
        self.props.len()
    }
}

/// Parse a settings schema from a file on disk.
pub fn load(path: &Path) -> anyhow::Result<SettingsSchema> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading settings schema {}", path.display()))?;
    parse(&text)
}

/// The active settings schema, read once from the resolved runtime path
/// ([`crate::paths::settings_schema_file`]). `None` if the file is absent (a
/// normal, recoverable state) or unparseable (logged) — callers degrade rather
/// than fail: the linter skips schema-valid, scaffolding errors with guidance.
pub fn active() -> Option<&'static SettingsSchema> {
    static SCHEMA: LazyLock<Option<SettingsSchema>> = LazyLock::new(|| {
        let path = crate::paths::settings_schema_file();
        if !path.exists() {
            return None; // absent — a recoverable state, callers handle it
        }
        match load(&path) {
            Ok(schema) => Some(schema),
            Err(e) => {
                eprintln!("[ways] settings schema at {} is unusable: {e:#}", path.display());
                None
            }
        }
    });
    SCHEMA.as_ref()
}

/// Load the repo's shipped schema copy for tests, bypassing runtime resolution.
#[cfg(test)]
pub(crate) fn test_schema() -> SettingsSchema {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../share/claude-code-settings.schema.json");
    load(&path).expect("repo settings schema must load in tests")
}

fn parse(json: &str) -> anyhow::Result<SettingsSchema> {
    let root: Value = serde_json::from_str(json)?;
    let props_obj = root
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("settings schema has no top-level `properties`"))?;
    let mut props = BTreeMap::new();
    for (key, spec) in props_obj {
        let (ty, placeholder) = analyze(&root, spec);
        let description = spec
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        props.insert(
            key.clone(),
            PropInfo {
                ty,
                description,
                placeholder,
            },
        );
    }
    Ok(SettingsSchema { props })
}

/// Determine `(validation type, template placeholder)` for a property.
///
/// The validation type is concrete **only** for a single unambiguous `type`.
/// Unions (`oneOf`/`anyOf`/`allOf`), multi-type arrays, nullable types, and
/// unresolved refs return [`PropType::Other`], which matches any value — so the
/// linter never false-errors on a legitimately-typed branch. The placeholder,
/// by contrast, always prefers a real concrete branch so scaffolds stay useful.
fn analyze(root: &Value, spec: &Value) -> (PropType, Value) {
    if let Some(t) = spec.get("type") {
        return match t {
            Value::String(s) => {
                let ty = from_type_str(s);
                (ty, ty.placeholder())
            }
            Value::Array(arr) => {
                let concretes: Vec<PropType> = arr
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|s| *s != "null")
                    .map(from_type_str)
                    .collect();
                let has_null = arr.iter().any(|x| x.as_str() == Some("null"));
                let placeholder = concretes
                    .first()
                    .copied()
                    .unwrap_or(PropType::Other)
                    .placeholder();
                // Enforce a type only when it is a single concrete type, no null.
                let ty = if concretes.len() == 1 && !has_null {
                    concretes[0]
                } else {
                    PropType::Other
                };
                (ty, placeholder)
            }
            _ => (PropType::Other, Value::Null),
        };
    }
    if let Some(r) = spec.get("$ref").and_then(Value::as_str) {
        if let Some(def) = resolve_ref(root, r) {
            return analyze(root, def);
        }
    }
    for combiner in ["oneOf", "anyOf", "allOf"] {
        if let Some(arr) = spec.get(combiner).and_then(Value::as_array) {
            // A union accepts any branch — validate permissively (Other) but pick
            // a structured branch for the placeholder when one exists.
            let mut ph = PropType::Other;
            for branch in arr {
                let (t, _) = analyze(root, branch);
                if matches!(t, PropType::Object | PropType::Array) {
                    ph = t;
                    break;
                }
                if ph == PropType::Other && t != PropType::Other {
                    ph = t;
                }
            }
            return (PropType::Other, ph.placeholder());
        }
    }
    (PropType::Other, Value::Null)
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
    fn shipped_schema_parses_and_is_broad() {
        let s = test_schema();
        // The SchemaStore schema carried 84 top-level keys when pinned.
        assert!(s.len() >= 80, "expected a broad schema, got {}", s.len());
    }

    #[test]
    fn known_keys_have_expected_types_and_descriptions() {
        let s = test_schema();
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
        // A key present in the settings schema but NOT in our hand-curated
        // scope-class overlay — the whole point of acquiring the schema.
        let s = test_schema();
        assert!(s.get("autoMemoryEnabled").is_some());
        assert!(super::super::schema::scope_class("autoMemoryEnabled").is_none());
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
        assert!(test_schema().get("totallyMadeUpKey").is_none());
    }

    #[test]
    fn union_typed_key_validates_permissively_with_a_concrete_placeholder() {
        // `strictPluginOnlyCustomization` is anyOf[boolean, array] in the schema.
        // It must validate as Other (accept either branch) so a valid `true`
        // doesn't false-error, yet still offer a concrete placeholder.
        let schema = test_schema();
        let info = schema.get("strictPluginOnlyCustomization").unwrap();
        assert_eq!(info.ty, PropType::Other, "a union must validate permissively");
        assert!(info.ty.matches(&serde_json::json!(true)));
        assert!(info.ty.matches(&serde_json::json!([])));
        assert!(!info.placeholder.is_null(), "placeholder should be a real branch");
    }
}
