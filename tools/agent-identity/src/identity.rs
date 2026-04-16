//! Per-agent display identity derived purely from identifying inputs.
//!
//! The contract: same inputs → same `Identity`, across runs, across
//! hosts, across crates. Two agents in different working directories
//! with the same basename still land on different nicknames because
//! the hash seed is the full path, not the basename.
//!
//! We intentionally use our own FNV-1a here rather than `std`'s
//! `DefaultHasher`: `DefaultHasher` is SipHash with a process-random
//! seed, so its output is not stable across runs. For display
//! identity we need determinism, not cryptographic strength.

use crate::names;
use crate::palette::{resolve, PaletteEntry, Resolved, Style, TermCaps};

/// Everything a consumer needs to render an agent's identity chip.
#[derive(Clone, Debug)]
pub struct Identity {
    /// Stable nickname. Lives for the life of the working directory —
    /// a claude restarting in the same cwd keeps the same name.
    pub nickname: &'static str,
    /// Last path segment of the agent's cwd, already truncated for
    /// display. Callers that want the raw cwd have it on the `Signal`.
    pub cwd_basename: String,
    /// Color + style resolved at the capability level the caller
    /// passed in. If the caller re-resolves under a different `caps`,
    /// they'll get a different entry but the same nickname.
    pub palette: PaletteEntry,
    pub style: Style,
    /// The 64-bit seed used to derive this identity. Exposed for
    /// callers that want to key their own data (e.g. unread counts) by
    /// the same identity without re-hashing.
    pub seed: u64,
}

impl Identity {
    /// Derive identity for an agent keyed on its **full** cwd path.
    ///
    /// Pass `user` if the sender is a human (from the `external:<user>`
    /// form) — users get an identity too, keyed on their username, so
    /// the human's avatar is consistent with how peers address them.
    pub fn for_cwd(cwd_path: &str, caps: TermCaps) -> Self {
        Self::for_key(cwd_path, cwd_basename(cwd_path), caps)
    }

    /// Derive identity for a human user, keyed on username. Basename
    /// for display is the passed-in `label` (typically a cwd basename
    /// so the human's chip reads "<user> / <projdir>").
    pub fn for_user(user: &str, label: &str, caps: TermCaps) -> Self {
        // Prefix the hash domain so a user named "Monet" and a cwd
        // hashing to "Monet" don't collide on the same nickname. Same
        // principle as namespacing HMAC inputs.
        let key = format!("user:{user}");
        Self::for_key(&key, label.to_string(), caps)
    }

    fn for_key(hash_input: &str, label: String, caps: TermCaps) -> Self {
        let seed = fnv1a_64(hash_input.as_bytes());
        let nickname = names::pick(seed);
        // Stir the seed before asking for palette + style so nickname
        // and color aren't trivially correlated — two agents that land
        // on the same color should still tend to differ on nickname
        // and vice versa.
        let Resolved { entry, style } = resolve(seed ^ 0x9e37_79b9_7f4a_7c15, caps);
        Identity {
            nickname,
            cwd_basename: label,
            palette: entry,
            style,
            seed,
        }
    }
}

