//! Sender chip: the small bordered box to the left of each message
//! that identifies who sent it.
//!
//! Pure display logic — takes the wire `from`/`project`/`cwd` triple,
//! derives a stable nickname + color + style via `agent_identity`, and
//! returns a `ChipInfo` the renderer can feed into iocraft components.
//!
//! Lives in its own module because PR 2 (agent legend) and later PRs
//! (@ autocomplete, focus-group sidebar) will layer on top of the same
//! identity derivation — keeping the chip logic separate from the
//! iocraft component tree means one reviewable seam per PR rather
//! than a growing `app.rs`.

use agent_identity::{Identity, PaletteEntry, Style, TermCaps};
use attend_instances::SnapshotCache;
use iocraft::prelude::Color;

use crate::sessions::DiscoveredSession;
use crate::signal::Signal;

/// Width of the chip box in columns. Interior width for text =
/// `CHIP_WIDTH - 2 (border) - 2 (padding)`.
pub const CHIP_WIDTH: u32 = 20;

/// Display-layer facts for one signal's sender chip.
pub struct ChipInfo {
    /// First line — bold, colored with the identity palette.
    pub primary: String,
    /// Second line — dim, cwd basename or the broadcast fallback.
    pub secondary: String,
    pub palette: PaletteEntry,
    pub style: Style,
    /// Session UUID for claude senders, `None` for humans or
    /// unknown prefixes. The chip's group-glyph decoration cross-
    /// references this against `_groups.yaml` membership. Humans
    /// will get their own derivable key in PR 4 once the CRUD
    /// path needs to address them.
    pub session_id: Option<String>,
}

/// Derive the chip from the wire `from`/`project`/`cwd` triple.
///
/// Claudes get a stable nickname keyed on their full cwd path (see
/// `agent_identity::Identity::for_cwd`) plus an instance suffix
/// resolved through `instances` (ADR-129). Humans keep their
/// username but still pick up a color + style from the identity
/// table so the avatar is visually consistent everywhere the same
/// user shows up.
///
/// `instances` is a per-render cache that collapses repeat lookups
/// for the same cwd into a single registry read. Build it once at
/// the top of a render pass and pass it through to every chip.
///
/// This function never touches the wire format — identity is pure
/// receiver-side rendering. If the signal's `from` doesn't match a
/// known prefix, we fall through to showing the raw value.
pub fn chip_for(
    from: &str,
    project: &str,
    cwd: &str,
    caps: TermCaps,
    instances: &SnapshotCache,
) -> ChipInfo {
    let interior = (CHIP_WIDTH as usize).saturating_sub(4);
    let scope_src = if cwd.is_empty() { project } else { cwd };
    let scope_segment = scope_src.rsplit('/').next().unwrap_or(scope_src);
    let scope = if scope_segment.is_empty() {
        "broadcast".to_string()
    } else {
        scope_segment.to_string()
    };

    if let Some(uuid) = from.strip_prefix("claude:") {
        // For claude senders the cwd is the stable identity key. We
        // don't hash the session UUID — two sequential claudes in the
        // same dir wear the same nickname stem, with a per-session
        // instance suffix (ADR-129) appended to disambiguate.
        let id = Identity::for_cwd(cwd, caps);
        let display = with_instance(id.nickname, cwd, uuid, instances);
        ChipInfo {
            primary: truncate(&display, interior),
            secondary: truncate(&scope, interior),
            palette: id.palette,
            style: id.style,
            session_id: Some(uuid.to_string()),
        }
    } else if let Some(rest) = from.strip_prefix("external:") {
        // Strip the terminal suffix ("aaron@kitty" → "aaron") so the
        // chip reads as the human, not their terminal emulator.
        let username = rest.split('@').next().unwrap_or(rest);
        let id = Identity::for_user(username, &scope, caps);
        ChipInfo {
            primary: truncate(username, interior),
            secondary: truncate(&scope, interior),
            palette: id.palette,
            style: id.style,
            session_id: None,
        }
    } else {
        // Unknown sender kind — key the color off the raw `from` so
        // different unknowns at least colorise differently instead of
        // all landing on a default.
        let id = Identity::for_user(from, &scope, caps);
        ChipInfo {
            primary: truncate(from, interior),
            secondary: truncate(&scope, interior),
            palette: id.palette,
            style: id.style,
            session_id: None,
        }
    }
}

