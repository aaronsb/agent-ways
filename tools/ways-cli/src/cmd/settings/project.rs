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
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Keys the reconciler co-owns — `project` must not touch them (MVP). Single
/// source of this ownership fact: `show` consults it too (for a key's live
/// status), so the two components cannot diverge.
pub(crate) fn reconciler_owned(key: &str) -> bool {
    matches!(key, "hooks" | "permissions")
}

pub fn run(store: &Path, scope_filter: Option<Scope>, dry_run: bool) -> Result<bool> {
    let baked = match compile::compile_store(store, scope_filter)? {
        Outcome::Refused(errors) => {
            compile::report_refusal(store, &errors);
            return Ok(true);
        }
        Outcome::Baked(scopes) => scopes,
    };
    let mut baked_map: HashMap<Scope, Value> = HashMap::new();
    for (scope, obj, _prov) in baked {
        baked_map.insert(scope, obj);
    }

    // Process a scope if it has fragments now OR a base file to clean up — the
    // latter is how deleting all of a scope's fragments GCs its keys from the
    // live settings. Managed is never auto-written, so it only appears when it
    // currently has fragments.
    let mut did = false;
    for scope in [Scope::User, Scope::Project, Scope::Managed] {
        if scope_filter.is_some_and(|f| f != scope) {
            continue;
        }
        if scope == Scope::Managed {
            if let Some(obj) = baked_map.get(&scope) {
                println!("# managed scope — paste into the enterprise console (not auto-written):");
                println!("{}", serde_json::to_string_pretty(obj)?);
                did = true;
            }
            continue;
        }
        let has_base = base_path(scope).exists();
        if !baked_map.contains_key(&scope) && !has_base {
            continue;
        }
        // An emptied scope (base exists, no fragments) projects an empty object,
        // which removes everything it previously owned.
        let ours = baked_map
            .get(&scope)
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        let target = if scope == Scope::User {
            crate::paths::settings_json()
        } else {
            project_settings_path()
        };
        project_into(scope, &target, &ours, dry_run)?;
        did = true;
    }
    if !did {
        bail!("nothing to project in {}", store.display());
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
        Ok(text) if text.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(text) => serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display())),
        // Only a genuinely-absent file is treated as empty; a real I/O error
        // (permissions, transient) must propagate — never rewrite a file we
        // couldn't read as if it held nothing.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(anyhow::Error::new(e)).with_context(|| format!("reading {}", path.display())),
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
    // Self-audit (analogous to the reconciler's stripped_user_view check): every
    // key we change must be one we own. By construction we only touch owned keys,
    // so this never fires — it's the net that turns any future bug into a refusal
    // instead of silent corruption of the user's live config.
    let owned: BTreeSet<&String> = ours
        .keys()
        .chain(base.as_object().map(|o| o.keys()).into_iter().flatten())
        .filter(|k| !reconciler_owned(k))
        .collect();
    let live_obj = live.as_object().cloned().unwrap_or_default();
    let all: BTreeSet<&String> = live_obj.keys().chain(result.keys()).collect();
    for key in all {
        if live_obj.get(key) != result.get(key) && !owned.contains(key) {
            bail!(
                "project self-audit: refusing to change unmanaged key `{key}` in {}",
                target.display()
            );
        }
    }

    if dry_run {
        println!("  (dry-run — nothing written)");
        return Ok(());
    }

    // Back up (distinct slot from the reconciler's) and write atomically.
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if target.exists() {
        let backup = target.with_extension("json.ways-project.bak");
        std::fs::copy(target, &backup).ok();
    }
    crate::cmd::settings_merge::write_json_atomic(target, &Value::Object(result))?;

    if let Some(parent) = base_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::cmd::settings_merge::write_json_atomic(base_file, &Value::Object(applied))?;
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
    fn coexists_with_reconciler_and_user_keys() {
        // One write that exercises all three ownership classes at once: our new
        // key applied, our dropped key removed, reconciler's hooks/perms and the
        // user's theme all preserved.
        let dir = tmpdir();
        let target = dir.join("settings.json");
        let base = dir.join("base.json");
        write(&base, &json!({ "oldKey": "x" }));
        write(
            &target,
            &json!({
                "hooks": { "SessionStart": [] },
                "permissions": { "allow": ["Bash(ways:*)"] },
                "theme": "dark",
                "oldKey": "x"
            }),
        );
        project_write(&target, &json!({ "model": "opus" }), false, &base, "user").unwrap();
        let after = read(&target);
        assert_eq!(after["model"], json!("opus"), "our key applied");
        assert!(after.get("oldKey").is_none(), "our dropped key removed");
        assert!(after["hooks"].is_object(), "reconciler hooks preserved");
        assert_eq!(after["permissions"]["allow"][0], json!("Bash(ways:*)"), "reconciler perms preserved");
        assert_eq!(after["theme"], json!("dark"), "user key preserved");
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
