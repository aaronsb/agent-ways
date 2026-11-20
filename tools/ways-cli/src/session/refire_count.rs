// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! How many times each way has fired in each agent thread.
//!
//! The refire back-off reads n, the fires the gate has recorded for a way in
//! one thread, and waits H × g^(n-1) before the next refire. The fire clock
//! lives in the session directory, which compaction removes; the count must
//! survive compaction, so it lives under the durable state root instead:
//!
//! ```text
//! $XDG_STATE_HOME/agent-ways/refire-counts/<session>/<agent>.json   {"<way>": n, ...}
//! $XDG_STATE_HOME/agent-ways/refire-counts/<session>/compactions    a number
//! ```
//!
//! `<agent>` is `main` or the subagent's key ([`super::agent_key`]), as the
//! session state names it. A missing or unparsable count file reads as no
//! fires, so state from before this record, and a file a crash cut short,
//! start each way at n = 0. `compactions` is the session's compaction epoch:
//! it grows by one at each compaction, which clears every thread at once.
//! Whole session directories are pruned by age at SessionStart, and
//! `ways session reset --confirm` removes one session's directory.
//!
//! References:
//! - ADR-711 §1: a fire count per agent thread, kept outside the session
//!   directory so compaction does not erase it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::is_plain_session_id;

type Counts = BTreeMap<String, u64>;

/// The store under the state root. Every reader and writer goes through
/// one, so the tests point it at a directory of their own.
struct Store {
    root: PathBuf,
}

impl Store {
    fn live() -> Store {
        Store { root: crate::paths::state_root().join("refire-counts") }
    }

    /// The session's directory, `None` for an id that is not one plain path
    /// component, so a payload's id never climbs out of the root.
    fn session(&self, session_id: &str) -> Option<PathBuf> {
        is_plain_session_id(session_id).then(|| self.root.join(session_id))
    }

    fn thread(&self, session_id: &str, agent: &str) -> Option<PathBuf> {
        self.session(session_id).map(|d| d.join(format!("{agent}.json")))
    }

    fn load(&self, session_id: &str, agent: &str) -> Counts {
        self.thread(session_id, agent)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn count(&self, session_id: &str, agent: &str, way_id: &str) -> u64 {
        self.load(session_id, agent).get(way_id).copied().unwrap_or(0)
    }

    fn bump(&self, session_id: &str, agent: &str, way_id: &str) -> u64 {
        let Some(path) = self.thread(session_id, agent) else { return 0 };
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(&self.root));
        let _lock = agent_settings::writer::Lock::acquire_kept(&path.with_extension("")).ok();
        let mut counts = self.load(session_id, agent);
        let n = counts.entry(way_id.to_string()).or_insert(0);
        *n += 1;
        let n = *n;
        if let Ok(body) = serde_json::to_string(&counts) {
            let _ = agent_settings::writer::write_atomic(&path, body);
        }
        n
    }

