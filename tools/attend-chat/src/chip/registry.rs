//! Identity registry: dedup'd list of peers the legend / completion /
//! `@`-resolver consume, built from buffered signals plus a seed of
//! discovered claude sessions.
//!
//! Pure function — no IO, no state. Called every render; the buffer
//! cap keeps the work bounded. The "registry" naming distinguishes
//! this from the top-level `crate::legend` (which renders the legend
//! UI strip); this module only builds the data the UI consumes.

use agent_identity::{Identity, PaletteEntry, Style};
use agent_theme::ColorDepth;
use attend_instances::SnapshotCache;

use super::render::with_instance;
use crate::sessions::DiscoveredSession;
use crate::signal::Signal;

/// One identity known to the TUI, extracted from buffered signals.
/// Carries enough to render the legend and resolve an `@name` to its
/// routing destination.
#[derive(Clone, Debug)]
pub struct KnownIdentity {
    /// Nickname derived from the sender's cwd (claude) or username
    /// (external). This is what `@foo` in the input matches against.
    pub nickname: String,
    /// Full cwd of the sender. For claudes this is the routing key:
    /// a directed `@Nickname` message writes to `signals_base/<encoded-cwd>/`.
    /// For humans the cwd is their working directory when they sent —
    /// we still carry it so the legend can show `@aaron (Projects)`.
    pub cwd: String,
    /// Whether this identity is a claude (routable) or an external
    /// human (not routable to a cwd inbox).
    pub is_claude: bool,
    pub palette: PaletteEntry,
    pub style: Style,
}

/// Build the registry of known identities from a slice of buffered
/// signals plus a seed of discovered claude sessions. Pure function —
/// no IO, no state. Called every render; the buffer cap keeps the
/// signal work bounded and the seed is already a materialized list.
///
/// The seed exists so `@Nickname` Tab-completion and routing work
/// on a fresh TUI launch, *before* any peer has emitted a signal
/// that lands in our buffer. Signal-derived entries take priority
/// so ordering (newest-seen-first) still biases the legend toward
/// active peers; seed-only entries fall in after.
pub fn known_identities(
    signals: &[Signal],
    seeds: &[DiscoveredSession],
    caps: ColorDepth,
    instances: &SnapshotCache,
) -> Vec<KnownIdentity> {
    // Production wrapper: gate on heartbeat freshness (ADR-129).
    known_identities_with_liveness(signals, seeds, caps, claude_is_live, instances)
}

/// Same as [`known_identities`] but with an injectable claude-liveness
/// predicate. Production calls the wrapper above (which uses the
/// heartbeat sidecar); tests pass `|_| true` so they can drive the
/// dedup / ordering invariants without standing up a heartbeat
/// fixture for every synthetic session id.
pub fn known_identities_with_liveness<F>(
    signals: &[Signal],
    seeds: &[DiscoveredSession],
    caps: ColorDepth,
    is_live: F,
    instances: &SnapshotCache,
) -> Vec<KnownIdentity>
where
    F: Fn(&str) -> bool,
{
    // Liveness gate (ADR-129): a claude whose attend has not touched its
    // heartbeat within grace is dropped. Its messages stay in history
    // (with their dim chips), but `@`-completion and the agent legend
    // should not point at peers nobody can reach. Humans are not
    // liveness-filtered: they have no session, and an `@<user>` mention
    // does not route to a session inbox.
    seen_identities(signals, seeds, caps, is_live, |_| true, |_| false, instances)
        .into_iter()
        .filter(|s| s.live)
        .map(|s| s.identity)
        .collect()
}

/// What the legend and completion of a pool of `seen` identities keep:
/// the live claudes and every human, in their order. The same list
/// [`known_identities_with_liveness`] builds, taken from a pool already
/// built for the left column instead of a second walk of the signals.
pub fn known_of(seen: &[Seen]) -> Vec<KnownIdentity> {
    seen.iter().filter(|s| s.live || !s.identity.is_claude).map(|s| s.identity.clone()).collect()
}

