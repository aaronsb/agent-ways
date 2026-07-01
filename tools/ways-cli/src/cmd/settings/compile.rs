//! Compile a fragment store into baked `settings.json` (ADR-147).
//!
//! Each scope's fragments are deep-merged in filename order into one baked
//! object, following Claude Code's documented merge law. The law is small — a
//! path-keyed strategy, not a merge engine:
//!
//! - **objects deep-merge** (recurse key by key);
//! - the documented **concat** paths (`permissions.allow/deny/ask`,
//!   `deniedMcpServers`) union their arrays with de-duplication;
//! - **everything else overrides** — the last fragment in filename order wins,
//!   which is exactly the case the duplicate-scalar lint warns about.
//!
//! Merging also yields a **provenance** map (`dotted path → contributing
//! fragment(s)`): the effective (last) source for an overridden key, every
//! contributor for a concatenated list. That is both `git blame`-for-config and
//! the future reconciler's merge base.

use super::fragment::{load_dir, Fragment, Scope};
use super::{lint, schema_doc};
use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// `dotted path → fragment file name(s)` that produced the effective value.
pub type Provenance = BTreeMap<String, Vec<String>>;

/// Compile a store: lint-gate, then merge each scope into a baked settings
/// object + provenance. Returns `true` if the lint gate failed (caller exits
/// non-zero). With `out`, writes `<scope>.settings.json` + `provenance.json`;
/// otherwise prints a single scope's baked blob to stdout.
pub fn run(store: &Path, scope_filter: Option<Scope>, out: Option<&Path>) -> Result<bool> {
    let frags = load_dir(store)?;

    // Lint gate — refuse to bake a store with errors (warnings are fine).
    let findings = lint::check(&frags, schema_doc::active());
    let errors: Vec<_> = findings
        .iter()
        .filter(|f| f.severity == lint::Severity::Error)
        .collect();
    if !errors.is_empty() {
        eprintln!(
            "compile refused: {} lint error(s) — fix first (`ways settings lint {}`):",
            errors.len(),
            store.display()
        );
        for e in &errors {
            eprintln!("  {}: {}", e.file, e.message);
        }
        return Ok(true);
    }

    // Merge each scope present (or the requested one), in filename order.
    let mut baked: BTreeMap<&str, Value> = BTreeMap::new();
    let mut provenance: BTreeMap<&str, Provenance> = BTreeMap::new();
    for scope in [Scope::User, Scope::Project, Scope::Managed] {
        if scope_filter.is_some_and(|f| f != scope) {
            continue;
        }
        let scoped: Vec<&Fragment> = frags.iter().filter(|fr| fr.scope == scope).collect();
        if scoped.is_empty() {
            continue;
        }
        let (obj, prov) = merge_scope(&scoped);
        baked.insert(scope.as_str(), obj);
        provenance.insert(scope.as_str(), prov);
    }

    if baked.is_empty() {
        bail!(
            "no fragments to compile in {}{}",
            store.display(),
            scope_filter
                .map(|s| format!(" at {} scope", s.as_str()))
                .unwrap_or_default()
        );
    }

    match out {
        Some(dir) => {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating {}", dir.display()))?;
            for (scope, obj) in &baked {
                let path = dir.join(format!("{scope}.settings.json"));
                std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(obj)?))
                    .with_context(|| format!("writing {}", path.display()))?;
                println!("Wrote {}", path.display());
            }
            let prov_path = dir.join("provenance.json");
            std::fs::write(
                &prov_path,
                format!("{}\n", serde_json::to_string_pretty(&provenance)?),
            )?;
            println!("Wrote {}", prov_path.display());
        }
        None => {
            if baked.len() > 1 {
                bail!(
                    "store has multiple scopes ({}); pass --scope or --out",
                    baked.keys().copied().collect::<Vec<_>>().join(", ")
                );
            }
            let obj = baked.values().next().expect("non-empty");
            println!("{}", serde_json::to_string_pretty(obj)?);
        }
    }
    Ok(false)
}

/// The documented concat-and-dedupe paths. Everything else overrides (objects
/// deep-merge). Kept explicit and minimal; extend as Claude Code's edge cases
/// surface (see ADR-147 Q3 — the merge law is a spec to mirror, not invent).
fn is_concat(path: &str) -> bool {
    matches!(
        path,
        "permissions.allow" | "permissions.deny" | "permissions.ask" | "deniedMcpServers"
    )
}

/// Deep-merge a scope's fragments (already filename-ordered) into one baked
/// settings object, plus the provenance of every leaf.
pub fn merge_scope(frags: &[&Fragment]) -> (Value, Provenance) {
    let mut acc = Value::Object(Map::new());
    let mut prov = Provenance::new();
    for frag in frags {
        let name = frag
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<fragment>")
            .to_string();
        if let Some(obj) = frag.settings.as_object() {
            merge_object(&mut acc, obj, "", &name, &mut prov);
        }
    }
    (acc, prov)
}

