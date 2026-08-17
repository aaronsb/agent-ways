//! The reconciler (ADR-144 §2): converge `~/.claude` toward the manifest.
//!
//! One idempotent engine drives the live projection tree toward the desired
//! state. This module implements the **symlink materialization** (the ADR-142
//! default): each projection root (`skills`, `hooks/ways`, a binary, …) becomes
//! a symlink into the source checkout, so a `git pull` in `$XDG_DATA` is live
//! with no further step and no drift.
//!
//! Reporting follows the framework's escalation convention: **silent when
//! nothing changed**, a short list of what was (re)linked otherwise.
//!
//! Safety: this is the *update/repair* posture only (silent, autonomous, low
//! blast radius). It refuses to run against a live in-place clone, because
//! clobbering a clone's tree with symlinks would strand the user's checkout.
//! That case routes to the pre-1.0 migrator, which shipped through
//! `ways-v1.8.3` and was removed in 1.9.0 (ADR-179).

use crate::cmd::manifest::{projection_roots, ProjectionRoot, RootKind};
use crate::paths;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

/// Materialization strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Symlink,
    Copy,
}

/// What reconciliation did to one root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Already correct — no change (the idempotent common case).
    Ok,
    /// Created a link/copy that was absent.
    Created,
    /// Replaced a wrong/stale link or path.
    Replaced,
    /// Would change, but `--dry-run`.
    Would,
}

struct Outcome {
    rel: String,
    action: Action,
    detail: String,
}

/// `ways reconcile` entrypoint.
///
/// The legacy-in-place guard is unconditional: a manual `ways reconcile` must
/// never clobber an in-place clone. The migrator used to bypass it (it called
/// reconcile mid-migration, over a backed-up and relocated tree that still
/// *looked* in-place); that bypass left with the migrator in 1.9.0 (ADR-179).
pub fn run(
    source: Option<String>,
    dest: Option<String>,
    mode: Option<String>,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    let source_root: PathBuf = source.map(PathBuf::from).unwrap_or_else(paths::data_root);
    let dest_root: PathBuf = dest.map(PathBuf::from).unwrap_or_else(paths::projection_root);

    let mode = match mode.as_deref() {
        None | Some("symlink") => Mode::Symlink,
        Some("copy") => Mode::Copy,
        Some(other) => bail!("unknown mode {other:?} (expected 'symlink' or 'copy')"),
    };

    if !source_root.is_dir() {
        bail!("source checkout not found: {}", source_root.display());
    }

    // Refuse to reconcile a live in-place clone — that path needs migration,
    // not the repair posture. An in-place clone is a dest that is itself the
    // agent-ways git repo (has a .git AND ships the app source).
    if is_legacy_in_place(&dest_root) {
        bail!(
            "{} looks like a legacy in-place agent-ways clone — reconcile won't \
             clobber it. Migration (ADR-144 §5) is the path off in-place. The \
             migrator was removed in 1.9.0 (ADR-179); run it from the last tag \
             that ships it:\n\
             \x20 git clone --branch ways-v1.8.3 https://github.com/aaronsb/agent-ways\n\
             \x20 cd agent-ways && make build && ./tools/target/release/ways migrate --what-if",
            dest_root.display()
        );
    }

    if mode == Mode::Copy {
        // Copy materialization + per-file orphan prune is the fallback path
        // (ADR-142 §2); not yet ported from sync-to-home.sh.
        bail!("copy mode not yet implemented; symlink mode is the default");
    }

    let roots = projection_roots(&source_root);
    if roots.is_empty() {
        bail!("no projection roots found under {}", source_root.display());
    }

    let mut outcomes = Vec::new();
    for root in &roots {
        outcomes.push(reconcile_symlink(&source_root, &dest_root, root, dry_run)?);
    }

    report(&outcomes, &source_root, &dest_root, dry_run, quiet);

    // The settings.json three-way merge — the one shared-write seam (ADR-142).
    // Skipped in dry-run; backed up + self-audited inside apply_to_files.
    if !dry_run {
        let src_settings = source_root.join("settings.json");
        if src_settings.exists() {
            let dest_settings = dest_root.join("settings.json");
            let base_path = paths::state_root().join("settings-applied.json");
            let summary =
                crate::cmd::settings_merge::apply_to_files(&src_settings, &dest_settings, &base_path)?;
            if !quiet {
                eprintln!("{summary}");
            }
        }
    }

    Ok(())
}