/// An identity the chat has seen, and whether it is live now.
#[derive(Clone, Debug)]
pub struct Seen {
    pub identity: KnownIdentity,
    pub live: bool,
    /// The id a channel's membership holds for it: a claude's session id,
    /// a human's sanitized username (ADR-170).
    pub member: String,
}

/// Every identity the chat has seen, live or not: the senders of the
/// buffered signals, newest first, then the seeded sessions. A claude is
/// live when `claude_live` holds for its session id; a human when
/// `human_live` holds for its username. An entry seen twice is live when
/// either sighting is. One seen under two session ids (a session
/// restarted in the same cwd) keeps the id a channel lists, as
/// `in_channel` says, so a menu acting on its membership finds it. The
/// left column lists these (ADR-405); the legend and completion keep the
/// live ones ([`known_identities_with_liveness`]).
pub fn seen_identities<F, H, M>(
    signals: &[Signal],
    seeds: &[DiscoveredSession],
    caps: ColorDepth,
    claude_live: F,
    human_live: H,
    in_channel: M,
    instances: &SnapshotCache,
) -> Vec<Seen>
where
    F: Fn(&str) -> bool,
    H: Fn(&str) -> bool,
    M: Fn(&str) -> bool,
{
    // Walk from newest to oldest so the first time we see a cwd we
    // also capture its most-recent-seen position.
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out: Vec<Seen> = Vec::new();
    let mut note = |key: String, identity: KnownIdentity, live: bool, member: String, out: &mut Vec<Seen>| match at.get(&key) {
        Some(&i) => {
            out[i].live |= live;
            if !in_channel(&out[i].member) && in_channel(&member) {
                out[i].member = member;
            }
        }
        None => {
            at.insert(key, out.len());
            out.push(Seen { identity, live, member });
        }
    };
    for sig in signals.iter().rev() {
        let (primary_label, is_claude, id, live, member) = if let Some(sid) = sig.from.strip_prefix("claude:") {
            let id = Identity::for_cwd(&sig.cwd, caps);
            // Instance suffix: `@Tamsin-alpha` is the addressable name
            // — same-cwd siblings differ on the suffix and dedupe as
            // distinct entries in the legend.
            let display = with_instance(id.nickname, &sig.cwd, sid, instances);
            (display, true, id, claude_live(sid), sid.to_string())
        } else if let Some(rest) = sig.from.strip_prefix("external:") {
            let username = rest.split('@').next().unwrap_or(rest).to_string();
            let scope = agent_identity::cwd_basename(&sig.cwd);
            let id = Identity::for_user(&username, &scope, caps);
            let live = human_live(&username);
            let member = agent_identity::sanitize_id_component(&username);
            (username, false, id, live, member)
        } else {
            continue; // unknown prefix — don't pollute the legend
        };

        // Dedupe shape differs by kind:
        // - Claudes are identified by cwd (the routing key). Two
        //   claudes in different cwds are genuinely different
        //   agents and should both appear in the legend.
        // - Humans have no routing cwd, and the same human
        //   sending from two terminals is still the same person.
        //   Collapse them on `(nickname, is_claude)` so the
        //   legend doesn't show `@aaron @aaron` when a user has
        //   attend-chat running in more than one cwd.
        let key = if is_claude {
            format!("{}\x1f1\x1f{}", primary_label, sig.cwd)
        } else {
            format!("{}\x1f0", primary_label)
        };
        let identity = KnownIdentity { nickname: primary_label, cwd: sig.cwd.clone(), is_claude, palette: id.palette, style: id.style };
        note(key, identity, live, member, &mut out);
    }

    // Seed from discovered sessions — any claude cwd we haven't
    // already registered from a signal. Keys match the signal-
    // branch format so the same cwd doesn't double-register.
    // A session.json on disk is not sufficient evidence that attend is
    // running for that session: liveness is the heartbeat's.
    for seed in seeds {
        let id = Identity::for_cwd(&seed.cwd, caps);
        let display = with_instance(id.nickname, &seed.cwd, &seed.session_id, instances);
        let key = format!("{}\x1f1\x1f{}", display, seed.cwd);
        let identity = KnownIdentity { nickname: display, cwd: seed.cwd.clone(), is_claude: true, palette: id.palette, style: id.style };
        note(key, identity, claude_live(&seed.session_id), seed.session_id.clone(), &mut out);
    }
    out
}

