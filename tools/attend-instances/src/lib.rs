// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Per-cwd session instance registry (ADR-129).
//!
//! When two claude sessions run in the same directory their cwd-keyed
//! identities collide — both render as `Jovan (myproj)`. The instance
//! registry gives each live session in a directory a name of its own:
//! the first is bare (`Jovan`), and a second one alongside it gets a
//! suffix (`Jovan-alpha`).
//!
//! ## Storage
//!
//! `<attend cache>/instances/<attend-key>.yaml` — one file per cwd, named
//! by `claude_sessions::attend_key` like the cwd's signal tray. Schema:
//!
//! ```yaml
//! <bare-session-uuid>:
//!   instance:
//!   registered_at: 2026-04-28T10:20:00Z
//!   last_seen: 2026-04-28T10:35:00Z
//! <session-uuid>:
//!   instance: alpha
//!   registered_at: 2026-04-28T10:30:00Z
//!   last_seen: 2026-04-28T10:35:00Z
//! <earlier-session-uuid>:
//!   instance: alpha
//!   registered_at: 2026-04-28T09:00:00Z
//!   last_seen: 2026-04-28T10:29:00Z
//!   alias_of: <session-uuid>
//! <ended-session-uuid>:
//!   instance: beta
//!   registered_at: 2026-04-28T08:00:00Z
//!   last_seen: 2026-04-28T08:40:00Z
//!   retired: true
//! ```
//!
//! The bare name is an `instance` line with an empty value; in memory it
//! is the empty string ([`BARE`]). A release that predates the bare name
//! (attend v0.36.0) parses that line as an empty instance and writes it
//! back unchanged on its minute refresh, so the row survives a sensor host
//! still running the old binary. That release renders the session as
//! `Jovan-` until it restarts. A row with no `instance` line is no row,
//! as before.
//!
//! ## Slot semantics
//!
//! Only a live session holds a name; liveness is
//! [`attend_presence::alive`], over a fresh process scan, at the moment
//! a name is handed out. A session keeps its name for its whole life, and
//! across `/clear` ([`Registry::rename`]). A new session takes the lowest
//! name no live session holds, bare first, so when the bare session ends
//! while `-alpha` runs, the next new session is bare. A resumed session
//! keeps its recorded name unless another live session holds it, and then
//! takes the lowest free name. The newcomer yields, never the live holder:
//! agents reason about themselves as `Jovan-alpha`, and a rename of a live
//! session would invalidate that self-reference plus every `@Jovan-alpha`
//! mention by peers.
//!
//! ## Aliases
//!
//! `/clear` gives a running session a new id. [`Registry::rename`] moves
//! the slot to the new id and leaves the old id's row as an alias: it
//! carries the new id's instance, so the messages sent under the old id
//! read as the live session's name, and `alias_of` names the id that took
//! the slot. An alias is not a slot. It blocks no name in allocation, and a
//! session resumed under an alias id is allocated as any resume is. Aliases
//! age out with the same GC as slots. An alias whose chain reaches no slot
//! in its cwd keeps reading the name it read.
//!
//! ## Retired rows
//!
//! When a name goes to a new session, the rows of ended sessions that held
//! it are retired, not dropped: a retired row keeps the name its session
//! held, so that session's earlier messages still read that name, and it
//! holds no slot. A retired row blocks no name, and a resume under its id
//! takes the name back when no live session holds it.
//!
//! ## Allocator
//!
//! The bare name first, then Greek letters `alpha..omega` (24), then a
//! numeric fallback (`Jovan-25`, `Jovan-26`, …) so a directory with dozens
//! of live sessions still issues a discriminator. Latin spelling, not
//! glyphs — keyboards without a compose key still type the suffix into
//! `@`-completion.
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
//! Entries with `last_seen` older than 7 days are removed, unless their
//! session is still live: a laptop suspended for over a week wakes with
//! every row stale, and a running session keeps its row and its name. The
//! liveness scan runs only when a row is past the cutoff.
//!
//! ## File size note
//!
//! Total file length crosses the 800-line code-quality threshold, but
//! production code is ~400 lines (this section ends near `mod tests`);
//! the remainder is the `#[cfg(test)]` block. Issue #78 left this as-is
//! deliberately rather than mechanically splitting tests into a
//! sibling file.


pub mod view;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use agent_settings::writer::Lock;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;


/// Default age past which an entry's slot is reclaimable. ADR-129
/// settled on 7 days as the trade between registry growth and resume
/// stability for typical usage.
pub const DEFAULT_GC_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// The instance of the session that holds the bare name: the first live
/// session in a directory shows as its nickname alone.
pub const BARE: &str = "";

/// Greek letter pool, handed out after the bare name.
/// Latin spelling so `@`-completion works on any keyboard.
pub const GREEK: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi",
    "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
];

/// The name a session in a directory wears: its nickname alone when it
/// holds the bare name or has no row, else `nickname-instance`. Every
/// surface that shows a session's name builds it here.
pub fn display_name(nickname: &str, instance: Option<&str>) -> String {
    match instance {
        Some(inst) if inst != BARE => format!("{nickname}-{inst}"),
        _ => nickname.to_string(),
    }
}

/// One row in the registry. Keep field set tight — every consumer
/// only really needs `instance`; the timestamps exist for GC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceEntry {
    /// The suffix, or [`BARE`] for the session that shows as its nickname
    /// alone.
    pub instance: String,
    pub registered_at: u64,
    pub last_seen: u64,
    /// The id that took this row's slot when the session's id changed
    /// (`/clear`). Set, the row is an alias and holds no slot.
    pub alias_of: Option<String>,
    /// The row of an ended session whose name went to a new session. It
    /// keeps the name for that session's earlier messages and holds no
    /// slot.
    pub retired: bool,
}

impl InstanceEntry {
    fn is_slot(&self) -> bool {
        self.alias_of.is_none() && !self.retired
    }
}

/// Which sessions hold their names when one is handed out.
#[derive(Clone)]
enum Liveness {
    /// [`attend_presence::alive`] over a process scan taken for each
    /// registry call: the process a Claude Code session record names still
    /// runs, judged with the record's start time so a pid a later process
    /// took does not count ([`attend_presence::sessions_still_running`]),
    /// or the session listens in a process that runs, or its heartbeat is
    /// fresh. GC spares a live row, so a reused pid counted as live would
    /// hold a killed session's name for as long as that process ran.
    Presence,
    /// A caller's predicate over session ids.
    Given(Arc<dyn Fn(&str) -> bool + Send + Sync>),
}

/// Whether a session id is live, as judged for one registry call.
type Predicate<'a> = Box<dyn Fn(&str) -> bool + 'a>;

impl Liveness {
    /// The predicate for one registry call. The process scan is taken once,
    /// here, under the registry lock.
    fn judge(&self) -> Predicate<'_> {
        match self {
            Liveness::Presence => {
                let running: HashSet<String> = attend_presence::sessions_still_running().into_iter().collect();
                Box::new(move |id| attend_presence::alive(id, &running))
            }
            Liveness::Given(live) => Box::new(move |id| live(id)),
        }
    }
}

/// [`Liveness`] for one registry call, judged on first use: a call that
/// never asks scans nothing, and one that asks twice scans once.
struct Judge<'a> {
    liveness: &'a Liveness,
    judged: std::cell::OnceCell<Predicate<'a>>,
}

