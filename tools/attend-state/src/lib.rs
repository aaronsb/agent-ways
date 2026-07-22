//! Shared per-session sensor state — checkpoint/restore for attend's
//! sensors and the consumption record the ADR-172 drain writes.
//!
//! Extracted from `attend::state` when `attend inbox --drain` became a
//! second writer to the seen-set (ADR-172): the drain (a one-shot
//! process at the turn boundary), the long-running sensor loop, and
//! `/purge`'s read-only consult in attend-chat all speak this format,
//! so one crate owns it — the same single-owner move attend-groups
//! made for `_groups.yaml`.
//!
//! **Merge/append discipline (ADR-172 Decision 3).** Every write path
//! goes through a read-union-write cycle under an advisory lock: the
//! on-disk seen-set and the writer's view are unioned, never
//! overwritten. A whole-set snapshot from either writer would fork the
//! "one" seen-set into two — a stale sensor checkpoint resurrecting a
//! message the drain already consumed (double delivery), or vice
//! versa. Union semantics make both writers append-only with respect
//! to each other's marks.
//!
//! Storage: `~/.cache/attend/state/{session-id}.state`, line-oriented,
//! no serde. Session-id keying is the ADR-171 stable identity: callers
//! construct the store with a *resolved* session id or `None` ("no
//! session, no persistence" — an unresolved `pid-<pid>` fallback must
//! never write, or it would alias sessions and corrupt the shared set).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Serializable state snapshot.
/// Simple line-oriented format — no serde dependency.
///
/// Format:
///   seen_signal: <dir>:<filename>
///   disclosed_thresholds: 40,50,65,...
///   context_pct: 31.2
///   reply_hint_shown: true
///   git_branch: main
///   git_head: abc1234
///   version: 0.1.0
#[derive(Debug, Default)]
pub struct StateSnapshot {
    pub seen_signals: HashSet<String>,
    pub disclosed_thresholds: Vec<u8>,
    pub context_pct: Option<f64>,
    pub reply_hint_shown: bool,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
}

impl StateSnapshot {
    /// Serialize to line-oriented format.
    fn serialize(&self) -> String {
        let mut lines = Vec::new();

        if !self.seen_signals.is_empty() {
            // Encode newlines in keys to avoid format confusion
            let signals: Vec<String> = self.seen_signals.iter()
                .map(|s| s.replace('\n', "\\n"))
                .collect();
            lines.push(format!("seen_signal_count: {}", signals.len()));
            for s in &signals {
                lines.push(format!("seen_signal: {}", s));
            }
        }

        if !self.disclosed_thresholds.is_empty() {
            let thresholds: Vec<String> = self.disclosed_thresholds.iter()
                .map(|t| t.to_string())
                .collect();
            lines.push(format!("disclosed_thresholds: {}", thresholds.join(",")));
        }

        if let Some(pct) = self.context_pct {
            lines.push(format!("context_pct: {:.1}", pct));
        }

        lines.push(format!("reply_hint_shown: {}", self.reply_hint_shown));

        if let Some(ref branch) = self.git_branch {
            lines.push(format!("git_branch: {}", branch));
        }
        if let Some(ref head) = self.git_head {
            lines.push(format!("git_head: {}", head));
        }

        lines.push(format!("version: {}", env!("CARGO_PKG_VERSION")));

        lines.join("\n") + "\n"
    }

    /// Deserialize from line-oriented format.
    fn deserialize(content: &str) -> Self {
        let mut state = StateSnapshot::default();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }

            if let Some((key, value)) = line.split_once(": ") {
                match key {
                    "seen_signal" => {
                        state.seen_signals.insert(value.replace("\\n", "\n"));
                    }
                    "disclosed_thresholds" => {
                        state.disclosed_thresholds = value.split(',')
                            .filter_map(|s| s.trim().parse().ok())
                            .collect();
                    }
                    "context_pct" => {
                        state.context_pct = value.parse().ok();
                    }
                    "reply_hint_shown" => {
                        state.reply_hint_shown = value == "true";
                    }
                    "git_branch" => {
                        state.git_branch = Some(value.to_string());
                    }
                    "git_head" => {
                        state.git_head = Some(value.to_string());
                    }
                    _ => {} // ignore unknown keys for forward compat
                }
            }
        }

        state
    }
}