/// Extract the last non-empty path segment from a cwd string. Pure
/// string work — we don't touch the filesystem.
pub fn cwd_basename(path: &str) -> String {
    if path.is_empty() {
        return "(home)".to_string();
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

/// FNV-1a 64-bit. Deterministic across runs and targets — this is the
/// property we need, and why we don't use `std::hash::DefaultHasher`.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_cwd_same_identity() {
        let a = Identity::for_cwd("/home/aaron/.claude", TermCaps::Rich);
        let b = Identity::for_cwd("/home/aaron/.claude", TermCaps::Rich);
        assert_eq!(a.nickname, b.nickname);
        assert_eq!(a.cwd_basename, b.cwd_basename);
        assert_eq!(a.palette, b.palette);
        assert_eq!(a.style, b.style);
        assert_eq!(a.seed, b.seed);
    }

    #[test]
    fn different_cwds_different_seeds() {
        // Different paths must produce different seeds. We don't
        // assert different nicknames — the pool is finite and
        // collisions are allowed — but the seed itself must differ or
        // all downstream derivations collapse.
        let a = Identity::for_cwd("/home/a", TermCaps::Rich);
        let b = Identity::for_cwd("/home/b", TermCaps::Rich);
        assert_ne!(a.seed, b.seed);
    }

    #[test]
    fn same_basename_different_paths_differ() {
        // Two repos both named "src" must not share an identity.
        let a = Identity::for_cwd("/home/me/proj-a/src", TermCaps::Rich);
        let b = Identity::for_cwd("/home/me/proj-b/src", TermCaps::Rich);
        assert_ne!(a.seed, b.seed);
        assert_eq!(a.cwd_basename, b.cwd_basename); // basename matches
    }

    #[test]
    fn user_namespace_separate_from_cwd() {
        // A username happening to equal a cwd path must not collide —
        // the `user:` prefix in the hash key enforces this.
        let a = Identity::for_user("aaron", "Projects", TermCaps::Rich);
        let b = Identity::for_cwd("aaron", TermCaps::Rich);
        assert_ne!(a.seed, b.seed);
    }

    #[test]
    fn basename_handles_trailing_slash() {
        assert_eq!(cwd_basename("/home/aaron/"), "aaron");
        assert_eq!(cwd_basename("/home/aaron"), "aaron");
    }

    #[test]
    fn basename_handles_root_and_empty() {
        assert_eq!(cwd_basename("/"), "/");
        assert_eq!(cwd_basename(""), "(home)");
    }

    #[test]
    fn basename_handles_single_segment() {
        assert_eq!(cwd_basename(".claude"), ".claude");
    }

    #[test]
    fn caps_affects_palette_not_nickname() {
        // Same cwd rendered under two different caps must keep the
        // nickname — it's the stable part of identity — but may pick
        // a different entry from a differently-sized palette.
        let rich = Identity::for_cwd("/x/y/z", TermCaps::Rich);
        let basic = Identity::for_cwd("/x/y/z", TermCaps::Basic);
        assert_eq!(rich.nickname, basic.nickname);
        assert_eq!(rich.cwd_basename, basic.cwd_basename);
    }

    #[test]
    fn mono_caps_produces_mono_entry() {
        let id = Identity::for_cwd("/x/y/z", TermCaps::Mono);
        assert_eq!(id.palette.name, "mono");
    }

    #[test]
    fn nickname_is_ascii() {
        // Sanity: any derivation must produce an ASCII-safe nickname,
        // since downstream `@<name>` autocomplete depends on it.
        for path in [
            "/home/a",
            "/home/b/c/d",
            "/tmp/x",
            "/home/me/code/rust/project-42",
        ] {
            let id = Identity::for_cwd(path, TermCaps::Rich);
            assert!(id.nickname.is_ascii(), "{:?} derived non-ASCII nickname {:?}", path, id.nickname);
        }
    }

    #[test]
    fn fnv1a_matches_reference_vector() {
        // FNV-1a of "foo" is the canonical test vector from the spec —
        // if we ever accidentally break our hash (e.g., swap offset/
        // prime, wrong order of ops), this catches it before real
        // identity drift hits a user.
        assert_eq!(fnv1a_64(b"foo"), 0xdcb27518_fed9d577);
    }

    #[test]
    fn collision_rate_is_reasonable() {
        // Over a synthetic population of 1000 cwd-like strings the
        // nickname collision rate should stay well under a visible
        // threshold. This is a smoke test for pool size × hash quality,
        // not a formal bound.
        use std::collections::HashMap;
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for i in 0..1000 {
            let path = format!("/home/user/project-{i:04}/src");
            let id = Identity::for_cwd(&path, TermCaps::Rich);
            *counts.entry(id.nickname).or_insert(0) += 1;
        }
        let unique = counts.len();
        // With ~200 names and 1000 samples, perfect uniformity would
        // put every name around 5. We just want coverage — at least
        // half the pool touched.
        assert!(unique >= names::NAMES.len() / 2, "only {unique} nicknames used from pool of {}", names::NAMES.len());
    }
}
