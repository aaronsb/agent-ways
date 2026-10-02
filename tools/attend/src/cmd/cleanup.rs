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

/// Is the project owning `encoded_name` still tracked by Claude Code?
fn project_live(projects: &Path, encoded_name: &str) -> bool {
    projects.join(encoded_name).is_dir()
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
    let mut stats = CleanupStats::default();
    if !base.is_dir() {
        return stats;
    }
    let projects = projects_base();

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
        let tray_dead = !shared && !project_live(&projects, &dir_name);

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
                    .map(|cwd| claude_sessions::find_project_dir_in(&projects, &cwd).is_none())
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
