//! Shared focus-group state I/O for attend and attend-chat (ADR-170).
//!
//! Groups are named signal namespaces that agents — and, since
//! ADR-170, humans — focus on and release. Storage: `@group-name/`
//! directories under the signals base, with membership tracked in
//! `_groups.yaml`.
//!
//! This crate is the single owner of the `_groups.yaml` wire format.
//! It was extracted from `attend::groups` when attend-chat gained its
//! `/join` write path; before that, attend-chat carried a read-only
//! byte-mirror of the parser kept honest by cross-crate golden tests.
//! Those tests now live here, next to the one implementation.
//!
//! **Member identity.** A member id is either a Claude Code session
//! UUID (claude sessions, via attend) or a sanitized username (humans,
//! via attend-chat). Both kinds are judged for liveness the same way:
//! against the heartbeat sidecar (`attend-heartbeat`, ADR-129). The
//! yaml does not distinguish them — liveness was always
//! heartbeat-shaped, not UUID-shaped.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Group membership entry.
#[derive(Debug, Clone, Default)]
pub struct GroupEntry {
    /// Whether this group persists when empty.
    pub pinned: bool,
    /// Member ids currently in this group: Claude Code session UUIDs
    /// for claude sessions, sanitized usernames for humans (ADR-170).
    pub members: Vec<String>,
}

/// Group state manager.
#[derive(Clone)]
pub struct Groups {
    base: PathBuf,
    member_id: String,
}

const GROUP_PREFIX: &str = "@";

impl Groups {
    /// `member_id` is the identity this manager acts for: a session
    /// UUID when the caller is attend, a sanitized username when the
    /// caller is attend-chat (ADR-170).
    ///
    /// Only the member-scoped operations (`join`, `leave`,
    /// `my_groups`, `joined_group_names`, `receive_dirs`) read it;
    /// the collection operations (`dissolve`, `cleanup_stale`,
    /// `all_groups`, `has_group`, `members`, `pin`/`unpin`) act on
    /// the group state as a whole and ignore it.
    pub fn new(signals_base: &Path, member_id: &str) -> Self {
        Self {
            base: signals_base.to_path_buf(),
            member_id: member_id.to_string(),
        }
    }

    /// Path to a named group's signal directory.
    pub fn group_dir(&self, name: &str) -> PathBuf {
        self.base.join(format!("{GROUP_PREFIX}{name}"))
    }

    /// Path to the groups state file.
    fn state_path(&self) -> PathBuf {
        self.base.join("_groups.yaml")
    }

    /// Join a named group. Creates the group if it doesn't exist.
    ///
    /// The yaml entry is saved *before* the dir is created: the orphan
    /// sweep in [`Groups::cleanup_stale`] treats "dir with no yaml
    /// entry" as removable, and `create_dir_all` on a pre-existing
    /// orphan dir does not bump its mtime — so writing the entry first
    /// means a concurrent sweep that re-reads the yaml sees the join,
    /// and a sweep that already deleted the old dir loses to our
    /// `create_dir_all` after it.
    pub fn join(&self, name: &str, pin: bool) -> Result<(), String> {
        validate_group_name(name)?;

        let mut state = self.load_state();
        let entry = state.entry(name.to_string()).or_insert(GroupEntry {
            pinned: false,
            members: Vec::new(),
        });
        if !entry.members.contains(&self.member_id) {
            entry.members.push(self.member_id.clone());
        }
        if pin {
            entry.pinned = true;
        }
        self.save_state(&state);

        let dir = self.group_dir(name);
        fs::create_dir_all(&dir).map_err(|e| format!("creating group dir: {e}"))?;
        Ok(())
    }