/// Heartbeat-based liveness check (ADR-129). Wrapped here so the
/// known_identities filter has a single name for the gate; the
/// underlying `attend_presence::alive` is the source of truth.
fn claude_is_live(session_id: &str) -> bool {
    attend_presence::alive(session_id, &Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cache() -> SnapshotCache {
        SnapshotCache::new()
    }

    fn sig(from: &str, cwd: &str) -> Signal {
        Signal {
            id: "t".into(),
            from: from.into(),
            project: cwd.rsplit('/').next().unwrap_or("?").into(),
            cwd: cwd.into(),
            reply_to: None,
            message: "msg".into(),
            ts: 0,
            channel: crate::signal::Channel::Open,
        }
    }

    #[test]
    fn registry_dedupes_repeat_senders() {
        let buf = vec![
            sig("claude:a", "/home/x"),
            sig("claude:a", "/home/x"), // same cwd, should dedup
            sig("claude:b", "/home/y"),
        ];
        let reg = known_identities_with_liveness(&buf, &[], ColorDepth::TrueColor, |_| true, &empty_cache());
        assert_eq!(reg.len(), 2, "expected 2 unique identities, got {}", reg.len());
    }

    #[test]
    fn registry_newest_first() {
        let buf = vec![
            sig("claude:a", "/home/x"),
            sig("claude:b", "/home/y"),
        ];
        let reg = known_identities_with_liveness(&buf, &[], ColorDepth::TrueColor, |_| true, &empty_cache());
        // Buffer order is oldest→newest; registry should surface the
        // most-recent cwd first so active peers lead the legend.
        let y_id = Identity::for_cwd("/home/y", ColorDepth::TrueColor);
        assert_eq!(reg[0].nickname, y_id.nickname);
    }

    #[test]
    fn the_legend_from_the_seen_pool_is_the_legend_built_afresh() {
        let buf = vec![sig("claude:a", "/home/x"), sig("external:aaron@kitty", "/home/aaron"), sig("claude:b", "/home/y")];
        let alive = |sid: &str| sid == "a";
        let seen = seen_identities(&buf, &[], ColorDepth::TrueColor, alive, |_| false, |_| false, &empty_cache());
        let names = |v: Vec<KnownIdentity>| v.into_iter().map(|k| k.nickname).collect::<Vec<_>>();
        let fresh = names(known_identities_with_liveness(&buf, &[], ColorDepth::TrueColor, alive, &empty_cache()));
        assert_eq!(names(known_of(&seen)), fresh, "live claudes and every human, away or not");
        assert_eq!(fresh.len(), 2);
    }

    #[test]
    fn one_name_seen_under_two_sessions_keeps_the_id_a_channel_lists() {
        // `old` is in a channel; `new` is the same cwd's later session.
        // The buffer is newest last, and the walk goes in reverse, so
        // `new` is met first.
        let buf = vec![sig("claude:old", "/home/x"), sig("claude:new", "/home/x")];
        let member = |in_channel: &dyn Fn(&str) -> bool| {
            let seen = seen_identities(&buf, &[], ColorDepth::TrueColor, |_| true, |_| false, in_channel, &empty_cache());
            assert_eq!(seen.len(), 1, "one name and cwd is one row");
            seen[0].member.clone()
        };
        assert_eq!(member(&|id| id == "old"), "old", "the id the channel holds");
        assert_eq!(member(&|id| id == "new"), "new");
        assert_eq!(member(&|_| false), "new", "in no channel: the newest");
    }

    #[test]
    fn registry_includes_humans() {
        let buf = vec![sig("external:aaron@kitty", "/home/aaron/Projects")];
        let reg = known_identities_with_liveness(&buf, &[], ColorDepth::TrueColor, |_| true, &empty_cache());
        assert_eq!(reg.len(), 1);
        assert_eq!(reg[0].nickname, "aaron");
        assert!(!reg[0].is_claude);
    }

    #[test]
    fn registry_collapses_same_human_across_cwds() {
        // Same user sending from two different cwds (e.g. attend-chat
        // running in ~/Projects and in ~/.claude) should surface once.
        // Humans aren't routable to a cwd, so distinguishing them by
        // cwd just pollutes the legend with `@aaron @aaron`.
        let buf = vec![
            sig("external:aaron@kitty", "/home/aaron/Projects"),
            sig("external:aaron@kitty", "/home/aaron/.claude"),
        ];
        let reg = known_identities_with_liveness(&buf, &[], ColorDepth::TrueColor, |_| true, &empty_cache());
        let aarons: Vec<_> = reg.iter().filter(|k| k.nickname == "aaron").collect();
        assert_eq!(aarons.len(), 1, "expected a single aaron entry, got {}", aarons.len());
    }

    #[test]
    fn registry_still_distinguishes_claudes_per_cwd() {
        // Two claudes in different cwds are different agents — they
        // must both appear.
        let buf = vec![
            sig("claude:a", "/home/me/proj-a"),
            sig("claude:b", "/home/me/proj-b"),
        ];
        let reg = known_identities_with_liveness(&buf, &[], ColorDepth::TrueColor, |_| true, &empty_cache());
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn registry_skips_unknown_prefix() {
        let buf = vec![sig("mystery:abc", "/tmp")];
        let reg = known_identities_with_liveness(&buf, &[], ColorDepth::TrueColor, |_| true, &empty_cache());
        assert!(reg.is_empty(), "unknown prefix should be ignored, got {reg:?}");
    }

    #[test]
    fn registry_seeds_from_sessions_when_buffer_empty() {
        // Cold-start case: no signals yet, but the sessions dir has
        // claudes. `@Nickname` autocomplete + routing must still work.
        let seeds = vec![
            DiscoveredSession { cwd: "/home/x".to_string(), session_id: "sx".into() },
            DiscoveredSession { cwd: "/home/y".to_string(), session_id: "sy".into() },
        ];
        let reg = known_identities_with_liveness(&[], &seeds, ColorDepth::TrueColor, |_| true, &empty_cache());
        assert_eq!(reg.len(), 2);
        assert!(reg.iter().all(|k| k.is_claude));
    }

    #[test]
    fn registry_signal_entries_precede_seed_only_entries() {
        // Ordering invariant: signal-derived entries keep their
        // newest-first position; seed-only entries fall in after.
        // A loop-order refactor that swapped the two passes would
        // flip this. `/X` is the signal, `/Y` is the seed — `/X`
        // must appear first so active peers lead the legend.
        let buf = vec![sig("claude:a", "/X")];
        let seeds = vec![DiscoveredSession {
            cwd: "/Y".to_string(),
            session_id: "sy".into(),
        }];
        let reg = known_identities_with_liveness(&buf, &seeds, ColorDepth::TrueColor, |_| true, &empty_cache());
        assert_eq!(reg.len(), 2);
        assert_eq!(reg[0].cwd, "/X", "signal-derived entry must lead");
        assert_eq!(reg[1].cwd, "/Y", "seed-only entry falls in after");
    }

    #[test]
    fn registry_seed_doesnt_duplicate_signal_derived_entry() {
        // If the same cwd is in both the signal buffer and the seed,
        // we must not list it twice.
        let buf = vec![sig("claude:a", "/home/x")];
        let seeds = vec![DiscoveredSession {
            cwd: "/home/x".to_string(),
            session_id: "sx".into(),
        }];
        let reg = known_identities_with_liveness(&buf, &seeds, ColorDepth::TrueColor, |_| true, &empty_cache());
        assert_eq!(reg.len(), 1);
    }
}
