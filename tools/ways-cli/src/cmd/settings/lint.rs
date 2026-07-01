//! The config-store linter (ADR-147): three deterministic checks over a loaded
//! fragment tree, plus a reporter.
//!
//! 1. **schema-valid** — every top-level `settings:` key exists in the curated
//!    schema (unknown → *warning*, never error) and its value matches the
//!    expected type (mismatch → *error*).
//! 2. **scope-legal** — a managed-only key authored at user/project scope is an
//!    *error* (Claude Code ignores it there); a managed-overridable key
//!    (`model`/`fallbackModel`/`availableModels`) authored outside managed scope
//!    is a *warning* (a managed endpoint replaces it). (ADR-147 interop.)
//! 3. **duplicate-scalar** — the same top-level *scalar* key set by two
//!    fragments is a *warning*: last-wins by filename order silently drops the
//!    earlier value. Objects deep-merge and arrays concatenate under Claude
//!    Code's merge law, so those are not lossy and are not flagged.

use super::fragment::{Fragment, Scope};
use super::schema::{lookup, ScopeClass};
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Finding severity. An `Error` means the compiled `settings.json` would be
/// broken or the setting silently ignored; a `Warning` is a smell worth a look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One lint finding. `message` is self-contained (it names the key); `key` is
/// broken out for JSON consumers.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    /// Which check produced it: `schema` | `scope` | `duplicate`.
    pub check: &'static str,
    /// Source fragment path.
    pub file: String,
    /// The settings key involved.
    pub key: String,
    pub message: String,
}

/// Run all three checks over an already-loaded, filename-ordered fragment list.
/// Pure and deterministic — the CLI and the tests share this entry point.
pub fn check(frags: &[Fragment]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Per-fragment checks: schema-valid + scope-legal, in fragment then key order.
    for frag in frags {
        let file = frag.path.display().to_string();
        let obj = match frag.settings.as_object() {
            Some(o) => o,
            None => continue, // loader guarantees an object; defensive.
        };
        for (key, value) in obj {
            match lookup(key) {
                None => findings.push(Finding {
                    severity: Severity::Warning,
                    check: "schema",
                    file: file.clone(),
                    key: key.clone(),
                    message: format!(
                        "unrecognized settings key `{key}` — not validated \
                         (may be newer or version-gated)"
                    ),
                }),
                Some(spec) => {
                    // schema-valid: type check (Any never mismatches).
                    if !spec.ty.matches(value) {
                        findings.push(Finding {
                            severity: Severity::Error,
                            check: "schema",
                            file: file.clone(),
                            key: key.clone(),
                            message: format!(
                                "`{key}` expects {}, got {}",
                                spec.ty.name(),
                                json_kind(value)
                            ),
                        });
                    }
                    // scope-legal.
                    scope_finding(spec.class, frag.scope, key, &file)
                        .map(|f| findings.push(f));
                }
            }
        }
    }

    // Cross-fragment check: duplicate top-level scalar.
    let mut seen: HashMap<&str, &Path> = HashMap::new();
    for frag in frags {
        let Some(obj) = frag.settings.as_object() else { continue };
        for (key, value) in obj {
            if !is_scalar(value) {
                continue;
            }
            if let Some(prev) = seen.get(key.as_str()) {
                findings.push(Finding {
                    severity: Severity::Warning,
                    check: "duplicate",
                    file: frag.path.display().to_string(),
                    key: key.clone(),
                    message: format!(
                        "scalar `{key}` is also set in {} — last wins by filename \
                         order, the earlier value is dropped",
                        prev.display()
                    ),
                });
            } else {
                seen.insert(key.as_str(), frag.path.as_path());
            }
        }
    }

    findings
}

/// scope-legal verdict for one key given its class and the fragment's scope.
fn scope_finding(class: ScopeClass, scope: Scope, key: &str, file: &str) -> Option<Finding> {
    match class {
        ScopeClass::ManagedOnly if scope != Scope::Managed => Some(Finding {
            severity: Severity::Error,
            check: "scope",
            file: file.to_string(),
            key: key.to_string(),
            message: format!(
                "`{key}` is a managed-only setting; at {} scope Claude Code ignores it",
                scope.as_str()
            ),
        }),
        ScopeClass::ManagedOverridable if scope != Scope::Managed => Some(Finding {
            severity: Severity::Warning,
            check: "scope",
            file: file.to_string(),
            key: key.to_string(),
            message: format!(
                "`{key}` is replaced by managed scope; on a managed endpoint this \
                 {} value is ignored",
                scope.as_str()
            ),
        }),
        _ => None,
    }
}

fn is_scalar(v: &serde_json::Value) -> bool {
    v.is_string() || v.is_number() || v.is_boolean()
}

