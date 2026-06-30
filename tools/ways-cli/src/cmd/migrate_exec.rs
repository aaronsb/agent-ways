//! The migration executor (ADR-144 §5) — phased, contract-checked, resumable.
//!
//! One code path serves both **`--what-if`** (dry-run: assert each phase's
//! contract without mutating — the PowerShell `-WhatIf` / Terraform `plan`
//! model) and **`--execute`** (mutate, verify the postcondition, write a resume
//! marker). Because the same phases run either way, what-if genuinely exercises
//! the executor's logic, not a parallel description of it.
//!
//! Each phase is **design-by-contract**: a precondition gate, an action, and a
//! postcondition that is *predicted* in what-if and *verified* in execute.
//!
//! **Recovery model.** There is no automatic rollback; recovery is *resume* plus
//! a *manual* escape hatch. Phases write `migration/<phase>.done` markers under
//! `$XDG_STATE`; a failed run aborts, and re-running resumes from the first
//! incomplete phase (phases are idempotent). The whole-tree backup taken in
//! phase 1 is the manual escape hatch — on failure the executor prints the exact
//! restore command. The phase order is chosen so that **no failure leaves a
//! non-functional `~/.claude`**: in particular the projection rebuild reconciles
//! the symlinks *before* removing any app-internal directory, so a reconcile
//! error never guts the live tree.

use crate::paths;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Resolved locations + mode for one migration run.
pub struct Ctx {
    pub dest: PathBuf,   // ~/.claude (the live in-place clone → becomes the projection)
    pub data: PathBuf,   // $XDG_DATA/agent-ways (the relocated app)
    pub config: PathBuf, // $XDG_CONFIG/agent-ways
    pub state: PathBuf,  // $XDG_STATE/agent-ways
    pub cache_old: PathBuf,
    pub cache_new: PathBuf,
    pub backup: PathBuf,
    pub markers: PathBuf, // $XDG_STATE/agent-ways/migration
    pub dry_run: bool,
}

impl Ctx {
    fn new(dest: PathBuf, dry_run: bool, stamp: u64) -> Ctx {
        let state = paths::state_root();
        Ctx {
            backup: dest.with_file_name(format!(
                "{}.backup-{}",
                dest.file_name().and_then(|s| s.to_str()).unwrap_or(".claude"),
                stamp
            )),
            data: paths::data_root(),
            config: paths::config_root(),
            cache_old: crate::util::xdg_cache_dir().join("claude-ways"),
            // Canonical (NOT fallback-aware): the rename destination must be the
            // new name even though it doesn't exist yet, or it collapses onto
            // cache_old and the rename no-ops.
            cache_new: paths::cache_root_canonical(),
            markers: state.join("migration"),
            state,
            dest,
            dry_run,
        }
    }

    fn marker(&self, phase: &str) -> PathBuf {
        self.markers.join(format!("{phase}.done"))
    }

    fn done(&self, phase: &str) -> bool {
        self.marker(phase).exists()
    }

    fn mark(&self, phase: &str) -> Result<()> {
        std::fs::create_dir_all(&self.markers)?;
        std::fs::write(self.marker(phase), "done\n")?;
        Ok(())
    }
}

