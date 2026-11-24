// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Live-peer roster for `/peers`, `/whois`, and the membership
//! commands (`/invite`, `/kick`).
//!
//! One session record = one persona (issue #394): the roster is keyed
//! by session id, with the record's cwd treated as an attribute. That
//! makes phantom siblings structurally impossible here — even if the
//! instance registry holds stale slots from an origin hop, a session
//! id can only ever produce one row.
//!
//! Distinct from `chip::known_identities`, which is a *rendering*
//! registry keyed by cwd (a chip must exist for whatever cwd a signal
//! carried, live or not). This module answers "who can I reach right
//! now, and where" — heartbeat-fresh sessions only, rooted at their
//! origin path (ADR-171).

use agent_identity::Identity;
use agent_theme::ColorDepth;
use attend_instances::SnapshotCache;

use crate::sessions::{discover as discover_sessions, DiscoveredSession};

/// One live peer session, resolvable by display name.
#[derive(Clone, Debug)]
pub struct Peer {
    /// Session UUID — the unit of existence (#394) and the membership
    /// key `_groups.yaml` stores for claudes.
    pub session_id: String,
    /// Origin ROOT path (ADR-171): the record's cwd with any managed-
    /// worktree suffix stripped, so a hop never changes the persona.
    /// Directed sends to this peer target `cwd_dir(&root)`.
    pub root: String,
    /// `Nickname-instance` as rendered everywhere else — what
    /// `/whois @name` and `/invite @name` match against.
    pub display: String,
}

/// Build the live roster from session records + heartbeats.
pub fn roster(caps: ColorDepth, instances: &SnapshotCache) -> Vec<Peer> {
    roster_with(
        &discover_sessions(),
        |sid| attend_presence::alive(sid, &Default::default()),
        caps,
        instances,
    )
}

/// Roster core with injectable seeds + liveness so tests can drive
/// the dedupe and normalization invariants without a heartbeat
/// sidecar or a real sessions dir.
pub fn roster_with<F: Fn(&str) -> bool>(
    seeds: &[DiscoveredSession],
    is_live: F,
    caps: ColorDepth,
    instances: &SnapshotCache,
) -> Vec<Peer> {
    let mut seen = std::collections::HashSet::new();
    let live: Vec<(&DiscoveredSession, String, Option<String>)> = seeds
        .iter()
        .filter(|s| is_live(&s.session_id))
        // Session id is the unit of existence: a second record (or a
        // registry slot at another cwd) for the same id must not mint
        // a sibling row (#394).
        .filter(|s| seen.insert(s.session_id.clone()))
        .map(|s| {
            // Identity anchors on the root so the name matches what
            // peers see on this session's signals even mid-hop. The
            // instance suffix may be registered under either the
            // record's live cwd or the root, so both are tried.
            let root = attend_presence::session::normalize_origin(&s.cwd);
            let instance = instances.lookup(&s.cwd, &s.session_id).or_else(|| instances.lookup(&root, &s.session_id));
            (s, root, instance)
        })
        .collect();
    live.iter()
        .map(|(s, root, instance)| {
            // The sessions with no row at this root, so their short ids differ.
            let rowless: Vec<&str> =
                live.iter().filter(|(_, r, inst)| inst.is_none() && r == root).map(|(o, ..)| o.session_id.as_str()).collect();
            let nickname = Identity::for_cwd(root, caps).nickname;
            Peer {
                session_id: s.session_id.clone(),
                root: root.clone(),
                display: attend_instances::session_name(nickname, &s.session_id, instance.as_deref(), &rowless),
            }
        })
        .collect()
}

