//! `ways settings project` — install a compiled store into the live
//! `settings.json` (ADR-147, the pipeline's final stage).
//!
//! This is a **second, disjoint writer** alongside the reconciler
//! ([`crate::cmd::settings_merge`]). The reconciler co-owns `{hooks, ways-perms}`;
//! `project` owns *the fragment store's keys* (`model`, `env`, `statusLine`, …).
//! Both preserve keys they don't own, so they coexist — the `kubectl apply`
//! field-ownership model with two controllers over disjoint fields. `project`
//! keeps its own last-applied base (`$XDG_STATE/agent-ways/settings-fragments-
//! <scope>.json`) so it can remove a key it stops setting without disturbing the
//! user's — or the reconciler's — fields.
//!
//! MVP scope: top-level keys only, override + cleanup. `hooks` and `permissions`
//! are **skipped with a warning** — the reconciler co-owns them, and recursively
//! co-merging the `permissions` object is deferred. Managed scope is never
//! auto-written to a system path; it prints the blob for the console.

use super::compile::{self, Outcome};
use super::fragment::Scope;
use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Keys the reconciler co-owns — `project` must not touch them (MVP).
fn reconciler_owned(key: &str) -> bool {
    matches!(key, "hooks" | "permissions")
}

pub fn run(store: &Path, scope_filter: Option<Scope>, dry_run: bool) -> Result<bool> {
    let scopes = match compile::compile_store(store, scope_filter)? {
        Outcome::Refused(errors) => {
            compile::report_refusal(store, &errors);
            return Ok(true);
        }
        Outcome::Baked(scopes) => scopes,
    };
    if scopes.is_empty() {
        bail!("nothing to project in {}", store.display());
    }

    for (scope, baked, _prov) in scopes {
        match scope {
            Scope::User => project_into(scope, &crate::paths::settings_json(), &baked, dry_run)?,
            Scope::Project => project_into(scope, &project_settings_path(), &baked, dry_run)?,
            Scope::Managed => {
                println!("# managed scope — paste into the enterprise console (not auto-written):");
                println!("{}", serde_json::to_string_pretty(&baked)?);
            }
        }
    }
    Ok(false)
}

/// `$CLAUDE_PROJECT_DIR` (or cwd) `/.claude/settings.json`.
fn project_settings_path() -> PathBuf {
    let root = std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    root.join(".claude").join("settings.json")
}

/// The last-applied base for a scope: `$XDG_STATE/agent-ways/settings-fragments-<scope>.json`.
fn base_path(scope: Scope) -> PathBuf {
    crate::paths::state_root().join(format!("settings-fragments-{}.json", scope.as_str()))
}

fn read_json(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", path.display())),
        Err(_) => Ok(Value::Object(Map::new())), // absent = empty
    }
}

/// Three-way merge the baked object into the live settings for one scope, using
/// this scope's persisted base.
fn project_into(scope: Scope, target: &Path, baked: &Value, dry_run: bool) -> Result<()> {
    project_write(target, baked, dry_run, &base_path(scope), scope.as_str())
}