/// Symlink one projection root: `dest/rel -> source/rel`, idempotently.
fn reconcile_symlink(
    source_root: &Path,
    dest_root: &Path,
    root: &ProjectionRoot,
    dry_run: bool,
) -> Result<Outcome> {
    let src = source_root.join(&root.rel);
    let dst = dest_root.join(&root.rel);

    // Already a symlink resolving to src → nothing to do.
    if let Ok(target) = std::fs::read_link(&dst) {
        let resolved = if target.is_absolute() {
            target
        } else {
            dst.parent().unwrap_or(dest_root).join(&target)
        };
        if same_path(&resolved, &src) {
            return Ok(Outcome { rel: root.rel.clone(), action: Action::Ok, detail: "linked".into() });
        }
    }

    let existed = dst.exists() || std::fs::symlink_metadata(&dst).is_ok();
    if dry_run {
        return Ok(Outcome {
            rel: root.rel.clone(),
            action: Action::Would,
            detail: if existed { "replace".into() } else { "create".into() },
        });
    }

    // Remove whatever is there (wrong link, or a real dir/file from a prior
    // copy-mode projection) and create the link fresh.
    if existed {
        remove_path(&dst)?;
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_dir = root.kind == RootKind::Tree;
    make_symlink(&src, &dst, is_dir)?;

    Ok(Outcome {
        rel: root.rel.clone(),
        action: if existed { Action::Replaced } else { Action::Created },
        detail: "linked".into(),
    })
}

/// Best-effort path equality: canonicalize both, fall back to lexical compare
/// (the link may resolve to a path that doesn't exist during a dry run).
fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

fn remove_path(p: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(p)?;
    if meta.file_type().is_dir() && !meta.file_type().is_symlink() {
        std::fs::remove_dir_all(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[cfg(unix)]
fn make_symlink(src: &Path, dst: &Path, _is_dir: bool) -> Result<()> {
    std::os::unix::fs::symlink(src, dst)?;
    Ok(())
}

#[cfg(windows)]
fn make_symlink(src: &Path, dst: &Path, is_dir: bool) -> Result<()> {
    if is_dir {
        std::os::windows::fs::symlink_dir(src, dst)?;
    } else {
        std::os::windows::fs::symlink_file(src, dst)?;
    }
    Ok(())
}

/// True if `dest` is a legacy in-place agent-ways clone (its own git repo that
/// also ships the app source) rather than a thin projection.
pub(crate) fn is_legacy_in_place(dest: &Path) -> bool {
    // A projection has symlinked/looked-up subtrees but no app source of its
    // own; a clone has .git AND the app's source dirs (tools/, docs/).
    dest.join(".git").exists() && dest.join("tools").is_dir() && dest.join("docs").is_dir()
}

fn report(outcomes: &[Outcome], source: &Path, dest: &Path, dry_run: bool, quiet: bool) {
    let changed: Vec<&Outcome> = outcomes.iter().filter(|o| o.action != Action::Ok).collect();

    if changed.is_empty() {
        // Silent-on-success: nothing to say unless explicitly asked.
        if !quiet {
            eprintln!("projection up to date ({} roots) — {}", outcomes.len(), dest.display());
        }
        return;
    }

    for o in &changed {
        let verb = match o.action {
            Action::Created => "linked",
            Action::Replaced => "relinked",
            Action::Would => "would link",
            Action::Ok => unreachable!(),
        };
        println!("{verb} {} ({})", o.rel, o.detail);
    }
    if !quiet {
        let what = if dry_run { "would reconcile" } else { "reconciled" };
        eprintln!(
            "{} {} of {} roots — {} → {}",
            what,
            changed.len(),
            outcomes.len(),
            source.display(),
            dest.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// A unique sandbox dir under the OS temp root (no Date/random available).
    fn sandbox(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("ways-reconcile-{}-{}-{}", std::process::id(), tag, n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Build a minimal fake source checkout with a couple projection roots.
    fn fake_source(root: &Path) {
        std::fs::create_dir_all(root.join("skills/wrap")).unwrap();
        std::fs::write(root.join("skills/wrap/SKILL.md"), "---\nx\n").unwrap();
        std::fs::create_dir_all(root.join("hooks/ways/meta")).unwrap();
        std::fs::write(root.join("hooks/ways/meta/a.md"), "---\nx\n").unwrap();
        std::fs::write(root.join("hooks/check-config-updates.sh"), "#!/bin/sh\n").unwrap();
    }

    #[test]
    fn symlink_projection_resolves_to_source() {
        let base = sandbox("resolve");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        fake_source(&src);

        run(Some(src.to_string_lossy().into()), Some(dst.to_string_lossy().into()), None, false, true).unwrap();

        // A file reached through the projection must be the source file.
        let via_projection = std::fs::read_to_string(dst.join("skills/wrap/SKILL.md")).unwrap();
        assert_eq!(via_projection, "---\nx\n");
        assert!(std::fs::symlink_metadata(dst.join("skills")).unwrap().file_type().is_symlink());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reconcile_is_idempotent() {
        let base = sandbox("idem");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        fake_source(&src);

        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        // First run creates; second run must find everything already correct.
        run(s(&src), s(&dst), None, false, true).unwrap();

        // Re-run in dry-run: zero roots should want changing.
        let roots = projection_roots(&src);
        for r in &roots {
            let o = reconcile_symlink(&src, &dst, r, true).unwrap();
            assert_eq!(o.action, Action::Ok, "root {} drifted on second run", r.rel);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn refuses_legacy_in_place_clone() {
        let base = sandbox("legacy");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        fake_source(&src);
        // Make dst look like an in-place clone.
        std::fs::create_dir_all(dst.join(".git")).unwrap();
        std::fs::create_dir_all(dst.join("tools")).unwrap();
        std::fs::create_dir_all(dst.join("docs")).unwrap();

        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        // The guard is unconditional since ADR-179 — the migrator's bypass was
        // its only exception and left with it.
        let err = run(s(&src), s(&dst), None, false, true).unwrap_err();
        assert!(err.to_string().contains("in-place"), "should refuse: {err}");
        // The dest is left untouched: no projection root was materialized.
        assert!(!dst.join("skills").exists(), "guard must not project over the clone");
        let _ = std::fs::remove_dir_all(&base);
    }
}
