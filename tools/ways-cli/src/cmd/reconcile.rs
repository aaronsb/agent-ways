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
    /// Withdrawal removed our symlink (ADR-184).
    Unlinked,
    /// Withdrawal left a path alone: a real path, or a symlink that is not ours.
    Kept,
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

    let mode = match mode.as_deref() {
        None | Some("symlink") => Mode::Symlink,
        Some("copy") => Mode::Copy,
        Some(other) => bail!("unknown mode {other:?} (expected 'symlink' or 'copy')"),
    };

    if !source_root.is_dir() {
        bail!("source checkout not found: {}", source_root.display());
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

    // An explicit --dest is a single-target run against that directory, with
    // the base that directory has always used. The targets list is not
    // consulted and not changed.
    if let Some(d) = dest {
        let dest_root = PathBuf::from(d);
        let base = base_path_for(&paths::state_root(), &dest_root);
        return converge_one(&source_root, &dest_root, &roots, &base, dry_run, quiet, force);
    }

    let cfg = crate::config::global();
    let targets = cfg.targets();
    run_targets_in(&paths::state_root(), &source_root, &roots, &targets, dry_run, quiet, force)?;

    // Migration (ADR-184 item 2): an install from before the targets key has
    // just converged its implicit default. Record it, so the install is
    // explicit from here and `ways config targets` reads the truth.
    if !cfg.targets_explicit() && !dry_run {
        match crate::config::Config::write_user_targets(&targets) {
            Ok(path) if !quiet => eprintln!(
                "recorded {} as the active target in {}",
                targets.iter().map(|t| t.path.as_str()).collect::<Vec<_>>().join(", "),
                path.display()
            ),
            Ok(_) => {}
            Err(e) => eprintln!("could not record the target in the user config: {e}"),
        }
    }
    Ok(())
}

/// Converge every enabled target and withdraw from every disabled one
/// (ADR-184). Each target is attempted; the first error is returned after the
/// rest have run, so one refused directory never blocks the others.
pub(crate) fn run_targets(
    source_root: &Path,
    roots: &[ProjectionRoot],
    targets: &[crate::config::Target],
    dry_run: bool,
    quiet: bool,
    force: bool,
) -> Result<()> {
    run_targets_in(&paths::state_root(), source_root, roots, targets, dry_run, quiet, force)
}

