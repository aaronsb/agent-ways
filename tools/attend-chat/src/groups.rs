//! Focus-group discovery and membership reads for the TUI.
//!
//! Read-only mirror of the data model owned by `attend::groups`. We
//! enumerate `@*/` subdirectories under the signals base (the source
//! of truth for "a group exists") and parse `_groups.yaml` for
//! membership (the source of truth for "who's in it").
//!
//! **Format mirror.** `_groups.yaml` is written by
//! `tools/attend/src/groups.rs::serialize_groups_yaml`. Our parser
//! mirrors its shape one-to-one — two-space indent, list items at
//! four-space indent, no comments emitted — so any format change
//! here requires the same change there. We re-parse rather than
//! share because this crate only *reads* the file today; a shared
//! I/O layer lands with PR 4's `/join` write path.
//!
//! Everything in this module is pure file reads. Callers re-invoke
//! `scan` each render; it's cheap (one `read_dir` + one
//! `read_to_string`) and always sees the current state without
//! invalidation bookkeeping.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use agent_identity::{Group, TermCaps};

use crate::signal::signals_base;

const GROUP_PREFIX: &str = "@";
const STATE_FILE: &str = "_groups.yaml";

/// Membership entry for one group.
#[derive(Debug, Clone, Default)]
pub struct GroupMembership {
    pub pinned: bool,
    /// Session IDs currently in this group. For claude sessions these
    /// are UUIDs; for future human entries they'll be derived hashes.
    pub members: Vec<String>,
}

/// One discovered group with its display identity baked in.
#[derive(Debug, Clone)]
pub struct KnownGroup {
    pub group: Group,
    pub membership: GroupMembership,
}

/// Scan the signals base for groups.
///
/// Returns every `@<name>/` directory that exists on disk, each
/// paired with its membership entry from `_groups.yaml` (or an empty
/// membership if the yaml doesn't mention it — a group dir can exist
/// before `_groups.yaml` catches up).
pub fn scan(caps: TermCaps) -> Vec<KnownGroup> {
    scan_in(&signals_base(), caps)
}

/// Scan under an arbitrary base. Exists so tests can drive the scan
/// against a scratch directory without touching `$HOME`.
pub fn scan_in(base: &Path, caps: TermCaps) -> Vec<KnownGroup> {
    let memberships = load_memberships(base);
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };
    let mut out: Vec<KnownGroup> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            if !e.file_type().ok()?.is_dir() {
                return None;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let bare = name.strip_prefix(GROUP_PREFIX)?.to_string();
            if bare.is_empty() {
                return None;
            }
            let membership = memberships.get(&bare).cloned().unwrap_or_default();
            Some(KnownGroup {
                group: Group::for_name(&bare, caps),
                membership,
            })
        })
        .collect();
    // Alphabetical order so the legend is stable across renders.
    // Groups aren't temporally ordered the way agents are (no
    // "newest-seen" concept) — alpha is the least-surprising default.
    out.sort_by(|a, b| a.group.name.cmp(&b.group.name));
    out
}

/// Return the set of group names a given session (by its ID) is in.
/// Pure map lookup over the parsed yaml.
pub fn groups_for_session<'a>(
    session_id: &str,
    known: &'a [KnownGroup],
) -> Vec<&'a KnownGroup> {
    known
        .iter()
        .filter(|k| k.membership.members.iter().any(|m| m == session_id))
        .collect()
}

fn load_memberships(base: &Path) -> HashMap<String, GroupMembership> {
    let path = base.join(STATE_FILE);
    let Ok(content) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    parse_groups_yaml(&content)
}

/// Parse the narrowly-shaped `_groups.yaml` attend writes.
///
/// Mirrors `attend::groups::parse_groups_yaml` exactly. We could pull
/// in a YAML crate, but the schema is two levels deep and a full
/// parser would burn compile time and binary size for nothing.
fn parse_groups_yaml(content: &str) -> HashMap<String, GroupMembership> {
    let mut out: HashMap<String, GroupMembership> = HashMap::new();
    let mut current: Option<String> = None;
    let mut pinned = false;
    let mut members: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        if indent == 0 && trimmed.ends_with(':') {
            if let Some(name) = current.take() {
                out.insert(
                    name,
                    GroupMembership {
                        pinned,
                        members: std::mem::take(&mut members),
                    },
                );
            }
            current = Some(trimmed.trim_end_matches(':').to_string());
            pinned = false;
            continue;
        }

        if indent == 2 {
            if let Some((key, value)) = trimmed.split_once(':') {
                if key.trim() == "pinned" {
                    pinned = value.trim() == "true";
                }
                // `members:` is an array header — items follow at
                // indent=4. Other keys we silently ignore so future
                // fields don't make old readers panic.
            }
        }

        if indent == 4 {
            if let Some(m) = trimmed.strip_prefix("- ") {
                members.push(m.to_string());
            }
        }
    }

    if let Some(name) = current {
        out.insert(name, GroupMembership { pinned, members });
    }
    out
}

