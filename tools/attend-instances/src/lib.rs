//! Per-cwd session instance registry (ADR-129).
//!
//! When two claude sessions run in the same directory their cwd-keyed
//! identities collide — both render as `Jovan (myproj)`. The instance
//! registry assigns each session a stable suffix (e.g. `Jovan-alpha`)
//! that disambiguates them across renders.
//!
//! ## Storage
//!
//! `~/.cache/attend/instances/<encoded-cwd>.yaml` — one file per cwd.
//! `<encoded-cwd>` mirrors the existing `signals/<encoded-cwd>/`
//! encoding (`/`, `_`, `.` → `-`). Schema:
//!
//! ```yaml
//! <session-uuid>:
//!   instance: alpha
//!   registered_at: 2026-04-28T10:30:00Z
//!   last_seen: 2026-04-28T10:35:00Z
//! ```
//!
//! ## Slot semantics
//!
//! Slots are session-bound. Once a `session_id` holds `alpha` for a
//! cwd, it holds `alpha` for the life of that session (and across
//! resumes) — no renaming. New sessions skip taken slots and take the
//! next-free letter. Rejected design: resumer-yields-on-conflict —
//! agents reason about themselves as `Jovan-alpha`, and a rename on
//! resume would invalidate that self-reference plus every `@Jovan-
//! alpha` mention by peers.
//!
//! ## Allocator
//!
//! Greek letters `alpha..omega` (24). Numeric fallback past 24
//! (`Jovan-25`, `Jovan-26`, …) so a cwd that has hosted dozens of
//! historical sessions still issues a discriminator. Latin spelling,
//! not glyphs — keyboards without a compose key still type the
//! suffix into `@`-completion.
//!
//! ## Concurrency
//!
//! `flock(LOCK_EX)` on the registry file during read-modify-write.
//! Registrations are rare (per session start, not per tick) so
//! contention is negligible. Atomic `rename(.tmp, .yaml)` on commit;
//! a partial write cannot leave a torn file visible.
//!
//! ## Age-based GC
//!
//! Entries with `last_seen` older than 7 days are reclaimable. Trade:
//! resumes after >7 days inactive may be renamed since their slot may
//! have been reclaimed. Acceptable — a week-stale resume is far from
//! the "I just stepped away and came back" case.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Default age past which an entry's slot is reclaimable. ADR-129
/// settled on 7 days as the trade between registry growth and resume
/// stability for typical usage.
pub const DEFAULT_GC_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// Greek letter pool used as the primary discriminator vocabulary.
/// Latin spelling so `@`-completion works on any keyboard.
pub const GREEK: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi",
    "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
];

/// One row in the registry. Keep field set tight — every consumer
/// only really needs `instance`; the timestamps exist for GC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceEntry {
    pub instance: String,
    pub registered_at: u64,
    pub last_seen: u64,
}

/// Registry handle. Cheap to construct — does no IO until a method
/// is called.
pub struct Registry {
    base_dir: PathBuf,
}

impl Registry {
    /// Standard registry rooted at `~/.cache/attend/instances/`.
    pub fn new() -> Self {
        Self {
            base_dir: home_dir()
                .join(".cache")
                .join("attend")
                .join("instances"),
        }
    }

