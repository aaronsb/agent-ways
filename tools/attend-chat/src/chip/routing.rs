//! `@Nickname` → cwd routing.
//!
//! The address resolution layer for outgoing directed messages.
//! Pure function over a `&[KnownIdentity]` slice; the registry that
//! produces that slice lives in `super::registry`.

use super::registry::KnownIdentity;

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
pub fn resolve_nickname(name: &str, known: &[KnownIdentity]) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::super::registry::{known_identities_with_liveness, KnownIdentity};
    use super::*;
    use agent_identity::{Identity, TermCaps};
    use attend_instances::SnapshotCache;

    fn empty_cache() -> SnapshotCache {
        SnapshotCache::new()
    }

    fn sig(from: &str, cwd: &str) -> crate::signal::Signal {
        crate::signal::Signal {
            id: "t".into(),
            from: from.into(),
            project: cwd.rsplit('/').next().unwrap_or("?").into(),
            cwd: cwd.into(),
            reply_to: None,
            message: "msg".into(),
            ts: 0,
        }
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
        // Construct a real tie: `aad` is distance 1 from both `aab`
        // and `aac`. Tie → None. Refusing here is the design choice;
        // silent routing to the wrong agent is worse than an
        // unknown-nickname error the user can correct.
        let reg = vec![known("aab", "/x"), known("aac", "/y")];
        assert_eq!(resolve_nickname("aad", &reg), None);
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