/// Run the migration. `dry_run = true` is `--what-if`.
pub fn run(dest: &Path, dry_run: bool) -> Result<()> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let ctx = Ctx::new(dest.to_path_buf(), dry_run, stamp);

    let header = if dry_run { "WHAT-IF — no changes will be made" } else { "EXECUTING migration" };
    println!("=== {header} ===\n  {} → {}\n", ctx.dest.display(), ctx.data.display());

    // (phase-id, label, fn). Order is load-bearing: backup first, projection
    // rebuild only after the app is safely relocated.
    type PhaseFn = fn(&Ctx) -> Result<Vec<String>>;
    let phases: &[(&str, &str, PhaseFn)] = &[
        ("01-backup", "Back up the whole tree", phase_backup),
        ("02-relocate", "Relocate the app to $XDG_DATA", phase_relocate),
        ("03-lift-state", "Lift telemetry to $XDG_STATE", phase_lift_state),
        ("04-lift-user", "Lift user ways to $XDG_CONFIG", phase_lift_user),
        ("05-projection", "Rebuild ~/.claude as the projection", phase_projection),
        ("06-cache", "Rename the engine cache", phase_cache),
        ("07-bootstrap", "Install the bootstrap shim", phase_bootstrap),
    ];

    for (id, label, f) in phases {
        if !dry_run && ctx.done(id) {
            println!("  [skip] {label} (already done)");
            continue;
        }
        let tag = if dry_run { "would" } else { "do" };
        println!("  [{tag}] {label}");
        let actions = match f(&ctx) {
            Ok(a) => a,
            Err(e) => {
                if !dry_run {
                    eprintln!("\nMIGRATION ABORTED at phase {id} ({label}):\n  {e:#}");
                    eprintln!("\nYour ~/.claude is still functional (no phase guts the live tree).");
                    eprintln!("Original backed up at: {}", ctx.backup.display());
                    eprintln!("Fix the cause and re-run `ways migrate --execute` to resume from this phase,");
                    eprintln!("or restore the original:");
                    eprintln!("  rm -rf {0} && cp -a {1} {0}", ctx.dest.display(), ctx.backup.display());
                }
                return Err(e.context(format!("phase {id} ({label})")));
            }
        };
        for a in &actions {
            println!("         - {a}");
        }
        if !dry_run {
            ctx.mark(id)?;
        }
    }

    if dry_run {
        println!("\nWhat-if complete — every phase's contract holds. Re-run with --execute to apply.");
    } else {
        println!(
            "\nMigration complete. Backup kept at {}.\nStart a NEW Claude Code session to pick up the migrated install.",
            ctx.backup.display()
        );
    }
    Ok(())
}

// --- phases ----------------------------------------------------------------

fn phase_backup(ctx: &Ctx) -> Result<Vec<String>> {
    let n = count_files(&ctx.dest);
    if ctx.dry_run {
        // Precondition: backup target must not already exist; parent writable.
        if ctx.backup.exists() {
            bail!("backup target already exists: {}", ctx.backup.display());
        }
        return Ok(vec![format!("copy {} files → {}", n, ctx.backup.display())]);
    }
    copy_tree(&ctx.dest, &ctx.backup, false)?;
    // Postcondition: backup exists and holds ~the same number of files.
    let got = count_files(&ctx.backup);
    if got < n {
        bail!("backup incomplete: {got}/{n} files copied to {}", ctx.backup.display());
    }
    Ok(vec![format!("backed up {got} files → {}", ctx.backup.display())])
}

