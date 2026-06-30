//! The legacy-in-place → XDG migrator (ADR-144 §5).
//!
//! This is the highest-trust operation agent-ways performs: it rewrites the
//! user's actual `~/.claude`. Per ADR-142's autonomy gradient it is **gated,
//! backed up, announced, and crash-safe** — never a silent SessionStart side
//! effect.
//!
//! This module currently implements the **planner** (`--plan`), which is
//! strictly read-only: it classifies the live in-place clone against git truth
//! and prints what the migration *would* do. Running it against a real
//! `~/.claude` is safe and is the recommended way to preview the swap. The
//! executor (the phased, marker-driven, resumable mutation) is built and
//! sandbox-tested separately and is gated behind `--execute`.
//!
//! Classification (the ADR-144 §1 set difference, applied to the old clone):
//! git-tracked files are the **app** → `$XDG_DATA`; everything else is the
//! user's or Claude Code's and is preserved — settings.json and `projects/`
//! stay (Claude-Code-owned), `stats/` lifts to `$XDG_STATE`, untracked user
//! ways lift to `$XDG_CONFIG`.

use crate::paths;
use crate::util::home_dir;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

struct Plan {
    dest: PathBuf,
    tracked_count: usize,
    projection_roots: Vec<String>,
    user_ways: Vec<String>,
    has_settings: bool,
    transcript_count: usize,
    has_stats: bool,
    cache_old: Option<PathBuf>,
}

/// True if `dest` is a git clone whose origin is the agent-ways repo — i.e. a
/// legacy in-place install, the only from-state the migrator handles.
fn is_agent_ways_clone(dest: &Path) -> bool {
    if !dest.join(".git").exists() {
        return false;
    }
    let url = git_output(dest, &["remote", "get-url", "origin"]).unwrap_or_default();
    url.contains("agent-ways")
}