/// Resolve an `@name` argument to a roster entry. Exact match only,
/// case-insensitive, `@` tolerated — these commands act on a specific
/// session (kick, invite), so the send path's fuzzy fallback would be
/// a misfeature here. Ambiguity refuses rather than picking.
pub fn resolve_peer<'a>(name: &str, peers: &'a [Peer]) -> Result<&'a Peer, String> {
    let bare = name.strip_prefix('@').unwrap_or(name);
    let lc = bare.to_ascii_lowercase();
    let hits: Vec<&Peer> = peers
        .iter()
        .filter(|p| p.display.to_ascii_lowercase() == lc)
        .collect();
    match hits.as_slice() {
        [single] => Ok(single),
        [] => Err(format!("@{bare}: unknown peer (try /peers)")),
        _ => Err(format!("@{bare}: ambiguous — multiple live sessions match")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cache() -> SnapshotCache {
        SnapshotCache::new()
    }

    fn seed(sid: &str, cwd: &str) -> DiscoveredSession {
        DiscoveredSession {
            cwd: cwd.to_string(),
            session_id: sid.to_string(),
        }
    }

    #[test]
    fn roster_filters_to_live_sessions() {
        let seeds = vec![seed("live-1", "/home/x"), seed("dead-1", "/home/y")];
        let r = roster_with(&seeds, |sid| sid == "live-1", ColorDepth::TrueColor, &empty_cache());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].session_id, "live-1");
    }

    #[test]
    fn roster_one_row_per_session_id() {
        // The #394 invariant: two records carrying the same session id
        // (e.g. a stale duplicate from an origin hop) collapse to one
        // persona — never two rows.
        let seeds = vec![
            seed("sess-a", "/home/proj"),
            seed("sess-a", "/home/proj/.claude/worktrees/hop"),
            seed("sess-b", "/home/other"),
        ];
        let r = roster_with(&seeds, |_| true, ColorDepth::TrueColor, &empty_cache());
        assert_eq!(r.len(), 2, "one row per session id, got {r:?}");
    }

    #[test]
    fn roster_root_strips_managed_worktree() {
        // A session whose record cwd sits inside a managed worktree
        // keeps its origin-root persona (ADR-171): same root, same
        // nickname as before the hop.
        let seeds = vec![seed("sess-a", "/home/me/proj/.claude/worktrees/feat")];
        let r = roster_with(&seeds, |_| true, ColorDepth::TrueColor, &empty_cache());
        assert_eq!(r[0].root, "/home/me/proj");
        assert_eq!(
            r[0].display,
            format!("{}·sess", Identity::for_cwd("/home/me/proj", ColorDepth::TrueColor).nickname)
        );
    }

    /// Two sessions with no registry row in one directory hold no name and
    /// show as the nickname and a short session id. Where the first four
    /// characters collide, the ids grow until they differ.
    #[test]
    fn two_rowless_sessions_render_distinctly() {
        let seeds = vec![
            seed("3fa2aaaa-0000-4000-8000-000000000001", "/home/x"),
            seed("3fa2bbbb-0000-4000-8000-000000000002", "/home/x"),
            seed("9c01cccc-0000-4000-8000-000000000003", "/home/x"),
        ];
        let r = roster_with(&seeds, |_| true, ColorDepth::TrueColor, &empty_cache());
        let nick = Identity::for_cwd("/home/x", ColorDepth::TrueColor).nickname;
        let shown: Vec<&str> = r.iter().map(|p| p.display.as_str()).collect();
        assert_eq!(shown, [format!("{nick}·3fa2a"), format!("{nick}·3fa2b"), format!("{nick}·9c01")]);
    }

    #[test]
    fn resolve_peer_exact_case_insensitive_with_optional_at() {
        let peers = vec![Peer {
            session_id: "s1".into(),
            root: "/home/x".into(),
            display: "Tamsin-alpha".into(),
        }];
        assert_eq!(resolve_peer("@tamsin-alpha", &peers).unwrap().session_id, "s1");
        assert_eq!(resolve_peer("Tamsin-alpha", &peers).unwrap().session_id, "s1");
        // No fuzzy fallback: membership actions must not guess.
        assert!(resolve_peer("@Tasmin-alpha", &peers).is_err());
    }

    #[test]
    fn resolve_peer_unknown_and_ambiguous_refuse() {
        let dup = |sid: &str| Peer {
            session_id: sid.into(),
            root: "/x".into(),
            display: "Cleo".into(),
        };
        let peers = vec![dup("s1"), dup("s2")];
        let err = resolve_peer("@Cleo", &peers).unwrap_err();
        assert!(err.contains("ambiguous"), "got: {err}");
        let err = resolve_peer("@Nobody", &peers).unwrap_err();
        assert!(err.contains("unknown peer"), "got: {err}");
    }
}