fn json_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// Print findings. Human-readable by default; a JSON array with `--json`.
pub fn report(findings: &[Finding], json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(findings).unwrap_or_else(|_| "[]".into())
        );
        return;
    }
    if findings.is_empty() {
        println!("settings lint: clean — no issues");
        return;
    }
    for f in findings {
        let tag = match f.severity {
            Severity::Error => "error",
            Severity::Warning => "warn ",
        };
        println!("  {tag} [{}] {}: {}", f.check, f.file, f.message);
    }
    let errors = findings.iter().filter(|f| f.severity == Severity::Error).count();
    let warns = findings.len() - errors;
    println!("settings lint: {errors} error(s), {warns} warning(s)");
}

/// Load, check, and report a config store. Returns `true` if any errors were
/// found (the caller maps that to a non-zero exit).
pub fn run(dir: &Path, json: bool) -> Result<bool> {
    let frags = super::fragment::load_dir(dir)?;
    let findings = check(&frags);
    report(&findings, json);
    Ok(findings.iter().any(|f| f.severity == Severity::Error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::settings::fragment::Fragment;
    use serde_json::json;
    use std::path::PathBuf;

    /// Build a Fragment in-memory (bypassing disk) for check() tests.
    fn frag(name: &str, scope: Scope, settings: serde_json::Value) -> Fragment {
        Fragment {
            path: PathBuf::from(name),
            order_prefix: None,
            scope,
            mandatory: false,
            settings,
            body: String::new(),
        }
    }

    fn of(findings: &[Finding], check: &str) -> Vec<Finding> {
        findings.iter().filter(|f| f.check == check).cloned().collect()
    }

    #[test]
    fn schema_type_mismatch_is_error() {
        let f = frag("10.md", Scope::User, json!({ "cleanupPeriodDays": "soon" }));
        let findings = check(&[f]);
        let schema = of(&findings, "schema");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].severity, Severity::Error);
        assert!(schema[0].message.contains("expects number"));
    }

    #[test]
    fn yaml_yes_becomes_schema_error_on_bool_key() {
        // The fidelity boundary, end-to-end: `autoUpdates: yes` loads as the
        // string "yes" (see fragment tests), which the Bool schema rejects.
        let f = frag("10.md", Scope::User, json!({ "autoUpdates": "yes" }));
        let schema = of(&check(&[f]), "schema");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].severity, Severity::Error);
        assert!(schema[0].message.contains("expects boolean"));
    }

    #[test]
    fn unknown_key_is_schema_warning_not_error() {
        let f = frag("10.md", Scope::User, json!({ "frobnicate": true }));
        let schema = of(&check(&[f]), "schema");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].severity, Severity::Warning);
        assert!(schema[0].message.contains("unrecognized"));
    }

    #[test]
    fn managed_only_at_user_scope_is_error() {
        let f = frag("10.md", Scope::User, json!({ "allowManagedHooksOnly": true }));
        let scope = of(&check(&[f]), "scope");
        assert_eq!(scope.len(), 1);
        assert_eq!(scope[0].severity, Severity::Error);
        assert!(scope[0].message.contains("managed-only"));
    }

    #[test]
    fn managed_only_at_managed_scope_is_clean() {
        let f = frag("10.md", Scope::Managed, json!({ "allowManagedHooksOnly": true }));
        assert!(of(&check(&[f]), "scope").is_empty());
    }

    #[test]
    fn managed_overridable_at_user_scope_is_warning() {
        let f = frag("10.md", Scope::User, json!({ "model": "opus" }));
        let scope = of(&check(&[f]), "scope");
        assert_eq!(scope.len(), 1);
        assert_eq!(scope[0].severity, Severity::Warning);
        assert!(scope[0].message.contains("managed scope"));
    }

    #[test]
    fn duplicate_scalar_across_fragments_warns_on_later() {
        let a = frag("10-a.md", Scope::User, json!({ "cleanupPeriodDays": 30 }));
        let b = frag("20-b.md", Scope::User, json!({ "cleanupPeriodDays": 90 }));
        let dup = of(&check(&[a, b]), "duplicate");
        assert_eq!(dup.len(), 1);
        assert_eq!(dup[0].severity, Severity::Warning);
        assert!(dup[0].file.contains("20-b.md"), "warning lands on the later file");
        assert!(dup[0].message.contains("10-a.md"), "and names the earlier one");
    }

    #[test]
    fn duplicate_ignores_objects_and_arrays() {
        // permissions (object) and an array key set twice — deep-merge/concat,
        // not lossy, so no duplicate finding.
        let a = frag("10.md", Scope::User, json!({ "permissions": { "allow": ["a"] }, "enabledMcpjsonServers": ["x"] }));
        let b = frag("20.md", Scope::User, json!({ "permissions": { "allow": ["b"] }, "enabledMcpjsonServers": ["y"] }));
        assert!(of(&check(&[a, b]), "duplicate").is_empty());
    }

    #[test]
    fn clean_store_yields_no_findings() {
        let f = frag(
            "10.md",
            Scope::User,
            json!({ "permissions": { "allow": ["Bash(git:*)"] }, "includeCoAuthoredBy": false }),
        );
        assert!(check(&[f]).is_empty());
    }
}
