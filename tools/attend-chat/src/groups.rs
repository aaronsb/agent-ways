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
///
/// `is_base` marks the synthetic `#open` base channel — prepended by
/// [`channels`] regardless of on-disk state. A base-channel entry has
/// no membership and no underlying `@open/` directory; it's the TUI
/// surface for `_broadcast/` (see [`BASE_CHANNEL_NAME`]).
#[derive(Debug, Clone)]
pub struct KnownGroup {
    pub group: Group,
    pub membership: GroupMembership,
    pub is_base: bool,
}

/// Display name for the base channel rendered as `#open`. Also the
/// disk name we filter out of the regular scan so a lingering
/// `@open/` dir can't double-render as a second `#open` chip.
pub const BASE_CHANNEL_NAME: &str = "open";

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
                is_base: false,
            })
        })
        .collect();
    // Alphabetical order so the legend is stable across renders.
    // Groups aren't temporally ordered the way agents are (no
    // "newest-seen" concept) — alpha is the least-surprising default.
    out.sort_by(|a, b| a.group.name.cmp(&b.group.name));
    out
}

/// Channel-bar view: base `#open` followed by every discovered group.
///
/// The base channel is synthesised at the head of the list so it
/// always renders leftmost and never depends on `@open/` existing on
/// disk (per ADR-124 §1). Any literal `open` directory surfaced by
/// [`scan`] is dropped — `#open` is the base's display name, and the
/// real traffic rides `_broadcast/` under the hood (ADR-124 §2).
///
/// Groups after `#open` preserve the order [`scan`] returns them
/// in — alphabetical today. Richer ordering (pinned, recent) is
/// deferred per ADR-124 §3.
pub fn channels(caps: TermCaps) -> Vec<KnownGroup> {
    channels_in(&signals_base(), caps)
}