/// Merge `incoming` into `acc` (both objects) at `prefix`, recording provenance.
fn merge_object(
    acc: &mut Value,
    incoming: &Map<String, Value>,
    prefix: &str,
    frag: &str,
    prov: &mut Provenance,
) {
    let acc_obj = acc.as_object_mut().expect("merge target must be an object");
    for (key, val) in incoming {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };

        if is_concat(&path) && val.is_array() {
            // Concat + dedupe (order-preserving) into the accumulated array.
            let entry = acc_obj
                .entry(key.clone())
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(arr) = entry.as_array_mut() {
                for item in val.as_array().expect("checked is_array") {
                    if !arr.contains(item) {
                        arr.push(item.clone());
                    }
                }
            } else {
                *entry = val.clone(); // prior value wasn't an array — replace
            }
            prov.entry(path).or_default().push(frag.to_string());
        } else if val.is_object() {
            // Deep-merge: ensure the slot is an object, then recurse.
            let entry = acc_obj
                .entry(key.clone())
                .or_insert_with(|| Value::Object(Map::new()));
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            merge_object(entry, val.as_object().expect("checked is_object"), &path, frag, prov);
        } else {
            // Override — last fragment wins; provenance is the effective source.
            acc_obj.insert(key.clone(), val.clone());
            prov.insert(path, vec![frag.to_string()]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fragment::Scope;
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn frag(name: &str, settings: Value) -> Fragment {
        Fragment {
            path: PathBuf::from(name),
            order_prefix: None,
            scope: Scope::User,
            mandatory: false,
            settings,
            body: String::new(),
        }
    }

    fn merge(frags: &[Fragment]) -> (Value, Provenance) {
        let refs: Vec<&Fragment> = frags.iter().collect();
        merge_scope(&refs)
    }

    #[test]
    fn scalar_override_last_wins() {
        let (baked, prov) = merge(&[
            frag("10-a.md", json!({ "model": "sonnet" })),
            frag("20-b.md", json!({ "model": "opus" })),
        ]);
        assert_eq!(baked["model"], json!("opus"));
        assert_eq!(prov["model"], vec!["20-b.md"]);
    }

    #[test]
    fn permission_lists_concat_and_dedupe() {
        let (baked, prov) = merge(&[
            frag("10-a.md", json!({ "permissions": { "allow": ["Bash(git:*)", "Bash(gh:*)"] } })),
            frag("20-b.md", json!({ "permissions": { "allow": ["Bash(gh:*)", "Bash(ls:*)"] } })),
        ]);
        assert_eq!(
            baked["permissions"]["allow"],
            json!(["Bash(git:*)", "Bash(gh:*)", "Bash(ls:*)"]),
            "union, de-duplicated, order-preserving"
        );
        assert_eq!(prov["permissions.allow"], vec!["10-a.md", "20-b.md"]);
    }

    #[test]
    fn objects_deep_merge_union_and_leaf_override() {
        let (baked, _) = merge(&[
            frag("10-a.md", json!({ "env": { "X": "1", "Y": "keep" } })),
            frag("20-b.md", json!({ "env": { "Y": "override", "Z": "2" } })),
        ]);
        assert_eq!(baked["env"], json!({ "X": "1", "Y": "override", "Z": "2" }));
    }

    #[test]
    fn nested_object_mixes_concat_and_override_in_one_merge() {
        // permissions.allow concatenates while permissions.defaultMode overrides,
        // in the same deep-merge of the `permissions` object.
        let (baked, prov) = merge(&[
            frag("10-a.md", json!({ "permissions": { "allow": ["a"], "defaultMode": "acceptEdits" } })),
            frag("20-b.md", json!({ "permissions": { "allow": ["b"], "defaultMode": "plan" } })),
        ]);
        assert_eq!(baked["permissions"]["allow"], json!(["a", "b"]));
        assert_eq!(baked["permissions"]["defaultMode"], json!("plan"));
        assert_eq!(prov["permissions.defaultMode"], vec!["20-b.md"]);
        assert_eq!(prov["permissions.allow"], vec!["10-a.md", "20-b.md"]);
    }

    #[test]
    fn plain_arrays_override_not_concat() {
        // A non-permission array (availableModels) overrides — most arrays do.
        let (baked, _) = merge(&[
            frag("10-a.md", json!({ "availableModels": ["opus"] })),
            frag("20-b.md", json!({ "availableModels": ["sonnet", "haiku"] })),
        ]);
        assert_eq!(baked["availableModels"], json!(["sonnet", "haiku"]));
    }

    #[test]
    fn single_fragment_passes_through_with_provenance() {
        let (baked, prov) = merge(&[frag(
            "10-a.md",
            json!({ "model": "opus", "permissions": { "allow": ["x"] } }),
        )]);
        assert_eq!(baked["model"], json!("opus"));
        assert_eq!(baked["permissions"]["allow"], json!(["x"]));
        assert_eq!(prov["model"], vec!["10-a.md"]);
        assert_eq!(prov["permissions.allow"], vec!["10-a.md"]);
    }
}
