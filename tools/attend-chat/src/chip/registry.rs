//! Identity registry: dedup'd list of peers the legend / completion /
//! `@`-resolver consume, built from buffered signals plus a seed of
//! discovered claude sessions.
//!
//! Pure function — no IO, no state. Called every render; the buffer
//! cap keeps the work bounded. The "registry" naming distinguishes
//! this from the top-level `crate::legend` (which renders the legend
//! UI strip); this module only builds the data the UI consumes.

use std::collections::{HashMap, HashSet};

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
    // Production wrapper: gate on attend's liveness (ADR-129).
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
/// buffered signals the operator view shows, newest first, then the seeded
/// sessions. A direct message between others names no one: its sender is
/// listed only when a session or another message lists it. A claude is
/// live when `claude_live` holds for its session id; a human when
/// `human_live` holds for its username. Each live claude session is an
/// entry of its own, whatever name it shares, so a direct message reaches
/// the live session it names (ADR-401, `to`). Dead sightings of one name
/// and cwd are one entry, which keeps the id a channel lists, as
/// `in_channel` says, so a menu acting on its membership finds it; it is
/// dropped when a live session answers to the same name and cwd and the
/// channel lists no dead id. The left column lists these (ADR-405); the
/// legend and completion keep the live ones
/// ([`known_identities_with_liveness`]).
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
    for sig in signals.iter().rev().filter(|s| crate::tabs::shows(s, &crate::tabs::Tab::Merged)) {
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
            claude_key(&primary_label, &sig.cwd, &member, live)
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
    // running for that session: liveness is attend's (`claude_is_live`).
    for seed in seeds {
        let id = Identity::for_cwd(&seed.cwd, caps);
        let display = with_instance(id.nickname, &seed.cwd, &seed.session_id, instances);
        let live = claude_live(&seed.session_id);
        let key = claude_key(&display, &seed.cwd, &seed.session_id, live);
        let identity = KnownIdentity { nickname: display, cwd: seed.cwd.clone(), is_claude: true, palette: id.palette, style: id.style };
        note(key, identity, live, seed.session_id.clone(), &mut out);
    }
    // A dead sighting gives way to a live session of the same name and cwd,
    // unless a channel still lists its id.
    let answered = |d: &Seen, out: &[Seen]| {
        out.iter().any(|l| l.live && l.identity.is_claude && l.identity.nickname == d.identity.nickname && l.identity.cwd == d.identity.cwd)
    };
    let dropped: Vec<bool> = out.iter().map(|s| s.identity.is_claude && !s.live && !in_channel(&s.member) && answered(s, &out)).collect();
    let mut i = 0;
    out.retain(|_| {
        i += 1;
        !dropped[i - 1]
    });
    out
}

/// The left column's rows once the session lineage and the tombstones are
/// read (ADR-404 D3e). `earlier` maps each id a session had before a
/// `/clear` to the session that carries it on now. A dead sighting under
/// such an id folds into that session's row when the session is live,
/// whatever name each was seen under, unless a channel still lists the
/// dead id (`in_channel`); the live row takes the place of the newer of the
/// two. A dead session that is tombstoned makes no row.
pub fn settle<M>(seen: Vec<Seen>, earlier: &HashMap<String, String>, tombstoned: &HashSet<String>, in_channel: M) -> Vec<Seen>
where
    M: Fn(&str) -> bool,
{
    let dead = |s: &Seen| s.identity.is_claude && !s.live;
    let live_at = |id: &str| seen.iter().position(|l| l.live && l.identity.is_claude && l.member == id);
    let mut out: Vec<Seen> = Vec::with_capacity(seen.len());
    let mut placed: HashSet<usize> = HashSet::new();
    for (i, s) in seen.iter().enumerate() {
        if placed.contains(&i) {
            continue;
        }
        if dead(s) && tombstoned.contains(&s.member) {
            continue;
        }
        let carried = earlier.get(&s.member).and_then(|id| live_at(id));
        match carried {
            Some(j) if dead(s) && !in_channel(&s.member) => {
                if placed.insert(j) {
                    out.push(seen[j].clone());
                }
            }
            _ => {
                placed.insert(i);
                out.push(s.clone());
            }
        }
    }
    out
}