/// A seen-set key is `<scan-dir>:<signal-filename>` (the peers sensor's
/// collision-proof form). The filename part never contains `:` (signal
/// ids are `[A-Za-z0-9_-]` + `.signal`), so the LAST colon splits the
/// key back into its path. Returns `None` for keys that don't parse —
/// those are kept forever rather than mis-pruned.
fn key_to_path(key: &str) -> Option<PathBuf> {
    let (dir, file) = key.rsplit_once(':')?;
    Some(Path::new(dir).join(file))
}

/// State store manages checkpoint/restore for a session.
pub struct StateStore {
    state_dir: PathBuf,
    session_id: Option<String>,
}

/// How long a writer spins for the advisory lock before proceeding
/// without it. Losing the race costs at worst a duplicate notification
/// (at-least-once), never a lost message, so waiting forever would be
/// the wrong trade at a turn boundary.
const LOCK_WAIT: Duration = Duration::from_millis(500);
/// A lock file older than this is a crashed writer's leftover; steal it.
const LOCK_STALE: Duration = Duration::from_secs(5);

impl StateStore {
    pub fn new(session_id: Option<String>) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        Self::new_in(
            PathBuf::from(home).join(".cache").join("attend").join("state"),
            session_id,
        )
    }

    /// Test seam: a store rooted at an explicit directory.
    pub fn new_in(state_dir: PathBuf, session_id: Option<String>) -> Self {
        Self { state_dir, session_id }
    }

    fn state_path(&self) -> Option<PathBuf> {
        self.session_id.as_ref().map(|id| {
            self.state_dir.join(format!("{}.state", id))
        })
    }

    /// Load state for this session without logging. `None` when there is
    /// no session id or no state file — callers distinguish "cold start"
    /// (baseline, don't flood) from "warm" (deliver the unseen).
    pub fn load(&self) -> Option<StateSnapshot> {
        let path = self.state_path()?;
        let content = fs::read_to_string(&path).ok()?;
        Some(StateSnapshot::deserialize(&content))
    }

    /// Try to load existing state for this session, logging the restore.
    pub fn restore(&self) -> Option<StateSnapshot> {
        let state = self.load()?;
        if let Some(path) = self.state_path() {
            eprintln!("[attend] state: restored from {} ({} seen signals, {} disclosed thresholds)",
                path.display(), state.seen_signals.len(), state.disclosed_thresholds.len());
        }
        Some(state)
    }

    /// Checkpoint current state to disk. Atomic write; seen-set is
    /// UNIONED with what's already on disk (ADR-172 Decision 3) so a
    /// long-running sensor's snapshot can never overwrite consumption
    /// marks the drain recorded since the sensor last read the file.
    /// Non-seen fields (thresholds, git, context) are taken from the
    /// caller's snapshot — the sensor loop is their only writer.
    pub fn checkpoint(&self, state: &StateSnapshot) {
        self.write_merged(state, false);
    }

    /// Record consumption of `keys` (the drain's write path). Seen-set
    /// unions like `checkpoint`; every other field is preserved from
    /// disk, because the drain has no sensor state to contribute.
    pub fn mark_seen<I, S>(&self, keys: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let seen: HashSet<String> = keys.into_iter().map(Into::into).collect();
        if seen.is_empty() {
            return;
        }
        let snap = StateSnapshot { seen_signals: seen, ..Default::default() };
        self.write_merged(&snap, true);
    }

    /// Shared read-union-prune-write cycle under the advisory lock.
    /// `fields_from_disk` selects which side's non-seen fields survive.
    fn write_merged(&self, mem: &StateSnapshot, fields_from_disk: bool) {
        let path = match self.state_path() {
            Some(p) => p,
            None => return,
        };
        fs::create_dir_all(&self.state_dir).ok();

        let _lock = LockGuard::acquire(&path.with_extension("state.lock"));

        let disk = fs::read_to_string(&path)
            .ok()
            .map(|c| StateSnapshot::deserialize(&c))
            .unwrap_or_default();

        let mut merged = StateSnapshot {
            seen_signals: HashSet::new(),
            ..if fields_from_disk {
                StateSnapshot {
                    disclosed_thresholds: disk.disclosed_thresholds.clone(),
                    context_pct: disk.context_pct,
                    reply_hint_shown: disk.reply_hint_shown,
                    git_branch: disk.git_branch.clone(),
                    git_head: disk.git_head.clone(),
                    ..Default::default()
                }
            } else {
                StateSnapshot {
                    disclosed_thresholds: mem.disclosed_thresholds.clone(),
                    context_pct: mem.context_pct,
                    reply_hint_shown: mem.reply_hint_shown,
                    git_branch: mem.git_branch.clone(),
                    git_head: mem.git_head.clone(),
                    ..Default::default()
                }
            }
        };

        // Union, then prune keys whose signal file is gone — a deleted
        // signal can never be re-delivered, so its mark is dead weight.
        // Pruning here (the single write path) keeps the file bounded by
        // the live ledger instead of growing for the session's lifetime.
        merged.seen_signals = disk
            .seen_signals
            .union(&mem.seen_signals)
            .filter(|k| key_to_path(k).map(|p| p.exists()).unwrap_or(true))
            .cloned()
            .collect();

        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, merged.serialize()).is_ok() {
            fs::rename(&tmp, &path).ok();
        }
    }

    /// Remove state file (on clean exit if desired).
    #[allow(dead_code)]
    pub fn clear(&self) {
        if let Some(path) = self.state_path() {
            fs::remove_file(&path).ok();
        }
    }
}