impl<'a> Judge<'a> {
    fn new(liveness: &'a Liveness) -> Self {
        Self { liveness, judged: std::cell::OnceCell::new() }
    }

    fn live(&self, id: &str) -> bool {
        (self.judged.get_or_init(|| self.liveness.judge()))(id)
    }
}

/// Which call reads or writes a session's row: a sensor host's start, or
/// its minute refresh.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// [`Registry::register`]: a session starts or resumes. It keeps the
    /// name its row records unless another live session holds it.
    Register,
    /// [`Registry::keep`]: a running session refreshes its row. An alias
    /// stays an alias, the name it holds is never contested, and a missing
    /// row comes back under the name the host holds when it is free.
    Keep,
}

/// Registry handle. Cheap to construct — does no IO until a method
/// is called.
pub struct Registry {
    base_dir: PathBuf,
    live: Liveness,
}

impl Registry {
    /// Standard registry rooted at `<attend cache>/instances/`.
    pub fn new() -> Self {
        Self::with_base(attend_presence::cache::dir().join("instances"))
    }

    /// Test / sandbox constructor — point at any directory. Liveness is
    /// [`attend_presence::alive`], as in [`new`](Self::new).
    pub fn with_base(base_dir: PathBuf) -> Self {
        Self { base_dir, live: Liveness::Presence }
    }

    /// A registry at `base_dir` that holds a session live when `live` says
    /// so, in place of the process scan. For tests that start and end
    /// sessions without processes.
    pub fn with_liveness(base_dir: PathBuf, live: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        Self { base_dir, live: Liveness::Given(Arc::new(live)) }
    }

    /// Path to the registry file for a cwd, named by
    /// `claude_sessions::attend_key`: the project slug and a hash of the
    /// path, so two projects with one slug never share a name pool.
    pub fn path_for(&self, cwd: &str) -> PathBuf {
        self.base_dir.join(format!("{}.yaml", claude_sessions::attend_key(cwd)))
    }

    /// Look up the instance assigned to `session_id` in `cwd`. Read
    /// only — no allocation, no GC, no write. Returns `None` when
    /// the registry file is absent or the session has no entry, and
    /// `Some(BARE)` for the session that holds the bare name.
    pub fn lookup(&self, cwd: &str, session_id: &str) -> Option<String> {
        let content = fs::read_to_string(self.path_for(cwd)).ok()?;
        let map = parse_registry(&content);
        map.get(session_id).map(|e| e.instance.clone())
    }