/// Test-seam counterpart to [`channels`] — same shape, arbitrary base.
pub fn channels_in(base: &Path, caps: TermCaps) -> Vec<KnownGroup> {
    let mut out = Vec::with_capacity(8);
    out.push(KnownGroup {
        group: Group::for_name(BASE_CHANNEL_NAME, caps),
        membership: GroupMembership::default(),
        is_base: true,
    });
    out.extend(
        scan_in(base, caps)
            .into_iter()
            .filter(|g| g.group.name != BASE_CHANNEL_NAME),
    );
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
///
/// Returns `None` if the named group has no on-disk dir (i.e. the
/// user typed an unknown group name). The special name `"open"`
/// resolves to `_broadcast/` — that's the base channel's
/// on-disk home (ADR-124 §2); there is no `@open/` directory.
pub fn resolve_group_dir(name: &str) -> Option<PathBuf> {
    if name == BASE_CHANNEL_NAME {
        return Some(crate::signal::broadcast_dir());
    }
    let dir = signals_base().join(format!("{GROUP_PREFIX}{name}"));
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Count of live (heartbeat-fresh) members of a focus group, derived
/// from `_groups.yaml` membership intersected with the heartbeat
/// liveness gate (ADR-129).
///
/// Mirrors the discipline `attend send --focus` enforces in
/// `tools/attend/src/cmd/send.rs` — closes the same silent-routing
/// trap (PR #75) on the attend-chat `#groupname` send path. A
/// signal written to a group with no live listeners would sit in
/// `@<name>/` until cleanup, with the sender seeing a confirmation
/// while no peer ever scans the file.
///
/// The base channel `#open` always returns a non-zero count — it
/// rides `_broadcast/`, which every attend scans, so liveness
/// validation is both unnecessary and would block legitimate
/// broadcasts when no other peers are present (a human typing in
/// chat is a valid send-only scenario).
pub fn live_peer_count(name: &str) -> usize {
    live_peer_count_in(&signals_base(), name)
}

/// Test-seam counterpart to [`live_peer_count`] — same logic against
/// an arbitrary base directory. Tests can populate `_groups.yaml`
/// + heartbeats under a tempdir without touching `$HOME`.
pub fn live_peer_count_in(base: &Path, name: &str) -> usize {
    if name == BASE_CHANNEL_NAME {
        // Base channel is always reachable; bypass the check.
        return usize::MAX;
    }
    let memberships = load_memberships(base);
    let Some(membership) = memberships.get(name) else {
        return 0;
    };
    membership
        .members
        .iter()
        .filter(|sid| {
            attend_heartbeat::is_fresh(sid, attend_heartbeat::DEFAULT_GRACE)
        })
        .count()
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
    fn channels_prepends_base_with_empty_scan() {
        // No groups on disk → the channel bar still shows `#open`.
        // This is the ADR-124 §1 invariant: base never hides.
        let base = tempdir_like();
        let ch = channels_in(&base, TermCaps::Rich);
        assert_eq!(ch.len(), 1);
        assert!(ch[0].is_base);
        assert_eq!(ch[0].group.name, BASE_CHANNEL_NAME);
        assert!(ch[0].membership.members.is_empty());
    }

    #[test]
    fn channels_base_leads_real_groups() {
        let base = tempdir_like();
        fs::create_dir_all(base.join("@deploy")).unwrap();
        fs::create_dir_all(base.join("@infra")).unwrap();
        let ch = channels_in(&base, TermCaps::Rich);
        let names: Vec<_> = ch.iter().map(|c| c.group.name.as_str()).collect();
        assert_eq!(names, vec!["open", "deploy", "infra"]);
        assert!(ch[0].is_base);
        assert!(!ch[1].is_base);
        assert!(!ch[2].is_base);
    }

    #[test]
    fn channels_drops_literal_open_dir() {
        // A lingering `@open/` dir from before ADR-124 must not
        // double-render. The synthetic base replaces it; real
        // traffic rides `_broadcast/`.
        let base = tempdir_like();
        fs::create_dir_all(base.join("@open")).unwrap();
        fs::create_dir_all(base.join("@deploy")).unwrap();
        let ch = channels_in(&base, TermCaps::Rich);
        let names: Vec<_> = ch.iter().map(|c| c.group.name.as_str()).collect();
        // Exactly one `open` — the synthetic base — and `deploy`
        // follows. The on-disk `@open/` is filtered out.
        assert_eq!(names, vec!["open", "deploy"]);
        assert!(ch[0].is_base);
    }

    #[test]
    fn live_peer_count_zero_for_unknown_group() {
        let base = tempdir_like();
        // No yaml, no entry → 0.
        assert_eq!(live_peer_count_in(&base, "ghost"), 0);
    }

    #[test]
    fn live_peer_count_zero_for_group_with_no_fresh_members() {
        // The yaml lists a member, but the heartbeat for that member
        // is missing — `is_fresh` returns false, so the live count
        // is zero. This is the silent-routing trap fix: a group
        // entry that survives an exited peer must not validate.
        let base = tempdir_like();
        write_yaml(
            &base,
            "temp:\n  pinned: false\n  members:\n    - dead-session-id\n",
        );
        assert_eq!(live_peer_count_in(&base, "temp"), 0);
    }

    #[test]
    fn live_peer_count_open_channel_is_unbounded() {
        // The base channel always returns a non-zero count so a
        // human typing `#open hello` is never blocked by the
        // "no live peers" check — broadcasts are always reachable.
        let base = tempdir_like();
        assert!(live_peer_count_in(&base, BASE_CHANNEL_NAME) > 0);
    }

    #[test]
    fn resolve_group_dir_open_routes_to_broadcast() {
        // `#open` writes land in `_broadcast/` — ADR-124 §2.
        // Point $HOME at a temp dir so the resolver doesn't touch
        // the real cache.
        let home = tempdir_like();
        std::env::set_var("HOME", &home);
        let dir = resolve_group_dir("open").expect("open must resolve");
        assert_eq!(dir, crate::signal::broadcast_dir());
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
