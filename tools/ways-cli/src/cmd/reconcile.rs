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
//! `ways-v1.8.3` and was removed in 1.9.0 (ADR-179). It also refuses to
//! replace a real directory or file it finds at a projection root: only a
//! symlink is ever removed, and `--force` moves a real path aside rather than
//! deleting it.

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
    /// A real (non-symlink) path sits at the root and `--force` was not
    /// given. Nothing was touched.
    Refused,
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
///
/// The real-path guard is the same posture one level down: a projection root
/// that is already a real directory or file (a user's own `~/.claude/skills`,
/// a pre-1.0 `make install` copy of `hooks/ways`) is never deleted. Without
/// `force` the run stops before touching any root and names the paths; with
/// `force` each such path is renamed to a timestamped sibling first.
pub fn run(
    source: Option<String>,
    dest: Option<String>,
    mode: Option<String>,
    dry_run: bool,
    quiet: bool,
    force: bool,
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
             migrator was removed in 1.9.0 (ADR-179); build it from the last tag \
             that ships it:\n\
             \x20 git clone --branch ways-v1.8.3 https://github.com/aaronsb/agent-ways /tmp/ways-migrator\n\
             \x20 cargo build --release --manifest-path /tmp/ways-migrator/tools/ways-cli/Cargo.toml\n\
             \x20 /tmp/ways-migrator/tools/target/release/ways migrate --what-if\n\
             Guide: docs/migration-1.0.md",
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

    // Pre-check, then act. Classify every root before any of them is touched,
    // so a refusal leaves the destination exactly as it was: no partial
    // projection, no settings merge.
    let foreign: Vec<PathBuf> =
        roots.iter().map(|r| dest_root.join(&r.rel)).filter(|dst| is_foreign(dst)).collect();
    if !foreign.is_empty() && !force && !dry_run {
        let list = foreign.iter().map(|p| format!("  {}", p.display())).collect::<Vec<_>>().join("\n");
        bail!(
            "{} projection root(s) under {} are real paths, not ways symlinks:\n{}\n\
             reconcile will not delete them. Move them aside yourself, or re-run with \
             --force to rename each to a timestamped sibling (<name>.ways-backup-<seconds>).",
            foreign.len(),
            dest_root.display(),
            list
        );
    }

    let mut outcomes = Vec::new();
    for root in &roots {
        outcomes.push(reconcile_symlink(&source_root, &dest_root, root, dry_run, force)?);
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
    force: bool,
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

    // symlink_metadata succeeds for a dangling link too, which `exists()` misses.
    let existed = std::fs::symlink_metadata(&dst).is_ok();
    let foreign = is_foreign(&dst);
    if dry_run {
        if foreign && !force {
            return Ok(Outcome {
                rel: root.rel.clone(),
                action: Action::Refused,
                detail: "real path, not a ways symlink; --force moves it aside".into(),
            });
        }
        return Ok(Outcome {
            rel: root.rel.clone(),
            action: Action::Would,
            detail: if existed { "replace".into() } else { "create".into() },
        });
    }

    // `run()` pre-checks every root, so this is the guard for direct callers.
    if foreign && !force {
        bail!(
            "{} is a real path, not a ways symlink; refusing to replace it (use --force to move it aside)",
            dst.display()
        );
    }

    // Only a symlink is removed here. A real path was either refused above or
    // is moved aside under --force; it is never deleted.
    let mut detail = String::from("linked");
    if existed {
        if foreign {
            let aside = move_aside(&dst)?;
            detail = format!("linked; moved aside to {}", aside.display());
        } else {
            remove_path(&dst)?;
        }
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_dir = root.kind == RootKind::Tree;
    make_symlink(&src, &dst, is_dir)?;

    Ok(Outcome {
        rel: root.rel.clone(),
        action: if existed { Action::Replaced } else { Action::Created },
        detail,
    })
}

/// True if something real (not a symlink) sits at `dst`: a directory or a
/// file the user or a prior copy-style install put there.
fn is_foreign(dst: &Path) -> bool {
    matches!(std::fs::symlink_metadata(dst), Ok(m) if !m.file_type().is_symlink())
}

/// Rename `p` to `<name>.ways-backup-<unix-seconds>` in the same parent
/// (`-1`, `-2`, ... on collision) and return the new path. A rename within one
/// directory is atomic and never crosses filesystems, so nothing is copied and
/// nothing is deleted.
fn move_aside(p: &Path) -> Result<PathBuf> {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".to_string());
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = format!("{name}.ways-backup-{secs}");
    let mut candidate = p.with_file_name(&stem);
    let mut n = 0u32;
    while std::fs::symlink_metadata(&candidate).is_ok() {
        n += 1;
        candidate = p.with_file_name(format!("{stem}-{n}"));
    }
    std::fs::rename(p, &candidate)?;
    Ok(candidate)
}

/// Best-effort path equality: canonicalize both, fall back to lexical compare
/// (the link may resolve to a path that doesn't exist during a dry run).
fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Only reached for symlinks (wrong target or dangling) after the foreign
/// check; a real directory never gets here.
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
            Action::Refused => "refused",
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

        run(Some(src.to_string_lossy().into()), Some(dst.to_string_lossy().into()), None, false, true, false).unwrap();

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
        run(s(&src), s(&dst), None, false, true, false).unwrap();

        // Re-run in dry-run: zero roots should want changing.
        let roots = projection_roots(&src);
        for r in &roots {
            let o = reconcile_symlink(&src, &dst, r, true, false).unwrap();
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
        let err = run(s(&src), s(&dst), None, false, true, false).unwrap_err();
        assert!(err.to_string().contains("in-place"), "should refuse: {err}");
        // The dest is left untouched: no projection root was materialized.
        assert!(!dst.join("skills").exists(), "guard must not project over the clone");
        let _ = std::fs::remove_dir_all(&base);
    }
    /// Names of `dst`'s siblings that look like a moved-aside root.
    fn backups_of(dst_parent: &Path, name: &str) -> Vec<PathBuf> {
        let prefix = format!("{name}.ways-backup-");
        let mut v: Vec<PathBuf> = std::fs::read_dir(dst_parent)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with(&prefix)).unwrap_or(false))
            .collect();
        v.sort();
        v
    }

    fn is_real_dir(p: &Path) -> bool {
        let m = std::fs::symlink_metadata(p).unwrap();
        m.file_type().is_dir() && !m.file_type().is_symlink()
    }

    #[test]
    fn refuses_real_dir_at_projection_root_without_force() {
        let base = sandbox("refuse-dir");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        fake_source(&src);
        // The user's own skills live at the projected root path.
        std::fs::create_dir_all(dst.join("skills/mine")).unwrap();
        std::fs::write(dst.join("skills/mine/SKILL.md"), "mine\n").unwrap();

        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        let err = run(s(&src), s(&dst), None, false, true, false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("real paths"), "should refuse: {msg}");
        assert!(msg.contains(&dst.join("skills").display().to_string()), "names the path: {msg}");

        // Untouched: still a real dir, the file is still there, byte for byte.
        assert!(is_real_dir(&dst.join("skills")));
        assert_eq!(std::fs::read_to_string(dst.join("skills/mine/SKILL.md")).unwrap(), "mine\n");
        // The pre-check stopped everything: no other root was linked, no sibling made.
        assert!(!dst.join("hooks/ways").exists(), "no root may be linked after a refusal");
        assert!(!dst.join("hooks/check-config-updates.sh").exists());
        assert!(backups_of(&dst, "skills").is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn refuses_real_file_and_binary_roots_without_force() {
        let base = sandbox("refuse-file");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        fake_source(&src);
        std::fs::create_dir_all(src.join("bin")).unwrap();
        std::fs::write(src.join("bin/ways"), "#!/bin/sh\n").unwrap();
        // Real files where the projected file roots go.
        std::fs::create_dir_all(dst.join("hooks")).unwrap();
        std::fs::create_dir_all(dst.join("bin")).unwrap();
        std::fs::write(dst.join("hooks/check-config-updates.sh"), "mine-hook\n").unwrap();
        std::fs::write(dst.join("bin/ways"), "mine-bin\n").unwrap();

        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        let err = run(s(&src), s(&dst), None, false, true, false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(&dst.join("hooks/check-config-updates.sh").display().to_string()), "{msg}");
        assert!(msg.contains(&dst.join("bin/ways").display().to_string()), "{msg}");

        for (rel, body) in [("hooks/check-config-updates.sh", "mine-hook\n"), ("bin/ways", "mine-bin\n")] {
            let p = dst.join(rel);
            assert!(!std::fs::symlink_metadata(&p).unwrap().file_type().is_symlink(), "{rel} replaced");
            assert_eq!(std::fs::read_to_string(&p).unwrap(), body, "{rel} content changed");
        }
        assert!(!dst.join("skills").exists());
        assert!(backups_of(&dst.join("hooks"), "check-config-updates.sh").is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn subset_of_source_is_still_refused() {
        // A real dir holding only a byte-identical copy of shipped files (a
        // pre-1.0 copy install) is refused too: symlink mode has no way to
        // tell app files from a user's edited copy, so it does not guess.
        let base = sandbox("refuse-subset");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        fake_source(&src);
        std::fs::create_dir_all(dst.join("skills/wrap")).unwrap();
        std::fs::write(dst.join("skills/wrap/SKILL.md"), "---\nx\n").unwrap();

        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        let err = run(s(&src), s(&dst), None, false, true, false).unwrap_err();
        assert!(err.to_string().contains("real paths"), "{err}");
        assert!(is_real_dir(&dst.join("skills")));
        assert_eq!(std::fs::read_to_string(dst.join("skills/wrap/SKILL.md")).unwrap(), "---\nx\n");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn force_moves_real_dir_aside_and_links() {
        let base = sandbox("force");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        fake_source(&src);
        std::fs::create_dir_all(dst.join("skills/mine")).unwrap();
        std::fs::write(dst.join("skills/mine/SKILL.md"), "mine\n").unwrap();

        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        run(s(&src), s(&dst), None, false, true, true).unwrap();

        // The root is now the projection.
        assert!(std::fs::symlink_metadata(dst.join("skills")).unwrap().file_type().is_symlink());
        assert!(same_path(&dst.join("skills"), &src.join("skills")));
        assert_eq!(std::fs::read_to_string(dst.join("skills/wrap/SKILL.md")).unwrap(), "---\nx\n");
        // The user's dir was renamed, not deleted, and not merged into the source.
        let backups = backups_of(&dst, "skills");
        assert_eq!(backups.len(), 1, "exactly one moved-aside sibling: {backups:?}");
        assert!(is_real_dir(&backups[0]));
        assert_eq!(std::fs::read_to_string(backups[0].join("mine/SKILL.md")).unwrap(), "mine\n");
        assert!(!src.join("skills/mine").exists(), "user content must not leak into the source");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn wrong_symlink_is_relinked_without_force() {
        let base = sandbox("wrong-link");
        let src = base.join("data");
        let dst = base.join("proj");
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("marker"), "keep\n").unwrap();
        fake_source(&src);
        make_symlink(&elsewhere, &dst.join("skills"), true).unwrap();

        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        run(s(&src), s(&dst), None, false, true, false).unwrap();

        assert!(same_path(&dst.join("skills"), &src.join("skills")), "stale link must be repointed");
        // Only the link was removed; what it pointed at is intact.
        assert_eq!(std::fs::read_to_string(elsewhere.join("marker")).unwrap(), "keep\n");
        assert!(backups_of(&dst, "skills").is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn dry_run_reports_refusal_and_touches_nothing() {
        let base = sandbox("dry-refuse");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        fake_source(&src);
        std::fs::create_dir_all(dst.join("skills/mine")).unwrap();
        std::fs::write(dst.join("skills/mine/SKILL.md"), "mine\n").unwrap();

        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        // Dry run is a preview, so it succeeds and reports rather than bails.
        run(s(&src), s(&dst), None, true, true, false).unwrap();

        let roots = projection_roots(&src);
        let skills = roots.iter().find(|r| r.rel == "skills").unwrap();
        let o = reconcile_symlink(&src, &dst, skills, true, false).unwrap();
        assert_eq!(o.action, Action::Refused);
        assert!(o.detail.contains("--force"), "detail should point at the way forward: {}", o.detail);
        // Under --force the preview says it would replace.
        let o = reconcile_symlink(&src, &dst, skills, true, true).unwrap();
        assert_eq!(o.action, Action::Would);
        assert_eq!(o.detail, "replace");

        assert!(is_real_dir(&dst.join("skills")));
        assert_eq!(std::fs::read_to_string(dst.join("skills/mine/SKILL.md")).unwrap(), "mine\n");
        assert!(!dst.join("hooks/ways").exists());
        assert!(backups_of(&dst, "skills").is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
