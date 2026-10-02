//! `attend cleanup` — reap signal files whose owning project is gone, and
//! prune the empty project dirs left behind. The core `run_cleanup` is
//! also used by the in-loop auto-sweep in `cmd::run`, so it and
//! `CleanupStats` are `pub(crate)`.
//!
//! Messages are NEVER reaped by age (ADR-136): a durable message waits in
//! its tray until the recipient reads it. Lifetime is bound by *project
//! liveness* instead — mirroring Claude Code, a project is live iff
//! `~/.claude/projects/<encoded-cwd>/` exists. When a project is gone:
//!   - its directed tray (`signals/<encoded-cwd>/`) is reaped wholesale —
//!     the recipient no longer exists, so its mail is moot;
//!   - in the shared rooms (`_broadcast`, `@group`), the individual
//!     signals *authored by* that dead project are reaped, keyed by the
//!     sender cwd in the wire format. A live recipient that has not yet
//!     read a shared signal is safe: a project leaves `~/.claude/projects`
//!     only when the user deletes it, long after delivery latency.
//!
//! A tray is named by `claude_sessions::attend_key`: the project slug, `-`,
//! and a hash of the path. The slug part is what liveness checks.

use std::path::Path;

use crate::util::{projects_base, signals_base};

/// Statistics from a cleanup sweep.
#[derive(Default, Debug)]
pub(crate) struct CleanupStats {
    pub(crate) examined: u64,
    pub(crate) removed: u64,
    pub(crate) bytes: u64,
    pub(crate) dirs_removed: u64,
}

/// Is the project owning the tray `tray` still tracked by Claude Code? The
/// tray's slug part names the project directory, exactly or, for a slug
/// Claude Code truncated, by its 200-character prefix.
fn tray_live(projects: &Path, tray: &str) -> bool {
    let Some(slug) = claude_sessions::attend_key_slug(tray) else {
        return false;
    };
    if projects.join(slug).is_dir() {
        return true;
    }
    if slug.len() <= claude_sessions::MAX_SLUG_LEN {
        return false;
    }
    let prefix = &slug[..=claude_sessions::MAX_SLUG_LEN];
    std::fs::read_dir(projects).into_iter().flatten().flatten().any(|e| {
        e.file_name().to_str().is_some_and(|n| n.starts_with(prefix)) && e.path().is_dir()
    })
}

/// Sender cwd from a signal's wire line `from|project|cwd|...`. Returns
/// `None` if the line is malformed (fewer than three fields).
fn sender_cwd(content: &str) -> Option<&str> {
    content.trim().split('|').nth(2)
}

/// Core cleanup routine, shared by `attend cleanup` and the in-loop sweep.
///
/// Pass 1 reaps signals by project liveness (or every signal, if
/// `nuke_all`):
///   - reserved shared rooms (`_broadcast`, `@group`): per-signal, by the
///     sender project named in the wire format;
///   - project trays (any other subdir): every signal, when that project
///     is gone.
///
/// Pass 2 removes now-empty project subdirs (never `_broadcast`/`@group`).
///
/// On `dry_run`, prints a line per candidate instead of deleting.
pub(crate) fn run_cleanup(base: &Path, dry_run: bool, nuke_all: bool) -> CleanupStats {
    run_cleanup_in(base, &projects_base(), dry_run, nuke_all)
}