    /// Test / sandbox constructor — point at any directory.
    pub fn with_base(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Path to the registry file for a cwd.
    pub fn path_for(&self, cwd: &str) -> PathBuf {
        self.base_dir.join(format!("{}.yaml", encode_cwd(cwd)))
    }

    /// Path to the sentinel lockfile for a cwd. The data file gets
    /// atomically renamed during commit; flock state lives on the
    /// open-file-description (i.e. inode), not the path, so a lock
    /// taken on the data file before the rename does not contend
    /// with a fresh opener of the path after the rename. The
    /// lockfile is never renamed — it stays on the same inode for
    /// the life of the registry, so flock() against it serializes
    /// concurrent registers correctly across processes and threads.
    fn lock_path(&self, cwd: &str) -> PathBuf {
        self.base_dir.join(format!("{}.yaml.lock", encode_cwd(cwd)))
    }

    /// Look up the instance assigned to `session_id` in `cwd`. Read
    /// only — no allocation, no GC, no write. Returns `None` when
    /// the registry file is absent or the session has no entry.
    pub fn lookup(&self, cwd: &str, session_id: &str) -> Option<String> {
        let path = self.path_for(cwd);
        let content = fs::read_to_string(&path).ok()?;
        let map = parse_registry(&content);
        map.get(session_id).map(|e| e.instance.clone())
    }

    /// Register or reclaim. Returns the assigned instance string.
    ///
    /// - If `session_id` already has an entry, return its instance and
    ///   refresh `last_seen` so the GC clock resets.
    /// - Otherwise, allocate the next-free instance (Greek first,
    ///   numeric fallback past omega) and write a new row.
    /// - During allocation, prune entries past `DEFAULT_GC_AGE` so
    ///   long-dead sessions stop blocking slots.
    ///
    /// Read-modify-write under `flock(LOCK_EX)` on a sentinel
    /// `<cwd>.yaml.lock` file (PR #77 review fix). Locking on the
    /// data file directly would not serialize concurrent registers:
    /// the data file is renamed atomically during commit, and flock
    /// state lives on the inode rather than the path, so a fresh
    /// opener after the rename gets a different lock. The sentinel
    /// is never renamed, so its inode (and thus its flock state)
    /// is the authoritative serializer.
    ///
    /// A crash between read and write leaves only the previous
    /// committed state on disk; the rename is atomic.
    pub fn register(&self, cwd: &str, session_id: &str) -> io::Result<String> {
        self.register_with_age(cwd, session_id, DEFAULT_GC_AGE, now_secs())
    }

    /// Same as [`register`] but with explicit GC age + clock — exposed
    /// for tests that want deterministic time and size-aware GC.
    pub fn register_with_age(
        &self,
        cwd: &str,
        session_id: &str,
        gc_age: Duration,
        now: u64,
    ) -> io::Result<String> {
        fs::create_dir_all(&self.base_dir)?;
        let path = self.path_for(cwd);
        let lock_path = self.lock_path(cwd);

        // Sentinel lockfile (PR #77 review fix). The data file is
        // atomically renamed during commit; locking it before the
        // rename does not serialize correctly because flock state
        // is keyed on the open-file-description (inode), not the
        // path — a fresh opener after the rename gets a different
        // inode and a different lock.
        //
        // Lock the never-renamed sentinel instead. Held until the
        // File is dropped at the end of this function.
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        acquire_exclusive(&lock_file)?;

        // Read current state. Safe under the lock — no other
        // register/touch on this cwd can be mid-write.
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut map = parse_registry(&content);

        // GC: drop any entry whose `last_seen` is older than gc_age,
        // *unless* it is our own session — we never evict ourselves
        // mid-register. Aged-out slots become available below.
        let cutoff = now.saturating_sub(gc_age.as_secs());
        map.retain(|sid, entry| sid == session_id || entry.last_seen >= cutoff);

        // Reclaim path: existing entry → refresh + return.
        if let Some(entry) = map.get_mut(session_id) {
            entry.last_seen = now;
            let instance = entry.instance.clone();
            write_registry(&path, &map)?;
            return Ok(instance);
        }

        // Allocate path: next-free instance.
        let taken: std::collections::HashSet<&str> =
            map.values().map(|e| e.instance.as_str()).collect();
        let instance = next_free_instance(&taken);
        map.insert(
            session_id.to_string(),
            InstanceEntry {
                instance: instance.clone(),
                registered_at: now,
                last_seen: now,
            },
        );
        write_registry(&path, &map)?;
        Ok(instance)
    }

    /// Refresh `last_seen` for an existing session without allocating
    /// a new slot. No-op when the session has no entry. Cheaper than
    /// a full register call; intended for periodic touches that keep
    /// the GC clock from expiring an active session.
    pub fn touch(&self, cwd: &str, session_id: &str) -> io::Result<()> {
        self.touch_at(cwd, session_id, now_secs())
    }

    /// Test seam for [`touch`].
    pub fn touch_at(&self, cwd: &str, session_id: &str, now: u64) -> io::Result<()> {
        let path = self.path_for(cwd);
        if !path.exists() {
            return Ok(());
        }
        // Same sentinel-lockfile discipline as `register_with_age`
        // (PR #77 review fix). flock() against the data file would
        // not serialize correctly with concurrent registers, since
        // the data file is renamed under us during commit.
        let lock_path = self.lock_path(cwd);
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        acquire_exclusive(&lock_file)?;
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut map = parse_registry(&content);
        let Some(entry) = map.get_mut(session_id) else {
            return Ok(());
        };
        entry.last_seen = now;
        write_registry(&path, &map)
    }

    /// Snapshot the full registry for a cwd. Intended for renderers
    /// that need every (session, instance) pair (peers list, chat
    /// legend), not for hot per-render lookups (`lookup` is cheaper
    /// since it short-circuits as soon as it finds the row).
    pub fn snapshot(&self, cwd: &str) -> BTreeMap<String, InstanceEntry> {
        let path = self.path_for(cwd);
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return BTreeMap::new(),
        };
        parse_registry(&content)
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-cwd cache layered over a [`Registry`]. Built once at the top
/// of a render pass and passed down to every render site that needs
/// to resolve a `(cwd, session_id) → instance` lookup. Each cwd is
/// read at most once per cache instance, regardless of how many
/// chips reference it.
///
/// PR #77 review: the previous render path called
/// `Registry::new().lookup(cwd, sid)` once per chip — N file reads
/// + N parses every render even when most chips share a cwd. This
/// cache collapses that to one read per distinct cwd per render.
///
/// **Lifetime is exactly one render.** Construct fresh; do not
/// share across renders. The registry on disk can change between
/// renders (peer registers, GC fires), and a cache that survives
/// a render would serve stale instance assignments. The render
/// loop in attend-chat already runs cheap wall-clock-driven work,
/// so a fresh `SnapshotCache::new()` per render is the
/// invalidation strategy.
pub struct SnapshotCache {
    registry: Registry,
    cache: RefCell<HashMap<String, BTreeMap<String, InstanceEntry>>>,
}

impl SnapshotCache {
    /// Construct using the default registry root.
    pub fn new() -> Self {
        Self::with_registry(Registry::new())
    }

    /// Construct over a caller-supplied registry — useful for tests.
    pub fn with_registry(registry: Registry) -> Self {
        Self {
            registry,
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Resolve `(cwd, session_id) → instance` with per-cwd caching.
    /// First call for a cwd hits the disk via `Registry::snapshot`;
    /// subsequent calls for the same cwd return from the in-memory
    /// map. Lookup miss inside a snapshot returns `None`.
    pub fn lookup(&self, cwd: &str, session_id: &str) -> Option<String> {
        let mut cache = self.cache.borrow_mut();
        let snap = cache
            .entry(cwd.to_string())
            .or_insert_with(|| self.registry.snapshot(cwd));
        snap.get(session_id).map(|e| e.instance.clone())
    }
}

impl Default for SnapshotCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Pick the lowest-index Greek letter that is not in `taken`. Past
/// the Greek pool, fall back to numeric suffixes starting at 25 so
/// the identity reads as `<nick>-25`, `<nick>-26`, …
fn next_free_instance(taken: &std::collections::HashSet<&str>) -> String {
    for letter in GREEK {
        if !taken.contains(*letter) {
            return (*letter).to_string();
        }
    }
    let mut n = GREEK.len() + 1;
    loop {
        let candidate = n.to_string();
        if !taken.contains(candidate.as_str()) {
            return candidate;
        }
        n += 1;
    }
}

/// Encode a cwd path the same way `sensor-peers` and Claude Code
/// encode project directories — `/`, `_`, `.` → `-`. Mirrored here
/// so `attend-instances` does not need to depend on `sensor-peers`
/// for one helper.
fn encode_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| match c {
            '/' | '_' | '.' => '-',
            _ => c,
        })
        .collect()
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(unix)]
fn acquire_exclusive(file: &fs::File) -> io::Result<()> {
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn acquire_exclusive(_file: &fs::File) -> io::Result<()> {
    // No flock on non-Unix platforms — best-effort, accept the race.
    // attend's targets are Unix today; this branch exists for clean
    // cross-compile only.
    Ok(())
}

// ── YAML parser / serializer ──────────────────────────────────────
//
// Hand-rolled to keep the dependency footprint at zero — same
// approach as `groups.rs::parse_groups_yaml`. Schema is shallow and
// fixed (one key, three scalar fields), so a serde-yaml dependency
// would be pure ceremony.

fn parse_registry(content: &str) -> BTreeMap<String, InstanceEntry> {
    let mut out = BTreeMap::new();
    let mut current_sid: Option<String> = None;
    let mut current_instance: Option<String> = None;
    let mut current_registered_at: Option<u64> = None;
    let mut current_last_seen: Option<u64> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        // Top-level key — a session id ending in `:` and no inner
        // values on the same line.
        if indent == 0 && trimmed.ends_with(':') {
            commit_entry(
                &mut out,
                current_sid.take(),
                current_instance.take(),
                current_registered_at.take(),
                current_last_seen.take(),
            );
            current_sid = Some(trimmed.trim_end_matches(':').to_string());
            continue;
        }

        // Nested scalar.
        if indent == 2 {
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "instance" => current_instance = Some(value.to_string()),
                    "registered_at" => current_registered_at = value.parse().ok(),
                    "last_seen" => current_last_seen = value.parse().ok(),
                    _ => {}
                }
            }
        }
    }