/// Run a git command in `dir`, returning trimmed stdout on success.
fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// NUL-safe `git ls-files` variant returning relative paths.
fn git_files(dir: &Path, args: &[&str]) -> Vec<String> {
    let mut full = vec!["ls-files", "-z"];
    full.extend_from_slice(args);
    let out = match std::process::Command::new("git").arg("-C").arg(dir).args(&full).output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn compute_plan(dest: &Path) -> Result<Plan> {
    let tracked_count = git_files(dest, &[]).len();

    // Projection roots that actually exist in the clone.
    let projection_roots =
        crate::cmd::manifest::projection_roots(dest).into_iter().map(|r| r.rel).collect();

    // User-authored ways = untracked (not ignored) files under hooks/ways.
    let user_ways =
        git_files(dest, &["--others", "--exclude-standard", "--", "hooks/ways"]);

    let has_settings = dest.join("settings.json").is_file();

    let transcript_count = std::fs::read_dir(dest.join("projects"))
        .map(|rd| rd.filter_map(|e| e.ok()).count())
        .unwrap_or(0);

    let has_stats = dest.join("stats").is_dir();

    // The legacy cache dir, if present (sibling of the projection, under XDG cache).
    let cache_old = {
        let p = crate::util::xdg_cache_dir().join("claude-ways");
        if p.is_dir() {
            Some(p)
        } else {
            None
        }
    };

    Ok(Plan {
        dest: dest.to_path_buf(),
        tracked_count,
        projection_roots,
        user_ways,
        has_settings,
        transcript_count,
        has_stats,
        cache_old,
    })
}

fn print_plan(p: &Plan) {
    let data = paths::data_root();
    let config = paths::config_root();
    let state = paths::state_root();
    let cache_new = paths::cache_root();

    println!("Migration plan for {} (legacy in-place agent-ways clone)\n", p.dest.display());

    println!("  BACKUP    {0} → {0}.backup-<timestamp>", p.dest.display());
    println!("            (whole tree copied before any change — the escape hatch)\n");

    println!("  APP       {} git-tracked files → {}", p.tracked_count, data.display());
    println!(
        "  PROJECT   {} → symlinks into {}\n",
        p.projection_roots.join(", "),
        data.display()
    );

    println!("  PRESERVE  (Claude-Code-owned — stay in {}):", p.dest.display());
    println!(
        "            settings.json {}",
        if p.has_settings { "(three-way merged, not replaced)" } else { "(none yet)" }
    );
    println!("            projects/ ({} session transcripts) + auto-memory\n", p.transcript_count);

    println!("  LIFT      (agent-ways-owned, out of the projection):");
    if p.has_stats {
        println!("            stats/ → {} (telemetry)", state.display());
    } else {
        println!("            stats/ → {} (none yet)", state.display());
    }
    println!(
        "            {} user-authored way(s) → {} (untracked in hooks/ways)\n",
        p.user_ways.len(),
        config.join("ways").display()
    );

    match &p.cache_old {
        Some(old) => println!("  CACHE     {} → {} (rename)\n", old.display(), cache_new.display()),
        None => println!("  CACHE     (no legacy claude-ways cache found; nothing to rename)\n"),
    }

    println!("  THEN      reconcile (symlink projection + settings merge), install bootstrap shim.");
    println!("  RESTART   one-time: start a new Claude Code session to pick up the migrated install.\n");

    if !p.user_ways.is_empty() {
        println!("  Note: {} untracked way(s) under hooks/ways will be lifted to user scope", p.user_ways.len());
        println!("        (preserved, not discarded):");
        for w in p.user_ways.iter().take(10) {
            println!("          {w}");
        }
        if p.user_ways.len() > 10 {
            println!("          … and {} more", p.user_ways.len() - 10);
        }
    }
}

/// `ways migrate` — plan (read-only classification, default), what-if (executor
/// dry-run with contract checks), or execute (gated mutation).
pub fn run(execute: bool, what_if: bool, dest: Option<String>) -> Result<()> {
    let dest: PathBuf = dest.map(PathBuf::from).unwrap_or_else(|| home_dir().join(".claude"));

    if !dest.exists() {
        bail!("{} does not exist — nothing to migrate", dest.display());
    }
    if !is_agent_ways_clone(&dest) {
        bail!(
            "{} is not a legacy in-place agent-ways clone (no .git with an agent-ways \
             origin) — nothing to migrate. A fresh XDG install needs `ways reconcile`, \
             not migration.",
            dest.display()
        );
    }

    // --what-if and --execute share the executor's phases; --what-if just runs
    // them in dry-run, asserting each contract without mutating.
    if what_if || execute {
        return crate::cmd::migrate_exec::run(&dest, !execute);
    }

    // Default: the read-only high-level classification plan.
    let plan = compute_plan(&dest)?;
    print_plan(&plan);
    println!("Run `ways migrate --what-if` to dry-run the executor and verify every phase's contract.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn sandbox(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("ways-migrate-{}-{}-{}", std::process::id(), tag, n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Make a fake in-place clone: a git repo with an agent-ways origin and some
    /// tracked app files + an untracked user way.
    fn fake_clone(root: &Path) {
        let run = |args: &[&str]| {
            std::process::Command::new("git").arg("-C").arg(root).args(args).output().unwrap();
        };
        run(&["init", "-q"]);
        run(&["remote", "add", "origin", "https://github.com/aaronsb/agent-ways"]);
        std::fs::create_dir_all(root.join("hooks/ways/meta")).unwrap();
        std::fs::write(root.join("hooks/ways/meta/a.md"), "---\nx\n").unwrap();
        std::fs::create_dir_all(root.join("skills/wrap")).unwrap();
        std::fs::write(root.join("skills/wrap/SKILL.md"), "---\nx\n").unwrap();
        run(&["add", "-A"]);
        run(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
        // An untracked user way (not added/committed).
        std::fs::write(root.join("hooks/ways/meta/my-own.md"), "---\nuser\n").unwrap();
    }

    #[test]
    fn detects_agent_ways_clone() {
        let base = sandbox("detect");
        fake_clone(&base);
        assert!(is_agent_ways_clone(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn non_clone_is_not_migratable() {
        let base = sandbox("nonclone");
        std::fs::create_dir_all(base.join("skills")).unwrap();
        // No .git → not a clone.
        assert!(!is_agent_ways_clone(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn plan_classifies_tracked_vs_untracked() {
        let base = sandbox("plan");
        fake_clone(&base);
        let plan = compute_plan(&base).unwrap();
        assert!(plan.tracked_count >= 2, "should see tracked app files");
        // The untracked user way must be classified as user scope, not app.
        assert_eq!(plan.user_ways.len(), 1, "the one untracked way is user scope");
        assert!(plan.user_ways[0].ends_with("my-own.md"));
        assert!(plan.projection_roots.iter().any(|r| r == "hooks/ways"));
        let _ = std::fs::remove_dir_all(&base);
    }

    // Executor behavior (what-if + execute) is covered by the sandboxed phase
    // tests in migrate_exec, which inject XDG roots into a tmpdir. We do NOT
    // call migrate::run(execute=true) from a test: it resolves the *real*
    // $XDG_DATA/$XDG_CONFIG and would mutate the developer's environment.
}
