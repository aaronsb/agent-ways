//! The projection manifest (ADR-144 §1).
//!
//! The manifest is the *desired state* of `~/.claude`: the list of entries the
//! reconciler materializes (as symlinks by default, copies on fallback). Each
//! entry is one relative path that exists identically under the source
//! (`$XDG_DATA/agent-ways`) and the projection (`~/.claude`).
//!
//! **Derivation (the ADR-144 correction).** The *projection surface* — which
//! subtrees belong in `~/.claude` at all — is a defined allowlist; docs, tools,
//! governance, scripts and the rest of the app stay in `$XDG_DATA`. But the
//! *contents* of those subtrees come from `git ls-files`, not a filesystem walk.
//! That is what makes the manifest do double duty as the app-vs-user
//! disentangler: a file sitting in `hooks/ways/` that git does **not** track is,
//! by construction, the user's — so it is never in the manifest, never
//! projected, and never pruned. The old `find`-based enumeration could not tell
//! those apart; `git ls-files` is the authoritative "what agent-ways ships."
//!
//! Binaries are the one non-git class: they are built artifacts (`bin/` is
//! gitignored), so they are enumerated by an explicit allowlist of names that
//! exist at the source.

use crate::paths;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Subtrees projected wholesale; contents enumerated via `git ls-files`.
const PROJECTED_TREES: &[&str] = &["skills", "agents", "commands", "hooks/ways"];

/// Individual top-level hook files settings.json references (git-tracked).
const PROJECTED_FILES: &[&str] = &["hooks/check-config-updates.sh", "hooks/refresh-claude-md.sh"];

/// Built binaries (NOT git-tracked) projected from `bin/`.
const PROJECTED_BINS: &[&str] = &["ways", "attend", "attend-chat", "way-embed"];

/// How an entry is sourced — which decides how it's kept current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryClass {
    /// Git-tracked source; authoritative membership is `git ls-files`.
    Tracked,
    /// Built artifact; membership is the binary allowlist + existence.
    Binary,
}

impl EntryClass {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryClass::Tracked => "tracked",
            EntryClass::Binary => "binary",
        }
    }
}

/// One projection entry: a path that exists identically under the source root
/// and the projection root.
#[derive(Debug, Clone)]
pub struct ManifestEntry {
    /// Path relative to both `$XDG_DATA/agent-ways` and `~/.claude`.
    pub rel: String,
    pub class: EntryClass,
}

/// Derive the desired-state manifest from a source checkout.
///
/// `source_root` is `$XDG_DATA/agent-ways` in production; tests and the dev
/// worktree pass their own root. Requires the source to be a git checkout for
/// the tracked class (the ADR-144 assumption); errors clearly if it is not.
pub fn build_manifest(source_root: &Path) -> Result<Vec<ManifestEntry>> {
    let mut entries: Vec<ManifestEntry> = Vec::new();

    // --- tracked: git ls-files over the projection allowlist ---
    // `-z` (NUL-delimited) so paths with spaces/newlines survive intact.
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(source_root)
        .args(["ls-files", "-z", "--"])
        .args(PROJECTED_TREES)
        .args(PROJECTED_FILES)
        .output()
        .with_context(|| format!("running git ls-files in {}", source_root.display()))?;

    if !out.status.success() {
        bail!(
            "git ls-files failed in {} (is it an agent-ways checkout?): {}",
            source_root.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    for rel in String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
    {
        entries.push(ManifestEntry { rel: rel.to_string(), class: EntryClass::Tracked });
    }

    // --- binary: allowlist that exists on disk ---
    for b in PROJECTED_BINS {
        let rel = format!("bin/{b}");
        if source_root.join(&rel).exists() {
            entries.push(ManifestEntry { rel, class: EntryClass::Binary });
        }
    }

    // Stable order: the manifest is diffed against the prior one and against the
    // live tree, so a deterministic sort keeps those comparisons clean.
    entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(entries)
}

/// `ways manifest` — print the desired-state manifest for inspection.
pub fn run(json_output: bool, source: Option<String>) -> Result<()> {
    let source_root: PathBuf = source.map(PathBuf::from).unwrap_or_else(paths::data_root);
    let projection_root = paths::projection_root();
    let entries = build_manifest(&source_root)?;

    if json_output {
        let arr: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "rel": e.rel,
                    "class": e.class.as_str(),
                    "source": source_root.join(&e.rel).to_string_lossy(),
                    "dest": projection_root.join(&e.rel).to_string_lossy(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "source_root": source_root.to_string_lossy(),
                "projection_root": projection_root.to_string_lossy(),
                "count": entries.len(),
                "entries": arr,
            }))?
        );
    } else {
        for e in &entries {
            println!("{:<8} {}", e.class.as_str(), e.rel);
        }
        eprintln!(
            "\n{} entries from {} → {}",
            entries.len(),
            source_root.display(),
            projection_root.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo root this crate lives in: CARGO_MANIFEST_DIR is …/tools/ways-cli,
    /// so two parents up is the agent-ways checkout (a git repo in dev + CI).
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("repo root")
            .to_path_buf()
    }

    #[test]
    fn manifest_is_nonempty_and_allowlisted() {
        let m = build_manifest(&repo_root()).expect("build manifest");
        assert!(!m.is_empty(), "manifest should not be empty");
        // Every tracked entry must fall under an allowlisted prefix — never a
        // stray app-internal path like docs/ or tools/.
        for e in m.iter().filter(|e| e.class == EntryClass::Tracked) {
            let ok = PROJECTED_TREES.iter().any(|t| e.rel.starts_with(&format!("{t}/")))
                || PROJECTED_FILES.contains(&e.rel.as_str());
            assert!(ok, "entry escaped the projection allowlist: {}", e.rel);
        }
    }

    #[test]
    fn manifest_excludes_app_internal_trees() {
        let m = build_manifest(&repo_root()).expect("build manifest");
        // docs/, tools/, governance/, scripts/ are app-internal — they stay in
        // $XDG_DATA and must never appear as projection entries.
        for forbidden in ["docs/", "tools/", "governance/", "scripts/", ".github/"] {
            assert!(
                !m.iter().any(|e| e.rel.starts_with(forbidden)),
                "app-internal path leaked into manifest: {forbidden}"
            );
        }
    }

    #[test]
    fn manifest_includes_way_content() {
        let m = build_manifest(&repo_root()).expect("build manifest");
        // The whole point: way files under hooks/ways are git-derived, so at
        // least one must show up without being hardcoded anywhere.
        assert!(
            m.iter().any(|e| e.rel.starts_with("hooks/ways/") && e.rel.ends_with(".md")),
            "expected at least one hooks/ways way file in the manifest"
        );
    }

    #[test]
    fn manifest_is_sorted() {
        let m = build_manifest(&repo_root()).expect("build manifest");
        let mut sorted = m.clone();
        sorted.sort_by(|a, b| a.rel.cmp(&b.rel));
        let got: Vec<&str> = m.iter().map(|e| e.rel.as_str()).collect();
        let want: Vec<&str> = sorted.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(got, want, "manifest must be deterministically sorted");
    }
}
