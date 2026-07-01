//! The config-store linter (ADR-147): three deterministic checks over a loaded
//! fragment tree, plus a reporter.
//!
//! 1. **schema-valid** — every top-level `settings:` key is known to the settings
//!    schema or the scope overlay (unknown → *warning*, never error) and its
//!    value matches the settings schema's type (mismatch → *error*; unions
//!    validate permissively).
//! 2. **scope-legal** — a managed-only key authored at user/project scope is an
//!    *error* (Claude Code ignores it there); a managed-overridable key
//!    (`model`/`fallbackModel`/`availableModels`) authored outside managed scope
//!    is a *warning* (a managed endpoint replaces it). (ADR-147 interop.)
//! 3. **duplicate-scalar** — the same top-level *scalar* key set by two
//!    fragments is a *warning*: last-wins by filename order silently drops the
//!    earlier value. Objects deep-merge and arrays concatenate under Claude
//!    Code's merge law, so those are not lossy and are not flagged.

use super::fragment::{Fragment, Scope};
use super::schema::{overlay_knows, scope_class, ScopeClass};
use super::schema_doc;
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
/// Pure and deterministic — the CLI and the tests share this entry point. The
/// settings schema is injected (`None` = unavailable) rather than resolved
/// inside, so it degrades explicitly and stays testable.
pub fn check(frags: &[Fragment], schema: Option<&schema_doc::SettingsSchema>) -> Vec<Finding> {
    let mut findings = Vec::new();

    // No schema available: note it once and skip schema-valid entirely — do not
    // flood one "unrecognized" per key. Scope-legal + duplicate still run off the
    // overlay, so the linter stays useful.
    if schema.is_none() {
        findings.push(Finding {
            severity: Severity::Warning,
            check: "schema",
            // Synthetic notice (not a fragment finding): `file` names the schema
            // path we looked for, and `key` is empty.
            file: crate::paths::settings_schema_file().display().to_string(),
            key: String::new(),
            message: "settings schema not found — schema-valid checks skipped; \
                      run `ways settings schema --refresh`"
                .to_string(),
        });
    }

    // Per-fragment checks: schema-valid + scope-legal, in fragment then key order.
    for frag in frags {
        let file = frag.path.display().to_string();
        let obj = match frag.settings.as_object() {
            Some(o) => o,
            None => continue, // loader guarantees an object; defensive.
        };
        for (key, value) in obj {
            let sclass = scope_class(key);

            // schema-valid — only when a schema is available. A key is "known" if
            // the schema has it OR the overlay does (the overlay carries keys
            // SchemaStore still lags). Unknown -> warning, never error. Known +
            // typed -> type-check (unions validate permissively; see schema_doc).
            if let Some(sc) = schema {
                match sc.get(key) {
                    Some(info) => {
                        if !info.ty.matches(value) {
                            findings.push(Finding {
                                severity: Severity::Error,
                                check: "schema",
                                file: file.clone(),
                                key: key.clone(),
                                message: format!(
                                    "`{key}` expects {}, got {}",
                                    info.ty.name(),
                                    super::fragment::json_kind(value)
                                ),
                            });
                        }
                    }
                    None if !overlay_knows(key) => findings.push(Finding {
                        severity: Severity::Warning,
                        check: "schema",
                        file: file.clone(),
                        key: key.clone(),
                        message: format!(
                            "unrecognized settings key `{key}` — not validated \
                             (may be newer or version-gated)"
                        ),
                    }),
                    None => {} // known to the overlay but not the schema.
                }
            }

            // scope-legal (overlay) — runs regardless of schema availability.
            if let Some(class) = sclass {
                if let Some(f) = scope_finding(class, frag.scope, key, &file) {
                    findings.push(f);
                }
            }
        }
    }

    // Cross-fragment check: duplicate top-level scalar, partitioned by scope.
    // Scopes compile to different destination files (user/project/managed), so a
    // scalar set once per scope is not a conflict — only a repeat *within* a
    // scope silently drops a value. `seen` tracks the most recent occurrence so a
    // 3+ chain points each warning at its immediate predecessor.
    let mut seen: HashMap<(Scope, &str), &Path> = HashMap::new();
    for frag in frags {
        let Some(obj) = frag.settings.as_object() else { continue };
        for (key, value) in obj {
            // A repeat is lossy exactly when compile would *override* it: scalars,
            // and arrays that aren't concat paths. Objects deep-merge and concat
            // lists union — those are not lossy, so they aren't flagged. The
            // concat law is shared with compile so the two can't disagree.
            let lossy = is_scalar(value)
                || (value.is_array() && !super::compile::is_concat_path(key));
            if !lossy {
                continue;
            }
            let slot = (frag.scope, key.as_str());
            if let Some(prev) = seen.get(&slot) {
                let prev = prev.display().to_string();
                findings.push(Finding {
                    severity: Severity::Warning,
                    check: "duplicate",
                    file: frag.path.display().to_string(),
                    key: key.clone(),
                    message: format!(
                        "`{key}` is also set in {prev} at {} scope — last wins by \
                         filename order, the earlier value is dropped",
                        frag.scope.as_str()
                    ),
                });
            }
            seen.insert(slot, frag.path.as_path());
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
    let findings = check(&frags, schema_doc::active());
    report(&findings, json);
    Ok(has_errors(&findings))
}

/// Whether any finding is an error — the shared "should this gate fail?" predicate
/// (used by lint's own exit code and by `compile`'s lint gate).
pub fn has_errors(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Error)
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

    /// Run check() with the shipped schema injected — the normal case.
    fn checked(frags: &[Fragment]) -> Vec<Finding> {
        check(frags, Some(&schema_doc::test_schema()))
    }

    #[test]
    fn absent_schema_degrades_gracefully() {
        // No schema: one notice, schema-valid skipped (no per-key "unrecognized"
        // flood), but scope-legal still fires. The linter stays useful.
        let f = frag(
            "10.md",
            Scope::User,
            json!({ "totallyMadeUpKey": 1, "model": "opus" }),
        );
        let findings = check(&[f], None);
        let schema = of(&findings, "schema");
        assert_eq!(schema.len(), 1, "exactly one 'schema skipped' notice");
        assert!(schema[0].message.contains("skipped"));
        assert_eq!(of(&findings, "scope").len(), 1, "scope-legal still runs");
    }

    #[test]
    fn schema_type_mismatch_is_error() {
        let f = frag("10.md", Scope::User, json!({ "cleanupPeriodDays": "soon" }));
        let findings = checked(&[f]);
        let schema = of(&findings, "schema");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].severity, Severity::Error);
        assert!(schema[0].message.contains("expects number"));
    }

    #[test]
    fn yaml_yes_becomes_schema_error_on_bool_key() {
        // The fidelity boundary, end-to-end: `autoMemoryEnabled: yes` loads as
        // the string "yes" (see fragment tests), which the settings schema's
        // boolean type rejects.
        let f = frag("10.md", Scope::User, json!({ "autoMemoryEnabled": "yes" }));
        let schema = of(&checked(&[f]), "schema");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].severity, Severity::Error);
        assert!(schema[0].message.contains("expects boolean"));
    }

    #[test]
    fn union_typed_key_does_not_false_error() {
        // Regression: strictPluginOnlyCustomization is anyOf[boolean,array]; a
        // boolean `true` must not raise a schema (type) error. Managed scope
        // keeps it scope-legal so we isolate the schema check.
        let f = frag("10.md", Scope::Managed, json!({ "strictPluginOnlyCustomization": true }));
        assert!(of(&checked(&[f]), "schema").is_empty());
        let f2 = frag("10.md", Scope::Managed, json!({ "strictPluginOnlyCustomization": [] }));
        assert!(of(&checked(&[f2]), "schema").is_empty());
    }

    #[test]
    fn schema_lagged_known_key_is_not_flagged() {
        // Regression: `autoUpdates` is valid but absent from SchemaStore; the
        // overlay marks it known, so no "unrecognized" warning.
        let f = frag("10.md", Scope::User, json!({ "autoUpdates": false }));
        assert!(of(&checked(&[f]), "schema").is_empty());
    }

    #[test]
    fn unknown_key_is_schema_warning_not_error() {
        let f = frag("10.md", Scope::User, json!({ "frobnicate": true }));
        let schema = of(&checked(&[f]), "schema");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].severity, Severity::Warning);
        assert!(schema[0].message.contains("unrecognized"));
    }

    #[test]
    fn managed_only_at_user_scope_is_error() {
        let f = frag("10.md", Scope::User, json!({ "allowManagedHooksOnly": true }));
        let scope = of(&checked(&[f]), "scope");
        assert_eq!(scope.len(), 1);
        assert_eq!(scope[0].severity, Severity::Error);
        assert!(scope[0].message.contains("managed-only"));
    }

    #[test]
    fn managed_only_at_managed_scope_is_clean() {
        let f = frag("10.md", Scope::Managed, json!({ "allowManagedHooksOnly": true }));
        assert!(of(&checked(&[f]), "scope").is_empty());
    }

    #[test]
    fn managed_overridable_at_user_scope_is_warning() {
        let f = frag("10.md", Scope::User, json!({ "model": "opus" }));
        let scope = of(&checked(&[f]), "scope");
        assert_eq!(scope.len(), 1);
        assert_eq!(scope[0].severity, Severity::Warning);
        assert!(scope[0].message.contains("managed scope"));
    }

    #[test]
    fn duplicate_scalar_across_fragments_warns_on_later() {
        let a = frag("10-a.md", Scope::User, json!({ "cleanupPeriodDays": 30 }));
        let b = frag("20-b.md", Scope::User, json!({ "cleanupPeriodDays": 90 }));
        let dup = of(&checked(&[a, b]), "duplicate");
        assert_eq!(dup.len(), 1);
        assert_eq!(dup[0].severity, Severity::Warning);
        assert!(dup[0].file.contains("20-b.md"), "warning lands on the later file");
        assert!(dup[0].message.contains("10-a.md"), "and names the earlier one");
    }

    #[test]
    fn duplicate_scalar_is_partitioned_by_scope() {
        // The same scalar at user vs project scope compiles to different files —
        // both apply, nothing is dropped, so no duplicate finding.
        let u = frag("10.md", Scope::User, json!({ "cleanupPeriodDays": 30 }));
        let p = frag("20.md", Scope::Project, json!({ "cleanupPeriodDays": 90 }));
        assert!(of(&checked(&[u, p]), "duplicate").is_empty());
    }

    #[test]
    fn denied_mcp_servers_at_user_scope_is_clean() {
        // Regression: deniedMcpServers concatenates from user scope (ADR-147
        // interop); it must not raise a scope-legal error.
        let f = frag("10.md", Scope::User, json!({ "deniedMcpServers": ["evil-server"] }));
        assert!(of(&checked(&[f]), "scope").is_empty());
    }

    #[test]
    fn duplicate_ignores_objects_and_concat_arrays() {
        // permissions (object, deep-merges) and deniedMcpServers (a concat path,
        // unions) set twice are not lossy, so no duplicate finding.
        let a = frag("10.md", Scope::User, json!({ "permissions": { "allow": ["a"] }, "deniedMcpServers": ["x"] }));
        let b = frag("20.md", Scope::User, json!({ "permissions": { "allow": ["b"] }, "deniedMcpServers": ["y"] }));
        assert!(of(&checked(&[a, b]), "duplicate").is_empty());
    }

    #[test]
    fn duplicate_warns_on_overriding_array() {
        // A non-concat array (enabledMcpjsonServers) set twice IS lossy — compile
        // overrides it (last wins), so lint must warn. Closes the lint/compile
        // divergence (PR #242 review H2).
        let a = frag("10.md", Scope::User, json!({ "enabledMcpjsonServers": ["x"] }));
        let b = frag("20.md", Scope::User, json!({ "enabledMcpjsonServers": ["y"] }));
        assert_eq!(of(&checked(&[a, b]), "duplicate").len(), 1);
    }

    #[test]
    fn clean_store_yields_no_findings() {
        let f = frag(
            "10.md",
            Scope::User,
            json!({ "permissions": { "allow": ["Bash(git:*)"] }, "includeCoAuthoredBy": false }),
        );
        assert!(checked(&[f]).is_empty());
    }
}