/// The merge core, with the base file injected — so tests supply their own base
/// path rather than mutating the process-global `$XDG_STATE_HOME`.
fn project_write(
    target: &Path,
    baked: &Value,
    dry_run: bool,
    base_file: &Path,
    scope_label: &str,
) -> Result<()> {
    let ours = baked.as_object().cloned().unwrap_or_default();
    let live = read_json(target)?;
    let base = read_json(base_file)?;
    let base_obj = base.as_object().cloned().unwrap_or_default();

    let mut result = live.as_object().cloned().unwrap_or_default();
    let mut applied = Map::new(); // what we end up owning — the next base
    let mut changes: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // Keys we currently set or previously set (for cleanup).
    let keys: BTreeSet<&String> = ours.keys().chain(base_obj.keys()).collect();
    for key in keys {
        if reconciler_owned(key) {
            if ours.contains_key(key) {
                skipped.push(key.clone());
            }
            continue;
        }
        match ours.get(key) {
            Some(val) => {
                // Own it: override the live value.
                if result.get(key) != Some(val) {
                    changes.push(format!("set {key}"));
                }
                result.insert(key.clone(), val.clone());
                applied.insert(key.clone(), val.clone());
            }
            None => {
                // We set it last time but not now — remove, unless the user has
                // since changed it from what we wrote (then leave it to them).
                if result.get(key) == base_obj.get(key) {
                    if result.remove(key).is_some() {
                        changes.push(format!("remove {key}"));
                    }
                } else {
                    changes.push(format!("keep {key} (changed since we set it)"));
                }
            }
        }
    }

    // Report.
    let label = format!("{} scope -> {}", scope_label, target.display());
    if changes.is_empty() {
        println!("{label}: already up to date");
    } else {
        println!("{label}:");
        for c in &changes {
            println!("  {c}");
        }
    }
    if !skipped.is_empty() {
        eprintln!(
            "  note: skipped reconciler-owned key(s) {} — the reconciler manages those; \
             project them via the managed blob or `ways reconcile` instead",
            skipped.join(", ")
        );
    }
    if dry_run {
        println!("  (dry-run — nothing written)");
        return Ok(());
    }

    // Write the merged settings (with a backup) and persist the new base.
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if target.exists() {
        let backup = target.with_extension("json.bak");
        std::fs::copy(target, &backup).ok();
    }
    std::fs::write(
        target,
        format!("{}\n", serde_json::to_string_pretty(&Value::Object(result))?),
    )
    .with_context(|| format!("writing {}", target.display()))?;

    if let Some(parent) = base_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        base_file,
        format!("{}\n", serde_json::to_string_pretty(&Value::Object(applied))?),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn tmpdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ways-project-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: &Path, v: &Value) {
        std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
    }
    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn overrides_owned_key_and_preserves_others() {
        let dir = tmpdir();
        let target = dir.join("settings.json");
        let base = dir.join("base.json");
        write(&target, &json!({ "theme": "dark", "model": "sonnet" }));
        project_write(&target, &json!({ "model": "opus" }), false, &base, "user").unwrap();
        let after = read(&target);
        assert_eq!(after["model"], json!("opus"), "our key overridden");
        assert_eq!(after["theme"], json!("dark"), "user's key preserved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_a_key_we_stopped_setting() {
        let dir = tmpdir();
        let target = dir.join("settings.json");
        let base = dir.join("base.json");
        // First projection sets cleanupPeriodDays.
        project_write(&target, &json!({ "cleanupPeriodDays": 30 }), false, &base, "user").unwrap();
        assert_eq!(read(&target)["cleanupPeriodDays"], json!(30));
        // Second projection drops it -> removed (base records we owned it).
        project_write(&target, &json!({}), false, &base, "user").unwrap();
        assert!(read(&target).get("cleanupPeriodDays").is_none(), "dropped key removed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_reconciler_owned_keys() {
        let dir = tmpdir();
        let target = dir.join("settings.json");
        let base = dir.join("base.json");
        write(&target, &json!({ "hooks": { "SessionStart": [] } }));
        project_write(
            &target,
            &json!({ "permissions": { "allow": ["x"] }, "model": "opus" }),
            false,
            &base,
            "user",
        )
        .unwrap();
        let after = read(&target);
        assert_eq!(after["model"], json!("opus"), "non-owned key applied");
        assert!(after.get("permissions").is_none(), "permissions skipped (reconciler-owned)");
        assert!(after["hooks"].is_object(), "reconciler's hooks preserved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_writes_nothing() {
        let dir = tmpdir();
        let target = dir.join("settings.json");
        let base = dir.join("base.json");
        write(&target, &json!({ "model": "sonnet" }));
        project_write(&target, &json!({ "model": "opus" }), true, &base, "user").unwrap();
        assert_eq!(read(&target)["model"], json!("sonnet"), "dry-run left the file untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
