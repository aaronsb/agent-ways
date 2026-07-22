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

use agent_identity::{Identity, TermCaps};
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
pub fn roster(caps: TermCaps, instances: &SnapshotCache) -> Vec<Peer> {
    roster_with(
        &discover_sessions(),
        |sid| attend_heartbeat::is_fresh(sid, attend_heartbeat::DEFAULT_GRACE),
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
    caps: TermCaps,
    instances: &SnapshotCache,
) -> Vec<Peer> {
    let mut seen = std::collections::HashSet::new();
    seeds
        .iter()
        .filter(|s| is_live(&s.session_id))
        // Session id is the unit of existence: a second record (or a
        // registry slot at another cwd) for the same id must not mint
        // a sibling row (#394).
        .filter(|s| seen.insert(s.session_id.clone()))
        .map(|s| {
            let root = attend_session::normalize_origin(&s.cwd);
            // Identity anchors on the root so the name matches what
            // peers see on this session's signals even mid-hop. The
            // instance suffix may be registered under either the
            // record's live cwd or the root — try both before
            // falling back to the bare nickname.
            let nickname = Identity::for_cwd(&root, caps).nickname;
            let display = instances
                .lookup(&s.cwd, &s.session_id)
                .or_else(|| instances.lookup(&root, &s.session_id))
                .map(|inst| format!("{nickname}-{inst}"))
                .unwrap_or_else(|| nickname.to_string());
            Peer {
                session_id: s.session_id.clone(),
                root,
                display,
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
        let r = roster_with(&seeds, |sid| sid == "live-1", TermCaps::Rich, &empty_cache());
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
        let r = roster_with(&seeds, |_| true, TermCaps::Rich, &empty_cache());
        assert_eq!(r.len(), 2, "one row per session id, got {r:?}");
    }

    #[test]
    fn roster_root_strips_managed_worktree() {
        // A session whose record cwd sits inside a managed worktree
        // keeps its origin-root persona (ADR-171): same root, same
        // nickname as before the hop.
        let seeds = vec![seed("sess-a", "/home/me/proj/.claude/worktrees/feat")];
        let r = roster_with(&seeds, |_| true, TermCaps::Rich, &empty_cache());
        assert_eq!(r[0].root, "/home/me/proj");
        assert_eq!(
            r[0].display,
            Identity::for_cwd("/home/me/proj", TermCaps::Rich).nickname
        );
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
