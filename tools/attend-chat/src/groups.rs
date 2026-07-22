//! Focus-group discovery and membership reads for the TUI.
//!
//! We enumerate `@*/` subdirectories under the signals base (the
//! source of truth for "a group exists") and read `_groups.yaml` for
//! membership (the source of truth for "who's in it") through the
//! shared `attend-groups` crate — the single owner of the wire format
//! since ADR-170. The pre-ADR-170 byte-mirror parser (and its golden
//! drift tests) lived here; both now reside in `attend-groups`.
//!
//! Discovery (`scan`/`channels`) is pure file reads, re-invoked each
//! render: one `read_dir` + one `read_to_string`, always current
//! without invalidation bookkeeping. The write path (`/join`,
//! `/leave`) goes through `attend_groups::Groups` from the key
//! handlers in `crate::app::keys`.

use std::fs;
use std::path::{Path, PathBuf};

use agent_identity::{Group, TermCaps};
use attend_groups::GroupEntry;

use crate::signal::signals_base;

const GROUP_PREFIX: &str = "@";

/// One discovered group with its display identity baked in.
///
/// `is_base` marks the synthetic `#open` base channel — prepended by
/// [`channels`] regardless of on-disk state. A base-channel entry has
/// no membership and no underlying `@open/` directory; it's the TUI
/// surface for `_broadcast/` (see [`BASE_CHANNEL_NAME`]).
#[derive(Debug, Clone)]
pub struct KnownGroup {
    pub group: Group,
    pub membership: GroupEntry,
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
    let memberships = attend_groups::load_groups(base);
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
        membership: GroupEntry::default(),
        is_base: true,
    });
    out.extend(
        scan_in(base, caps)
            .into_iter()
            .filter(|g| g.group.name != BASE_CHANNEL_NAME),
    );
    out
}

/// Return the set of group names a given member (session UUID for
/// claudes, username for humans — ADR-170) is in. Pure filter over
/// the scanned groups.
pub fn groups_for_session<'a>(
    member_id: &str,
    known: &'a [KnownGroup],
) -> Vec<&'a KnownGroup> {
    known
        .iter()
        .filter(|k| k.membership.members.iter().any(|m| m == member_id))
        .collect()
}

/// Resolve a `#name` addressed send to the group's signal directory.
///
/// Returns `None` if the group is unknown — no on-disk dir *and* no
/// `_groups.yaml` entry. The yaml fallback is the self-heal for the
/// orphan-sweep race (ADR-170): a join whose dir got swept between
/// the sweep's yaml re-read and its `remove_dir_all` leaves an entry
/// with no dir, and `write_signal` re-creates the dir on the next
/// send — so a yaml-listed group is addressable even dir-less. The
/// special name `"open"` resolves to `_broadcast/` — the base
/// channel's on-disk home (ADR-124 §2); there is no `@open/` dir.
pub fn resolve_group_dir(name: &str) -> Option<PathBuf> {
    resolve_group_dir_in(&signals_base(), name)
}

/// Test-seam counterpart to [`resolve_group_dir`] — same logic
/// against an arbitrary base, immune to `$HOME` races in parallel
/// tests.
pub fn resolve_group_dir_in(base: &Path, name: &str) -> Option<PathBuf> {
    if name == BASE_CHANNEL_NAME {
        return Some(base.join("_broadcast"));
    }
    let dir = base.join(format!("{GROUP_PREFIX}{name}"));
    if dir.is_dir() || attend_groups::load_groups(base).contains_key(name) {
        Some(dir)
    } else {
        None
    }
}

/// Count of live (heartbeat-fresh) members of a focus group *other
/// than the sender*, derived from `_groups.yaml` membership
/// intersected with the heartbeat liveness gate (ADR-129). Human
/// members (ADR-170) heartbeat their username while a chat is open,
/// so they count the same way.
///
/// Mirrors the discipline `attend send --focus` enforces in
/// `tools/attend/src/cmd/send.rs` — closes the same silent-routing
/// trap (PR #75) on the attend-chat `#groupname` send path. A
/// signal written to a group with no live listeners would sit in
/// `@<name>/` until cleanup, with the sender seeing a confirmation
/// while no peer ever scans the file.
///
/// `exclude_member` is the sender's own membership key — since
/// ADR-170 the human is a heartbeating member, so without the
/// exclusion a human alone in a group would count *themselves* as
/// the live listener and the trap this gate closes would silently
/// reopen (the agent side excludes itself identically).
///
/// The base channel `#open` always returns a non-zero count — it
/// rides `_broadcast/`, which every attend scans, so liveness
/// validation is both unnecessary and would block legitimate
/// broadcasts when no other peers are present (a human typing in
/// chat is a valid send-only scenario).
pub fn live_peer_count(name: &str, exclude_member: &str) -> usize {
    live_peer_count_in(&signals_base(), name, exclude_member)
}

