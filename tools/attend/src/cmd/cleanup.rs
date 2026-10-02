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
    if let Some(slug) = claude_sessions::attend_key_slug(tray) {
        if projects.join(slug).is_dir() {
            return true;
        }
        if slug.len() > claude_sessions::MAX_SLUG_LEN {
            let prefix = &slug[..=claude_sessions::MAX_SLUG_LEN];
            let any = std::fs::read_dir(projects).into_iter().flatten().flatten().any(|e| {
                e.file_name().to_str().is_some_and(|n| n.starts_with(prefix)) && e.path().is_dir()
            });
            if any {
                return true;
            }
        }
    }
    // transition read: removed by #701 (ADR-506)
    // A tray named under attend's old rule for a path of letters, digits,
    // `/`, `_` and `.` has the project directory's own name, or, over 200
    // characters, shares its first 200 and the `-` Claude Code puts next.
    if projects.join(tray).is_dir() {
        return true;
    }
    let slug = claude_sessions::project_slug(tray);
    slug.len() > claude_sessions::MAX_SLUG_LEN
        && !claude_sessions::prefix_candidates_in(projects, tray).is_empty()
}

/// Move pending signals out of trays named under attend's old rule into
/// the project's [`claude_sessions::attend_key`] tray, then remove the
/// emptied old trays. Runs before any sweep, at `attend run` start and in
/// every cleanup, so mail in an old tray is neither stranded nor reaped.
///
/// The project of an old tray is found among `known_paths` (this session's
/// origin and the session records' cwds) and the paths the project
/// directories record; an old tray matches a path when it is one of that
/// path's [`claude_sessions::legacy_tray_names`]; a tray two paths claim
/// is left in place. Signal filenames are
/// unique ids, so a move never overwrites; one already present is left in
/// place. Returns the number of signals moved.
// transition read: removed by #701 (ADR-506)
pub(crate) fn migrate_legacy_trays(base: &Path, projects: &Path, known_paths: &[String], dry_run: bool) -> u64 {
    let Ok(entries) = std::fs::read_dir(base) else { return 0 };
    let trays: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('_') && !n.starts_with('@'))
        .collect();
    let mut moved = 0;
    for tray in &trays {
        // An old-rule name encodes to the project directory's name; over
        // 200 characters, to its prefix (the hash differs: it is taken over
        // the tray name, not the path).
        let dir_name = claude_sessions::project_slug(tray);
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        if projects.join(&dir_name).is_dir() {
            dirs.push(projects.join(&dir_name));
        }
        dirs.extend(claude_sessions::prefix_candidates_in(projects, tray));
        let resolved: Vec<String> = dirs
            .iter()
            .filter_map(|d| d.file_name()?.to_str().map(str::to_string))
            .filter_map(|n| claude_sessions::resolve_project_path(projects, &n))
            .collect();
        let mut claims: Vec<&str> = known_paths
            .iter()
            .map(String::as_str)
            .chain(resolved.iter().map(String::as_str))
            .filter(|p| claude_sessions::legacy_tray_names(p).iter().any(|n| n == tray))
            .collect();
        claims.sort_unstable();
        claims.dedup();
        // An old tray two projects' names both map to was shared; moving it
        // to one of them would hide its mail from the other. Leave it.
        let [path] = claims[..] else { continue };
        let target = base.join(claude_sessions::attend_key(path));
        let old = base.join(tray);
        let Ok(files) = std::fs::read_dir(&old) else { continue };
        for f in files.flatten() {
            let name = f.file_name();
            if !name.to_string_lossy().ends_with(".signal") || target.join(&name).exists() {
                continue;
            }
            if dry_run {
                println!("would move {} to {}", f.path().display(), target.display());
                continue;
            }
            if std::fs::create_dir_all(&target).is_ok() && std::fs::rename(f.path(), target.join(&name)).is_ok() {
                moved += 1;
            }
        }
        if !dry_run {
            let _ = std::fs::remove_dir(&old);
        }
    }
    moved
}

/// The paths cleanup and `attend run` can name trays for without reading
/// the project directories: this session's origin and every session
/// record's cwd.
pub(crate) fn known_project_paths() -> Vec<String> {
    let mut paths = vec![crate::util::own_origin_cwd()];
    paths.extend(
        claude_sessions::ClaudeDir::user()
            .session_records()
            .into_iter()
            .filter_map(|r| r.cwd.map(|c| attend_session::normalize_origin(&c))),
    );
    paths.retain(|p| !p.is_empty());
    paths.dedup();
    paths
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
    run_cleanup_in(base, &projects_base(), &known_project_paths(), dry_run, nuke_all)
}

