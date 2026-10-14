//! `attend message cleanup` — reap signal files whose owning project is gone, and
//! prune the empty project dirs left behind. The core `run_cleanup` is
//! also used by the in-loop auto-sweep in `cmd::run`, so it and
//! `CleanupStats` are `pub(crate)`.
//!
//! An unmarked message is never reaped by age (ADR-136): a durable message
//! waits in its tray until the recipient reads it. Lifetime is bound by *project
//! liveness* instead — mirroring Claude Code, a project is live iff
//! `~/.claude/projects/<encoded-cwd>/` exists. When a project is gone:
//!   - its directed tray (`signals/<encoded-cwd>/`) is reaped wholesale —
//!     the recipient no longer exists, so its mail is moot;
//!   - in the shared rooms (`_broadcast`, `@group`) and the person trays
//!     (`_people/<member-id>/`), the individual signals *authored by* that
//!     dead project are reaped, keyed by the sender cwd in the wire format.
//!     A live recipient that has not yet read one is safe: a project leaves
//!     `~/.claude/projects` only when the user deletes it, long after
//!     delivery latency. A person tray itself is never reaped.
//!
//! A tray is named by `claude_sessions::attend_key`: the project slug, `-`,
//! and a hash of the path. The slug part is what liveness checks.
//!
//! The same sweep removes the messages a delete marked (ADR-404 D3e), with
//! their marks, once a mark is [`MARK_REAP_DAYS`] old, or, for a message in
//! a project tray, once that project has started [`MARK_REAP_SESSIONS`]
//! Claude Code sessions since the mark. A session start is a transcript
//! under the project's `~/.claude/projects/<slug>/` born after the mark,
//! one per session id, so each `/clear` counts. A message no delete marked
//! is never removed this way.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::util::{projects_base, signals_base};

/// Days after which cleanup removes a marked message.
pub(crate) const MARK_REAP_DAYS: u64 = 7;

/// Claude Code sessions started in a project after which cleanup removes a
/// marked message in that project's tray.
pub(crate) const MARK_REAP_SESSIONS: usize = 3;

/// Statistics from a cleanup sweep.
#[derive(Default, Debug)]
pub(crate) struct CleanupStats {
    pub(crate) examined: u64,
    pub(crate) removed: u64,
    pub(crate) bytes: u64,
    pub(crate) dirs_removed: u64,
    /// Of `removed`, the messages a delete had marked.
    pub(crate) marked_removed: u64,
}

/// The Claude Code project directory of the project owning tray `tray`.
/// The tray's slug part names it, exactly or, for a slug Claude Code
/// truncated, by its 200-character prefix.
fn project_dir(projects: &Path, tray: &str) -> Option<PathBuf> {
    let slug = claude_sessions::attend_key_slug(tray)?;
    let exact = projects.join(slug);
    if exact.is_dir() {
        return Some(exact);
    }
    if slug.len() <= claude_sessions::MAX_SLUG_LEN {
        return None;
    }
    let prefix = &slug[..=claude_sessions::MAX_SLUG_LEN];
    std::fs::read_dir(projects).into_iter().flatten().flatten().map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(prefix)) && p.is_dir()
    })
}

/// Is the project owning the tray `tray` still tracked by Claude Code?
fn tray_live(projects: &Path, tray: &str) -> bool {
    project_dir(projects, tray).is_some()
}

/// When each Claude Code session in project directory `dir` started: its
/// transcript's birth time, or where the filesystem keeps none, the first
/// timestamp written in it.
fn session_starts(dir: &Path) -> Vec<SystemTime> {
    claude_sessions::transcripts_in(dir)
        .iter()
        .filter_map(|t| std::fs::metadata(t).and_then(|m| m.created()).ok().or_else(|| first_timestamp(t)))
        .collect()
}

/// The first `"timestamp"` in the transcript at `path`, among its first lines.
fn first_timestamp(path: &Path) -> Option<SystemTime> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    std::io::BufReader::new(file).lines().take(20).map_while(Result::ok).find_map(|line| {
        let v: serde_json::Value = serde_json::from_str(&line).ok()?;
        let secs = agent_fmt::when::parse_utc_iso(v.get("timestamp")?.as_str()?)?;
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    })
}

/// Whether a message marked at `at` has outlived its mark: [`MARK_REAP_DAYS`]
/// have passed, or `starts`, the sessions of the project whose tray holds
/// it, counts [`MARK_REAP_SESSIONS`] begun since.
fn mark_expired(at: SystemTime, now: SystemTime, starts: Option<&[SystemTime]>) -> bool {
    now.duration_since(at).unwrap_or_default() >= Duration::from_secs(MARK_REAP_DAYS * 86_400)
        || starts.is_some_and(|s| s.iter().filter(|t| **t > at).count() >= MARK_REAP_SESSIONS)
}

