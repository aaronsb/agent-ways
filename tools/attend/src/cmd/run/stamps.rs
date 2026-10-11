//! Per-session stamps in the state dir: where the banner stamp lives, and
//! the startup sweep that removes stamps of sessions long gone.
//!
//! `state/<key>.banner` holds the startup banner fingerprint and
//! `state/<key>.last-inbound` the message `attend reply` threads to. Both
//! are written per session and nothing else removes them, so a machine
//! that runs many sessions collects one pair per session for ever.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long a session's stamps outlive its last sign of life.
pub(super) const STAMP_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The most state-dir entries one sweep examines (ADR-137): the sweep runs
/// on every start, and the directory grows with every session ever seen.
const SWEEP_MAX_ENTRIES: usize = 2000;

/// The stamps the sweep removes. The seen-set (`.state`) has its own
/// lifecycle and is only read here, as a sign of life.
const SWEPT: &[&str] = &["banner", "last-inbound"];

/// Every file a session keeps in the state dir, read for its newest mtime.
const LIFE_SIGNS: &[&str] = &["state", "banner", "last-inbound"];

/// The key a session's banner stamp is filed under. A resolved session
/// uses its session id. A pid-fallback session has none that survives a
/// Monitor re-arm (its id names the `attend run` pid, new on every
/// start), so it uses the Claude Code process it runs under, by pid and
/// start time; with no such process found it keeps no stamp and gets the
/// full banner on every start.
pub(super) fn stamp_key(session_id: &str, resolved: bool, claude: Option<(u32, String)>) -> Option<String> {
    if resolved {
        return Some(session_id.to_string());
    }
    claude.map(|(pid, start)| format!("claude-{pid}-{start}"))
}

/// The banner stamp path for this process's session, if it can keep one.
pub(super) fn banner_stamp_path(session_id: &str, resolved: bool) -> Option<PathBuf> {
    let claude = if resolved {
        None
    } else {
        attend_presence::process::claude_ancestor(std::process::id())
            .and_then(|pid| Some((pid, attend_presence::process::start_time(pid)?)))
    };
    let key = stamp_key(session_id, resolved, claude)?;
    Some(attend_presence::cache::state_dir().join(format!("{key}.banner")))
}

/// Remove the banner and last-inbound stamps of every session that has no
/// live Claude Code record in `live` and has shown no sign of life (any of
/// its state files touched) for `max_age`. Examines at most
/// [`SWEEP_MAX_ENTRIES`] entries and says so when it stops short. Returns
/// how many files it removed.
pub(super) fn sweep_stale(dir: &Path, live: &HashSet<String>, now: SystemTime, max_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut keys: HashSet<String> = HashSet::new();
    let mut examined = 0;
    for entry in entries.flatten() {
        examined += 1;
        if examined > SWEEP_MAX_ENTRIES {
            crate::emit::log(&format!("stamp sweep stopped at {SWEEP_MAX_ENTRIES} entries in {}; the rest wait for the next start", dir.display()));
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some((key, _)) = SWEPT.iter().find_map(|ext| name.strip_suffix(&format!(".{ext}")).map(|k| (k.to_string(), ext))) {
            keys.insert(key);
        }
    }
    let mut removed = 0;
    for key in keys.iter().filter(|k| !live.contains(*k)) {
        let newest = LIFE_SIGNS
            .iter()
            .filter_map(|ext| std::fs::metadata(dir.join(format!("{key}.{ext}"))).ok()?.modified().ok())
            .max();
        let stale = newest.is_some_and(|t| now.duration_since(t).is_ok_and(|age| age > max_age));
        if stale {
            for ext in SWEPT {
                if std::fs::remove_file(dir.join(format!("{key}.{ext}"))).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    removed
}

/// The session ids that have a Claude Code record right now.
pub(super) fn live_sessions() -> HashSet<String> {
    claude_sessions::read_session_records(&claude_sessions::ClaudeDir::user().sessions_dir())
        .into_iter()
        .map(|r| r.session_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resolved_session_is_keyed_by_its_id() {
        assert_eq!(stamp_key("sess-a", true, Some((7, "99".into()))).as_deref(), Some("sess-a"));
    }

    /// A pid-fallback session re-armed every 30 minutes gets a new `attend
    /// run` pid each time; its stamp follows the Claude Code process, so
    /// the re-arm reads `restarted (unchanged)`, not `started`.
    #[test]
    fn a_pid_fallback_session_is_keyed_by_its_claude_process() {
        let first = stamp_key("pid-100", false, Some((42, "123".into())));
        let rearm = stamp_key("pid-200", false, Some((42, "123".into())));
        assert_eq!(first, rearm);
        assert_eq!(first.as_deref(), Some("claude-42-123"));
        assert_ne!(first, stamp_key("pid-300", false, Some((42, "456".into()))), "a reused pid is another process");
        assert_eq!(stamp_key("pid-100", false, None), None);
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("attend-stamps-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), "x").unwrap();
    }

    #[test]
    fn the_sweep_removes_only_long_gone_sessions_stamps() {
        let dir = scratch("sweep");
        for key in ["gone", "live", "recent"] {
            touch(&dir, &format!("{key}.banner"));
            touch(&dir, &format!("{key}.last-inbound"));
        }
        touch(&dir, "gone.state");
        let live = HashSet::from(["live".to_string()]);
        let week = STAMP_MAX_AGE;
        // Eight days on, "gone" and "recent" are both past the age; only
        // a live record keeps "live".
        let later = SystemTime::now() + week + Duration::from_secs(24 * 3600);
        assert_eq!(sweep_stale(&dir, &live, SystemTime::now(), week), 0, "nothing is old yet");
        assert_eq!(sweep_stale(&dir, &live, later, week), 4);
        assert!(dir.join("live.banner").exists() && dir.join("live.last-inbound").exists());
        assert!(!dir.join("gone.banner").exists() && !dir.join("recent.last-inbound").exists());
        assert!(dir.join("gone.state").exists(), "the seen-set is not the sweep's to remove");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_state_dir_sweeps_nothing() {
        let dir = std::env::temp_dir().join(format!("attend-stamps-none-{}", std::process::id()));
        assert_eq!(sweep_stale(&dir, &HashSet::new(), SystemTime::now(), STAMP_MAX_AGE), 0);
    }
}