/// Resolve a `#name` addressed send to the group's signal directory.
/// Returns `None` if the named group has no on-disk dir (i.e. the
/// user typed an unknown group name).
pub fn resolve_group_dir(name: &str) -> Option<PathBuf> {
    let dir = signals_base().join(format!("{GROUP_PREFIX}{name}"));
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tempdir_like() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "attend-chat-groups-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_yaml(base: &Path, content: &str) {
        let mut f = fs::File::create(base.join("_groups.yaml")).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn scan_finds_at_prefixed_dirs() {
        let base = tempdir_like();
        fs::create_dir_all(base.join("@deploy")).unwrap();
        fs::create_dir_all(base.join("@infra")).unwrap();
        fs::create_dir_all(base.join("_broadcast")).unwrap(); // ignored
        fs::create_dir_all(base.join("-home-x")).unwrap(); // cwd dir, ignored
        let groups = scan_in(&base, TermCaps::Rich);
        let names: Vec<_> = groups.iter().map(|g| g.group.name.as_str()).collect();
        assert_eq!(names, vec!["deploy", "infra"]);
    }

    #[test]
    fn scan_pairs_membership_from_yaml() {
        let base = tempdir_like();
        fs::create_dir_all(base.join("@deploy")).unwrap();
        write_yaml(
            &base,
            "deploy:\n  pinned: true\n  members:\n    - sess-a\n    - sess-b\n",
        );
        let groups = scan_in(&base, TermCaps::Rich);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group.name, "deploy");
        assert!(groups[0].membership.pinned);
        assert_eq!(groups[0].membership.members.len(), 2);
    }

    #[test]
    fn scan_empty_if_no_base() {
        let base = tempdir_like().join("nope"); // never created
        let groups = scan_in(&base, TermCaps::Rich);
        assert!(groups.is_empty());
    }

    #[test]
    fn scan_dir_without_yaml_has_empty_membership() {
        let base = tempdir_like();
        fs::create_dir_all(base.join("@orphan")).unwrap();
        // no _groups.yaml at all
        let groups = scan_in(&base, TermCaps::Rich);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].membership.members.is_empty());
        assert!(!groups[0].membership.pinned);
    }

    #[test]
    fn groups_for_session_filters_by_uuid() {
        let base = tempdir_like();
        fs::create_dir_all(base.join("@deploy")).unwrap();
        fs::create_dir_all(base.join("@infra")).unwrap();
        write_yaml(
            &base,
            "deploy:\n  pinned: false\n  members:\n    - sess-a\ninfra:\n  pinned: false\n  members:\n    - sess-b\n",
        );
        let groups = scan_in(&base, TermCaps::Rich);
        let mine = groups_for_session("sess-a", &groups);
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].group.name, "deploy");
    }

    #[test]
    fn parse_yaml_handles_blank_lines_and_comments() {
        let yaml = "\n# a comment\ndeploy:\n  pinned: true\n  members:\n    - sess-a\n\n# another comment\ninfra:\n  pinned: false\n  members: []\n";
        let parsed = parse_groups_yaml(yaml);
        assert_eq!(parsed.len(), 2);
        assert!(parsed["deploy"].pinned);
        assert_eq!(parsed["deploy"].members, vec!["sess-a"]);
        assert!(parsed["infra"].members.is_empty());
    }

    #[test]
    fn parse_yaml_unknown_keys_ignored() {
        // Forward compatibility: if attend adds a field, we shouldn't
        // panic — just skip it.
        let yaml = "deploy:\n  pinned: false\n  magic_flag: true\n  members:\n    - sess-a\n";
        let parsed = parse_groups_yaml(yaml);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed["deploy"].members, vec!["sess-a"]);
    }

    #[test]
    fn empty_name_dir_ignored() {
        let base = tempdir_like();
        fs::create_dir_all(base.join("@")).unwrap();
        let groups = scan_in(&base, TermCaps::Rich);
        assert!(groups.is_empty());
    }

    /// Mirror of `tools/attend/src/groups.rs::tests::GROUPS_YAML_GOLDEN`.
    /// Any edit here requires the matching edit there — both parsers
    /// must agree on the identical byte sequence. Drift-detection via
    /// `golden_matches_mirror_parser` below.
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
    fn golden_matches_mirror_parser() {
        // Wire-format drift guard. `tools/attend/src/groups.rs` has
        // an identical test with the same golden string — if either
        // parser stops producing this exact HashMap, the mirror
        // contract is broken and one side has drifted.
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