/// Core cleanup routine, shared by `attend message cleanup` and the in-loop sweep.
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

    let marks = attend_groups::tombstones::message_marks(base);
    let now = SystemTime::now();
    // Session starts per project tray, read once, and only for a tray that
    // holds a marked message.
    let mut starts: HashMap<String, Vec<SystemTime>> = HashMap::new();
    // The keys of the signal files the sweep leaves, whose marks stay.
    let mut present: HashSet<String> = HashSet::new();

    // Pass 1: reap signals. The person trays sit one level down, under
    // `_people/`, and each is swept as a shared room.
    let mut rooms: Vec<(std::path::PathBuf, String)> =
        entries.flatten().filter_map(|e| Some((e.path(), e.file_name().to_str()?.to_string()))).collect();
    let people = base.join(attend_groups::PEOPLE_DIR);
    rooms.extend(std::fs::read_dir(&people).into_iter().flatten().flatten().map(|e| (e.path(), attend_groups::PEOPLE_DIR.to_string())));
    for (subpath, dir_name) in rooms {
        if !subpath.is_dir() || subpath == people {
            continue;
        }
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
                // gone. A record that does not parse, or cannot be read,
                // is left alone.
                agent_identity::signal::read_signal_file(&path)
                    .ok()
                    .and_then(|c| {
                        agent_identity::parse_signal(agent_identity::wire_line(&c))
                            .map(|sig| claude_sessions::find_project_dir_in(projects, sig.cwd).is_none())
                    })
                    .unwrap_or(false)
            } else {
                tray_dead
            };
            let key = attend_state::seen_key(name);
            let marked_out = !reap
                && marks.get(&key).is_some_and(|at| {
                    let project = (!shared).then(|| {
                        starts
                            .entry(dir_name.clone())
                            .or_insert_with(|| project_dir(projects, &dir_name).map(|d| session_starts(&d)).unwrap_or_default())
                            .as_slice()
                    });
                    mark_expired(*at, now, project)
                });
            if !reap && !marked_out {
                present.insert(key);
                continue;
            }

            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            if dry_run {
                println!("would remove {} ({} bytes)", path.display(), size);
            } else if std::fs::remove_file(&path).is_ok() {
                stats.removed += 1;
                stats.bytes += size;
                stats.marked_removed += u64::from(marked_out);
            } else {
                present.insert(key);
            }
        }
    }

    // A mark goes with its message, and with it a batch record that owns no
    // mark any more.
    if !dry_run && !marks.is_empty() {
        attend_groups::tombstones::prune(base, &present);
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
            "cleaned up {} signal file(s), {} of them marked for deletion, freed {} bytes (examined {}); removed {} empty project dir(s)",
            stats.removed, stats.marked_removed, stats.bytes, stats.examined, stats.dirs_removed,
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

    /// In a shared room each signal goes by its sender's project, read from
    /// the record the way every other reader parses it.
    #[test]
    fn a_shared_room_signal_is_reaped_by_its_senders_project() {
        let (root, base, projects) = fixture("shared");
        std::fs::create_dir_all(projects.join("-srv-live")).unwrap();
        let open = base.join("_broadcast");
        std::fs::create_dir_all(&open).unwrap();
        for (name, line) in [
            ("live.signal", "claude:a|live|/srv/live|hi\n"),
            ("dead.signal", "claude:b|dead|/srv/dead|re:live|a reply\n"),
            ("torn.signal", "claude:c|dead|/srv/dead"),
        ] {
            std::fs::write(open.join(name), line).unwrap();
        }
        let stats = run_cleanup_in(&base, &projects, false, false);
        assert_eq!(stats.removed, 1);
        assert!(open.join("live.signal").exists());
        assert!(!open.join("dead.signal").exists());
        assert!(open.join("torn.signal").exists(), "a record that does not parse is left alone");
        std::fs::remove_dir_all(&root).ok();
    }

    /// A signal in a person tray goes when its sender's project is gone, as
    /// in a shared room; the tray itself stays, empty or not (ADR-401, `to`).
    #[test]
    fn a_person_tray_signal_is_reaped_by_its_senders_project_and_the_tray_kept() {
        let (root, base, projects) = fixture("people");
        std::fs::create_dir_all(projects.join("-srv-live")).unwrap();
        let tray = attend_groups::person_dir(&base, "aaron");
        std::fs::create_dir_all(&tray).unwrap();
        std::fs::write(tray.join("live.signal"), "claude:a|live|/srv/live|to:aaron|hi\n").unwrap();
        std::fs::write(tray.join("dead.signal"), "claude:b|dead|/srv/dead|to:aaron|hi\n").unwrap();
        let empty = attend_groups::person_dir(&base, "nobody");
        std::fs::create_dir_all(&empty).unwrap();
        let stats = run_cleanup_in(&base, &projects, false, false);
        assert_eq!(stats.removed, 1);
        assert!(tray.join("live.signal").exists());
        assert!(!tray.join("dead.signal").exists());
        assert!(empty.is_dir(), "a person tray is never reaped");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Mark the signal `name` and date the mark `ago` back.
    fn mark(base: &Path, name: &str, ago: Duration) {
        attend_groups::tombstones::mark_messages(base, &[name.to_string()]).unwrap();
        let path = base.join("_tombstones/messages").join(name);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() - ago).unwrap();
    }

    /// `n` sessions started in the project at `path` now.
    fn sessions(projects: &Path, path: &str, n: usize) {
        let dir = projects.join(claude_sessions::project_slug(path));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..n {
            std::fs::write(dir.join(format!("session-{i}.jsonl")), "{}\n").unwrap();
        }
    }

    const DAY: Duration = Duration::from_secs(86_400);

    /// In `#open` a mark goes by its days alone, however many sessions any
    /// project started since.
    #[test]
    fn a_marked_open_message_is_removed_after_the_days_limit() {
        let (root, base, projects) = fixture("mark-days");
        sessions(&projects, "/srv/live", 5);
        let open = base.join("_broadcast");
        for name in ["old.signal", "recent.signal", "unmarked.signal"] {
            std::fs::create_dir_all(&open).unwrap();
            std::fs::write(open.join(name), "claude:a|live|/srv/live|hi\n").unwrap();
        }
        mark(&base, "old.signal", 8 * DAY);
        mark(&base, "recent.signal", DAY);

        let dry = run_cleanup_in(&base, &projects, true, false);
        assert_eq!(dry.removed, 0);
        assert!(open.join("old.signal").exists(), "a dry run removes nothing");

        let stats = run_cleanup_in(&base, &projects, false, false);
        assert_eq!((stats.removed, stats.marked_removed), (1, 1));
        assert!(!open.join("old.signal").exists());
        assert!(open.join("recent.signal").exists() && open.join("unmarked.signal").exists());
        let marks: Vec<String> = attend_groups::tombstones::message_marks(&base).into_keys().collect();
        assert_eq!(marks, ["recent.signal"], "the removed message's mark goes with it");
        std::fs::remove_dir_all(&root).ok();
    }

    /// In a project tray a mark also goes once the project has started the
    /// session limit since; mail no delete marked stays, whoever sent it.
    #[test]
    fn a_marked_tray_message_is_removed_after_the_session_limit() {
        let (root, base, projects) = fixture("mark-sessions");
        let tray = base.join(claude_sessions::attend_key("/srv/live"));
        std::fs::create_dir_all(&tray).unwrap();
        for name in ["marked.signal", "from-gone.signal"] {
            std::fs::write(tray.join(name), "claude:gone|gone|/srv/gone|to:someone|hi\n").unwrap();
        }
        mark(&base, "marked.signal", DAY);

        sessions(&projects, "/srv/live", MARK_REAP_SESSIONS - 1);
        run_cleanup_in(&base, &projects, false, false);
        assert!(tray.join("marked.signal").exists(), "two sessions since the mark");

        sessions(&projects, "/srv/live", MARK_REAP_SESSIONS);
        let stats = run_cleanup_in(&base, &projects, false, false);
        assert_eq!(stats.marked_removed, 1);
        assert!(!tray.join("marked.signal").exists());
        assert!(tray.join("from-gone.signal").exists(), "unmarked mail of a session that has gone stays");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Sessions that started before the mark do not count.
    #[test]
    fn sessions_before_the_mark_do_not_count() {
        let at = SystemTime::now();
        let before = [at - DAY, at - DAY, at - DAY];
        assert!(!mark_expired(at, at + Duration::from_secs(60), Some(&before)));
        let after = [at + Duration::from_secs(1); MARK_REAP_SESSIONS];
        assert!(mark_expired(at, at + Duration::from_secs(60), Some(&after)));
        assert!(!mark_expired(at, at + Duration::from_secs(60), None));
        assert!(mark_expired(at, at + MARK_REAP_DAYS as u32 * DAY, None));
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