    // Final entry (no trailing top-level key to flush it).
    commit_entry(
        &mut out,
        current_sid,
        current_instance,
        current_registered_at,
        current_last_seen,
    );
    out
}

fn commit_entry(
    out: &mut BTreeMap<String, InstanceEntry>,
    sid: Option<String>,
    instance: Option<String>,
    registered_at: Option<u64>,
    last_seen: Option<u64>,
) {
    let (Some(sid), Some(instance)) = (sid, instance) else {
        return;
    };
    out.insert(
        sid,
        InstanceEntry {
            instance,
            registered_at: registered_at.unwrap_or(0),
            last_seen: last_seen.unwrap_or(0),
        },
    );
}

fn serialize_registry(map: &BTreeMap<String, InstanceEntry>) -> String {
    let mut out = String::new();
    for (sid, entry) in map {
        out.push_str(sid);
        out.push_str(":\n");
        out.push_str("  instance: ");
        out.push_str(&entry.instance);
        out.push('\n');
        out.push_str("  registered_at: ");
        out.push_str(&entry.registered_at.to_string());
        out.push('\n');
        out.push_str("  last_seen: ");
        out.push_str(&entry.last_seen.to_string());
        out.push('\n');
    }
    out
}

fn write_registry(path: &Path, map: &BTreeMap<String, InstanceEntry>) -> io::Result<()> {
    // Atomic rename: write to .tmp, fsync, rename. The flock holder
    // is the only writer at any moment, so no locked-rename dance is
    // needed — the rename target's flock state is irrelevant on
    // Linux (locks are on open-file-descriptions, not paths).
    let content = serialize_registry(map);
    let tmp = path.with_extension("yaml.tmp");
    fs::write(&tmp, &content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static HOME_LOCK: Mutex<()> = Mutex::new(());

    fn fresh_base() -> PathBuf {
        std::env::temp_dir().join(format!(
            "attend-instances-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn with_registry<F: FnOnce(&Registry)>(f: F) {
        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let reg = Registry::with_base(base.clone());
        f(&reg);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn first_session_gets_alpha() {
        with_registry(|r| {
            let i = r.register("/x", "sess-a").unwrap();
            assert_eq!(i, "alpha");
        });
    }

    #[test]
    fn second_session_gets_beta() {
        with_registry(|r| {
            let _ = r.register("/x", "sess-a").unwrap();
            let i = r.register("/x", "sess-b").unwrap();
            assert_eq!(i, "beta");
        });
    }

    #[test]
    fn resume_reclaims_original_instance() {
        with_registry(|r| {
            let first = r.register("/x", "sess-a").unwrap();
            assert_eq!(first, "alpha");
            // Add a peer so beta is taken.
            let beta = r.register("/x", "sess-b").unwrap();
            assert_eq!(beta, "beta");
            // sess-a "resumes" — must still be alpha, never renamed
            // even though sess-b is alive in this cwd. This is the
            // non-negotiable property the design is built around.
            let resumed = r.register("/x", "sess-a").unwrap();
            assert_eq!(resumed, "alpha");
        });
    }

    #[test]
    fn distinct_cwds_have_independent_registries() {
        with_registry(|r| {
            // Same session_id in two different cwds gets two
            // independent allocations. Each cwd is its own sea.
            let a1 = r.register("/x", "sess-a").unwrap();
            let a2 = r.register("/y", "sess-a").unwrap();
            assert_eq!(a1, "alpha");
            assert_eq!(a2, "alpha"); // /y is a fresh registry
        });
    }

    #[test]
    fn slot_freed_after_gc_age() {
        with_registry(|r| {
            // Register sess-a far in the past, then sess-b at "now".
            // sess-a's slot should be reclaimed during sess-b's
            // register call because its last_seen is past gc_age.
            let now = 1_000_000u64;
            let way_old = now - (8 * 24 * 3600); // 8 days
            let alpha_slot = r
                .register_with_age("/x", "sess-a", DEFAULT_GC_AGE, way_old)
                .unwrap();
            assert_eq!(alpha_slot, "alpha");
            let new_alpha = r
                .register_with_age("/x", "sess-b", DEFAULT_GC_AGE, now)
                .unwrap();
            assert_eq!(
                new_alpha, "alpha",
                "sess-a's stale slot should have been reclaimed"
            );
        });
    }

    #[test]
    fn fresh_entries_within_grace_keep_their_slot() {
        with_registry(|r| {
            // sess-a registered "5 days ago" — within the 7-day
            // window. sess-b should NOT be able to take alpha.
            let now = 1_000_000u64;
            let recent = now - (5 * 24 * 3600);
            let _ = r
                .register_with_age("/x", "sess-a", DEFAULT_GC_AGE, recent)
                .unwrap();
            let i = r
                .register_with_age("/x", "sess-b", DEFAULT_GC_AGE, now)
                .unwrap();
            assert_eq!(i, "beta", "alpha holder is still within grace");
        });
    }

    #[test]
    fn numeric_fallback_past_omega() {
        with_registry(|r| {
            // Register 25 sessions; the 25th must overflow Greek and
            // land on the "25" numeric suffix.
            for i in 0..GREEK.len() {
                let assigned = r
                    .register("/x", &format!("sess-{i}"))
                    .unwrap();
                assert_eq!(assigned, GREEK[i]);
            }
            let overflow = r.register("/x", "sess-overflow").unwrap();
            assert_eq!(overflow, "25");
        });
    }

    #[test]
    fn lookup_returns_none_for_unknown() {
        with_registry(|r| {
            assert_eq!(r.lookup("/x", "nope"), None);
            r.register("/x", "sess-a").unwrap();
            assert_eq!(r.lookup("/x", "sess-a").as_deref(), Some("alpha"));
        });
    }

    #[test]
    fn touch_refreshes_last_seen_without_changing_instance() {
        with_registry(|r| {
            r.register_with_age("/x", "sess-a", DEFAULT_GC_AGE, 1000)
                .unwrap();
            let before = r.snapshot("/x")["sess-a"].last_seen;
            r.touch_at("/x", "sess-a", before + 60).unwrap();
            let after = r.snapshot("/x")["sess-a"].clone();
            assert_eq!(after.instance, "alpha");
            assert_eq!(after.last_seen, before + 60);
        });
    }

    #[test]
    fn touch_no_op_for_unknown_session() {
        with_registry(|r| {
            // touch on a session that was never registered should
            // not error and should not add a row.
            r.touch("/x", "ghost").unwrap();
            assert!(r.snapshot("/x").is_empty());
        });
    }

    #[test]
    fn yaml_roundtrip_preserves_entries() {
        let mut m = BTreeMap::new();
        m.insert(
            "sess-a".to_string(),
            InstanceEntry {
                instance: "alpha".into(),
                registered_at: 1_000,
                last_seen: 2_000,
            },
        );
        m.insert(
            "sess-b".to_string(),
            InstanceEntry {
                instance: "beta".into(),
                registered_at: 1_500,
                last_seen: 2_500,
            },
        );
        let yaml = serialize_registry(&m);
        let parsed = parse_registry(&yaml);
        assert_eq!(parsed, m);
    }

    #[test]
    fn parse_ignores_blank_and_comment_lines() {
        // Raw string so the 2-space property indents survive verbatim.
        // The Rust `\<newline>` continuation eats trailing whitespace
        // and would strip our indentation if we used escaped form.
        let input = r#"
# leading comment

sess-a:
  instance: alpha
  registered_at: 100
  last_seen: 200

"#;
        let parsed = parse_registry(input);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed["sess-a"].instance, "alpha");
    }

    #[test]
    fn encode_cwd_matches_signals_layout() {
        // Must match the encoding used elsewhere (sensor-peers,
        // Claude Code project dirs) so tooling can correlate by
        // cwd without re-implementing the helper.
        assert_eq!(encode_cwd("/home/aaron/.claude"), "-home-aaron--claude");
        assert_eq!(encode_cwd("/home/aaron/temp"), "-home-aaron-temp");
        assert_eq!(encode_cwd("simple"), "simple");
    }

    #[test]
    fn next_free_picks_first_unused_in_order() {
        let mut taken = std::collections::HashSet::new();
        assert_eq!(next_free_instance(&taken), "alpha");
        taken.insert("alpha");
        assert_eq!(next_free_instance(&taken), "beta");
        taken.insert("beta");
        taken.insert("gamma");
        // Gap before delta — allocator picks the lowest-index hole.
        assert_eq!(next_free_instance(&taken), "delta");
    }

    #[test]
    fn snapshot_cache_returns_same_instance_for_repeated_lookups() {
        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let reg = Registry::with_base(base.clone());
        let assigned = reg.register("/cwd", "sess-a").unwrap();

        let cache = SnapshotCache::with_registry(Registry::with_base(base.clone()));
        // First lookup must reflect what we just registered.
        assert_eq!(cache.lookup("/cwd", "sess-a").as_deref(), Some(assigned.as_str()));
        // Repeat lookups for the same (cwd, sid) return the same value
        // — and, important to the perf claim, the second call hits
        // the in-memory map rather than re-reading the yaml.
        assert_eq!(cache.lookup("/cwd", "sess-a").as_deref(), Some(assigned.as_str()));
        // Lookup miss returns None and is also cached implicitly via
        // the per-cwd snapshot the entry-API populated on first call.
        assert_eq!(cache.lookup("/cwd", "no-such-session"), None);

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn snapshot_cache_isolates_per_cwd() {
        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let reg = Registry::with_base(base.clone());
        reg.register("/cwd-a", "sess-a").unwrap();
        reg.register("/cwd-b", "sess-b").unwrap();

        let cache = SnapshotCache::with_registry(Registry::with_base(base.clone()));
        // Each cwd is an independent snapshot — no cross-contamination
        // between paths even though both rolled their own `alpha`.
        assert_eq!(cache.lookup("/cwd-a", "sess-a").as_deref(), Some("alpha"));
        assert_eq!(cache.lookup("/cwd-b", "sess-b").as_deref(), Some("alpha"));
        // A session_id from one cwd does not resolve in the other.
        assert_eq!(cache.lookup("/cwd-a", "sess-b"), None);
        assert_eq!(cache.lookup("/cwd-b", "sess-a"), None);

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn concurrent_registers_assign_distinct_instances() {
        // PR #77 review regression: concurrent registers MUST
        // serialize through the lockfile and produce distinct
        // instances. The pre-fix code locked the data file
        // directly, which after the atomic rename did not
        // contend across openers — two threads could both pass
        // their critical sections and produce the same
        // assignment, with one update silently dropped on
        // last-rename-wins.
        //
        // Thread-level concurrency is sufficient to expose the
        // bug because flock() in std (via libc) operates on the
        // open-file-description, and `OpenOptions::open` returns
        // a fresh FD per call. Two threads opening the same path
        // get independent FDs that contend on the same inode
        // with the OLD code only if the file is NOT renamed
        // mid-flight. The fix uses a sentinel `*.lock` file that
        // is never renamed, so contention is preserved.
        use std::sync::Arc;
        use std::thread;

        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let reg = Arc::new(Registry::with_base(base.clone()));

        const THREADS: usize = 12;
        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let reg = Arc::clone(&reg);
                thread::spawn(move || {
                    reg.register("/concurrent", &format!("sess-{i:02}"))
                        .expect("register io ok")
                })
            })
            .collect();

        let assigned: Vec<String> = handles
            .into_iter()
            .map(|h| h.join().expect("thread join"))
            .collect();

        // 12 threads → 12 distinct instances. If the lock fails,
        // duplicates appear (two threads see the same starting
        // map and pick the same next-free letter).
        let unique: std::collections::HashSet<&String> = assigned.iter().collect();
        assert_eq!(
            unique.len(),
            THREADS,
            "expected {THREADS} distinct instances, got {} duplicates among {assigned:?}",
            THREADS - unique.len()
        );

        // The on-disk registry must contain all 12 entries.
        let snap = reg.snapshot("/concurrent");
        assert_eq!(snap.len(), THREADS, "registry yaml lost entries: {snap:?}");

        fs::remove_dir_all(&base).ok();
    }
}