/// The key a claude sighting merges under: a live one by its session id,
/// so two live sessions never share an entry; dead ones by name and cwd.
fn claude_key(display: &str, cwd: &str, session_id: &str, live: bool) -> String {
    if live {
        format!("{display}\x1f1\x1f{cwd}\x1f{session_id}")
    } else {
        format!("{display}\x1f1\x1f{cwd}")
    }
}

/// Liveness check (ADR-129): a fresh heartbeat, or a listening session
/// whose Claude Code process still runs. Wrapped here so the
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
            tray: String::new(),
            beside_post: false,
        }
    }

    /// The sender of a direct message between others, read from another
    /// tray, adds no identity: only what the operator view shows does.
    #[test]
    fn a_dm_between_others_adds_no_identity() {
        let mut aside = sig("claude:c", "/home/z");
        aside.channel = crate::signal::Channel::Elsewhere("b".into());
        let buf = vec![sig("claude:a", "/home/x"), aside];
        let seen = seen_identities(&buf, &[], ColorDepth::TrueColor, |_| true, |_| false, |_| false, &empty_cache());
        assert_eq!(seen.iter().map(|s| s.member.as_str()).collect::<Vec<_>>(), ["a"]);
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
        // `old` is the same cwd's earlier session, now dead; `new` is live.
        // The buffer is newest last, and the walk goes in reverse, so `new`
        // is met first.
        let buf = vec![sig("claude:old", "/home/x"), sig("claude:new", "/home/x")];
        let rows = |in_channel: &dyn Fn(&str) -> bool| {
            seen_identities(&buf, &[], ColorDepth::TrueColor, |sid| sid == "new", |_| false, in_channel, &empty_cache())
                .into_iter()
                .map(|s| (s.member, s.live))
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(&|id| id == "old"), [("new".to_string(), true), ("old".to_string(), false)], "the id the channel holds stays listed");
        assert_eq!(rows(&|_| false), [("new".to_string(), true)], "in no channel, the dead sighting gives way");
    }

    /// Review of #974, D2: a dead session's sighting and a live seed at one
    /// cwd once merged into one live entry carrying the dead id, so an
    /// `@Name` send was addressed to a session nobody reads as.
    #[test]
    fn a_live_seed_is_not_carried_by_a_dead_sighting_of_its_name() {
        let dead = "aaaaaaaa-0000-4000-8000-000000000001";
        let live = "bbbbbbbb-0000-4000-8000-000000000002";
        let buf = vec![sig(&format!("claude:{dead}"), "/home/x")];
        let seeds = vec![DiscoveredSession { cwd: "/home/x".into(), session_id: live.into() }];
        let seen = seen_identities(&buf, &seeds, ColorDepth::TrueColor, |sid| sid == live, |_| false, |_| false, &empty_cache());
        let rows: Vec<(&str, bool)> = seen.iter().map(|s| (s.member.as_str(), s.live)).collect();
        assert_eq!(rows, [(live, true)]);
    }

    /// A message sent before `/clear` replaced the session's id reads as the
    /// live session's name, so the dead sighting merges into its row.
    #[test]
    fn a_sighting_from_the_id_before_a_clear_is_the_live_sessions_row() {
        let base = std::env::temp_dir().join(format!("attend-chat-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let registry = attend_instances::Registry::with_base(base.clone());
        registry.register("/home/x", "other").unwrap();
        registry.register("/home/x", "old").unwrap();
        registry.rename("/home/x", "old", "new").unwrap();
        let cache = SnapshotCache::with_registry(attend_instances::Registry::with_base(base.clone()));
        let buf = vec![sig("claude:old", "/home/x")];
        let seeds = vec![DiscoveredSession { cwd: "/home/x".into(), session_id: "new".into() }];
        let seen = seen_identities(&buf, &seeds, ColorDepth::TrueColor, |sid| sid == "new", |_| false, |_| false, &cache);
        std::fs::remove_dir_all(&base).ok();
        let nickname = Identity::for_cwd("/home/x", ColorDepth::TrueColor).nickname;
        let rows: Vec<(&str, &str, bool)> = seen.iter().map(|s| (s.identity.nickname.as_str(), s.member.as_str(), s.live)).collect();
        assert_eq!(rows, [(format!("{nickname}-beta").as_str(), "new", true)]);
    }

    /// Two live sessions at one cwd with one name are two entries.
    #[test]
    fn two_live_sessions_of_one_name_are_two_entries() {
        let seeds = vec![
            DiscoveredSession { cwd: "/home/x".into(), session_id: "s1".into() },
            DiscoveredSession { cwd: "/home/x".into(), session_id: "s2".into() },
        ];
        let seen = seen_identities(&[], &seeds, ColorDepth::TrueColor, |_| true, |_| false, |_| false, &empty_cache());
        let members: Vec<&str> = seen.iter().map(|s| s.member.as_str()).collect();
        assert_eq!(members, ["s1", "s2"]);
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
            session_id: "a".into(),
        }];
        let reg = known_identities_with_liveness(&buf, &seeds, ColorDepth::TrueColor, |_| true, &empty_cache());
        assert_eq!(reg.len(), 1);
    }

    /// Two sessions of one cwd, each holding its own slot, so their names
    /// differ: `old` dead and seen in the buffer, `new` live.
    fn two_names() -> (Vec<Seen>, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("attend-chat-lineage-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&base);
        let registry = attend_instances::Registry::with_base(base.clone());
        registry.register("/home/x", "old").unwrap();
        registry.register("/home/x", "new").unwrap();
        let cache = SnapshotCache::with_registry(attend_instances::Registry::with_base(base.clone()));
        let buf = vec![sig("claude:old", "/home/x")];
        let seeds = vec![DiscoveredSession { cwd: "/home/x".into(), session_id: "new".into() }];
        (seen_identities(&buf, &seeds, ColorDepth::TrueColor, |sid| sid == "new", |_| false, |_| false, &cache), base)
    }

    fn members(seen: &[Seen]) -> Vec<(&str, bool)> {
        seen.iter().map(|s| (s.member.as_str(), s.live)).collect()
    }

    /// A dead sighting under an id the live session had before a `/clear`
    /// folds into the live row, though the two were seen under two names;
    /// the live row takes the dead one's place, the newer.
    #[test]
    fn a_dead_sighting_under_an_earlier_id_folds_into_the_live_row() {
        let (seen, base) = two_names();
        std::fs::remove_dir_all(&base).ok();
        assert_ne!(seen[0].identity.nickname, seen[1].identity.nickname, "the names alone fold nothing");
        assert_eq!(members(&seen), [("old", false), ("new", true)]);
        let earlier: HashMap<String, String> = [("old".to_string(), "new".to_string())].into();
        assert_eq!(members(&settle(seen.clone(), &earlier, &HashSet::new(), |_| false)), [("new", true)]);
        assert_eq!(members(&settle(seen.clone(), &earlier, &HashSet::new(), |id| id == "old")), [("old", false), ("new", true)], "a channel still lists the dead id");
        assert_eq!(members(&settle(seen, &HashMap::new(), &HashSet::new(), |_| false)), [("old", false), ("new", true)], "no lineage, no fold");
    }

    /// A tombstoned session makes no row, from a sighting or a seed; a live
    /// one, back under `--resume`, keeps its row.
    #[test]
    fn a_tombstoned_session_makes_no_row() {
        let buf = vec![sig("claude:gone", "/home/x"), sig("claude:back", "/home/y")];
        let seeds = vec![DiscoveredSession { cwd: "/home/z".into(), session_id: "seeded".into() }];
        let seen = seen_identities(&buf, &seeds, ColorDepth::TrueColor, |sid| sid == "back", |_| false, |_| false, &empty_cache());
        assert_eq!(members(&seen).len(), 3);
        let tombstoned: HashSet<String> = ["gone", "back", "seeded"].map(String::from).into();
        assert_eq!(members(&settle(seen, &HashMap::new(), &tombstoned, |_| false)), [("back", true)]);
    }
}
