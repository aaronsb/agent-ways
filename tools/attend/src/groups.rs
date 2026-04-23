//! Named group management for attend (ADR-118).
//!
//! Groups are named signal namespaces that agents focus on and release.
//! Storage: `@group-name/` directories under the signals base, with membership
//! tracked in `_groups.yaml`.
//!
//! Every agent is always in its implicit project group (from cwd).
//! Named groups are explicit and opt-in via `attend focus on <name>`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Group membership entry.
#[derive(Debug, Clone)]
pub struct GroupEntry {
    /// Whether this group persists when empty.
    pub pinned: bool,
    /// Session IDs currently in this group.
    pub members: Vec<String>,
}

/// Group state manager.
#[derive(Clone)]
pub struct Groups {
    base: PathBuf,
    session_id: String,
}

const GROUP_PREFIX: &str = "@";

impl Groups {
    pub fn new(signals_base: &Path, session_id: &str) -> Self {
        Self {
            base: signals_base.to_path_buf(),
            session_id: session_id.to_string(),
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
    pub fn join(&self, name: &str, pin: bool) -> Result<(), String> {
        validate_group_name(name)?;
        let dir = self.group_dir(name);
        fs::create_dir_all(&dir).map_err(|e| format!("creating group dir: {e}"))?;

        let mut state = self.load_state();
        let entry = state.entry(name.to_string()).or_insert(GroupEntry {
            pinned: false,
            members: Vec::new(),
        });
        if !entry.members.contains(&self.session_id) {
            entry.members.push(self.session_id.clone());
        }
        if pin {
            entry.pinned = true;
        }
        self.save_state(&state);
        Ok(())
    }

    /// Leave a named group.
    pub fn leave(&self, name: &str) -> Result<(), String> {
        let mut state = self.load_state();
        if let Some(entry) = state.get_mut(name) {
            entry.members.retain(|m| m != &self.session_id);
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

    /// List rooms this session has joined.
    pub fn my_groups(&self) -> Vec<(String, bool)> {
        let state = self.load_state();
        state
            .iter()
            .filter(|(_, entry)| entry.members.contains(&self.session_id))
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

    /// List session IDs in a named group, or None if the group does not exist.
    /// Returns raw `_groups.yaml` membership — callers that need a
    /// liveness-checked view should intersect with
    /// `PeerSensor::live_session_ids` (see `cmd_send` for the routing-
    /// validation shape).
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

    /// Get group names this session is focused on (for signal routing).
    pub fn joined_group_names(&self) -> Vec<String> {
        self.my_groups().into_iter().map(|(name, _)| name).collect()
    }

    /// Get signal directories for all rooms this session should receive from.
    #[allow(dead_code)] // Used by signal routing (task 27)
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

    /// Clean up stale members (sessions that no longer exist).
    pub fn cleanup_stale(&self) {
        let mut state = self.load_state();
        let mut changed = false;

        for entry in state.values_mut() {
            let before = entry.members.len();
            entry.members.retain(|sid| session_alive(sid));
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
    }

    // ── State persistence ──────────────────────────────────────

    fn load_state(&self) -> HashMap<String, GroupEntry> {
        let path = self.state_path();
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return HashMap::new(),
        };
        parse_groups_yaml(&content)
    }

    fn save_state(&self, state: &HashMap<String, GroupEntry>) {
        let path = self.state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let content = serialize_groups_yaml(state);
        let tmp = path.with_extension("yaml.tmp");
        if fs::write(&tmp, &content).is_ok() {
            fs::rename(&tmp, &path).ok();
        }
    }

}

// ── Helpers ────────────────────────────────────────────────────

fn validate_group_name(name: &str) -> Result<(), String> {
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

/// One-shot migration for the legacy `@open/` focus group.
///
/// Pre-ADR-124 the `open` scene would create an `@open/` dir alongside
/// `_broadcast/`; post-ADR the base channel is `_broadcast/` and the
/// display name is `#open` (no `@open/` on disk). This helper makes
/// `attend run` idempotently clean up lingering state on the next
/// startup after an upgrade:
///
/// - move any `*.signal` files from `@open/` into `_broadcast/`
/// - remove the `@open/` dir and — if present — strip the `open:`
///   entry from `_groups.yaml`
///
/// **Non-signal files are destroyed.** The directory is removed
/// wholesale after the signal files are migrated; any `.tmp`, stray
/// lockfiles, or hand-placed notes under `@open/` go with it.
/// That's acceptable under attend's "only attend writes to its own
/// signal base" contract — no legitimate caller should have put
/// anything else there — but noted explicitly so nobody is surprised
/// later.
///
/// Returns the number of signal files moved, or `None` if there was
/// nothing to migrate (the common case after the first post-upgrade
/// run). `Some(0)` means `@open/` existed but had no signal files
/// to move — we still removed the dir, which is worth logging.
pub fn migrate_legacy_open_group(signals_base: &Path, groups: &Groups) -> Option<usize> {
    let open_dir = signals_base.join("@open");
    if !open_dir.is_dir() {
        return None;
    }
    let broadcast_dir = signals_base.join("_broadcast");
    fs::create_dir_all(&broadcast_dir).ok();

    let mut moved = 0;
    if let Ok(entries) = fs::read_dir(&open_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let src = entry.path();
            if src.extension().and_then(|s| s.to_str()) != Some("signal") {
                continue;
            }
            let Some(name) = src.file_name() else { continue };
            let dst = broadcast_dir.join(name);
            // Prefer rename (atomic, intra-fs); fall back to copy+remove
            // when rename fails (e.g. cross-device — unlikely here but
            // defensive). Ignore errors per-file: best-effort migration.
            if fs::rename(&src, &dst).is_ok() {
                moved += 1;
            } else if fs::copy(&src, &dst).is_ok() {
                fs::remove_file(&src).ok();
                moved += 1;
            }
        }
    }
    // Only drive `dissolve` (full `_groups.yaml` rewrite) when the
    // yaml actually has an `open:` entry to remove — otherwise the
    // migration pays a read-modify-write on every startup for no
    // reason, widening a race window against peer-session edits on
    // shared signal bases.
    if groups.has_group("open") {
        groups.dissolve("open");
    } else {
        fs::remove_dir_all(&open_dir).ok();
    }
    Some(moved)
}

/// Check if a Claude Code session is still alive by looking for its lock dir.
fn session_alive(session_id: &str) -> bool {
    // Claude sessions create dirs under ~/.claude/projects/
    // A session is alive if its PID parent process exists
    // For now, be conservative: assume alive (stale cleanup is best-effort)
    let _ = session_id;
    true // TODO: implement proper liveness check
}

/// Encode a project path: '/', '_', '.' → '-'
#[allow(dead_code)] // Used by receive_dirs
fn encode_project(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            '/' | '_' | '.' => '-',
            _ => c,
        })
        .collect()
}

// ── Minimal YAML parser/serializer ─────────────────────────────
// Keeps attend dependency-free on serde.

fn parse_groups_yaml(content: &str) -> HashMap<String, GroupEntry> {
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

fn serialize_groups_yaml(state: &HashMap<String, GroupEntry>) -> String {
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
    fn migrate_legacy_open_noop_when_absent() {
        // No `@open/` → returns None. Idempotent fast path on
        // subsequent startups.
        let base = std::env::temp_dir().join(format!(
            "attend-migrate-noop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&base).unwrap();
        let mgr = Groups::new(&base, "sess-test");
        assert!(migrate_legacy_open_group(&base, &mgr).is_none());
    }

    #[test]
    fn migrate_legacy_open_skips_yaml_rewrite_when_no_entry() {
        // PR #66 review S3: the migration must not rewrite
        // _groups.yaml when `open:` isn't present. A user who
        // hand-made `@open/` without an attend scene shouldn't
        // trigger a yaml churn that races peer sessions.
        let base = std::env::temp_dir().join(format!(
            "attend-migrate-noyaml-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(base.join("@open")).unwrap();
        fs::write(
            base.join("@open").join("a.signal"),
            "from|proj|/x|hi\n",
        )
        .unwrap();
        // Seed _groups.yaml with a *different* group so we can
        // observe whether the migration rewrites it. If it rewrites
        // without real work, the mtime will bump.
        let yaml_path = base.join("_groups.yaml");
        fs::write(
            &yaml_path,
            "infra:\n  pinned: false\n  members:\n    - sess-x\n",
        )
        .unwrap();
        let before = fs::metadata(&yaml_path).unwrap().modified().unwrap();

        let mgr = Groups::new(&base, "sess-test");
        let moved = migrate_legacy_open_group(&base, &mgr).unwrap();
        assert_eq!(moved, 1);
        assert!(base.join("_broadcast").join("a.signal").exists());
        assert!(!base.join("@open").exists());

        // The canary yaml was not rewritten — the mtime is
        // unchanged. (On fast filesystems the resolution may match,
        // but the actual file-content invariant is the reliable
        // signal, so check both.)
        let after = fs::metadata(&yaml_path).unwrap().modified().unwrap();
        let contents = fs::read_to_string(&yaml_path).unwrap();
        assert_eq!(before, after, "yaml must not be rewritten");
        assert!(contents.contains("infra:"));
        assert!(!contents.contains("open:"));
    }

    #[test]
    fn migrate_legacy_open_moves_signals_and_drops_dir() {
        let base = std::env::temp_dir().join(format!(
            "attend-migrate-move-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let open_dir = base.join("@open");
        fs::create_dir_all(&open_dir).unwrap();
        fs::write(open_dir.join("a.signal"), "from|proj|/x|hi\n").unwrap();
        fs::write(open_dir.join("b.signal"), "from|proj|/x|bye\n").unwrap();
        // A non-signal file — must be left alone (still in
        // @open/, until dissolve drops the dir).
        fs::write(open_dir.join("notes.txt"), "scratch").unwrap();

        // Pre-seed the `open` group in _groups.yaml so dissolve has
        // something to remove. Written directly because `join` now
        // rejects `"open"` as reserved — this simulates state left
        // by a pre-ADR-124 binary.
        fs::write(
            base.join("_groups.yaml"),
            "open:\n  pinned: false\n  members:\n    - sess-test\n",
        )
        .unwrap();
        let mgr = Groups::new(&base, "sess-test");

        let moved = migrate_legacy_open_group(&base, &mgr).unwrap();
        assert_eq!(moved, 2);

        let broadcast_dir = base.join("_broadcast");
        assert!(broadcast_dir.join("a.signal").exists());
        assert!(broadcast_dir.join("b.signal").exists());
        assert!(!open_dir.exists(), "@open/ should be gone after dissolve");
        // _groups.yaml should no longer mention `open`.
        let yaml = fs::read_to_string(base.join("_groups.yaml")).unwrap_or_default();
        assert!(!yaml.contains("open:"));
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

    #[test]
    fn test_encode_project() {
        assert_eq!(encode_project("/home/aaron/.claude"), "-home-aaron--claude");
    }

    /// The exact byte sequence parsed by the drift-detection test.
    /// Mirrored in `tools/attend-chat/src/groups.rs::tests::GROUPS_YAML_GOLDEN`.
    /// If you change this string, update the mirror — and add whatever
    /// new feature (new field, new nesting) to both parsers.
    pub(super) const GROUPS_YAML_GOLDEN: &str = concat!(
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
    fn golden_matches_mirror_parser() {
        // Wire-format drift guard. The attend-chat mirror parser has
        // an identical test with the same golden string — if either
        // parser stops producing this exact HashMap, one of the two
        // has drifted and the mirror contract is broken.
        let parsed = parse_groups_yaml(GROUPS_YAML_GOLDEN);
        assert_eq!(parsed.len(), 3);
        assert!(parsed["deploy"].pinned);
        assert_eq!(parsed["deploy"].members, vec!["sess-a", "sess-b"]);
        assert!(!parsed["infra"].pinned);
        assert!(parsed["infra"].members.is_empty());
        assert!(!parsed["collab"].pinned);
        assert_eq!(parsed["collab"].members, vec!["sess-c"]);
    }
}