    /// Leave a named group.
    pub fn leave(&self, name: &str) -> Result<(), String> {
        let mut state = self.load_state();
        if let Some(entry) = state.get_mut(name) {
            entry.members.retain(|m| m != &self.member_id);
        }
        // Clean up empty unpinned groups in one write
        if state.get(name).is_some_and(|e| e.members.is_empty() && !e.pinned) {
            let dir = self.group_dir(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).ok();
            }
            state.remove(name);
        }
        self.save_state(&state);
        Ok(())
    }

    /// Remove an arbitrary member from a group — the operator's
    /// `/kick` (ADR-173 / issue #393). Removal is unilateral by
    /// design: it only *reduces* the target's routing reach, unlike
    /// entry, which requires the member's own `join` (invite-consent
    /// asymmetry). Same empty-group cleanup as [`leave`]. Errors on
    /// an unknown group or a non-member so the TUI can report
    /// precisely.
    pub fn kick(&self, name: &str, member: &str) -> Result<(), String> {
        let mut state = self.load_state();
        let Some(entry) = state.get_mut(name) else {
            return Err(format!("#{name}: unknown group"));
        };
        let before = entry.members.len();
        entry.members.retain(|m| m != member);
        if entry.members.len() == before {
            return Err(format!("{member} is not a member of #{name}"));
        }
        if state.get(name).is_some_and(|e| e.members.is_empty() && !e.pinned) {
            let dir = self.group_dir(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).ok();
            }
            state.remove(name);
        }
        self.save_state(&state);
        Ok(())
    }

    /// Pin a group so it persists when empty.
    pub fn pin(&self, name: &str) {
        let mut state = self.load_state();
        if let Some(entry) = state.get_mut(name) {
            entry.pinned = true;
            self.save_state(&state);
        }
    }

    /// Unpin a group.
    pub fn unpin(&self, name: &str) {
        let mut state = self.load_state();
        if let Some(entry) = state.get_mut(name) {
            entry.pinned = false;
        }
        // Clean up if now empty and unpinned — one write
        if state.get(name).is_some_and(|e| e.members.is_empty() && !e.pinned) {
            let dir = self.group_dir(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).ok();
            }
            state.remove(name);
        }
        self.save_state(&state);
    }

    /// Dissolve a group — remove it and notify members.
    pub fn dissolve(&self, name: &str) -> Vec<String> {
        let mut state = self.load_state();
        let members = state
            .remove(name)
            .map(|e| e.members)
            .unwrap_or_default();
        self.save_state(&state);

        // Remove the signal directory
        let dir = self.group_dir(name);
        if dir.is_dir() {
            fs::remove_dir_all(&dir).ok();
        }
        members
    }

    /// List rooms this member has joined.
    pub fn my_groups(&self) -> Vec<(String, bool)> {
        let state = self.load_state();
        state
            .iter()
            .filter(|(_, entry)| entry.members.contains(&self.member_id))
            .map(|(name, entry)| (name.clone(), entry.pinned))
            .collect()
    }

    /// Whether `_groups.yaml` currently has an entry for `name`.
    /// Exists so callers (notably the ADR-124 migration) can avoid
    /// triggering a full `save_state` rewrite when the work is
    /// already done — narrows the read-modify-write window against
    /// peer sessions editing the same file.
    pub fn has_group(&self, name: &str) -> bool {
        self.load_state().contains_key(name)
    }

    /// List member ids in a named group, or None if the group does not
    /// exist. Returns raw `_groups.yaml` membership — callers that need
    /// a liveness-checked view should filter with
    /// `attend_heartbeat::is_fresh` (see attend's `cmd_send` for the
    /// routing-validation shape).
    pub fn members(&self, name: &str) -> Option<Vec<String>> {
        self.load_state().get(name).map(|e| e.members.clone())
    }

    /// List all active rooms with member counts and pin state.
    pub fn all_groups(&self) -> Vec<(String, usize, bool)> {
        let state = self.load_state();
        let mut rooms: Vec<(String, usize, bool)> = state
            .iter()
            .map(|(name, entry)| (name.clone(), entry.members.len(), entry.pinned))
            .collect();
        rooms.sort_by(|a, b| a.0.cmp(&b.0));
        rooms
    }

    /// Get group names this member is focused on (for signal routing).
    pub fn joined_group_names(&self) -> Vec<String> {
        self.my_groups().into_iter().map(|(name, _)| name).collect()
    }

    /// Get signal directories for all rooms this member should receive from.
    /// Includes: project scope, joined focus groups, broadcast.
    pub fn receive_dirs(&self, project_dir: &str) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // Project scope (always)
        dirs.push(self.base.join(encode_project(project_dir)));

        // Named rooms
        for name in self.joined_group_names() {
            dirs.push(self.group_dir(&name));
        }

        // Broadcast (always)
        dirs.push(self.base.join("_broadcast"));

        dirs
    }

    /// Clean up stale members (sessions or humans whose heartbeat has
    /// gone stale — see [`member_alive`]), then sweep orphan group
    /// dirs.
    ///
    /// The orphan sweep (ADR-170) covers what the member pass can't
    /// see: a `@name/` directory with no `_groups.yaml` entry at all.
    /// Those accumulate when an entry is removed without the dir (or
    /// was never written) and would otherwise render in the chat's
    /// channel bar forever. Two guards narrow racing a concurrent
    /// `join` to a sub-millisecond window: only dirs older than the
    /// grace window are candidates, and the yaml is re-read
    /// immediately before each removal (`join` saves its entry before
    /// touching the dir). The residual window self-heals — signal
    /// writers `create_dir_all` their target, and the chat's group
    /// resolver falls back to the yaml entry.
    ///
    /// Reserved names are never swept: a lingering `@open/` belongs
    /// to the ADR-124 migration (`attend run` moves its signals into
    /// `_broadcast/`), and sweeping it here would silently destroy
    /// what that migration exists to preserve.
    pub fn cleanup_stale(&self) {
        self.cleanup_stale_with(member_alive, attend_heartbeat::DEFAULT_GRACE);
    }

    /// [`Groups::cleanup_stale`] with an injectable liveness predicate
    /// and orphan-age grace, so tests can drive both passes without a
    /// heartbeat sidecar or mtime manipulation.
    pub fn cleanup_stale_with<F: Fn(&str) -> bool>(
        &self,
        is_live: F,
        orphan_grace: std::time::Duration,
    ) {
        let mut state = self.load_state();
        let mut changed = false;

        for entry in state.values_mut() {
            let before = entry.members.len();
            entry.members.retain(|sid| is_live(sid));
            if entry.members.len() != before {
                changed = true;
            }
        }

        if changed {
            self.save_state(&state);
        }

        // Clean up empty unpinned rooms
        let to_remove: Vec<String> = state
            .iter()
            .filter(|(_, e)| e.members.is_empty() && !e.pinned)
            .map(|(name, _)| name.clone())
            .collect();

        for name in &to_remove {
            let dir = self.group_dir(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).ok();
            }
        }

        if !to_remove.is_empty() {
            for name in &to_remove {
                state.remove(name);
            }
            self.save_state(&state);
        }

        // Orphan-dir sweep: on-disk `@name/` dirs the yaml doesn't
        // know about, aged past the grace window.
        let Ok(entries) = fs::read_dir(&self.base) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(bare) = name.strip_prefix(GROUP_PREFIX) else {
                continue;
            };
            // Reserved names route elsewhere (`open` → the ADR-124
            // migration) — never sweep them here.
            if bare.is_empty() || bare == "open" || bare == "broadcast" {
                continue;
            }
            if state.contains_key(bare) {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let old_enough = fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age > orphan_grace);
            if !old_enough {
                continue;
            }
            // Re-read the yaml at the last moment: a `join` that
            // completed after our snapshot has already saved its
            // entry (entry-before-dir ordering), so this check is
            // what keeps a just-joined orphan dir out of the sweep.
            if load_groups(&self.base).contains_key(bare) {
                continue;
            }
            fs::remove_dir_all(&path).ok();
        }
    }

    // ── State persistence ──────────────────────────────────────

    fn load_state(&self) -> HashMap<String, GroupEntry> {
        load_groups(&self.base)
    }

    fn save_state(&self, state: &HashMap<String, GroupEntry>) {
        let path = self.state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let content = serialize_groups_yaml(state);
        // Per-writer tmp name. A shared `_groups.yaml.tmp` would let two
        // concurrent writers (multiple attend sessions + a chat instance)
        // interleave truncate-writes into the same file and publish a
        // torn hybrid via rename — worse than either writer's state,
        // and the line parser would round-trip the garbage as truth.
        // With unique tmps the failure mode collapses to plain
        // last-writer-wins on the rename, which callers already accept.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = path.with_extension(format!("yaml.{}-{}.tmp", std::process::id(), nanos));
        if fs::write(&tmp, &content).is_ok() {
            fs::rename(&tmp, &path).ok();
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────

/// Read-only load of the group state under `base`. This is the entry
/// point for consumers that render membership without acting for a
/// member (attend-chat's channel legend and chip glyphs).
pub fn load_groups(base: &Path) -> HashMap<String, GroupEntry> {
    let path = base.join("_groups.yaml");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    parse_groups_yaml(&content)
}

pub fn validate_group_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("group name cannot be empty".to_string());
    }
    if name.starts_with('_') || name.starts_with('@') {
        return Err("group name cannot start with _ or @".to_string());
    }
    if name.contains('/') || name.contains(' ') {
        return Err("group name cannot contain / or spaces".to_string());
    }
    // `broadcast` backs `_broadcast/`; `open` is the display name of
    // the base channel (ADR-124) — both would alias the commons if
    // we let a user create an explicit group with either name.
    if name == "broadcast" || name == "open" {
        return Err(format!("'{name}' is reserved"));
    }
    Ok(())
}