/// [`run_cleanup`] against explicit roots, for tests.
pub(crate) fn run_cleanup_in(
    base: &Path,
    projects: &Path,
    dry_run: bool,
    nuke_all: bool,
) -> CleanupStats {
    let mut stats = CleanupStats::default();
    if !base.is_dir() {
        return stats;
    }
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return stats,
    };

    // Pass 1: reap signals.
    for sub in entries.flatten() {
        let subpath = sub.path();
        if !subpath.is_dir() {
            continue;
        }
        let dir_name = match subpath.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Shared rooms (`_broadcast`, `@group`) are decided per-signal by
        // sender; everything else is a project tray decided by its owner.
        let shared = dir_name.starts_with('_') || dir_name.starts_with('@');
        // A tray in no attend-key form names no project attend reads for,
        // so it is dead by its name (ADR-506).
        let tray_dead = !shared && !tray_live(projects, &dir_name);

        let files = match std::fs::read_dir(&subpath) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for f in files.flatten() {
            let path = f.path();
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.ends_with(".signal") {
                continue;
            }
            stats.examined += 1;

            let reap = if nuke_all {
                true
            } else if shared {
                // Reap a shared-room signal when its author's project is
                // gone. Malformed/unreadable lines are left alone.
                std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|c| sender_cwd(&c).map(str::to_string))
                    .map(|cwd| claude_sessions::find_project_dir_in(projects, &cwd).is_none())
                    .unwrap_or(false)
            } else {
                tray_dead
            };
            if !reap {
                continue;
            }

            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            if dry_run {
                println!("would remove {} ({} bytes)", path.display(), size);
            } else if std::fs::remove_file(&path).is_ok() {
                stats.removed += 1;
                stats.bytes += size;
            }
        }
    }

    // Pass 2: remove now-empty project subdirs (shells left behind). Never
    // touches `_broadcast`, `@group`, or any dir that still has files.
    if let Ok(entries) = std::fs::read_dir(base) {
        for sub in entries.flatten() {
            let subpath = sub.path();
            if !subpath.is_dir() {
                continue;
            }
            let name = match subpath.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if name.starts_with('_') || name.starts_with('@') {
                continue;
            }
            let empty = std::fs::read_dir(&subpath)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false);
            if !empty {
                continue;
            }
            if dry_run {
                println!("would remove empty project dir {}", subpath.display());
            } else if std::fs::remove_dir(&subpath).is_ok() {
                stats.dirs_removed += 1;
            }
        }
    }

    stats
}

pub(crate) fn cmd_cleanup(dry_run: bool, nuke_all: bool) {
    let base = signals_base();
    if !base.is_dir() {
        println!("no signals base at {} — nothing to clean", base.display());
        return;
    }

    let stats = run_cleanup(&base, dry_run, nuke_all);

    if dry_run {
        println!("\ndry run: examined {} signal file(s)", stats.examined);
    } else {
        println!(
            "cleaned up {} signal file(s), freed {} bytes (examined {}); removed {} empty project dir(s)",
            stats.removed, stats.bytes, stats.examined, stats.dirs_removed,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("attend-cleanup-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (base, projects) = (root.join("signals"), root.join("projects"));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&projects).unwrap();
        (root, base, projects)
    }

    fn signal(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), "claude:x|p|/src|hi\n").unwrap();
    }

    #[test]
    fn an_old_named_tray_is_reaped_not_moved() {
        // ADR-506: attend reads only `attend_key` trays. A tray under the old
        // rule (the space kept) names no project, even a live one, so the
        // sweep reaps it and moves nothing.
        let (root, base, projects) = fixture("old");
        std::fs::create_dir_all(projects.join("-srv-my-proj")).unwrap();
        signal(&base.join("-srv-my proj"), "m1.signal");
        let stats = run_cleanup_in(&base, &projects, false, false);
        assert_eq!(stats.removed, 1);
        assert!(!base.join(claude_sessions::attend_key("/srv/my proj")).exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_long_key_tray_is_live_by_prefix() {
        // A 250-character path: Claude Code's dir is the 200-char prefix and
        // a hash of its own, so the key's slug matches it only by prefix.
        let (root, _base, projects) = fixture("long-live");
        let project = format!("/srv/{}", "b".repeat(250));
        std::fs::create_dir_all(projects.join(claude_sessions::project_slug(&project))).unwrap();
        assert!(tray_live(&projects, &claude_sessions::attend_key(&project)));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_dead_projects_key_tray_is_reaped_and_a_live_one_kept() {
        let (root, base, projects) = fixture("keys");
        std::fs::create_dir_all(projects.join("-srv-live")).unwrap();
        let live = base.join(claude_sessions::attend_key("/srv/live"));
        let dead = base.join(claude_sessions::attend_key("/srv/dead"));
        signal(&live, "a.signal");
        signal(&dead, "b.signal");
        run_cleanup_in(&base, &projects, false, false);
        assert!(live.join("a.signal").exists());
        assert!(!dead.exists());
        std::fs::remove_dir_all(&root).ok();
    }
}