    fn epoch(&self, session_id: &str) -> u64 {
        self.session(session_id)
            .and_then(|d| std::fs::read_to_string(d.join("compactions")).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    fn bump_epoch(&self, session_id: &str) -> u64 {
        let Some(dir) = self.session(session_id) else { return 0 };
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("compactions");
        let _lock = agent_settings::writer::Lock::acquire_kept(&path).ok();
        let next = self.epoch(session_id) + 1;
        let _ = agent_settings::writer::write_atomic(&path, next.to_string());
        next
    }

    /// Remove the session's counts and its epoch. Returns the number of
    /// files removed, `None` when the session has none.
    fn clear(&self, session_id: &str) -> Option<usize> {
        let dir = self.session(session_id).filter(|d| d.is_dir())?;
        let files = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter(|f| f.path().is_file()).count();
        let _ = std::fs::remove_dir_all(&dir);
        Some(files)
    }

    fn prune(&self, max_age: std::time::Duration) {
        let Ok(sessions) = std::fs::read_dir(&self.root) else { return };
        for session in sessions.flatten() {
            let dir = session.path();
            if !dir.is_dir() {
                continue;
            }
            let fresh = std::fs::read_dir(&dir).into_iter().flatten().flatten().any(|f| {
                f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_none_or(|age| age <= max_age)
            });
            if !fresh {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
}

/// The fires recorded for `way_id` in `agent`'s thread of `session_id`.
pub fn fire_count_for(session_id: &str, agent: &str, way_id: &str) -> u64 {
    Store::live().count(session_id, agent, way_id)
}

/// [`fire_count_for`] the agent this process serves.
pub fn fire_count(session_id: &str, way_id: &str) -> u64 {
    fire_count_for(session_id, &super::current_agent(), way_id)
}

/// Count one fire of `way_id` in the current agent's thread and return the
/// new n. Every way of a thread shares one file, so parallel hooks recording
/// different ways take the thread's lock around the read and the write, and
/// the write replaces the file whole. Best effort: a store that cannot be
/// written leaves n where it was, which only shortens the next wait.
pub fn bump_fire_count(session_id: &str, way_id: &str) -> u64 {
    Store::live().bump(session_id, &super::current_agent(), way_id)
}

/// The session's compaction epoch: how many compactions it has had.
pub fn compaction_epoch(session_id: &str) -> u64 {
    Store::live().epoch(session_id)
}

/// Record one compaction of the session and return the new epoch.
pub fn bump_compaction_epoch(session_id: &str) -> u64 {
    Store::live().bump_epoch(session_id)
}

/// Remove the session's thread counts and its compaction epoch, so every
/// thread starts again at n = 0. `ways session reset --confirm` runs it;
/// SessionStart and compaction keep the counts. Returns the files
/// removed, `None` when the session has none.
pub fn clear(session_id: &str) -> Option<usize> {
    Store::live().clear(session_id)
}

/// Remove the counts of sessions untouched for longer than `max_age`. A
/// session is touched when any of its files is written, so a long session
/// that still fires keeps its counts. Run at SessionStart.
pub fn prune(max_age: std::time::Duration) {
    Store::live().prune(max_age);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store in a directory of the test's own, removed on drop.
    struct Scratch(Store);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let root = std::env::temp_dir().join(format!("ways-refire-count-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            Scratch(Store { root })
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0.root);
        }
    }

    #[test]
    fn a_missing_count_reads_as_no_fires_and_bumps_from_one() {
        let scratch = Scratch::new("bump");
        let s = &scratch.0;
        assert_eq!(s.count("s1", "main", "d/w"), 0);
        assert_eq!(s.bump("s1", "main", "d/w"), 1);
        assert_eq!(s.bump("s1", "main", "d/w"), 2);
        assert_eq!(s.bump("s1", "main", "d/x"), 1);
        assert_eq!((s.count("s1", "main", "d/w"), s.count("s1", "main", "d/x")), (2, 1));
        assert_eq!(s.count("s1", "asub", "d/w"), 0, "another thread counts apart");
        assert_eq!(s.count("s2", "main", "d/w"), 0, "another session counts apart");
    }

    /// A count file that does not parse, such as one cut short, reads as no
    /// fires, and the next bump writes a whole one.
    #[test]
    fn an_unparsable_count_file_reads_as_no_fires() {
        let scratch = Scratch::new("garbage");
        let s = &scratch.0;
        let path = s.thread("s1", "main").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{\"d/w\": 3").unwrap();
        assert_eq!(s.count("s1", "main", "d/w"), 0);
        assert_eq!(s.bump("s1", "main", "d/w"), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"d/w\":1}");
    }

    #[test]
    fn an_id_that_is_not_plain_stores_nothing() {
        let scratch = Scratch::new("plain");
        let s = &scratch.0;
        assert_eq!(s.bump("../escape", "main", "d/w"), 0);
        assert_eq!(s.bump_epoch("a/b"), 0);
        assert!(!s.root.exists());
    }

    #[test]
    fn the_compaction_epoch_grows_by_one() {
        let scratch = Scratch::new("epoch");
        let s = &scratch.0;
        assert_eq!(s.epoch("s1"), 0);
        assert_eq!(s.bump_epoch("s1"), 1);
        assert_eq!(s.bump_epoch("s1"), 2);
        assert_eq!(s.epoch("s1"), 2);
        assert_eq!(s.epoch("s2"), 0);
    }

    #[test]
    fn clear_removes_one_sessions_counts_and_epoch() {
        let scratch = Scratch::new("clear");
        let s = &scratch.0;
        s.bump("s1", "main", "d/w");
        s.bump("s1", "asub", "d/w");
        s.bump_epoch("s1");
        s.bump("s2", "main", "d/w");
        // Three state files and the locks beside them.
        assert!(s.clear("s1").is_some_and(|n| n >= 3));
        assert_eq!((s.count("s1", "main", "d/w"), s.count("s1", "asub", "d/w"), s.epoch("s1")), (0, 0, 0));
        assert_eq!(s.count("s2", "main", "d/w"), 1, "another session keeps its counts");
        assert_eq!(s.clear("s1"), None);
        assert_eq!(s.clear("../s2"), None);
    }

    #[test]
    fn prune_removes_only_sessions_older_than_the_age() {
        let scratch = Scratch::new("prune");
        let s = &scratch.0;
        s.bump("old", "main", "d/w");
        s.bump("new", "main", "d/w");
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for f in std::fs::read_dir(s.root.join("old")).unwrap().flatten() {
            std::fs::File::options().write(true).open(f.path()).unwrap().set_modified(past).unwrap();
        }
        s.prune(std::time::Duration::from_secs(60));
        assert!(!s.root.join("old").exists(), "the stale session is removed");
        assert_eq!(s.count("new", "main", "d/w"), 1, "the fresh one stays");
    }
}