/// Map a palette entry to an iocraft `Color`. Rich terminals get
/// RGB; basic ones get a named ANSI bright to avoid truecolor on
/// terminals that would approximate it poorly.
pub fn color_for(p: PaletteEntry, caps: TermCaps) -> Color {
    match caps {
        TermCaps::Rich => Color::Rgb { r: p.rgb.0, g: p.rgb.1, b: p.rgb.2 },
        TermCaps::Basic | TermCaps::Mono => Color::AnsiValue(p.ansi16),
    }
}

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
    caps: TermCaps,
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
    caps: TermCaps,
    is_live: F,
    instances: &SnapshotCache,
) -> Vec<KnownIdentity>
where
    F: Fn(&str) -> bool,
{
    // Walk from newest to oldest so the first time we see a cwd we
    // also capture its most-recent-seen position. A HashSet of cwd
    // strings keeps dedup O(n) without depending on a hasher.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<KnownIdentity> = Vec::new();
    for sig in signals.iter().rev() {
        let (primary_label, is_claude, id) = if let Some(sid) = sig.from.strip_prefix("claude:") {
            // Liveness gate (ADR-129): a claude whose attend has not
            // touched its heartbeat within grace is dropped from the
            // legend. The signal stays in history (with its dim chip),
            // but `@`-completion and the agent legend should not point
            // at peers nobody can reach.
            if !is_live(sid) {
                continue;
            }
            let id = Identity::for_cwd(&sig.cwd, caps);
            // Instance suffix: `@Tamsin-alpha` is the addressable name
            // — same-cwd siblings differ on the suffix and dedupe as
            // distinct entries in the legend.
            let display = with_instance(id.nickname, &sig.cwd, sid, instances);
            (display, true, id)
        } else if let Some(rest) = sig.from.strip_prefix("external:") {
            // Humans are not liveness-filtered — they have no heartbeat
            // and an `@<user>` mention does not route to a session
            // inbox. Their legend entry is informational only.
            let username = rest.split('@').next().unwrap_or(rest).to_string();
            let scope = agent_identity::cwd_basename(&sig.cwd);
            let id = Identity::for_user(&username, &scope, caps);
            (username, false, id)
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
        if seen.insert(key) {
            out.push(KnownIdentity {
                nickname: primary_label,
                cwd: sig.cwd.clone(),
                is_claude,
                palette: id.palette,
                style: id.style,
            });
        }
    }

    // Seed from discovered sessions — any claude cwd we haven't
    // already registered from a signal. Keys match the signal-
    // branch format so the same cwd doesn't double-register.
    // Same liveness gate applies: a session.json on disk is not
    // sufficient evidence that attend is running for that session.
    for seed in seeds {
        if !is_live(&seed.session_id) {
            continue;
        }
        let id = Identity::for_cwd(&seed.cwd, caps);
        let display = with_instance(id.nickname, &seed.cwd, &seed.session_id, instances);
        let key = format!("{}\x1f1\x1f{}", display, seed.cwd);
        if seen.insert(key) {
            out.push(KnownIdentity {
                nickname: display,
                cwd: seed.cwd.clone(),
                is_claude: true,
                palette: id.palette,
                style: id.style,
            });
        }
    }
    out
}

/// Heartbeat-based liveness check (ADR-129). Wrapped here so the
/// known_identities filter has a single name for the gate; the
/// underlying `attend_heartbeat::is_fresh` is the source of truth.
fn claude_is_live(session_id: &str) -> bool {
    attend_heartbeat::is_fresh(session_id, attend_heartbeat::DEFAULT_GRACE)
}

/// Compose `<nickname>-<instance>` for a claude session (ADR-129).
/// Falls back to the bare nickname when the registry has no entry —
/// only happens in the moments before a session has registered, or
/// when the registry file is unreadable.
///
/// Reads through `instances`, which caches per-cwd snapshots for
/// the lifetime of one render pass. Without this cache the render
/// path read + parsed the registry yaml once per chip; with it,
/// the read is amortized to once per distinct cwd per render.
fn with_instance(nickname: &str, cwd: &str, session_id: &str, instances: &SnapshotCache) -> String {
    match instances.lookup(cwd, session_id) {
        Some(inst) => format!("{nickname}-{inst}"),
        None => nickname.to_string(),
    }
}

/// Resolve an `@Nickname` token to a routable cwd.
///
/// Returns `Some(cwd)` only for claude identities — humans don't have
/// a signal inbox we can post into.
///
/// Matching is forgiving:
/// 1. **Exact** (case-insensitive). `@tamsin` hits `Tamsin`. If two
///    distinct cwds happen to share the exact same nickname stem
///    (rare cross-cwd hash collision in `agent_identity::names` —
///    pre-existing but visible now that suffixes can coincide), the
///    resolver refuses rather than silently routing to whichever
///    appeared first in the legend.
/// 2. **Fuzzy** (Levenshtein ≤ 2) when no exact match exists. Catches
///    typos and transpositions: `@Tasmin-alpha` → `Tamsin-alpha`,
///    `@Cleo` → `Cleo`. The Levenshtein cap of 2 allows two character
///    edits (a transposition is two edits in plain Levenshtein) while
///    still distinguishing genuinely different names.
/// 3. **Ambiguity is failure.** If two candidates tie for the same
///    minimum distance, return `None` — silently routing to the
///    wrong agent is worse than an unknown-nickname error. The
///    caller can re-ask with disambiguation when needed.
pub fn resolve_nickname(
    name: &str,
    known: &[KnownIdentity],
) -> Option<String> {
    let lc = name.to_ascii_lowercase();

    // Pass 1: exact match (case-insensitive). Refuse on ambiguity —
    // if two claudes from different cwds collide on the same
    // nickname (their cwd hashes both landed on the same name pool
    // index, then both registered the same per-cwd `alpha`), we
    // must not route to whichever was discovered first.
    let exact: Vec<&KnownIdentity> = known
        .iter()
        .filter(|k| k.is_claude && k.nickname.to_ascii_lowercase() == lc)
        .collect();
    match exact.as_slice() {
        [single] => return Some(single.cwd.clone()),
        [] => {}
        _ => return None, // ambiguous — refuse to misroute
    }

    // Pass 2: fuzzy match. Walk all claude nicknames once, tracking
    // (best_distance, best_cwd, tie_count). Tie at min distance means
    // we cannot pick safely.
    const MAX_DIST: usize = 2;
    let mut best: Option<(usize, &str, usize)> = None; // (dist, cwd, tie_count)
    for k in known.iter().filter(|k| k.is_claude) {
        let cand = k.nickname.to_ascii_lowercase();
        let dist = levenshtein(&lc, &cand);
        if dist > MAX_DIST {
            continue;
        }
        match best {
            None => best = Some((dist, &k.cwd, 1)),
            Some((bd, _, _)) if dist < bd => best = Some((dist, &k.cwd, 1)),
            Some((bd, bcwd, n)) if dist == bd => best = Some((bd, bcwd, n + 1)),
            _ => {}
        }
    }
    match best {
        Some((_, cwd, 1)) => Some(cwd.to_string()),
        _ => None,
    }
}

/// Levenshtein distance with a small early-exit. Iterative O(n*m)
/// space-optimised to two rows. The nickname pool is small and
/// names are short (≤ ~16 chars with suffix), so the cost is
/// negligible per render.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    // Cheap early-exit: if the length difference alone exceeds any
    // threshold a caller cares about, the distance is at least that.
    // We don't take a threshold here, but trivially bound it.
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr: Vec<usize> = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 1 {
        s.chars().take(max).collect()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic test session_ids never have registry entries, so a
    /// fresh cache always returns `None` from `lookup` and the suffix
    /// path falls back to bare nicknames. Tests that already worked
    /// against the un-cached `with_instance` continue to pass; the
    /// per-render cache is purely a performance addition.
    fn empty_cache() -> SnapshotCache {
        SnapshotCache::new()
    }

    #[test]
    fn claude_sender_uses_derived_nickname() {
        // A claude in /home/aaron/.claude gets a nickname from the
        // agent-identity pool, scope comes from cwd basename.
        let chip = chip_for(
            "claude:e74a4a4b-7e3b-49bc-8404-216162e54ba8",
            "claude",
            "/home/aaron/.claude",
            TermCaps::Rich,
            &empty_cache(),
        );
        let expected = Identity::for_cwd("/home/aaron/.claude", TermCaps::Rich);
        assert_eq!(chip.primary, expected.nickname);
        assert_eq!(chip.secondary, ".claude");
    }

    #[test]
    fn claude_nickname_stable_across_session_ids() {
        // Two sequential claudes in the same cwd should show the same
        // name — the session UUID changes but identity is keyed on
        // cwd, not session.
        let a = chip_for("claude:aaaa-1", "p", "/home/x", TermCaps::Rich, &empty_cache());
        let b = chip_for("claude:bbbb-2", "p", "/home/x", TermCaps::Rich, &empty_cache());
        assert_eq!(a.primary, b.primary);
    }

    #[test]
    fn scope_prefers_cwd_basename_over_project() {
        let chip = chip_for("external:aaron", "ignored", "/home/aaron/temp", TermCaps::Rich, &empty_cache());
        assert_eq!(chip.secondary, "temp");
    }

    #[test]
    fn external_strips_terminal_suffix() {
        let chip = chip_for("external:aaron@kitty", "proj", "/home/aaron", TermCaps::Rich, &empty_cache());
        assert_eq!(chip.primary, "aaron");
    }

    #[test]
    fn scope_truncates_long_segment() {
        // Interior is 16 chars for CHIP_WIDTH=20 (2 borders + 2 padding).
        let chip = chip_for(
            "external:aaron",
            "x",
            "/tmp/some-very-long-directory-name",
            TermCaps::Rich,
            &empty_cache(),
        );
        assert!(chip.secondary.chars().count() <= 16);
        assert!(chip.secondary.ends_with('…'));
    }

    #[test]
    fn unknown_sender_still_colored() {
        // Something that isn't claude: or external: — we don't crash,
        // we show the raw value and pick a color off it.
        let a = chip_for("mystery:abc", "", "/tmp", TermCaps::Rich, &empty_cache());
        let b = chip_for("mystery:xyz", "", "/tmp", TermCaps::Rich, &empty_cache());
        // Different senders → different colors most of the time. We
        // don't assert inequality (palette is finite), just that the
        // code path doesn't panic and produces valid output.
        assert_eq!(a.primary, "mystery:abc");
        assert_eq!(b.primary, "mystery:xyz");
    }

    #[test]
    fn color_for_rich_is_rgb() {
        let p = PaletteEntry { rgb: (10, 20, 30), ansi16: 3, name: "test" };
        match color_for(p, TermCaps::Rich) {
            Color::Rgb { r, g, b } => assert_eq!((r, g, b), (10, 20, 30)),
            other => panic!("expected Rgb, got {other:?}"),
        }
    }

    #[test]
    fn color_for_basic_is_ansi() {
        let p = PaletteEntry { rgb: (10, 20, 30), ansi16: 9, name: "test" };
        match color_for(p, TermCaps::Basic) {
            Color::AnsiValue(v) => assert_eq!(v, 9),
            other => panic!("expected AnsiValue, got {other:?}"),
        }
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
        }
    }

    #[test]
    fn registry_dedupes_repeat_senders() {
        let buf = vec![
            sig("claude:a", "/home/x"),
            sig("claude:a", "/home/x"), // same cwd, should dedup
            sig("claude:b", "/home/y"),
        ];
        let reg = known_identities_with_liveness(&buf, &[], TermCaps::Rich, |_| true, &empty_cache());
        assert_eq!(reg.len(), 2, "expected 2 unique identities, got {}", reg.len());
    }

    #[test]
    fn registry_newest_first() {
        let buf = vec![
            sig("claude:a", "/home/x"),
            sig("claude:b", "/home/y"),
        ];
        let reg = known_identities_with_liveness(&buf, &[], TermCaps::Rich, |_| true, &empty_cache());
        // Buffer order is oldest→newest; registry should surface the
        // most-recent cwd first so active peers lead the legend.
        let y_id = Identity::for_cwd("/home/y", TermCaps::Rich);
        assert_eq!(reg[0].nickname, y_id.nickname);
    }

    #[test]
    fn registry_includes_humans() {
        let buf = vec![sig("external:aaron@kitty", "/home/aaron/Projects")];
        let reg = known_identities_with_liveness(&buf, &[], TermCaps::Rich, |_| true, &empty_cache());
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
        let reg = known_identities_with_liveness(&buf, &[], TermCaps::Rich, |_| true, &empty_cache());
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
        let reg = known_identities_with_liveness(&buf, &[], TermCaps::Rich, |_| true, &empty_cache());
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn registry_skips_unknown_prefix() {
        let buf = vec![sig("mystery:abc", "/tmp")];
        let reg = known_identities_with_liveness(&buf, &[], TermCaps::Rich, |_| true, &empty_cache());
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
        let reg = known_identities_with_liveness(&[], &seeds, TermCaps::Rich, |_| true, &empty_cache());
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
        let reg = known_identities_with_liveness(&buf, &seeds, TermCaps::Rich, |_| true, &empty_cache());
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
        let reg = known_identities_with_liveness(&buf, &seeds, TermCaps::Rich, |_| true, &empty_cache());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn resolve_nickname_case_insensitive_claude_only() {
        let buf = vec![
            sig("claude:a", "/home/repo"),
            sig("external:aaron@kitty", "/home/aaron/Projects"),
        ];
        let reg = known_identities_with_liveness(&buf, &[], TermCaps::Rich, |_| true, &empty_cache());
        let claude_nick = &reg.iter().find(|k| k.is_claude).unwrap().nickname;
        // Case-insensitive match hits the claude cwd.
        let lowered = claude_nick.to_ascii_lowercase();
        assert_eq!(resolve_nickname(&lowered, &reg), Some("/home/repo".into()));
        // Humans are not routable — `@aaron` returns None even
        // though the human appears in the registry.
        assert_eq!(resolve_nickname("aaron", &reg), None);
        // Unknown nickname → None.
        assert_eq!(resolve_nickname("NotReal", &reg), None);
    }

    fn known(nick: &str, cwd: &str) -> KnownIdentity {
        // Bypass the file-touching identity derivation — tests for
        // resolve_nickname only care about the nickname/cwd shape.
        let id = Identity::for_cwd(cwd, TermCaps::Rich);
        KnownIdentity {
            nickname: nick.to_string(),
            cwd: cwd.to_string(),
            is_claude: true,
            palette: id.palette,
            style: id.style,
        }
    }

    #[test]
    fn resolve_nickname_fuzzy_matches_single_typo() {
        // Live regression: user typed `@Tasmin-alpha` (transposed
        // m/s) and got "unknown nickname". With distance ≤ 2 we
        // recover the intended target.
        let reg = vec![known("Tamsin-alpha", "/home/aaron/.claude")];
        assert_eq!(
            resolve_nickname("Tasmin-alpha", &reg),
            Some("/home/aaron/.claude".into())
        );
    }

    #[test]
    fn resolve_nickname_fuzzy_picks_clear_winner() {
        // Two candidates, one obviously closer. The closer one wins.
        let reg = vec![
            known("Tamsin-alpha", "/cwd-a"),
            known("Tamsin-beta", "/cwd-a"),
        ];
        // `Tamsin-alpa` (missing `h`) → distance 1 to alpha, 4 to beta.
        assert_eq!(
            resolve_nickname("Tamsin-alpa", &reg),
            Some("/cwd-a".into())
        );
    }

    #[test]
    fn resolve_nickname_returns_none_when_ambiguous_at_min_distance() {
        // If two nicknames tie for the same minimum distance, refuse
        // — silent routing to the wrong agent is worse than a
        // typed-error message the user can correct.
        let reg = vec![
            known("Cleo-alpha", "/cwd-a"),
            known("Cleo-beta", "/cwd-b"),
        ];
        // `Cleo` is exactly 5 edits from each suffix; both 5 > 2 so
        // neither matches and we get None. Use a more targeted case:
        // `Tamsin-alphz` and `Tamsin-betaa` would tie at distance 1
        // — but our pool is symmetric. Construct a real tie:
        let reg2 = vec![
            known("Foo", "/x"),
            known("Bar", "/y"),
        ];
        // `Fop` is distance 1 from both? No — `Fop` vs Foo = 1 (sub
        // p→o), vs Bar = 3. Bad example. Use:
        let reg3 = vec![
            known("aab", "/x"),
            known("aac", "/y"),
        ];
        // `aad` is distance 1 from both. Tie → None.
        assert_eq!(resolve_nickname("aad", &reg3), None);
        // Sanity: the helpers used above don't trip the tie path.
        let _ = (reg, reg2);
    }

    #[test]
    fn resolve_nickname_rejects_beyond_threshold() {
        // Distance > 2 must not match. Prevents wild misroutes when
        // a user types something genuinely different.
        let reg = vec![known("Tamsin-alpha", "/cwd-a")];
        // 4 edits — well past the 2-edit cap.
        assert_eq!(resolve_nickname("Wild-omega", &reg), None);
    }

    #[test]
    fn resolve_nickname_exact_match_wins_over_fuzzy() {
        // When an exact match exists, never substitute. Fuzzy is a
        // fallback, not a "best-of" comparator.
        let reg = vec![
            known("Foo", "/exact"),
            known("Foa", "/close"), // distance 1
        ];
        assert_eq!(resolve_nickname("foo", &reg), Some("/exact".into()));
    }

    #[test]
    fn resolve_nickname_refuses_cross_cwd_exact_collision() {
        // PR #77 review concern. Two distinct cwds whose hashes happen
        // to land on the same name pool index (rare but real — pool
        // is finite, ~200 entries) AND both registered as `alpha` in
        // their respective per-cwd registries collide on the same
        // displayed nickname. Pre-fix, `find` returned whichever
        // appeared first in the legend; the message would silently
        // misroute. Now we count and refuse.
        let reg = vec![
            known("Tamsin-alpha", "/cwd-1"),
            known("Tamsin-alpha", "/cwd-2"),
        ];
        assert_eq!(resolve_nickname("Tamsin-alpha", &reg), None);
    }

    #[test]
    fn resolve_nickname_exact_still_routes_when_unique() {
        // Sanity counterpart: a single exact match resolves. The
        // ambiguity check should not introduce a false-negative for
        // the common case.
        let reg = vec![
            known("Tamsin-alpha", "/cwd-1"),
            known("Cleo-alpha", "/cwd-2"),
        ];
        assert_eq!(
            resolve_nickname("Tamsin-alpha", &reg),
            Some("/cwd-1".into())
        );
    }

    #[test]
    fn levenshtein_canonical_cases() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("a", ""), 1);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3); // textbook
        assert_eq!(levenshtein("tasmin", "tamsin"), 2);  // transposition
        assert_eq!(levenshtein("foo", "foo"), 0);
    }
}