/// Test-seam counterpart to [`live_peer_count`] — same logic against
/// an arbitrary base directory. Tests can populate `_groups.yaml`
/// + heartbeats under a tempdir without touching `$HOME`.
pub fn live_peer_count_in(base: &Path, name: &str, exclude_member: &str) -> usize {
    live_peer_count_with(base, name, exclude_member, |sid| {
        attend_heartbeat::is_fresh(sid, attend_heartbeat::DEFAULT_GRACE)
    })
}

/// Innermost seam with an injectable freshness predicate, so the
/// self-exclusion rule is testable without a heartbeat sidecar.
fn live_peer_count_with<F: Fn(&str) -> bool>(
    base: &Path,
    name: &str,
    exclude_member: &str,
    is_fresh: F,
) -> usize {
    if name == BASE_CHANNEL_NAME {
        // Base channel is always reachable; bypass the check.
        return usize::MAX;
    }
    let memberships = attend_groups::load_groups(base);
    let Some(membership) = memberships.get(name) else {
        return 0;
    };
    membership
        .members
        .iter()
        .filter(|sid| sid.as_str() != exclude_member && is_fresh(sid))
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
        assert_eq!(live_peer_count_in(&base, "ghost", ""), 0);
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
        assert_eq!(live_peer_count_in(&base, "temp", ""), 0);
    }

    #[test]
    fn live_peer_count_excludes_the_sender() {
        // ADR-170 regression guard: the human is now a heartbeating
        // member, so a group where the sender is the only fresh
        // member must count zero live *peers* — otherwise a solo
        // `/join` + `#group send` self-validates and the message
        // sits unread (the silent-routing trap, PR #75).
        let base = tempdir_like();
        write_yaml(
            &base,
            "solo:\n  pinned: false\n  members:\n    - aaron\n",
        );
        assert_eq!(live_peer_count_with(&base, "solo", "aaron", |_| true), 0);
        // A different sender sees aaron as a live peer.
        assert_eq!(live_peer_count_with(&base, "solo", "someone-else", |_| true), 1);
    }

    #[test]
    fn resolve_group_dir_falls_back_to_yaml_entry() {
        // Orphan-sweep race self-heal: yaml entry present, dir
        // missing → still addressable (write_signal re-creates the
        // dir on send).
        let base = tempdir_like();
        write_yaml(
            &base,
            "phantom:\n  pinned: false\n  members:\n    - aaron\n",
        );
        let dir = resolve_group_dir_in(&base, "phantom")
            .expect("yaml-listed group must resolve");
        assert_eq!(dir, base.join("@phantom"));
        assert!(resolve_group_dir_in(&base, "never-existed").is_none());
    }

    #[test]
    fn live_peer_count_open_channel_is_unbounded() {
        // The base channel always returns a non-zero count so a
        // human typing `#open hello` is never blocked by the
        // "no live peers" check — broadcasts are always reachable.
        let base = tempdir_like();
        assert!(live_peer_count_in(&base, BASE_CHANNEL_NAME, "") > 0);
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
    fn groups_for_session_filters_by_member_id() {
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
    fn groups_for_session_matches_human_usernames() {
        // ADR-170: membership lists mix session UUIDs and usernames;
        // the filter must match a human's username key the same way.
        let base = tempdir_like();
        fs::create_dir_all(base.join("@deploy")).unwrap();
        write_yaml(
            &base,
            "deploy:\n  pinned: false\n  members:\n    - sess-a\n    - aaron\n",
        );
        let groups = scan_in(&base, TermCaps::Rich);
        let mine = groups_for_session("aaron", &groups);
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].group.name, "deploy");
    }
}