/// [`run_cleanup`] against explicit roots, for tests.
pub(crate) fn run_cleanup_in(
    base: &Path,
    projects: &Path,
    known_paths: &[String],
    dry_run: bool,
    nuke_all: bool,
) -> CleanupStats {
    let mut stats = CleanupStats::default();
    if !base.is_dir() {
        return stats;
    }
    // Before any sweep: an old-named tray of a live project is otherwise
    // judged dead by its name and reaped with its unread mail.
    // transition read: removed by #701 (ADR-506)
    migrate_legacy_trays(base, projects, known_paths, dry_run);

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
        // transition read: removed by #701 (ADR-506)
        // A tray in no attend-key form that the migration could not place
        // is kept while it holds signals: its project cannot be judged.
        let unplaced = claude_sessions::attend_key_slug(&dir_name).is_none();
        let tray_dead = !shared && !unplaced && !tray_live(projects, &dir_name);

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
    fn a_live_projects_old_named_tray_is_moved_not_reaped() {
        // `/srv/my proj` is live (Claude Code's dir `-srv-my-proj`), and its
        // tray was named under attend's old rule, which kept the space. The
        // sweep judged it dead by name and deleted its unread mail.
        let (root, base, projects) = fixture("move");
        let project = "/srv/my proj";
        std::fs::create_dir_all(projects.join("-srv-my-proj")).unwrap();
        signal(&base.join("-srv-my proj"), "m1.signal");

        let stats = run_cleanup_in(&base, &projects, &[project.to_string()], false, false);
        assert_eq!(stats.removed, 0);
        let key = claude_sessions::attend_key(project);
        assert!(base.join(&key).join("m1.signal").exists(), "the signal moved into the key tray");
        assert!(!base.join("-srv-my proj").exists(), "the emptied old tray is removed");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_long_live_projects_old_tray_is_moved_not_reaped() {
        // A 250-character path: Claude Code's dir is the 200-char prefix
        // and a hash, so the old tray's name matches it only by prefix. No
        // session names the path; only its transcript does.
        let (root, base, projects) = fixture("long");
        let project = format!("/srv/{}", "a".repeat(250));
        let dir = projects.join(claude_sessions::project_slug(&project));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s1.jsonl"), format!("{{\"cwd\":\"{project}\"}}\n")).unwrap();
        let old = claude_sessions::legacy_tray_names(&project).remove(0);
        signal(&base.join(&old), "m1.signal");

        let stats = run_cleanup_in(&base, &projects, &[], false, false);
        assert_eq!(stats.removed, 0);
        let key = base.join(claude_sessions::attend_key(&project));
        assert!(key.join("m1.signal").exists(), "moved into the key tray");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_long_old_tray_is_live_by_prefix() {
        let (root, _base, projects) = fixture("long-live");
        let project = format!("/srv/{}", "b".repeat(250));
        std::fs::create_dir_all(projects.join(claude_sessions::project_slug(&project))).unwrap();
        let old = claude_sessions::legacy_tray_names(&project).remove(0);
        assert!(tray_live(&projects, &old));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_tray_two_paths_claim_is_left_in_place() {
        // `/x/a:b` (under the wider old rule) and `/x/a/b` both named their
        // tray `-x-a-b`. Moving it to either would hide mail from the other.
        let (root, base, projects) = fixture("shared");
        std::fs::create_dir_all(projects.join("-x-a-b")).unwrap();
        signal(&base.join("-x-a-b"), "m3.signal");
        let known = ["/x/a:b".to_string(), "/x/a/b".to_string()];
        run_cleanup_in(&base, &projects, &known, false, false);
        assert!(base.join("-x-a-b").join("m3.signal").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unplaced_old_tray_keeps_its_signals() {
        // The project is not known to this machine: the tray cannot be
        // judged, so its mail stays.
        let (root, base, projects) = fixture("unplaced");
        signal(&base.join("-gone-a b"), "m2.signal");
        run_cleanup_in(&base, &projects, &[], false, false);
        assert!(base.join("-gone-a b").join("m2.signal").exists());
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
        run_cleanup_in(&base, &projects, &[], false, false);
        assert!(live.join("a.signal").exists());
        assert!(!dead.exists());
        std::fs::remove_dir_all(&root).ok();
    }
}