/// Read-only consult: the seen-set another session has recorded.
/// `None` when that session has no state file (never checkpointed).
/// This is `/purge`'s seam (ADR-170 tightening, co-shipped with the
/// drain per ADR-172 Decision 5): a live session's unconsumed message
/// must survive a purge.
pub fn seen_keys_for(session_id: &str) -> Option<HashSet<String>> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    seen_keys_for_in(
        &PathBuf::from(home).join(".cache").join("attend").join("state"),
        session_id,
    )
}

/// Test seam for [`seen_keys_for`].
pub fn seen_keys_for_in(state_dir: &Path, session_id: &str) -> Option<HashSet<String>> {
    let content = fs::read_to_string(state_dir.join(format!("{session_id}.state"))).ok()?;
    Some(StateSnapshot::deserialize(&content).seen_signals)
}

/// Advisory create-new lock file with stale-steal. Best-effort by
/// design: if the lock can't be won inside LOCK_WAIT the writer
/// proceeds anyway — the failure mode is a duplicate notification,
/// which at-least-once delivery tolerates; a stuck turn boundary
/// would not be.
struct LockGuard {
    path: Option<PathBuf>,
}

impl LockGuard {
    fn acquire(path: &Path) -> Self {
        let deadline = std::time::Instant::now() + LOCK_WAIT;
        loop {
            match fs::OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => return Self { path: Some(path.to_path_buf()) },
                Err(_) => {
                    // Steal locks left by a crashed writer.
                    let stale = fs::metadata(path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > LOCK_STALE);
                    if stale {
                        fs::remove_file(path).ok();
                        continue;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Self { path: None };
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if let Some(ref p) = self.path {
            fs::remove_file(p).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "attend-state-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Golden wire-format round-trip: the drain, the sensor loop, and
    /// purge's consult all parse this exact shape — a format change
    /// must be a deliberate, versioned decision, not a refactor side
    /// effect.
    #[test]
    fn golden_wire_format_round_trip() {
        let mut snap = StateSnapshot {
            disclosed_thresholds: vec![40, 50],
            context_pct: Some(31.2),
            reply_hint_shown: true,
            git_branch: Some("main".into()),
            git_head: Some("abc1234".into()),
            ..Default::default()
        };
        snap.seen_signals.insert("/tmp/sig:abc-123.signal".into());

        let text = snap.serialize();
        assert!(text.contains("seen_signal: /tmp/sig:abc-123.signal"));
        assert!(text.contains("disclosed_thresholds: 40,50"));
        assert!(text.contains("context_pct: 31.2"));
        assert!(text.contains("reply_hint_shown: true"));
        assert!(text.contains("git_branch: main"));
        assert!(text.contains("git_head: abc1234"));

        let back = StateSnapshot::deserialize(&text);
        assert_eq!(back.seen_signals, snap.seen_signals);
        assert_eq!(back.disclosed_thresholds, snap.disclosed_thresholds);
        assert_eq!(back.context_pct, snap.context_pct);
        assert_eq!(back.reply_hint_shown, snap.reply_hint_shown);
        assert_eq!(back.git_branch, snap.git_branch);
        assert_eq!(back.git_head, snap.git_head);
    }

    /// ADR-172 Decision 3: a sensor checkpoint whose in-memory set
    /// predates a drain mark must not erase that mark.
    #[test]
    fn checkpoint_unions_with_drain_marks_on_disk() {
        let dir = temp_dir("union");
        let sig_dir = dir.join("signals");
        fs::create_dir_all(&sig_dir).unwrap();
        // Both signal files exist, so pruning keeps them.
        fs::write(sig_dir.join("drained.signal"), "x").unwrap();
        fs::write(sig_dir.join("sensor.signal"), "x").unwrap();
        let drained_key = format!("{}:drained.signal", sig_dir.display());
        let sensor_key = format!("{}:sensor.signal", sig_dir.display());

        let store = StateStore::new_in(dir.clone(), Some("s1".into()));
        // Drain marks a message the sensor has never seen.
        store.mark_seen([drained_key.clone()]);
        // Sensor checkpoints a snapshot that lacks the drain's mark.
        let mut mem = StateSnapshot::default();
        mem.seen_signals.insert(sensor_key.clone());
        store.checkpoint(&mem);

        let after = store.load().unwrap();
        assert!(after.seen_signals.contains(&drained_key),
            "sensor checkpoint erased the drain's consumption mark");
        assert!(after.seen_signals.contains(&sensor_key));
        fs::remove_dir_all(&dir).ok();
    }

    /// mark_seen must not clobber sensor-owned fields.
    #[test]
    fn mark_seen_preserves_disk_fields() {
        let dir = temp_dir("fields");
        let sig_dir = dir.join("signals");
        fs::create_dir_all(&sig_dir).unwrap();
        fs::write(sig_dir.join("a.signal"), "x").unwrap();
        let key = format!("{}:a.signal", sig_dir.display());

        let store = StateStore::new_in(dir.clone(), Some("s2".into()));
        let mem = StateSnapshot {
            disclosed_thresholds: vec![40],
            reply_hint_shown: true,
            ..Default::default()
        };
        store.checkpoint(&mem);

        store.mark_seen([key.clone()]);
        let after = store.load().unwrap();
        assert_eq!(after.disclosed_thresholds, vec![40]);
        assert!(after.reply_hint_shown);
        assert!(after.seen_signals.contains(&key));
        fs::remove_dir_all(&dir).ok();
    }

    /// Keys whose signal file is gone are pruned at write time; keys
    /// whose file still exists survive.
    #[test]
    fn write_prunes_keys_for_deleted_signals() {
        let dir = temp_dir("prune");
        let sig_dir = dir.join("signals");
        fs::create_dir_all(&sig_dir).unwrap();
        fs::write(sig_dir.join("live.signal"), "x").unwrap();
        let live_key = format!("{}:live.signal", sig_dir.display());
        let dead_key = format!("{}:gone.signal", sig_dir.display());

        let store = StateStore::new_in(dir.clone(), Some("s3".into()));
        store.mark_seen([live_key.clone(), dead_key.clone()]);
        // dead_key's file never existed → pruned on the next write.
        store.mark_seen([live_key.clone()]);

        let after = store.load().unwrap();
        assert!(after.seen_signals.contains(&live_key));
        assert!(!after.seen_signals.contains(&dead_key),
            "mark for a deleted signal should be pruned");
        fs::remove_dir_all(&dir).ok();
    }

    /// "No session, no persistence": a store with no id never writes.
    #[test]
    fn no_session_id_never_writes() {
        let dir = temp_dir("noid");
        let store = StateStore::new_in(dir.clone(), None);
        store.mark_seen(["x:y.signal".to_string()]);
        store.checkpoint(&StateSnapshot::default());
        assert!(fs::read_dir(&dir).unwrap().next().is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seen_keys_for_reads_other_sessions() {
        let dir = temp_dir("consult");
        let sig_dir = dir.join("signals");
        fs::create_dir_all(&sig_dir).unwrap();
        fs::write(sig_dir.join("m.signal"), "x").unwrap();
        let key = format!("{}:m.signal", sig_dir.display());

        let store = StateStore::new_in(dir.clone(), Some("peer-1".into()));
        store.mark_seen([key.clone()]);

        let seen = seen_keys_for_in(&dir, "peer-1").unwrap();
        assert!(seen.contains(&key));
        assert!(seen_keys_for_in(&dir, "peer-2").is_none());
        fs::remove_dir_all(&dir).ok();
    }
}