/// Check whether a member is still alive, using attend's heartbeat
/// sidecar (ADR-129). A claude session's attend touches its heartbeat
/// on every tick; a human's attend-chat touches the username heartbeat
/// on its refresh tick (ADR-170). Anything without a fresh heartbeat —
/// claude exited, attend never started, chat closed — is stale.
///
/// PID-aliveness is intentionally not checked: a claude with no running
/// attend cannot participate in the focus-group mesh, so for cleanup
/// purposes it is functionally identical to a dead claude.
pub fn member_alive(member_id: &str) -> bool {
    attend_heartbeat::is_fresh(member_id, attend_heartbeat::DEFAULT_GRACE)
}

/// Encode a project path: '/', '_', '.' → '-'
fn encode_project(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            '/' | '_' | '.' => '-',
            _ => c,
        })
        .collect()
}

// ── Minimal YAML parser/serializer ─────────────────────────────
// Keeps the crate dependency-free on serde.

pub fn parse_groups_yaml(content: &str) -> HashMap<String, GroupEntry> {
    let mut rooms = HashMap::new();
    let mut current_room: Option<String> = None;
    let mut current_pinned = false;
    let mut current_members: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Top-level: group name
        if indent == 0 && trimmed.ends_with(':') {
            // Save previous group
            if let Some(ref name) = current_room {
                rooms.insert(
                    name.clone(),
                    GroupEntry {
                        pinned: current_pinned,
                        members: current_members.clone(),
                    },
                );
            }
            current_room = Some(trimmed.trim_end_matches(':').to_string());
            current_pinned = false;
            current_members = Vec::new();
            continue;
        }

        // Second-level: group properties
        if indent == 2 {
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "pinned" => current_pinned = value == "true",
                    "members" => {} // array header, items follow
                    _ => {}
                }
            }
        }

        // Third-level: member list items
        if indent == 4 {
            if let Some(member) = trimmed.strip_prefix("- ") {
                current_members.push(member.to_string());
            }
        }
    }

    // Save last group
    if let Some(ref name) = current_room {
        rooms.insert(
            name.clone(),
            GroupEntry {
                pinned: current_pinned,
                members: current_members,
            },
        );
    }

    rooms
}