/// `run_targets` with the state root explicit, so tests keep their merge
/// bases in a sandbox without touching the process environment.
pub(crate) fn run_targets_in(
    state_root: &Path,
    source_root: &Path,
    roots: &[ProjectionRoot],
    targets: &[crate::config::Target],
    dry_run: bool,
    quiet: bool,
    force: bool,
) -> Result<()> {
    if targets.is_empty() {
        if !quiet {
            eprintln!(
                "no targets: agent-ways is installed and inactive. \
                 `ways config target add <dir>` activates a Claude Code config directory; \
                 `ways config targets` lists them."
            );
        }
        return Ok(());
    }
    let mut first_err: Option<anyhow::Error> = None;
    for t in targets {
        let dest_root = t.dir();
        let base = base_path_for(state_root, &dest_root);
        let result = if t.enabled {
            converge_one(source_root, &dest_root, roots, &base, dry_run, quiet, force)
        } else {
            withdraw_one(source_root, &dest_root, roots, &base, dry_run, quiet)
        };
        if let Err(e) = result {
            eprintln!("target {}: {e:#}", dest_root.display());
            if first_err.is_none() {
                first_err = Some(e);
            }
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Resolve the source and roots once and run the given targets. The config
/// verbs (`ways config target …`) reach reconcile through here.
pub fn run_for_targets(targets: &[crate::config::Target], dry_run: bool, quiet: bool, force: bool) -> Result<()> {
    let source_root = paths::data_root();
    if !source_root.is_dir() {
        bail!("source checkout not found: {}", source_root.display());
    }
    let roots = projection_roots(&source_root);
    if roots.is_empty() {
        bail!("no projection roots found under {}", source_root.display());
    }
    run_targets(&source_root, &roots, targets, dry_run, quiet, force)
}

/// The plan for one target against the installed source.
pub fn plan_target(dest_root: &Path) -> Result<Plan> {
    let source_root = paths::data_root();
    let roots = projection_roots(&source_root);
    plan_for(&paths::state_root(), &source_root, dest_root, &roots)
}

/// The settings merge base for one target. The default projection root keeps
/// the path every install before ADR-184 wrote, so an existing base is honored;
/// every other target gets its own under `state/targets/<key>/`.
pub(crate) fn base_path_for(state_root: &Path, dest_root: &Path) -> PathBuf {
    if same_path(dest_root, &paths::projection_root()) {
        state_root.join("settings-applied.json")
    } else {
        let key = ways_core::util::encode_project_key(dest_root);
        state_root.join("targets").join(key).join("settings-applied.json")
    }
}

// ── Preflight (ADR-184 item 3) ─────────────────────────────────

/// What activating a target would do, computed without touching it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Plan {
    pub dest: String,
    pub roots: Vec<RootPlan>,
    /// `None` when the source ships no settings.json.
    pub settings: Option<SettingsPlan>,
    /// True when something the operator owns would be refused or removed.
    pub blocked: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RootPlan {
    pub rel: String,
    /// `linked`, `link`, `relink`, or `refused`.
    pub state: String,
    pub detail: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SettingsPlan {
    pub hooks_added: Vec<HookRef>,
    /// Entries the ownership predicate claims that the base never recorded:
    /// a hook the user wrote under `.claude/hooks/`. These block.
    pub hooks_replaced: Vec<HookRef>,
    /// Entries the base recorded as ours that the merge refreshes, after a
    /// source upgrade changed a command. These do not block.
    pub hooks_refreshed: Vec<HookRef>,
    /// Entries that are not structurally ours and would still be dropped.
    pub hooks_removed: Vec<HookRef>,
    pub user_hooks_kept: usize,
    pub perms_added: Vec<String>,
    pub deny_added: Vec<String>,
    pub unchanged: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HookRef {
    pub event: String,
    pub command: String,
}

/// Classify every root and dry-run the settings merge in memory.
pub fn plan_for(state_root: &Path, source_root: &Path, dest_root: &Path, roots: &[ProjectionRoot]) -> Result<Plan> {
    let mut root_plans = Vec::new();
    for root in roots {
        let src = source_root.join(&root.rel);
        let dst = dest_root.join(&root.rel);
        let (state, detail) = match std::fs::read_link(&dst) {
            Ok(target) => {
                let resolved =
                    if target.is_absolute() { target } else { dst.parent().unwrap_or(dest_root).join(&target) };
                if same_path(&resolved, &src) {
                    ("linked", "already ours".to_string())
                } else {
                    ("relink", format!("symlink to {}", resolved.display()))
                }
            }
            Err(_) if is_foreign(&dst) => ("refused", describe_real_path(&dst)),
            Err(_) => ("link", "absent".to_string()),
        };
        root_plans.push(RootPlan { rel: root.rel.clone(), state: state.to_string(), detail });
    }

    let src_settings = source_root.join("settings.json");
    let settings = if src_settings.exists() {
        use crate::cmd::settings_merge as sm;
        let desired: serde_json::Value = sm::read_json_or_empty(&src_settings)?;
        let desired_hooks = desired.get("hooks").cloned().unwrap_or(serde_json::Value::Object(Default::default()));
        let dest_settings = dest_root.join("settings.json");
        let live: serde_json::Value = sm::read_json_or_empty(&dest_settings)?;
        let base_path = base_path_for(state_root, dest_root);
        let base = sm::base_for(&live, &base_path)?;
        let merged = sm::merge(&live, &desired_hooks, &base, crate::config::global().secret_path_deny)?;
        // Classify against the base as it exists on disk. A first apply's
        // seeded base claims entries by shape, and the plan must show those
        // as claimed rather than as ours from a prior version.
        let recorded = if base_path.exists() { base } else { sm::Owned::default() };
        Some(diff_settings(&live, &merged.settings, &recorded))
    } else {
        None
    };

    // A replaced entry blocks too: the merge would claim a hook the user wrote
    // under .claude/hooks/, and withdrawal would later leave it alone only
    // because it is not shipped, so the operator should see it now.
    let blocked = root_plans.iter().any(|r| r.state == "refused")
        || settings.as_ref().map(|s| !s.hooks_removed.is_empty() || !s.hooks_replaced.is_empty()).unwrap_or(false);
    Ok(Plan { dest: dest_root.to_string_lossy().to_string(), roots: root_plans, settings, blocked })
}

fn describe_real_path(p: &Path) -> String {
    match std::fs::read_dir(p) {
        Ok(rd) => format!("real directory, {} entries", rd.count()),
        Err(_) => "real file".to_string(),
    }
}

fn hook_refs(event: &str, entry: &serde_json::Value) -> HookRef {
    let command = entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
                .collect::<Vec<_>>()
                .join(" && ")
        })
        .unwrap_or_default();
    HookRef { event: event.to_string(), command }
}

fn diff_settings(
    live: &serde_json::Value,
    merged: &serde_json::Value,
    base: &crate::cmd::settings_merge::Owned,
) -> SettingsPlan {
    use crate::cmd::settings_merge::entry_is_ours;
    let empty = serde_json::Map::new();
    let live_hooks = live.get("hooks").and_then(|h| h.as_object()).unwrap_or(&empty);
    let merged_hooks = merged.get("hooks").and_then(|h| h.as_object()).unwrap_or(&empty);
    let mut events: Vec<&String> = live_hooks.keys().collect();
    for k in merged_hooks.keys() {
        if !events.contains(&k) {
            events.push(k);
        }
    }
    let (mut added, mut replaced, mut refreshed, mut removed, mut kept) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0usize);
    for event in events {
        let l = live_hooks.get(event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let m = merged_hooks.get(event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let recorded = base.hooks.get(event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        for e in &m {
            if !l.contains(e) {
                added.push(hook_refs(event, e));
            }
        }
        for e in &l {
            if m.contains(e) {
                if !entry_is_ours(e) {
                    kept += 1;
                }
            } else if recorded.contains(e) {
                refreshed.push(hook_refs(event, e));
            } else if entry_is_ours(e) {
                replaced.push(hook_refs(event, e));
            } else {
                removed.push(hook_refs(event, e));
            }
        }
    }
    let strings = |v: &serde_json::Value, key: &str| -> Vec<String> {
        v.get("permissions")
            .and_then(|p| p.get(key))
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let live_allow = strings(live, "allow");
    let live_deny = strings(live, "deny");
    let perms_added = strings(merged, "allow").into_iter().filter(|p| !live_allow.contains(p)).collect();
    let deny_added = strings(merged, "deny").into_iter().filter(|p| !live_deny.contains(p)).collect();
    SettingsPlan {
        hooks_added: added,
        hooks_replaced: replaced,
        hooks_refreshed: refreshed,
        hooks_removed: removed,
        user_hooks_kept: kept,
        perms_added,
        deny_added,
        unchanged: live == merged,
    }
}

/// Project every root into `dest_root` and merge the hooks block into its
/// `settings.json`. The pre-ADR-184 body of `run`, on one target.
fn converge_one(
    source_root: &Path,
    dest_root: &Path,
    roots: &[ProjectionRoot],
    base_path: &Path,
    dry_run: bool,
    quiet: bool,
    force: bool,
) -> Result<()> {
    // Refuse to reconcile a live in-place clone — that path needs migration,
    // not the repair posture. An in-place clone is a dest that is itself the
    // agent-ways git repo (has a .git AND ships the app source).
    if is_legacy_in_place(dest_root) {
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
    for root in roots {
        outcomes.push(reconcile_symlink(source_root, dest_root, root, dry_run, force)?);
    }

    report(&outcomes, source_root, dest_root, dry_run, quiet, false);

    // The settings.json three-way merge — the one shared-write seam (ADR-142).
    // Skipped in dry-run; backed up + self-audited inside apply_to_files.
    if !dry_run {
        let src_settings = source_root.join("settings.json");
        if src_settings.exists() {
            let dest_settings = dest_root.join("settings.json");
            let summary =
                crate::cmd::settings_merge::apply_to_files(&src_settings, &dest_settings, base_path)?;
            if !quiet {
                eprintln!("{summary}");
            }
        }
    }

    Ok(())
}

/// Withdraw from a disabled target (ADR-184 item 4): remove every symlink of
/// ours, remove our hooks block through the merge base, touch nothing else.
/// A real path at a root is left where it is, and so is a symlink that points
/// anywhere but our source.
fn withdraw_one(
    source_root: &Path,
    dest_root: &Path,
    roots: &[ProjectionRoot],
    base_path: &Path,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    if is_legacy_in_place(dest_root) {
        bail!("{} looks like a legacy in-place agent-ways clone; nothing to withdraw", dest_root.display());
    }
    // Settings first: if the withdrawal's self-audit reverts, the links are
    // still in place and no hook entry points at a removed path.
    let mut settings_summary = None;
    if !dry_run {
        let dest_settings = dest_root.join("settings.json");
        let src_settings = source_root.join("settings.json");
        if dest_settings.exists() {
            settings_summary = Some(crate::cmd::settings_merge::withdraw_from_files(&src_settings, &dest_settings, base_path)?);
        }
    }
    let mut outcomes = Vec::new();
    for root in roots {
        let src = source_root.join(&root.rel);
        let dst = dest_root.join(&root.rel);
        let outcome = match std::fs::read_link(&dst) {
            Ok(target) => {
                let resolved =
                    if target.is_absolute() { target } else { dst.parent().unwrap_or(dest_root).join(&target) };
                if same_path(&resolved, &src) {
                    if dry_run {
                        Outcome { rel: root.rel.clone(), action: Action::Would, detail: "unlink".into() }
                    } else {
                        remove_symlink(&dst)?;
                        Outcome { rel: root.rel.clone(), action: Action::Unlinked, detail: "unlinked".into() }
                    }
                } else {
                    Outcome { rel: root.rel.clone(), action: Action::Kept, detail: "symlink elsewhere".into() }
                }
            }
            Err(_) if std::fs::symlink_metadata(&dst).is_ok() => {
                Outcome { rel: root.rel.clone(), action: Action::Kept, detail: "real path".into() }
            }
            Err(_) => Outcome { rel: root.rel.clone(), action: Action::Ok, detail: "absent".into() },
        };
        outcomes.push(outcome);
    }

    report(&outcomes, source_root, dest_root, dry_run, quiet, true);
    if let (Some(summary), false) = (settings_summary, quiet) {
        eprintln!("{summary}");
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
    if meta.file_type().is_symlink() {
        return remove_symlink(p);
    }
    if meta.file_type().is_dir() {
        std::fs::remove_dir_all(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

/// Remove a symlink and only the symlink. On Windows a directory symlink is a
/// directory entry and `remove_file` is refused with "access is denied"; a
/// dangling one has no target to inspect, so try the directory call first.
fn remove_symlink(p: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        if std::fs::remove_dir(p).is_ok() {
            return Ok(());
        }
    }
    std::fs::remove_file(p)?;
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

fn report(outcomes: &[Outcome], source: &Path, dest: &Path, dry_run: bool, quiet: bool, withdrawing: bool) {
    let changed: Vec<&Outcome> =
        outcomes.iter().filter(|o| o.action != Action::Ok && o.action != Action::Kept).collect();

    if changed.is_empty() {
        // Silent-on-success: nothing to say unless explicitly asked.
        if !quiet {
            if withdrawing {
                eprintln!("withdrawn ({} roots) — {}", outcomes.len(), dest.display());
            } else {
                eprintln!("projection up to date ({} roots) — {}", outcomes.len(), dest.display());
            }
        }
        return;
    }

    for o in &changed {
        let verb = match o.action {
            Action::Created => "linked",
            Action::Replaced => "relinked",
            Action::Would if withdrawing => "would unlink",
            Action::Would => "would link",
            Action::Refused => "refused",
            Action::Unlinked => "unlinked",
            Action::Ok | Action::Kept => unreachable!(),
        };
        if quiet {
            eprintln!("{verb} {} ({})", o.rel, o.detail);
        } else {
            println!("{verb} {} ({})", o.rel, o.detail);
        }
    }
    if !quiet {
        let what = match (dry_run, withdrawing) {
            (true, true) => "would withdraw",
            (false, true) => "withdrew",
            (true, false) => "would reconcile",
            (false, false) => "reconciled",
        };
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

    /// A source settings.json with one hook of ours, in the shape the app ships.
    fn fake_settings(root: &Path) {
        std::fs::write(
            root.join("settings.json"),
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"${HOME}/.claude/hooks/ways/check-setup.sh"}]}]}}"#,
        )
        .unwrap();
    }

    fn user_settings() -> &'static str {
        r#"{"model":"opus","hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"echo user-start"}]}],"Stop":[{"hooks":[{"type":"command","command":"echo user-stop"}]}]},"permissions":{"allow":["Bash(make:*)"]}}"#
    }

    fn target(dst: &Path, enabled: bool) -> crate::config::Target {
        crate::config::Target { path: dst.to_string_lossy().to_string(), enabled, observe: None, config: None }
    }

    fn hook_commands(settings: &serde_json::Value, event: &str) -> Vec<String> {
        settings["hooks"][event]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| e["hooks"][0]["command"].as_str().map(|c| c.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn enabled_target_converges_and_disabled_target_withdraws() {
        let base = sandbox("targets");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        fake_source(&src);
        fake_settings(&src);
        std::fs::write(dst.join("settings.json"), user_settings()).unwrap();
        let roots = projection_roots(&src);
        // The base for a non-default dest lives under the state root; point it
        // into the sandbox so the test never touches real state.
        let state = base.join("state");

        run_targets_in(&state, &src, &roots, &[target(&dst, true)], false, true, false).unwrap();
        assert!(std::fs::symlink_metadata(dst.join("skills")).unwrap().file_type().is_symlink());
        let live: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dst.join("settings.json")).unwrap()).unwrap();
        let start = hook_commands(&live, "SessionStart");
        assert!(start.iter().any(|c| c.contains("check-setup.sh")), "ours added: {start:?}");
        assert!(start.iter().any(|c| c == "echo user-start"), "user kept: {start:?}");
        assert_eq!(live["model"], "opus");

        run_targets_in(&state, &src, &roots, &[target(&dst, false)], false, true, false).unwrap();
        assert!(std::fs::symlink_metadata(dst.join("skills")).is_err(), "our link removed");
        assert!(std::fs::symlink_metadata(dst.join("hooks/check-config-updates.sh")).is_err());
        let live: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dst.join("settings.json")).unwrap()).unwrap();
        assert_eq!(hook_commands(&live, "SessionStart"), vec!["echo user-start".to_string()]);
        assert_eq!(hook_commands(&live, "Stop"), vec!["echo user-stop".to_string()]);
        assert_eq!(live["model"], "opus");
        let allow: Vec<&str> = live["permissions"]["allow"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(allow, vec!["Bash(make:*)"], "user permission kept, ours gone");
        assert!(live["permissions"].get("deny").is_none(), "our deny baseline gone");

        // Withdrawing again changes nothing.
        run_targets_in(&state, &src, &roots, &[target(&dst, false)], false, true, false).unwrap();
        let again = std::fs::read_to_string(dst.join("settings.json")).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&again).unwrap(), live);

        // Re-enabling after withdrawal starts from the empty base and converges.
        run_targets_in(&state, &src, &roots, &[target(&dst, true)], false, true, false).unwrap();
        let live: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dst.join("settings.json")).unwrap()).unwrap();
        assert_eq!(hook_commands(&live, "SessionStart").len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn withdrawal_keeps_a_user_hook_the_predicate_claims_and_an_over_recorded_base_entry() {
        // Two hooks withdrawal must not touch: one the user added under
        // .claude/hooks/ after activation (the ownership predicate claims it),
        // and one a base seeded before #502 recorded as ours (it is the
        // user's). Withdrawal removes only what the app ships.
        let base = sandbox("withdraw-claimed");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        fake_source(&src);
        fake_settings(&src);
        std::fs::write(dst.join("settings.json"), user_settings()).unwrap();
        let roots = projection_roots(&src);
        let state = base.join("state");
        run_targets_in(&state, &src, &roots, &[target(&dst, true)], false, true, false).unwrap();

        // After activation the user adds a hook under .claude/hooks/, and an
        // old base is rewritten to claim the user's Stop hook as ours.
        let mut live: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dst.join("settings.json")).unwrap()).unwrap();
        live["hooks"]["PreToolUse"] = serde_json::json!([{"hooks":[{"type":"command","command":"${HOME}/.claude/hooks/my-own.sh"}]}]);
        std::fs::write(dst.join("settings.json"), serde_json::to_string_pretty(&live).unwrap()).unwrap();
        let base_path = base_path_for(&state, &dst);
        let mut recorded: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&base_path).unwrap()).unwrap();
        recorded["hooks"]["Stop"] = serde_json::json!([{"hooks":[{"type":"command","command":"echo user-stop"}]}]);
        std::fs::write(&base_path, serde_json::to_string_pretty(&recorded).unwrap()).unwrap();

        run_targets_in(&state, &src, &roots, &[target(&dst, false)], false, true, false).unwrap();
        let live: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dst.join("settings.json")).unwrap()).unwrap();
        assert!(hook_commands(&live, "PreToolUse").iter().any(|c| c.contains("my-own.sh")), "claimed user hook kept: {live}");
        assert_eq!(hook_commands(&live, "Stop"), vec!["echo user-stop".to_string()], "over-recorded user hook kept: {live}");
        assert!(!hook_commands(&live, "SessionStart").iter().any(|c| c.contains("check-setup.sh")), "shipped hook removed");
        assert_eq!(hook_commands(&live, "SessionStart"), vec!["echo user-start".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_source_upgrade_refreshes_our_entries_without_blocking() {
        let base = sandbox("upgrade");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        fake_source(&src);
        fake_settings(&src);
        std::fs::write(dst.join("settings.json"), user_settings()).unwrap();
        let roots = projection_roots(&src);
        let state = base.join("state");
        run_targets_in(&state, &src, &roots, &[target(&dst, true)], false, true, false).unwrap();
        // The next version ships a changed command.
        std::fs::write(
            src.join("settings.json"),
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"${HOME}/.claude/hooks/ways/check-setup.sh --v2"}]}]}}"#,
        )
        .unwrap();
        let plan = plan_for(&state, &src, &dst, &roots).unwrap();
        let s = plan.settings.as_ref().unwrap();
        assert_eq!(s.hooks_refreshed.len(), 1, "{s:?}");
        assert!(s.hooks_replaced.is_empty(), "{s:?}");
        assert!(!plan.blocked, "an upgrade of our own entry must not block");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_permission_the_user_already_had_survives_withdrawal() {
        let base = sandbox("perm-kept");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        fake_source(&src);
        fake_settings(&src);
        std::fs::write(
            dst.join("settings.json"),
            r#"{"permissions":{"allow":["Bash(ways:*)","Bash(make:*)"],"deny":["Read(~/.ssh/**)"]}}"#,
        )
        .unwrap();
        let roots = projection_roots(&src);
        let state = base.join("state");
        run_targets_in(&state, &src, &roots, &[target(&dst, true)], false, true, false).unwrap();
        run_targets_in(&state, &src, &roots, &[target(&dst, false)], false, true, false).unwrap();
        let live: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dst.join("settings.json")).unwrap()).unwrap();
        let allow: Vec<&str> = live["permissions"]["allow"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(allow, vec!["Bash(ways:*)", "Bash(make:*)"], "{live}");
        let deny: Vec<&str> = live["permissions"]["deny"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(deny, vec!["Read(~/.ssh/**)"], "{live}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn withdraw_leaves_real_paths_and_foreign_symlinks() {
        let base = sandbox("withdraw-keep");
        let src = base.join("data");
        let dst = base.join("proj");
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(dst.join("skills")).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(dst.join("skills/mine.md"), "mine").unwrap();
        std::fs::create_dir_all(dst.join("hooks")).unwrap();
        make_symlink(&elsewhere, &dst.join("hooks/ways"), true).unwrap();
        fake_source(&src);
        let roots = projection_roots(&src);
        let state = base.join("state");
        run_targets_in(&state, &src, &roots, &[target(&dst, false)], false, true, false).unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("skills/mine.md")).unwrap(), "mine");
        assert!(std::fs::read_link(dst.join("hooks/ways")).is_ok(), "foreign symlink kept");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn empty_target_list_touches_nothing() {
        let base = sandbox("no-targets");
        let src = base.join("data");
        std::fs::create_dir_all(&src).unwrap();
        fake_source(&src);
        let roots = projection_roots(&src);
        run_targets_in(&base.join("state"), &src, &roots, &[], false, true, false).unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn plan_names_refused_roots_kept_user_hooks_and_added_entries() {
        let base = sandbox("plan");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(dst.join("skills/own")).unwrap();
        std::fs::write(dst.join("skills/own/SKILL.md"), "mine").unwrap();
        fake_source(&src);
        fake_settings(&src);
        std::fs::write(dst.join("settings.json"), user_settings()).unwrap();
        let roots = projection_roots(&src);
        let state = base.join("state");
        let plan = plan_for(&state, &src, &dst, &roots).unwrap();
        let skills = plan.roots.iter().find(|r| r.rel == "skills").unwrap();
        assert_eq!(skills.state, "refused");
        assert!(skills.detail.starts_with("real directory, 1 entries"));
        let hooks_ways = plan.roots.iter().find(|r| r.rel == "hooks/ways").unwrap();
        assert_eq!(hooks_ways.state, "link");
        let s = plan.settings.as_ref().unwrap();
        assert_eq!(s.user_hooks_kept, 2);
        assert_eq!(s.hooks_added.len(), 1);
        assert!(s.hooks_added[0].command.contains("check-setup.sh"));
        assert!(s.hooks_removed.is_empty());
        assert!(s.perms_added.iter().any(|p| p.starts_with("Bash(")));
        assert!(plan.blocked, "a refused root blocks");
        // Nothing was touched by planning.
        assert!(std::fs::symlink_metadata(dst.join("hooks/ways")).is_err());
        assert_eq!(std::fs::read_to_string(dst.join("settings.json")).unwrap(), user_settings());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn plan_flags_a_user_hook_the_predicate_would_claim() {
        // The documented trade-off: a user hook under .claude/hooks/ reads as ours
        // and would be replaced. The plan must say so, and block.
        let base = sandbox("plan-claim");
        let src = base.join("data");
        let dst = base.join("proj");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        fake_source(&src);
        fake_settings(&src);
        std::fs::write(
            dst.join("settings.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"${HOME}/.claude/hooks/my-own.sh"}]}]}}"#,
        )
        .unwrap();
        let roots = projection_roots(&src);
        let state = base.join("state");
        let plan = plan_for(&state, &src, &dst, &roots).unwrap();
        let s = plan.settings.as_ref().unwrap();
        assert_eq!(s.hooks_replaced.len(), 1, "claimed as ours: {s:?}");
        assert!(s.hooks_replaced[0].command.contains("my-own.sh"));
        assert!(plan.blocked, "a claimed user hook blocks activation");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn default_root_keeps_the_legacy_base_path() {
        let state = Path::new("/tmp/ways-state-test");
        let legacy = state.join("settings-applied.json");
        assert_eq!(base_path_for(state, &paths::projection_root()), legacy);
        let other = base_path_for(state, Path::new("/tmp/some-other-claude"));
        assert_ne!(other, legacy);
        assert!(other.to_string_lossy().contains("targets"));
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