    /// Register or reclaim. Returns the assigned instance string, [`BARE`]
    /// for the bare name.
    ///
    /// - If `session_id` holds a slot whose name no other live session
    ///   holds, return its instance and refresh `last_seen` so the GC
    ///   clock resets. This is a resume, a `/clear`'s new host, or a host
    ///   restarting under a running session.
    /// - Otherwise, allocate the lowest name no live session holds (bare,
    ///   then Greek, then numeric) and write a new row. An alias row under
    ///   `session_id` (a resume of an id that `/clear` moved on) is
    ///   replaced; the resume keeps the name it read when no live session
    ///   holds that name.
    /// - Prune entries past `DEFAULT_GC_AGE` whose session is not live.
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
        self.register_with_age(cwd, session_id, DEFAULT_GC_AGE, agent_fmt::when::now_secs())
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
        self.upsert(cwd, session_id, gc_age, now, Pass::Register, None).map(Option::unwrap_or_default)
    }

    /// The periodic refresh of `session_id`'s slot in `cwd`, which a sensor
    /// host runs every minute: [`register`](Self::register), except that a
    /// row that is an alias stays an alias and nothing is written. After
    /// `/clear` the new host renames the old id's slot to the new id
    /// ([`rename`](Self::rename)); the old host, still running until the mod
    /// ends it, must not then take a fresh slot for the old id and undo the
    /// rename. A slot keeps its name, whoever else holds it: a running
    /// session is never renamed. `held` is the name the host was given;
    /// when the row is gone (a writer dropped it) or retired, the session
    /// takes `held` back unless a live session holds it, and only then the
    /// lowest free name. Returns the instance held, or `None` for an alias.
    pub fn keep(&self, cwd: &str, session_id: &str, held: Option<&str>) -> io::Result<Option<String>> {
        self.upsert(cwd, session_id, DEFAULT_GC_AGE, agent_fmt::when::now_secs(), Pass::Keep, held)
    }

    /// [`register_with_age`](Self::register_with_age) and [`keep`](Self::keep),
    /// as `pass` says. `held` is the name `keep`'s host was given.
    fn upsert(&self, cwd: &str, session_id: &str, gc_age: Duration, now: u64, pass: Pass, held: Option<&str>) -> io::Result<Option<String>> {
        fs::create_dir_all(&self.base_dir)?;
        let path = self.path_for(cwd);

        // Sentinel lockfile (PR #77 review fix). The data file is
        // atomically renamed during commit; locking it before the
        // rename does not serialize correctly because flock state
        // is keyed on the open-file-description (inode), not the
        // path — a fresh opener after the rename gets a different
        // inode and a different lock.
        //
        // Lock the never-renamed sentinel instead. Held until the
        // File is dropped at the end of this function.
        let lock_file = Lock::acquire_kept(&path)?;

        // Read current state. Safe under the lock — no other
        // register/touch on this cwd can be mid-write.
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut map = parse_registry(&content);
        if pass == Pass::Keep && map.get(session_id).is_some_and(|e| e.alias_of.is_some()) {
            return Ok(None);
        }
        let judge = Judge::new(&self.live);

        // GC: drop any entry whose `last_seen` is older than gc_age,
        // *unless* it is our own session — we never evict ourselves
        // mid-register — or a session still live, whose host has not
        // refreshed it (a suspended machine). Liveness is judged only
        // for the rows past the cutoff.
        let cutoff = now.saturating_sub(gc_age.as_secs());
        map.retain(|sid, entry| sid == session_id || entry.last_seen >= cutoff || judge.live(sid));

        // Reclaim path: existing slot → refresh + return. The
        // migration sweep runs here too (PR #395 finding 4): every
        // register after the first takes this branch, and skipping
        // the sweep would leave stale siblings until 7-day GC. A
        // register whose name another live session holds (a resume
        // while a newcomer took the name) falls through and yields.
        let reclaim = map.get(session_id).filter(|e| e.is_slot()).map(|e| e.instance.clone());
        if let Some(instance) = reclaim.filter(|inst| pass == Pass::Keep || !held_elsewhere(&map, session_id, inst, &judge)) {
            if let Some(entry) = map.get_mut(session_id) {
                entry.last_seen = now;
            }
            write_registry(&path, &map)?;
            drop(lock_file);
            self.evict_elsewhere(cwd, session_id);
            return Ok(Some(instance));
        }

        let instance = allocate(&mut map, session_id, now, held, &judge);
        write_registry(&path, &map)?;
        drop(lock_file);

        // Migration, not minting (issue #394): one session record is
        // one persona, so an origin change (a worktree hop before
        // origins were normalized, or any future legitimate move)
        // must not leave a sibling entry rendering as a phantom agent
        // in the old cwd's registry until GC. Best-effort by design —
        // a failed eviction is exactly the pre-existing GC-bounded
        // staleness, never worse.
        self.evict_elsewhere(cwd, session_id);
        Ok(Some(instance))
    }

    /// Move `old`'s slot in `cwd` to `new`, for a session whose id changed
    /// under the same process (`/clear`). `new` takes `old`'s instance
    /// name, or keeps its own when it already holds a slot. `old` stays as
    /// an alias of `new` with `new`'s instance until GC, so the messages it
    /// sent read as the live session's name. Does nothing when `old` holds
    /// no slot: only a sensor host registers. Returns `new`'s instance when
    /// it has one.
    pub fn rename(&self, cwd: &str, old: &str, new: &str) -> io::Result<Option<String>> {
        fs::create_dir_all(&self.base_dir)?;
        let path = self.path_for(cwd);
        let _lock = Lock::acquire_kept(&path)?;
        let mut map = parse_registry(&fs::read_to_string(&path).unwrap_or_default());
        let held = |map: &BTreeMap<String, InstanceEntry>, id: &str| {
            map.get(id).filter(|e| e.is_slot()).map(|e| e.instance.clone())
        };
        if old == new || held(&map, old).is_none() {
            return Ok(held(&map, new));
        }
        let instance = match held(&map, new) {
            Some(instance) => instance,
            None => {
                detach(&mut map, new);
                let mut slot = map[old].clone();
                slot.last_seen = agent_fmt::when::now_secs();
                let instance = slot.instance.clone();
                map.insert(new.to_string(), slot);
                instance
            }
        };
        if let Some(alias) = map.get_mut(old) {
            alias.alias_of = Some(new.to_string());
        }
        relabel_followers(&mut map, new, &instance);
        write_registry(&path, &map)?;
        Ok(Some(instance))
    }

    /// The session that holds the slot `session_id` gave up, following
    /// `alias_of` through every `/clear` since. `None` when `session_id`
    /// is no alias in `cwd`.
    pub fn successor(&self, cwd: &str, session_id: &str) -> Option<String> {
        chain_end(&self.snapshot(cwd), session_id)
    }

    /// Remove `session_id` from every registry file other than
    /// `keep_cwd`'s. Each file is edited under its own sentinel lock,
    /// mirroring `register`/`touch` discipline.
    fn evict_elsewhere(&self, keep_cwd: &str, session_id: &str) {
        let keep = self.path_for(keep_cwd);
        let Ok(entries) = fs::read_dir(&self.base_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path == keep
                || path.extension().and_then(|e| e.to_str()) != Some("yaml")
            {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            if !content.contains(session_id) {
                continue;
            }
            let Ok(_lock) = Lock::acquire_kept(&path) else {
                continue;
            };
            // Re-read under the lock; the pre-check above was only a
            // cheap filter.
            let content = fs::read_to_string(&path).unwrap_or_default();
            let mut map = parse_registry(&content);
            if detach(&mut map, session_id).is_some() {
                write_registry(&path, &map).ok();
            }
        }
    }

    /// Refresh `last_seen` for an existing session without allocating
    /// a new slot. No-op when the session has no entry. Cheaper than
    /// a full register call; intended for periodic touches that keep
    /// the GC clock from expiring an active session.
    pub fn touch(&self, cwd: &str, session_id: &str) -> io::Result<()> {
        self.touch_at(cwd, session_id, agent_fmt::when::now_secs())
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
        let _lock = Lock::acquire_kept(&path)?;
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
        let content = match fs::read_to_string(self.path_for(cwd)) {
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
/// plus N parses every render even when most chips share a cwd. This
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

    /// [`Registry::successor`], from the cached snapshot of `cwd`.
    pub fn successor(&self, cwd: &str, session_id: &str) -> Option<String> {
        let mut cache = self.cache.borrow_mut();
        let snap = cache.entry(cwd.to_string()).or_insert_with(|| self.registry.snapshot(cwd));
        chain_end(snap, session_id)
    }
}

impl Default for SnapshotCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a live session other than `session_id` holds a slot named
/// `instance` in `map`. Liveness is judged only when another slot carries
/// the name.
fn held_elsewhere(map: &BTreeMap<String, InstanceEntry>, session_id: &str, instance: &str, judge: &Judge) -> bool {
    map.iter().any(|(id, e)| id != session_id && e.is_slot() && e.instance == instance && judge.live(id))
}

/// Give `session_id` a slot of its own: the name its row recorded, or else
/// `held` (the name its host was given), when no live session holds it (a
/// resume, or a row a writer dropped); otherwise the lowest name no live
/// session holds, bare first. The row under `session_id`, slot, alias or
/// retired, is detached first. The slots of ended sessions that held the
/// name handed out are retired: they keep the name for those sessions'
/// earlier messages and hold no slot.
fn allocate(map: &mut BTreeMap<String, InstanceEntry>, session_id: &str, now: u64, held: Option<&str>, judge: &Judge) -> String {
    let recorded = detach(map, session_id).map(|e| e.instance).or_else(|| held.map(str::to_string));
    let taken: HashSet<&str> =
        map.iter().filter(|(id, e)| e.is_slot() && judge.live(id)).map(|(_, e)| e.instance.as_str()).collect();
    let instance = match recorded {
        Some(name) if !taken.contains(name.as_str()) => name,
        _ => next_free_instance(&taken),
    };
    for entry in map.values_mut().filter(|e| e.is_slot() && e.instance == instance) {
        entry.retired = true;
    }
    map.insert(
        session_id.to_string(),
        InstanceEntry { instance: instance.clone(), registered_at: now, last_seen: now, alias_of: None, retired: false },
    );
    relabel_followers(map, session_id, &instance);
    instance
}

/// Every alias whose chain ends at `id`, earlier ones included, reads
/// `instance`, the name of the slot it leads to.
fn relabel_followers(map: &mut BTreeMap<String, InstanceEntry>, id: &str, instance: &str) {
    let followers: Vec<String> = map.keys().filter(|a| chain_end(map, a).as_deref() == Some(id)).cloned().collect();
    for a in followers {
        if let Some(alias) = map.get_mut(&a) {
            alias.instance = instance.to_string();
        }
    }
}

/// Remove `id`'s row. When it was an alias, the aliases that pointed at it
/// point at its target, so each chain still ends at the slot it led to.
/// When it was a slot, they keep pointing at `id` for the caller to settle.
fn detach(map: &mut BTreeMap<String, InstanceEntry>, id: &str) -> Option<InstanceEntry> {
    let entry = map.remove(id)?;
    if let Some(target) = &entry.alias_of {
        for alias in map.values_mut() {
            if alias.alias_of.as_deref() == Some(id) {
                alias.alias_of = Some(target.clone());
            }
        }
    }
    Some(entry)
}

/// The id at the end of `id`'s `alias_of` chain: the first one that is no
/// alias. `None` when `id` is no alias, or when the chain loops.
fn chain_end(map: &BTreeMap<String, InstanceEntry>, id: &str) -> Option<String> {
    let mut id = map.get(id)?.alias_of.clone()?;
    for _ in 0..map.len() {
        match map.get(&id).and_then(|e| e.alias_of.clone()) {
            Some(next) => id = next,
            None => return Some(id),
        }
    }
    None
}

/// Pick the lowest name not in `taken`: the bare name, then the Greek
/// letters in order, then numeric suffixes starting at 25 so the identity
/// reads as `<nick>-25`, `<nick>-26`, …
fn next_free_instance(taken: &HashSet<&str>) -> String {
    for name in std::iter::once(&BARE).chain(GREEK) {
        if !taken.contains(*name) {
            return (*name).to_string();
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

// ── YAML parser / serializer ──────────────────────────────────────
//
// Hand-rolled to keep the dependency footprint at zero — same
// approach as `groups.rs::parse_groups_yaml`. Schema is shallow and
// fixed (one key, four scalar fields), so a serde-yaml dependency
// would be pure ceremony.

fn parse_registry(content: &str) -> BTreeMap<String, InstanceEntry> {
    let mut out = BTreeMap::new();
    let mut current_sid: Option<String> = None;
    let mut current_instance: Option<String> = None;
    let mut current_registered_at: Option<u64> = None;
    let mut current_last_seen: Option<u64> = None;
    let mut current_alias_of: Option<String> = None;
    let mut current_retired = false;

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
                current_alias_of.take(),
                std::mem::take(&mut current_retired),
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
                    "alias_of" => current_alias_of = Some(value.to_string()),
                    "retired" => current_retired = value == "true",
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
        current_alias_of,
        current_retired,
    );
    out
}

fn commit_entry(
    out: &mut BTreeMap<String, InstanceEntry>,
    sid: Option<String>,
    instance: Option<String>,
    registered_at: Option<u64>,
    last_seen: Option<u64>,
    alias_of: Option<String>,
    retired: bool,
) {
    // A row with no `instance` line is no row; the bare name is the line
    // with an empty value.
    let (Some(sid), Some(instance)) = (sid, instance) else {
        return;
    };
    out.insert(
        sid,
        InstanceEntry {
            instance,
            registered_at: registered_at.unwrap_or(0),
            last_seen: last_seen.unwrap_or(0),
            alias_of,
            retired,
        },
    );
}

fn serialize_registry(map: &BTreeMap<String, InstanceEntry>) -> String {
    let mut out = String::new();
    for (sid, entry) in map {
        out.push_str(sid);
        out.push_str(":\n");
        // The bare name is an empty value. A release that predates it keeps
        // the line as an empty instance and writes it back as it read it.
        out.push_str("  instance:");
        if entry.instance != BARE {
            out.push(' ');
            out.push_str(&entry.instance);
        }
        out.push('\n');
        out.push_str("  registered_at: ");
        out.push_str(&entry.registered_at.to_string());
        out.push('\n');
        out.push_str("  last_seen: ");
        out.push_str(&entry.last_seen.to_string());
        out.push('\n');
        if let Some(target) = &entry.alias_of {
            out.push_str("  alias_of: ");
            out.push_str(target);
            out.push('\n');
        }
        if entry.retired {
            out.push_str("  retired: true\n");
        }
    }
    out
}

fn write_registry(path: &Path, map: &BTreeMap<String, InstanceEntry>) -> io::Result<()> {
    // Atomic rename: write to .tmp, fsync, rename. The flock holder
    // is the only writer at any moment, so no locked-rename dance is
    // needed — the rename target's flock state is irrelevant on
    // Linux (locks are on open-file-descriptions, not paths).
    agent_settings::writer::write_atomic(path, serialize_registry(map))
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
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// A registry where every session is live until `end` ends it, and a
    /// resume (`start`) brings it back.
    struct Sessions {
        reg: Registry,
        ended: Arc<Mutex<HashSet<String>>>,
    }

    impl Sessions {
        fn end(&self, id: &str) {
            self.ended.lock().unwrap().insert(id.to_string());
        }
        fn start(&self, id: &str) {
            self.ended.lock().unwrap().remove(id);
        }
    }

    impl std::ops::Deref for Sessions {
        type Target = Registry;
        fn deref(&self) -> &Registry {
            &self.reg
        }
    }

    fn with_sessions<F: FnOnce(&Sessions)>(f: F) {
        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let ended = Arc::new(Mutex::new(HashSet::<String>::new()));
        let judged = Arc::clone(&ended);
        let reg = Registry::with_liveness(base.clone(), move |id| !judged.lock().unwrap().contains(id));
        f(&Sessions { reg, ended });
        fs::remove_dir_all(&base).ok();
    }

    /// Every session registered is live for the whole test.
    fn with_registry<F: FnOnce(&Registry)>(f: F) {
        with_sessions(|s| f(&s.reg));
    }

    #[test]
    fn a_solo_session_is_bare() {
        with_registry(|r| {
            assert_eq!(r.register("/x", "sess-a").unwrap(), BARE);
            assert_eq!(r.lookup("/x", "sess-a").as_deref(), Some(BARE));
            assert_eq!(display_name("Urban", r.lookup("/x", "sess-a").as_deref()), "Urban");
        });
    }

    #[test]
    fn a_second_concurrent_session_is_alpha() {
        with_registry(|r| {
            r.register("/x", "sess-a").unwrap();
            assert_eq!(r.register("/x", "sess-b").unwrap(), "alpha");
            assert_eq!(r.register("/x", "sess-c").unwrap(), "beta");
            assert_eq!(display_name("Urban", r.lookup("/x", "sess-b").as_deref()), "Urban-alpha");
        });
    }

    /// The bare holder keeps its name when a second session joins, through
    /// its minute refresh and through a host restart's register.
    #[test]
    fn the_bare_holder_stays_bare_when_a_second_joins() {
        with_registry(|r| {
            r.register("/x", "sess-a").unwrap();
            r.register("/x", "sess-b").unwrap();
            assert_eq!(r.keep("/x", "sess-a", None).unwrap().as_deref(), Some(BARE));
            assert_eq!(r.register("/x", "sess-a").unwrap(), BARE);
            assert_eq!(r.keep("/x", "sess-b", None).unwrap().as_deref(), Some("alpha"));
        });
    }

    /// An ended session's name goes to the next session, lowest first: with
    /// the bare holder gone and `-alpha` running, the next one is bare. The
    /// ended row is retired: it holds no slot.
    #[test]
    fn an_ended_sessions_name_is_reused() {
        with_sessions(|s| {
            s.register("/x", "sess-a").unwrap();
            s.register("/x", "sess-b").unwrap();
            s.end("sess-a");
            assert_eq!(s.register("/x", "sess-c").unwrap(), BARE);
            assert!(s.snapshot("/x")["sess-a"].retired, "the ended row is retired and holds no slot");
            assert_eq!(s.lookup("/x", "sess-b").as_deref(), Some("alpha"), "the live holder is untouched");
            s.end("sess-b");
            assert_eq!(s.register("/x", "sess-d").unwrap(), "alpha");
        });
    }

    /// An ended session's row is kept until its name goes to someone, so a
    /// resume takes its recorded name back, even with a lower one free.
    #[test]
    fn a_resume_keeps_its_name_when_no_live_session_holds_it() {
        with_sessions(|s| {
            s.register("/x", "sess-a").unwrap();
            s.register("/x", "sess-b").unwrap();
            s.end("sess-a");
            s.end("sess-b");
            s.start("sess-b");
            assert_eq!(s.register("/x", "sess-b").unwrap(), "alpha");
        });
    }

    /// A resume whose recorded name a live session took in the meantime
    /// takes the lowest free name; the live holder keeps its own.
    #[test]
    fn a_resume_takes_a_free_name_when_a_live_session_holds_its_own() {
        with_sessions(|s| {
            s.register("/x", "sess-a").unwrap();
            s.end("sess-a");
            assert_eq!(s.register("/x", "sess-c").unwrap(), BARE);
            s.start("sess-a");
            assert_eq!(s.register("/x", "sess-a").unwrap(), "alpha");
            assert_eq!(s.lookup("/x", "sess-c").as_deref(), Some(BARE));
        });
    }

    /// Two slots with one name (a liveness misjudged, or a registry two
    /// releases wrote): the one that registers yields while the other is
    /// live, and a minute refresh never renames a running session.
    #[test]
    fn a_register_yields_a_name_another_live_slot_holds() {
        with_sessions(|s| {
            fs::create_dir_all(&s.base_dir).unwrap();
            let slot = |at| InstanceEntry { instance: BARE.to_string(), registered_at: at, last_seen: agent_fmt::when::now_secs(), alias_of: None, retired: false };
            write_registry(&s.path_for("/x"), &BTreeMap::from([("sess-a".to_string(), slot(1)), ("sess-c".to_string(), slot(2))])).unwrap();
            assert_eq!(s.keep("/x", "sess-a", None).unwrap().as_deref(), Some(BARE), "a refresh keeps the name");
            assert_eq!(s.register("/x", "sess-a").unwrap(), "alpha");
            assert_eq!(s.register("/x", "sess-c").unwrap(), BARE);
            // With the other holder ended, a register keeps the name.
            s.end("sess-a");
            write_registry(&s.path_for("/x"), &BTreeMap::from([("sess-a".to_string(), slot(1)), ("sess-c".to_string(), slot(2))])).unwrap();
            assert_eq!(s.register("/x", "sess-c").unwrap(), BARE);
        });
    }

    /// A sensor host on a released attend (v0.36.0) rewrites the registry
    /// with its own parse and write rules every minute. A bare row passes
    /// through that round trip and comes back bare.
    #[test]
    fn a_bare_row_survives_the_released_parser_and_writer() {
        let bare = InstanceEntry { instance: BARE.into(), registered_at: 100, last_seen: 200, alias_of: None, retired: false };
        let lettered = InstanceEntry { instance: "alpha".into(), registered_at: 110, last_seen: 210, alias_of: None, retired: false };
        let ours = BTreeMap::from([("sess-a".to_string(), bare), ("sess-b".to_string(), lettered)]);
        let released = v036::parse_registry(&serialize_registry(&ours));
        assert_eq!(released.get("sess-a").map(|e| e.instance.as_str()), Some(""), "the released parser keeps the bare row");
        let back = parse_registry(&v036::serialize_registry(&released));
        assert_eq!(back, ours, "the released writer keeps it bare");
    }

    /// An ended session's row is retired, not dropped, when a new session
    /// takes its name: its earlier messages still read the name it held,
    /// never the live bare holder's, and the retired row blocks no name.
    #[test]
    fn an_ended_sessions_messages_keep_the_name_it_held() {
        with_sessions(|s| {
            s.register("/x", "sess-a").unwrap();
            s.register("/x", "sess-b").unwrap();
            s.end("sess-b");
            assert_eq!(s.register("/x", "sess-c").unwrap(), "alpha");
            assert_eq!(s.lookup("/x", "sess-b").as_deref(), Some("alpha"), "the retired row reads the name it held");
            assert_ne!(display_name("Urban", s.lookup("/x", "sess-b").as_deref()), display_name("Urban", s.lookup("/x", "sess-a").as_deref()));
            assert_eq!(s.register("/x", "sess-d").unwrap(), "beta");
            s.end("sess-c");
            assert_eq!(s.register("/x", "sess-e").unwrap(), "alpha", "neither the retired row nor the ended slot blocks alpha");
        });
    }

    /// The 7-day GC spares a live session whose row went unrefreshed (a
    /// laptop suspended for over a week): it keeps its row and its name.
    #[test]
    fn gc_spares_a_live_sessions_row() {
        with_sessions(|s| {
            let now = 1_000_000u64;
            let way_old = now - (8 * 24 * 3600);
            s.register_with_age("/x", "sess-a", DEFAULT_GC_AGE, way_old).unwrap();
            assert_eq!(s.register_with_age("/x", "sess-b", DEFAULT_GC_AGE, now).unwrap(), "alpha");
            assert_eq!(s.lookup("/x", "sess-a").as_deref(), Some(BARE), "the live session's row survives GC");
        });
    }

    #[test]
    fn distinct_cwds_have_independent_registries() {
        with_registry(|r| {
            // Same session_id in two different cwds gets two
            // independent allocations. Each cwd is its own sea.
            assert_eq!(r.register("/x", "sess-a").unwrap(), BARE);
            assert_eq!(r.register("/y", "sess-a").unwrap(), BARE); // /y is a fresh registry
        });
    }

    #[test]
    fn an_ended_sessions_row_unseen_past_gc_age_is_removed() {
        with_sessions(|s| {
            let now = 1_000_000u64;
            let way_old = now - (8 * 24 * 3600); // 8 days
            s.register_with_age("/x", "sess-a", DEFAULT_GC_AGE, way_old).unwrap();
            s.end("sess-a");
            s.register_with_age("/x", "sess-b", DEFAULT_GC_AGE, now).unwrap();
            assert_eq!(s.lookup("/x", "sess-a"), None, "sess-a's stale row is collected");
        });
    }

    /// A live session holds its name however long its row went unrefreshed:
    /// liveness, not the clock, decides who holds a name.
    #[test]
    fn a_live_holder_keeps_its_name_within_gc_age() {
        with_registry(|r| {
            let now = 1_000_000u64;
            let recent = now - (5 * 24 * 3600);
            r.register_with_age("/x", "sess-a", DEFAULT_GC_AGE, recent).unwrap();
            assert_eq!(r.register_with_age("/x", "sess-b", DEFAULT_GC_AGE, now).unwrap(), "alpha");
        });
    }

    #[test]
    fn numeric_fallback_past_omega() {
        with_registry(|r| {
            // The bare name, then the 24 letters; the 26th session
            // overflows Greek and lands on the "25" numeric suffix.
            assert_eq!(r.register("/x", "sess-bare").unwrap(), BARE);
            for (i, expected) in GREEK.iter().enumerate() {
                assert_eq!(r.register("/x", &format!("sess-{i}")).unwrap(), *expected);
            }
            assert_eq!(r.register("/x", "sess-overflow").unwrap(), "25");
        });
    }

    #[test]
    fn lookup_returns_none_for_unknown() {
        with_registry(|r| {
            assert_eq!(r.lookup("/x", "nope"), None);
            r.register("/x", "sess-a").unwrap();
            assert_eq!(r.lookup("/x", "sess-a").as_deref(), Some(BARE));
        });
    }

    #[test]
    fn touch_refreshes_last_seen_without_changing_instance() {
        with_registry(|r| {
            r.register_with_age("/x", "sess-a", DEFAULT_GC_AGE, 1000).unwrap();
            r.register_with_age("/x", "sess-b", DEFAULT_GC_AGE, 1000).unwrap();
            let before = r.snapshot("/x")["sess-b"].last_seen;
            r.touch_at("/x", "sess-b", before + 60).unwrap();
            let after = r.snapshot("/x")["sess-b"].clone();
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
        let entry = |instance: &str, registered_at, last_seen, alias_of: Option<&str>| InstanceEntry {
            instance: instance.into(),
            registered_at,
            last_seen,
            alias_of: alias_of.map(str::to_string),
            retired: false,
        };
        let m = BTreeMap::from([
            ("sess-a".to_string(), entry("alpha", 1_000, 2_000, None)),
            ("sess-b".to_string(), entry("beta", 1_500, 2_500, Some("sess-a"))),
            ("sess-c".to_string(), entry(BARE, 1_700, 2_700, None)),
            ("sess-d".to_string(), entry(BARE, 1_800, 2_800, Some("sess-c"))),
            ("sess-e".to_string(), InstanceEntry { retired: true, ..entry("gamma", 1_900, 2_900, None) }),
        ]);
        let yaml = serialize_registry(&m);
        assert_eq!(parse_registry(&yaml), m);
    }

    /// The bare name is an `instance` line with an empty value, and a row
    /// with no `instance` line is no row.
    #[test]
    fn a_bare_row_has_an_empty_instance_line() {
        let row = InstanceEntry { instance: BARE.into(), registered_at: 100, last_seen: 200, alias_of: None, retired: false };
        let yaml = serialize_registry(&BTreeMap::from([("sess-a".to_string(), row)]));
        assert_eq!(yaml, "sess-a:\n  instance:\n  registered_at: 100\n  last_seen: 200\n");
        assert!(parse_registry("sess-a:\n  registered_at: 100\n  last_seen: 200\n").is_empty());
    }

    /// A registry the released attend wrote, every row lettered, loads as it
    /// was, and a new session there takes the bare name when no live session
    /// holds it.
    #[test]
    fn a_registry_written_before_the_bare_name_loads() {
        with_registry(|r| {
            fs::create_dir_all(&r.base_dir).unwrap();
            let now = agent_fmt::when::now_secs();
            let released = format!(
                "sess-a:\n  instance: alpha\n  registered_at: 1\n  last_seen: {now}\nsess-b:\n  instance: delta\n  registered_at: 2\n  last_seen: {now}\nsess-old:\n  instance: alpha\n  registered_at: 1\n  last_seen: {now}\n  alias_of: sess-a\n"
            );
            fs::write(r.path_for("/x"), released).unwrap();
            assert_eq!(r.lookup("/x", "sess-a").as_deref(), Some("alpha"));
            assert_eq!(r.successor("/x", "sess-old").as_deref(), Some("sess-a"));
            assert_eq!(r.register("/x", "sess-b").unwrap(), "delta", "a lettered session keeps its letter");
            assert_eq!(r.register("/x", "sess-c").unwrap(), BARE);
            assert_eq!(r.register("/x", "sess-d").unwrap(), "beta");
        });
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
        assert_eq!(parsed["sess-a"].alias_of, None, "a row without the field holds a slot");
    }

    #[test]
    fn registry_files_are_named_by_the_attend_key() {
        with_registry(|reg| {
            assert!(reg.path_for("/srv/my proj").ends_with("-srv-my-proj-bte5w6.yaml"));
            // One slug, two projects, two registries.
            assert_ne!(reg.path_for("/srv/my proj"), reg.path_for("/srv/my-proj"));
        });
    }

    #[test]
    fn rename_keeps_the_instance_name() {
        with_registry(|reg| {
            reg.register("/x", "sess-a").unwrap();
            assert_eq!(reg.register("/x", "sess-b").unwrap(), "alpha");
            assert_eq!(reg.rename("/x", "sess-b", "sess-c").unwrap().as_deref(), Some("alpha"));
            let snap = reg.snapshot("/x");
            assert_eq!(snap.get("sess-c").map(|e| e.instance.as_str()), Some("alpha"));
            // The old id keeps the name, so its earlier messages still read
            // `-alpha` rather than the bare nickname.
            assert_eq!(snap.get("sess-b").map(|e| e.instance.as_str()), Some("alpha"));
            assert_eq!(snap["sess-b"].alias_of.as_deref(), Some("sess-c"));
            assert_eq!(reg.successor("/x", "sess-b").as_deref(), Some("sess-c"));
            assert_eq!(reg.successor("/x", "sess-c"), None);
            // No old slot: nothing is registered for the new id.
            assert_eq!(reg.rename("/x", "gone", "sess-d").unwrap(), None);
            assert!(!reg.snapshot("/x").contains_key("sess-d"));
        });
    }

    /// `/clear` keeps the bare name: the new id takes it, the old id reads it
    /// as an alias, the new host's register keeps it, and a second session
    /// alongside is `-alpha`.
    #[test]
    fn a_clear_keeps_the_bare_name() {
        with_sessions(|s| {
            assert_eq!(s.register("/x", "sess-a").unwrap(), BARE);
            assert_eq!(s.rename("/x", "sess-a", "sess-a2").unwrap().as_deref(), Some(BARE));
            s.end("sess-a");
            assert_eq!(s.register("/x", "sess-a2").unwrap(), BARE);
            assert_eq!(s.keep("/x", "sess-a", None).unwrap(), None, "the old host leaves the alias alone");
            let snap = s.snapshot("/x");
            assert_eq!(snap["sess-a2"].instance, BARE);
            assert_eq!(snap["sess-a"].instance, BARE, "the old id reads the bare name");
            assert_eq!(snap["sess-a"].alias_of.as_deref(), Some("sess-a2"));
            assert_eq!(s.register("/x", "sess-b").unwrap(), "alpha");
            assert_eq!(s.successor("/x", "sess-a").as_deref(), Some("sess-a2"));
        });
    }

    /// A dying old host's minute refresh after `/clear` leaves the old id an
    /// alias of the new one, where `register` would take it a fresh slot.
    /// `keep` refreshes a slot, and allocates for an id with no row.
    #[test]
    fn keep_leaves_an_alias_an_alias() {
        with_registry(|reg| {
            reg.register("/x", "a").unwrap();
            reg.register("/x", "b").unwrap();
            reg.rename("/x", "b", "c").unwrap();
            assert_eq!(reg.keep("/x", "b", None).unwrap(), None);
            let snap = reg.snapshot("/x");
            assert_eq!(snap["b"].alias_of.as_deref(), Some("c"), "b is still an alias");
            assert_eq!(reg.successor("/x", "b").as_deref(), Some("c"));
            assert_eq!(reg.keep("/x", "c", None).unwrap().as_deref(), Some("alpha"), "the slot is refreshed");
            assert_eq!(reg.keep("/x", "d", None).unwrap().as_deref(), Some("beta"), "an id with no row is allocated");
        });
    }

    #[test]
    fn a_resume_of_the_id_before_a_clear_takes_a_fresh_name() {
        with_registry(|reg| {
            reg.register("/x", "sess-a").unwrap();
            reg.register("/x", "sess-b").unwrap();
            reg.rename("/x", "sess-b", "sess-c").unwrap();
            // `claude --resume sess-b` while sess-c runs as alpha.
            assert_eq!(reg.register("/x", "sess-b").unwrap(), "beta");
            assert_eq!(reg.register("/x", "sess-c").unwrap(), "alpha");
            let snap = reg.snapshot("/x");
            assert_eq!(snap["sess-b"].alias_of, None, "the resumed id holds a slot of its own");
            assert_eq!(reg.register("/x", "sess-d").unwrap(), "gamma");
        });
    }

    /// A resume of the id before a `/clear`, once the session that took its
    /// slot has ended, takes back the name it read.
    #[test]
    fn a_resume_of_the_id_before_a_clear_takes_back_a_free_name() {
        with_sessions(|s| {
            s.register("/x", "sess-a").unwrap();
            s.register("/x", "sess-b").unwrap();
            s.rename("/x", "sess-b", "sess-c").unwrap();
            s.end("sess-b");
            s.end("sess-c");
            s.start("sess-b");
            assert_eq!(s.register("/x", "sess-b").unwrap(), "alpha");
            assert!(s.snapshot("/x")["sess-c"].retired, "the ended successor is retired");
        });
    }

    #[test]
    fn a_chain_of_clears_keeps_one_name() {
        with_registry(|reg| {
            assert_eq!(reg.register("/x", "sess-a").unwrap(), BARE);
            reg.rename("/x", "sess-a", "sess-b").unwrap();
            assert_eq!(reg.rename("/x", "sess-b", "sess-c").unwrap().as_deref(), Some(BARE));
            let snap = reg.snapshot("/x");
            for id in ["sess-a", "sess-b", "sess-c"] {
                assert_eq!(snap[id].instance, BARE, "{id}");
            }
            assert_eq!(reg.successor("/x", "sess-a").as_deref(), Some("sess-c"));
            assert_eq!(reg.successor("/x", "sess-b").as_deref(), Some("sess-c"));
            assert_eq!(reg.register("/x", "sess-d").unwrap(), "alpha", "the aliases block no name");
            // A resume of the middle id leaves the first leading to sess-c.
            assert_eq!(reg.register("/x", "sess-b").unwrap(), "beta");
            assert_eq!(reg.successor("/x", "sess-a").as_deref(), Some("sess-c"));
            assert_eq!(reg.lookup("/x", "sess-a").as_deref(), Some(BARE));
        });
    }

    /// An alias whose chain reaches no slot in its cwd blocks no name and
    /// keeps reading the name it read, for its earlier messages.
    #[test]
    fn an_orphaned_alias_keeps_the_name_it_reads() {
        with_registry(|reg| {
            reg.register("/x", "sess-a").unwrap();
            reg.rename("/x", "sess-a", "sess-c").unwrap();
            // sess-c moves to another cwd; its alias in /x outlives it.
            reg.register("/y", "sess-c").unwrap();
            assert_eq!(reg.lookup("/x", "sess-a").as_deref(), Some(BARE));
            assert_eq!(reg.register("/x", "sess-d").unwrap(), BARE);
            assert_eq!(reg.lookup("/x", "sess-a").as_deref(), Some(BARE));
        });
    }

    /// A row a writer dropped (a released host's rewrite, a misjudged
    /// liveness) comes back on the minute refresh under the name the host
    /// holds, not the lowest free one, unless a live session took it.
    #[test]
    fn keep_restores_a_missing_row_under_its_own_name() {
        with_sessions(|s| {
            s.register("/x", "sess-a").unwrap();
            s.register("/x", "sess-b").unwrap();
            assert_eq!(s.register("/x", "sess-c").unwrap(), "beta");
            s.end("sess-b");
            let mut map = s.snapshot("/x");
            map.remove("sess-c");
            write_registry(&s.path_for("/x"), &map).unwrap();
            assert_eq!(s.keep("/x", "sess-c", Some("beta")).unwrap().as_deref(), Some("beta"), "alpha is free, but sess-c holds beta");
            // A name a live session took meanwhile is not taken back.
            let mut map = s.snapshot("/x");
            map.remove("sess-a");
            write_registry(&s.path_for("/x"), &map).unwrap();
            assert_eq!(s.register("/x", "sess-d").unwrap(), BARE);
            assert_eq!(s.keep("/x", "sess-a", Some(BARE)).unwrap().as_deref(), Some("alpha"));
        });
    }

    #[test]
    fn a_rename_onto_an_alias_id_keeps_the_chain_on_its_slot() {
        with_registry(|reg| {
            reg.register("/x", "sess-a").unwrap();
            reg.rename("/x", "sess-a", "sess-b").unwrap();
            reg.rename("/x", "sess-b", "sess-c").unwrap();
            assert_eq!(reg.register("/x", "sess-x").unwrap(), "alpha");
            assert_eq!(reg.rename("/x", "sess-x", "sess-b").unwrap().as_deref(), Some("alpha"));
            assert_eq!(reg.successor("/x", "sess-a").as_deref(), Some("sess-c"));
            assert_eq!(reg.lookup("/x", "sess-a").as_deref(), Some(BARE));
            assert_eq!(reg.successor("/x", "sess-x").as_deref(), Some("sess-b"));
            assert_eq!(reg.snapshot("/x")["sess-b"].alias_of, None);
        });
    }

    #[test]
    fn a_middle_alias_resumed_elsewhere_keeps_the_chain() {
        with_registry(|reg| {
            reg.register("/x", "sess-a").unwrap();
            reg.rename("/x", "sess-a", "sess-b").unwrap();
            reg.rename("/x", "sess-b", "sess-c").unwrap();
            reg.register("/y", "sess-b").unwrap();
            assert_eq!(reg.lookup("/x", "sess-b"), None, "sess-b moved to /y");
            assert_eq!(reg.successor("/x", "sess-a").as_deref(), Some("sess-c"));
        });
    }

    #[test]
    fn a_rename_onto_a_held_slot_aliases_the_old_id_to_it() {
        with_registry(|reg| {
            reg.register("/x", "sess-a").unwrap();
            reg.register("/x", "sess-b").unwrap();
            assert_eq!(reg.rename("/x", "sess-a", "sess-b").unwrap().as_deref(), Some("alpha"));
            let snap = reg.snapshot("/x");
            assert_eq!(snap["sess-a"].instance, "alpha", "the old id reads the live name");
            assert_eq!(snap["sess-a"].alias_of.as_deref(), Some("sess-b"));
            assert_eq!(reg.register("/x", "sess-e").unwrap(), BARE, "the bare name is free again");
        });
    }

    #[test]
    fn a_registry_under_an_old_name_is_not_read() {
        // ADR-506: attend reads only `attend_key` names. A file written under
        // the old rule (the space kept) is neither read nor moved.
        with_registry(|reg| {
            fs::create_dir_all(&reg.base_dir).unwrap();
            let old = reg.base_dir.join("-srv-a b.yaml");
            let entry = InstanceEntry { instance: "beta".to_string(), registered_at: 1, last_seen: agent_fmt::when::now_secs(), alias_of: None, retired: false };
            write_registry(&old, &BTreeMap::from([("sess-a".to_string(), entry)])).unwrap();
            assert_eq!(reg.lookup("/srv/a b", "sess-a"), None);
            reg.touch_at("/srv/a b", "sess-a", 500).unwrap();
            assert!(reg.snapshot("/srv/a b").is_empty());
            assert_eq!(reg.register("/srv/a b", "sess-a").unwrap(), BARE);
            assert!(old.exists(), "the old file is left alone");
        });
    }

    #[test]
    fn next_free_picks_first_unused_in_order() {
        let mut taken = HashSet::new();
        assert_eq!(next_free_instance(&taken), BARE);
        taken.insert(BARE);
        assert_eq!(next_free_instance(&taken), "alpha");
        taken.insert("alpha");
        taken.insert("beta");
        // Gap before gamma — allocator picks the lowest-index hole.
        taken.insert("delta");
        assert_eq!(next_free_instance(&taken), "gamma");
        taken.remove(BARE);
        assert_eq!(next_free_instance(&taken), BARE, "the bare name comes first");
    }

    #[test]
    fn display_name_is_bare_for_the_bare_name_and_no_row() {
        assert_eq!(display_name("Urban", None), "Urban");
        assert_eq!(display_name("Urban", Some(BARE)), "Urban");
        assert_eq!(display_name("Urban", Some("alpha")), "Urban-alpha");
        assert_eq!(display_name("Urban", Some("25")), "Urban-25");
    }

    #[test]
    fn snapshot_cache_returns_same_instance_for_repeated_lookups() {
        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let reg = Registry::with_liveness(base.clone(), |_| true);
        reg.register("/cwd", "sess-a").unwrap();
        let assigned = reg.register("/cwd", "sess-b").unwrap();

        let cache = SnapshotCache::with_registry(Registry::with_base(base.clone()));
        // First lookup must reflect what we just registered.
        assert_eq!(cache.lookup("/cwd", "sess-b").as_deref(), Some(assigned.as_str()));
        // Repeat lookups for the same (cwd, sid) return the same value
        // — and, important to the perf claim, the second call hits
        // the in-memory map rather than re-reading the yaml.
        assert_eq!(cache.lookup("/cwd", "sess-b").as_deref(), Some(assigned.as_str()));
        assert_eq!(cache.lookup("/cwd", "sess-a").as_deref(), Some(BARE));
        // Lookup miss returns None and is also cached implicitly via
        // the per-cwd snapshot the entry-API populated on first call.
        assert_eq!(cache.lookup("/cwd", "no-such-session"), None);

        fs::remove_dir_all(&base).ok();
    }

    /// The cache follows an alias as the registry does, from the snapshot
    /// it read once: a rename after that read is not seen.
    #[test]
    fn snapshot_cache_follows_an_alias_from_its_snapshot() {
        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let reg = Registry::with_liveness(base.clone(), |_| true);
        reg.register("/cwd", "sess-a").unwrap();
        reg.rename("/cwd", "sess-a", "sess-b").unwrap();
        let cache = SnapshotCache::with_registry(Registry::with_base(base.clone()));
        assert_eq!(cache.successor("/cwd", "sess-a").as_deref(), Some("sess-b"));
        assert_eq!(cache.successor("/cwd", "sess-b"), None);
        reg.rename("/cwd", "sess-b", "sess-c").unwrap();
        assert_eq!(cache.successor("/cwd", "sess-a").as_deref(), Some("sess-b"), "read once");
        assert_eq!(reg.successor("/cwd", "sess-a").as_deref(), Some("sess-c"));
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn snapshot_cache_isolates_per_cwd() {
        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let reg = Registry::with_liveness(base.clone(), |_| true);
        reg.register("/cwd-a", "sess-a").unwrap();
        reg.register("/cwd-b", "sess-b").unwrap();

        let cache = SnapshotCache::with_registry(Registry::with_base(base.clone()));
        // Each cwd is an independent snapshot — no cross-contamination
        // between paths even though both hold their own bare name.
        assert_eq!(cache.lookup("/cwd-a", "sess-a").as_deref(), Some(BARE));
        assert_eq!(cache.lookup("/cwd-b", "sess-b").as_deref(), Some(BARE));
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
        use std::thread;

        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = fresh_base();
        let reg = Arc::new(Registry::with_liveness(base.clone(), |_| true));

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
        // map and pick the same next-free name).
        let unique: HashSet<&String> = assigned.iter().collect();
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

/// The registry's parse and write rules as attend v0.36.0 released them
/// (`git show attend-v0.36.0:tools/attend-instances/src/lib.rs`), copied
/// verbatim but for the entry type, so a test can run a row through a
/// released sensor host's minute rewrite.
#[cfg(test)]
mod v036 {
    use std::collections::BTreeMap;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct InstanceEntry {
        pub instance: String,
        pub registered_at: u64,
        pub last_seen: u64,
        pub alias_of: Option<String>,
    }

    pub fn parse_registry(content: &str) -> BTreeMap<String, InstanceEntry> {
        let mut out = BTreeMap::new();
        let mut current_sid: Option<String> = None;
        let mut current_instance: Option<String> = None;
        let mut current_registered_at: Option<u64> = None;
        let mut current_last_seen: Option<u64> = None;
        let mut current_alias_of: Option<String> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            if indent == 0 && trimmed.ends_with(':') {
                commit_entry(
                    &mut out,
                    current_sid.take(),
                    current_instance.take(),
                    current_registered_at.take(),
                    current_last_seen.take(),
                    current_alias_of.take(),
                );
                current_sid = Some(trimmed.trim_end_matches(':').to_string());
                continue;
            }
            if indent == 2 {
                if let Some((key, value)) = trimmed.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();
                    match key {
                        "instance" => current_instance = Some(value.to_string()),
                        "registered_at" => current_registered_at = value.parse().ok(),
                        "last_seen" => current_last_seen = value.parse().ok(),
                        "alias_of" => current_alias_of = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }
        commit_entry(&mut out, current_sid, current_instance, current_registered_at, current_last_seen, current_alias_of);
        out
    }

    fn commit_entry(
        out: &mut BTreeMap<String, InstanceEntry>,
        sid: Option<String>,
        instance: Option<String>,
        registered_at: Option<u64>,
        last_seen: Option<u64>,
        alias_of: Option<String>,
    ) {
        let (Some(sid), Some(instance)) = (sid, instance) else {
            return;
        };
        out.insert(sid, InstanceEntry { instance, registered_at: registered_at.unwrap_or(0), last_seen: last_seen.unwrap_or(0), alias_of });
    }

    pub fn serialize_registry(map: &BTreeMap<String, InstanceEntry>) -> String {
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
            if let Some(target) = &entry.alias_of {
                out.push_str("  alias_of: ");
                out.push_str(target);
                out.push('\n');
            }
        }
        out
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    /// Issue #394 regression: registering at a new cwd evicts the same
    /// session's entry from every other cwd's registry — an origin
    /// move migrates the persona instead of minting a phantom sibling.
    #[test]
    fn register_evicts_same_session_from_other_cwds() {
        let base = std::env::temp_dir().join(format!(
            "attend-instances-394-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let reg = Registry::with_base(base.clone());

        reg.register("/proj/.claude/worktrees/x", "sess-1").unwrap();
        reg.register("/proj/.claude/worktrees/x", "sess-other").unwrap();
        reg.register("/proj", "sess-1").unwrap();

        // sess-1 migrated: present at the new cwd, gone from the old.
        assert!(reg.lookup("/proj", "sess-1").is_some());
        assert!(
            reg.lookup("/proj/.claude/worktrees/x", "sess-1").is_none(),
            "old-cwd entry must be evicted on migration"
        );
        // Unrelated sessions in the old cwd are untouched.
        assert!(reg.lookup("/proj/.claude/worktrees/x", "sess-other").is_some());
        fs::remove_dir_all(&base).ok();
    }
}