pub fn serialize_groups_yaml(state: &HashMap<String, GroupEntry>) -> String {
    let mut out = String::new();
    let mut names: Vec<&String> = state.keys().collect();
    names.sort();

    for name in names {
        let entry = &state[name];
        out.push_str(&format!("{name}:\n"));
        out.push_str(&format!("  pinned: {}\n", entry.pinned));
        out.push_str("  members:\n");
        for member in &entry.members {
            out.push_str(&format!("    - {member}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir_like() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "attend-groups-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn test_validate_group_name() {
        assert!(validate_group_name("deploy").is_ok());
        assert!(validate_group_name("my-room").is_ok());
        assert!(validate_group_name("").is_err());
        assert!(validate_group_name("_internal").is_err());
        assert!(validate_group_name("@bad").is_err());
        assert!(validate_group_name("has/slash").is_err());
        assert!(validate_group_name("has space").is_err());
        assert!(validate_group_name("broadcast").is_err());
        // ADR-124: `open` is the base channel display name — reserved
        // so nobody can shadow it with a real focus group.
        assert!(validate_group_name("open").is_err());
    }

    #[test]
    fn test_yaml_roundtrip() {
        let mut state = HashMap::new();
        state.insert(
            "deploy".to_string(),
            GroupEntry {
                pinned: true,
                members: vec!["session-abc".to_string(), "session-xyz".to_string()],
            },
        );
        state.insert(
            "collab".to_string(),
            GroupEntry {
                pinned: false,
                members: vec!["session-abc".to_string()],
            },
        );

        let yaml = serialize_groups_yaml(&state);
        let parsed = parse_groups_yaml(&yaml);

        assert_eq!(parsed.len(), 2);
        assert!(parsed["deploy"].pinned);
        assert_eq!(parsed["deploy"].members.len(), 2);
        assert!(!parsed["collab"].pinned);
        assert_eq!(parsed["collab"].members.len(), 1);
    }

    /// The exact byte sequence both former parsers (attend's and
    /// attend-chat's pre-ADR-170 mirror) agreed on. Kept as the wire-
    /// format anchor now that there is a single implementation: if
    /// parsing this string ever produces a different map, the on-disk
    /// format contract has changed and every deployed reader is at
    /// risk.
    const GROUPS_YAML_GOLDEN: &str = concat!(
        "\n",
        "# leading comment\n",
        "deploy:\n",
        "  pinned: true\n",
        "  members:\n",
        "    - sess-a\n",
        "    - sess-b\n",
        "\n",
        "infra:\n",
        "  pinned: false\n",
        "  members: []\n",
        "collab:\n",
        "  pinned: false\n",
        "  members:\n",
        "    - sess-c\n",
    );

    #[test]
    fn golden_parses_stably() {
        let parsed = parse_groups_yaml(GROUPS_YAML_GOLDEN);
        assert_eq!(parsed.len(), 3);
        assert!(parsed["deploy"].pinned);
        assert_eq!(parsed["deploy"].members, vec!["sess-a", "sess-b"]);
        assert!(!parsed["infra"].pinned);
        assert!(parsed["infra"].members.is_empty());
        assert!(!parsed["collab"].pinned);
        assert_eq!(parsed["collab"].members, vec!["sess-c"]);
    }

    #[test]
    fn join_then_leave_roundtrip() {
        let base = tempdir_like();
        let g = Groups::new(&base, "member-a");
        g.join("deploy", false).unwrap();
        assert!(base.join("@deploy").is_dir());
        assert_eq!(g.members("deploy").unwrap(), vec!["member-a"]);

        g.leave("deploy").unwrap();
        // Last member out of an unpinned group dissolves it.
        assert!(g.members("deploy").is_none());
        assert!(!base.join("@deploy").exists());
    }

    #[test]
    fn join_is_idempotent() {
        let base = tempdir_like();
        let g = Groups::new(&base, "member-a");
        g.join("deploy", false).unwrap();
        g.join("deploy", false).unwrap();
        assert_eq!(g.members("deploy").unwrap(), vec!["member-a"]);
    }

    #[test]
    fn leave_keeps_pinned_group() {
        let base = tempdir_like();
        let g = Groups::new(&base, "member-a");
        g.join("deploy", true).unwrap();
        g.leave("deploy").unwrap();
        // Pinned groups persist when empty.
        assert_eq!(g.members("deploy").unwrap(), Vec::<String>::new());
        assert!(base.join("@deploy").is_dir());
    }

    #[test]
    fn mixed_member_kinds_coexist() {
        // ADR-170: a members list can hold a claude session UUID and a
        // human username side by side; neither manager disturbs the
        // other's entry.
        let base = tempdir_like();
        let claude = Groups::new(&base, "2f2632d7-3f59-4c24-9aae-000000000000");
        let human = Groups::new(&base, "aaron");
        claude.join("deploy", false).unwrap();
        human.join("deploy", false).unwrap();
        let members = claude.members("deploy").unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&"aaron".to_string()));

        human.leave("deploy").unwrap();
        let members = claude.members("deploy").unwrap();
        assert_eq!(members, vec!["2f2632d7-3f59-4c24-9aae-000000000000"]);
    }

    #[test]
    fn load_groups_missing_yaml_is_empty() {
        let base = tempdir_like();
        assert!(load_groups(&base).is_empty());
    }

    #[test]
    fn cleanup_drops_stale_members_and_dissolves_empty_unpinned() {
        let base = tempdir_like();
        let g = Groups::new(&base, "dead-sess");
        g.join("temp", false).unwrap();
        // Everyone is stale → member removed, empty unpinned group
        // dissolved, dir gone.
        g.cleanup_stale_with(|_| false, attend_heartbeat::DEFAULT_GRACE);
        assert!(g.members("temp").is_none());
        assert!(!base.join("@temp").exists());
    }

    #[test]
    fn cleanup_keeps_live_members() {
        let base = tempdir_like();
        let g = Groups::new(&base, "live-sess");
        g.join("deploy", false).unwrap();
        g.cleanup_stale_with(|_| true, attend_heartbeat::DEFAULT_GRACE);
        assert_eq!(g.members("deploy").unwrap(), vec!["live-sess"]);
        assert!(base.join("@deploy").is_dir());
    }

    #[test]
    fn cleanup_sweeps_aged_orphan_dir() {
        let base = tempdir_like();
        fs::create_dir_all(base.join("@orphan")).unwrap();
        // Zero grace: any positive age qualifies — the dir was
        // created a moment ago, which is already "older than zero".
        std::thread::sleep(std::time::Duration::from_millis(5));
        Groups::new(&base, "x").cleanup_stale_with(|_| true, std::time::Duration::ZERO);
        assert!(!base.join("@orphan").exists());
    }

    #[test]
    fn cleanup_spares_fresh_orphan_dir() {
        // A just-created dir (e.g. a join mid-flight) survives the
        // sweep under the real grace window.
        let base = tempdir_like();
        fs::create_dir_all(base.join("@fresh")).unwrap();
        Groups::new(&base, "x").cleanup_stale_with(|_| true, attend_heartbeat::DEFAULT_GRACE);
        assert!(base.join("@fresh").is_dir());
    }

    #[test]
    fn cleanup_spares_yaml_listed_dir_regardless_of_age() {
        let base = tempdir_like();
        let g = Groups::new(&base, "live-sess");
        g.join("deploy", false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        g.cleanup_stale_with(|_| true, std::time::Duration::ZERO);
        assert!(base.join("@deploy").is_dir());
    }

    #[test]
    fn cleanup_never_sweeps_reserved_open_dir() {
        // `@open/` is the ADR-124 migration's responsibility — it
        // moves pending legacy signals into `_broadcast/`. The sweep
        // must not destroy them first, however old the dir is.
        let base = tempdir_like();
        fs::create_dir_all(base.join("@open")).unwrap();
        fs::write(base.join("@open").join("a.signal"), "from|p|/x|hi\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        Groups::new(&base, "x").cleanup_stale_with(|_| true, std::time::Duration::ZERO);
        assert!(base.join("@open").join("a.signal").exists());
    }

    #[test]
    fn test_encode_project() {
        assert_eq!(encode_project("/home/aaron/.claude"), "-home-aaron--claude");
    }
}

#[cfg(test)]
mod kick_tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("attend-groups-kick-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn kick_removes_target_and_keeps_others() {
        let b = base("basic");
        Groups::new(&b, "agent-1").join("deploy", false).unwrap();
        Groups::new(&b, "agent-2").join("deploy", false).unwrap();

        let op = Groups::new(&b, "operator");
        op.kick("deploy", "agent-1").unwrap();

        let members = op.members("deploy").expect("group survives");
        assert!(!members.contains(&"agent-1".to_string()));
        assert!(members.contains(&"agent-2".to_string()));
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn kick_reports_unknown_group_and_non_member() {
        let b = base("errors");
        Groups::new(&b, "agent-1").join("deploy", false).unwrap();
        let op = Groups::new(&b, "operator");
        assert!(op.kick("ghost", "agent-1").unwrap_err().contains("unknown group"));
        assert!(op.kick("deploy", "stranger").unwrap_err().contains("not a member"));
        fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn kicking_last_member_cleans_up_unpinned_group() {
        let b = base("cleanup");
        Groups::new(&b, "agent-1").join("temp", false).unwrap();
        let op = Groups::new(&b, "operator");
        op.kick("temp", "agent-1").unwrap();
        assert!(op.members("temp").is_none(), "empty unpinned group is removed");
        assert!(!b.join("@temp").exists());
        fs::remove_dir_all(&b).ok();
    }
}