fn phase_relocate(ctx: &Ctx) -> Result<Vec<String>> {
    let tracked = git_tracked_count(&ctx.dest);
    // Uncommitted edits to *tracked* app files would be replaced by their
    // committed versions in the clean checkout. They are preserved in the backup,
    // but the user must know — surface them rather than silently discarding.
    let dirty = git_files(&ctx.dest, &["--modified"]);
    let dirty_note = |actions: &mut Vec<String>| {
        if !dirty.is_empty() {
            actions.push(format!(
                "WARNING: {} tracked app file(s) have uncommitted edits — they will be replaced \
                 by their committed versions (your edits remain in the backup): {}",
                dirty.len(),
                dirty.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    };
    if ctx.dry_run {
        if !ctx.dest.join(".git").exists() {
            bail!("source is not a git checkout — cannot relocate cleanly");
        }
        let mut out = vec![format!(
            "materialize a clean checkout of {tracked} tracked files (+ .git, + bin/) → {}",
            ctx.data.display()
        )];
        dirty_note(&mut out);
        return Ok(out);
    }
    // A pristine app checkout: copy .git, then materialize tracked files from it
    // (so no user/CC files leak into $XDG_DATA), then copy the built bin/.
    if ctx.data.exists() {
        std::fs::remove_dir_all(&ctx.data).ok();
    }
    std::fs::create_dir_all(&ctx.data)?;
    copy_tree(&ctx.dest.join(".git"), &ctx.data.join(".git"), false)?;
    git_run(&ctx.data, &["reset", "--hard", "HEAD"])?;
    if ctx.dest.join("bin").is_dir() {
        copy_tree(&ctx.dest.join("bin"), &ctx.data.join("bin"), true)?;
    }
    // Postcondition: it's a checkout with the same tracked count.
    let got = git_tracked_count(&ctx.data);
    if got != tracked {
        bail!("relocated checkout has {got} tracked files, expected {tracked}");
    }
    let mut out = vec![format!("relocated {got} tracked files → {}", ctx.data.display())];
    dirty_note(&mut out);
    Ok(out)
}

fn phase_lift_state(ctx: &Ctx) -> Result<Vec<String>> {
    let stats = ctx.dest.join("stats");
    if !stats.is_dir() {
        return Ok(vec!["no stats/ to lift".into()]);
    }
    if ctx.dry_run {
        return Ok(vec![format!("move {} → {}", stats.display(), ctx.state.display())]);
    }
    std::fs::create_dir_all(&ctx.state)?;
    // Move each entry under stats/ into $XDG_STATE.
    for entry in std::fs::read_dir(&stats)?.flatten() {
        let to = ctx.state.join(entry.file_name());
        move_path(&entry.path(), &to)?;
    }
    Ok(vec![format!("lifted telemetry → {}", ctx.state.display())])
}

fn phase_lift_user(ctx: &Ctx) -> Result<Vec<String>> {
    let user_ways = git_files(&ctx.dest, &["--others", "--exclude-standard", "--", "hooks/ways"]);
    let target = ctx.config.join("ways");
    if user_ways.is_empty() {
        return Ok(vec!["no untracked user ways to lift".into()]);
    }
    if ctx.dry_run {
        return Ok(vec![format!("move {} user way(s) → {}", user_ways.len(), target.display())]);
    }
    for rel in &user_ways {
        // rel is like hooks/ways/<...>; re-root under $XDG_CONFIG/agent-ways/ways/<...>.
        let sub = rel.strip_prefix("hooks/ways/").unwrap_or(rel);
        let to = target.join(sub);
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        move_path(&ctx.dest.join(rel), &to)?;
    }
    Ok(vec![format!("lifted {} user way(s) → {}", user_ways.len(), target.display())])
}

fn phase_projection(ctx: &Ctx) -> Result<Vec<String>> {
    // What to remove from dest: the git-tracked top-level entries that are NOT
    // projection roots (docs, tools, governance, scripts, Makefile, top-level
    // *.md …), plus .git. We do NOT touch the projection-root tops (skills,
    // agents, commands, hooks, bin) — reconcile turns those into symlinks — nor
    // settings.json (the Claude-Code-owned floor, three-way merged), nor
    // projects/ and other untracked CC-owned entries.
    //
    // Enumerate from `dest` (the clone): it still has .git here in both modes, so
    // what-if is accurate even when $XDG_DATA doesn't exist yet.
    let proj_root_tops: Vec<String> = crate::cmd::manifest::projection_roots(&ctx.dest)
        .into_iter()
        .map(|r| r.rel.split('/').next().unwrap_or("").to_string())
        .collect();
    let mut to_remove: Vec<String> = git_top_level(&ctx.dest)
        .into_iter()
        .filter(|t| t != "settings.json" && !proj_root_tops.contains(t))
        .collect();
    to_remove.push(".git".into());

    if ctx.dry_run {
        return Ok(vec![
            format!("reconcile symlinks + settings merge from {}", ctx.data.display()),
            format!(
                "then remove {} app-internal entr{} from {} (kept: projection roots, settings.json, projects/, CC-owned)",
                to_remove.len(),
                if to_remove.len() == 1 { "y" } else { "ies" },
                ctx.dest.display()
            ),
        ]);
    }

    // 1. Reconcile FIRST — symlinks + settings merge. reconcile replaces each
    // projection root atomically and is idempotent, so even if this errors the
    // live tree is never gutted (nothing has been removed yet).
    crate::cmd::reconcile::run(
        Some(ctx.data.to_string_lossy().into()),
        Some(ctx.dest.to_string_lossy().into()),
        None,
        false,
        true,
        true, // allow_in_place: migrator is the legitimate mid-migration reconcile
    )?;
    // Postcondition gate before the destructive removal: a projection root must
    // now resolve as a symlink. If it doesn't, abort BEFORE deleting anything.
    let skills = ctx.dest.join("skills");
    if std::fs::symlink_metadata(&skills).map(|m| !m.file_type().is_symlink()).unwrap_or(true) {
        bail!("projection check failed: {} is not a symlink — aborting before any removal", skills.display());
    }
    // 2. Only now remove the app-internal leftovers.
    for t in &to_remove {
        let p = ctx.dest.join(t);
        if p.exists() || std::fs::symlink_metadata(&p).is_ok() {
            remove_any(&p)?;
        }
    }
    Ok(vec![format!("rebuilt {} as projection ({} app-internal entries removed)", ctx.dest.display(), to_remove.len())])
}

fn phase_cache(ctx: &Ctx) -> Result<Vec<String>> {
    if !ctx.cache_old.is_dir() {
        return Ok(vec!["no legacy claude-ways cache to rename".into()]);
    }
    if ctx.dry_run {
        if ctx.cache_new.exists() {
            return Ok(vec![format!("cache target {} exists — would merge/skip", ctx.cache_new.display())]);
        }
        return Ok(vec![format!("rename {} → {}", ctx.cache_old.display(), ctx.cache_new.display())]);
    }
    if !ctx.cache_new.exists() {
        if let Some(p) = ctx.cache_new.parent() {
            std::fs::create_dir_all(p)?;
        }
        move_path(&ctx.cache_old, &ctx.cache_new)?;
    }
    Ok(vec![format!("renamed cache → {}", ctx.cache_new.display())])
}

fn phase_bootstrap(ctx: &Ctx) -> Result<Vec<String>> {
    // The exemption (ADR-144 §3): a COPIED (non-symlink) entrypoint at a stable
    // path that re-runs the reconciler. It must not be a link into the tree it
    // heals. Thin by design — nothing to drift.
    let shim = ctx.dest.join("hooks").join("agent-ways-bootstrap.sh");
    let body = "#!/bin/sh\n\
# agent-ways bootstrap shim — the one deliberate copy (ADR-144 §3).\n\
# Re-materializes the projection from the XDG app on session start.\n\
DATA=\"${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways\"\n\
exec \"$DATA/bin/ways\" reconcile --quiet 2>/dev/null\n";
    if ctx.dry_run {
        return Ok(vec![format!("write copied bootstrap shim → {} (the exemption)", shim.display())]);
    }
    if let Some(p) = shim.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&shim, body)?;
    make_executable(&shim)?;
    Ok(vec![format!("wrote bootstrap shim → {}", shim.display())])
}

// --- helpers ---------------------------------------------------------------

fn count_files(root: &Path) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count()
}

fn git_run(dir: &Path, args: &[&str]) -> Result<()> {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !out.status.success() {
        bail!("git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn git_tracked_count(dir: &Path) -> usize {
    git_files(dir, &[]).len()
}

fn git_files(dir: &Path, args: &[&str]) -> Vec<String> {
    let mut full = vec!["ls-files", "-z"];
    full.extend_from_slice(args);
    let out = match std::process::Command::new("git").arg("-C").arg(dir).args(&full).output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out).split('\0').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Unique top-level path components of the tracked tree.
fn git_top_level(dir: &Path) -> Vec<String> {
    let mut seen = Vec::new();
    for f in git_files(dir, &[]) {
        let top = f.split('/').next().unwrap_or(&f).to_string();
        if !seen.contains(&top) {
            seen.push(top);
        }
    }
    seen
}

/// Recursively copy `src` to `dst`. With `deref`, symlinks are followed and their
/// targets copied as standalone files (used for `bin/`, whose entries are
/// symlinks into `tools/target/` that the migration later removes — the migrated
/// app must hold real binaries, not links into a deleted build dir). Without
/// `deref`, symlinks are recreated verbatim (faithful backup / .git copy).
fn copy_tree(src: &Path, dst: &Path, deref: bool) -> Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let to = dst.join(entry.file_name());
            let ft = entry.file_type()?;
            if ft.is_symlink() {
                if deref {
                    // std::fs::copy follows symlinks and preserves the file mode
                    // (so the executable bit survives). Skip a dangling link
                    // rather than abort the whole relocate.
                    if entry.path().exists() {
                        std::fs::copy(entry.path(), &to)?;
                    }
                } else {
                    let target = std::fs::read_link(entry.path())?;
                    symlink_raw(&target, &to)?;
                }
            } else if ft.is_dir() {
                copy_tree(&entry.path(), &to, deref)?;
            } else {
                std::fs::copy(entry.path(), &to)?;
            }
        }
    } else {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

/// Move via rename, falling back to copy+remove across filesystems.
fn move_path(src: &Path, dst: &Path) -> Result<()> {
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_tree(src, dst, false)?;
    remove_any(src)
}

fn remove_any(p: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(p)?;
    if meta.file_type().is_dir() && !meta.file_type().is_symlink() {
        std::fs::remove_dir_all(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_raw(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}
#[cfg(windows)]
fn symlink_raw(target: &Path, link: &Path) -> Result<()> {
    // Best effort on Windows; dir vs file is ambiguous from a raw target.
    std::os::windows::fs::symlink_file(target, link).or_else(|_| std::os::windows::fs::symlink_dir(target, link))?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(p)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(p, perms)?;
    Ok(())
}
#[cfg(windows)]
fn make_executable(_p: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn sandbox(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("ways-mexec-{}-{}-{}", std::process::id(), tag, n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A fake in-place clone with tracked app files, a built bin, an untracked
    /// user way, a stats dir, and a user settings.json — exercising every phase.
    fn fake_clone(root: &Path) {
        std::fs::create_dir_all(root).unwrap(); // git -C needs the dir to exist first
        let g = |args: &[&str]| {
            std::process::Command::new("git").arg("-C").arg(root).args(args).output().unwrap();
        };
        g(&["init", "-q"]);
        g(&["remote", "add", "origin", "https://github.com/aaronsb/agent-ways"]);
        // Include tools/ AND docs/ so the clone fully matches is_legacy_in_place
        // (.git + tools + docs) — the real ~/.claude shape. Without tools/, the
        // reconcile guard never fired in the sandbox and masked the mid-migration
        // abort bug.
        for f in ["hooks/ways/meta/a.md", "skills/wrap/SKILL.md", "docs/x.md", "tools/x.rs", "settings.json"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, if f == "settings.json" { "{\"hooks\":{}}" } else { "---\nx\n" }).unwrap();
        }
        g(&["add", "-A"]);
        g(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
        // Built binary: in a real in-place install bin/ways is a SYMLINK into
        // tools/target/ (the build output, gitignored). Reproduce that exactly —
        // the migration removes tools/, so a preserved symlink would dangle. This
        // is the shape that broke on cube and that the deref copy must handle.
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("tools/target/release")).unwrap();
        std::fs::write(root.join("tools/target/release/ways"), "#!/bin/sh\necho ways\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("tools/target/release/ways"), root.join("bin/ways")).unwrap();
        #[cfg(not(unix))]
        std::fs::write(root.join("bin/ways"), "#!/bin/sh\n").unwrap();
        // Untracked user way + a user settings.json overlay + CC transcripts + stats.
        std::fs::write(root.join("hooks/ways/meta/mine.md"), "---\nuser\n").unwrap();
        std::fs::write(root.join("settings.json"), "{\"model\":\"opus\",\"hooks\":{}}").unwrap();
        std::fs::create_dir_all(root.join("projects/p1")).unwrap();
        std::fs::write(root.join("projects/p1/sess.jsonl"), "{}\n").unwrap();
        std::fs::create_dir_all(root.join("stats")).unwrap();
        std::fs::write(root.join("stats/events.jsonl"), "{}\n").unwrap();
    }

    fn ctx_for(dest: &Path, dry: bool, base: &Path) -> Ctx {
        // Point all XDG roots inside the sandbox so nothing escapes.
        let mut c = Ctx::new(dest.to_path_buf(), dry, 12345);
        c.data = base.join("data/agent-ways");
        c.config = base.join("config/agent-ways");
        c.state = base.join("state/agent-ways");
        c.cache_old = base.join("cache/claude-ways");
        c.cache_new = base.join("cache/agent-ways");
        c.markers = c.state.join("migration");
        c.backup = dest.with_file_name(".claude.backup-test");
        c
    }

    #[test]
    fn what_if_mutates_nothing() {
        let base = sandbox("whatif");
        let dest = base.join(".claude");
        fake_clone(&dest);
        let before = count_files(&dest);
        let ctx = ctx_for(&dest, true, &base);
        for f in [phase_backup, phase_relocate, phase_lift_state, phase_lift_user, phase_projection, phase_cache, phase_bootstrap] {
            f(&ctx).unwrap();
        }
        assert_eq!(count_files(&dest), before, "what-if must not change the tree");
        assert!(!ctx.data.exists(), "what-if must not create $XDG_DATA");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn execute_produces_expected_xdg_layout() {
        let base = sandbox("exec");
        let dest = base.join(".claude");
        fake_clone(&dest);
        // Seed the legacy cache so the rename phase has something to do.
        std::fs::create_dir_all(base.join("cache/claude-ways/user")).unwrap();
        std::fs::write(base.join("cache/claude-ways/user/corpus.jsonl"), "{}\n").unwrap();

        let ctx = ctx_for(&dest, false, &base);
        for (id, f) in [
            ("01", phase_backup as fn(&Ctx) -> Result<Vec<String>>),
            ("02", phase_relocate),
            ("03", phase_lift_state),
            ("04", phase_lift_user),
            ("05", phase_projection),
            ("06", phase_cache),
            ("07", phase_bootstrap),
        ] {
            f(&ctx).unwrap_or_else(|e| panic!("phase {id} failed: {e}"));
        }

        // App relocated, clean checkout.
        assert!(ctx.data.join(".git").exists(), "app .git in $XDG_DATA");
        assert!(ctx.data.join("hooks/ways/meta/a.md").exists(), "tracked way in $XDG_DATA");
        assert!(!ctx.data.join("projects").exists(), "CC transcripts must NOT be in $XDG_DATA");
        assert!(!ctx.data.join("stats").exists(), "stats must NOT be in $XDG_DATA");

        // Telemetry lifted to state.
        assert!(ctx.state.join("events.jsonl").exists(), "events lifted to $XDG_STATE");
        // User way lifted to config.
        assert!(ctx.config.join("ways/meta/mine.md").exists(), "user way lifted to $XDG_CONFIG");

        // dest is now a projection: skills is a symlink, docs (app-internal) is gone,
        // settings.json + projects (CC-owned) are preserved.
        assert!(std::fs::symlink_metadata(dest.join("skills")).unwrap().file_type().is_symlink());
        assert!(!dest.join("docs").exists(), "app-internal docs removed from projection");
        assert!(dest.join("projects/p1/sess.jsonl").exists(), "CC transcripts preserved");
        let settings = std::fs::read_to_string(dest.join("settings.json")).unwrap();
        assert!(settings.contains("opus"), "user settings.json preserved (model:opus)");

        // The relocated binary must be a REAL file, not a dangling symlink into
        // the removed tools/target — and the projection must reach it (the cube bug).
        #[cfg(unix)]
        {
            let app_bin = ctx.data.join("bin/ways");
            assert!(app_bin.is_file(), "migrated bin/ways must exist");
            assert!(
                !std::fs::symlink_metadata(&app_bin).unwrap().file_type().is_symlink(),
                "migrated bin/ways must be a real file (deref-copied), not a symlink into the removed tools/"
            );
            // ~/.claude/bin/ways → $XDG_DATA/agent-ways/bin/ways → real content.
            assert!(
                std::fs::read_to_string(dest.join("bin/ways")).unwrap().contains("echo ways"),
                "projected bin/ways must resolve to the real binary content"
            );
        }

        // Cache renamed; bootstrap shim is a real file (not a symlink).
        assert!(ctx.cache_new.join("user/corpus.jsonl").exists(), "cache renamed");
        let shim = dest.join("hooks/agent-ways-bootstrap.sh");
        assert!(shim.is_file() && !std::fs::symlink_metadata(&shim).unwrap().file_type().is_symlink());

        // Backup is the escape hatch.
        assert!(ctx.backup.join("docs/x.md").exists(), "full backup taken");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn relocate_is_resumable_via_markers() {
        let base = sandbox("resume");
        let dest = base.join(".claude");
        fake_clone(&dest);
        let ctx = ctx_for(&dest, false, &base);
        ctx.mark("02-relocate").unwrap();
        assert!(ctx.done("02-relocate"), "marker should register as done for resume");
        let _ = std::fs::remove_dir_all(&base);
    }
}
