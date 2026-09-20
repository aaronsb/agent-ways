//! One sender display form for every attend conduit.
//!
//! A message reaches a session over two conduits (ADR-172): the
//! Monitor-hosted `peers` sensor and the Stop-hook drain. Both render
//! the sender through this crate, so one message wears one name on
//! both — `Nickname-instance (project)` — instead of a path on one
//! and a persona on the other (issue #534). The canonical id stays the
//! wire `from` field (`claude:<session-id>`, ADR-171); this crate is
//! presentation over that key, never a substitute for it.
//!
//! attend-chat draws a two-line chip rather than this one-line label,
//! but its first line comes from the same [`with_instance`] derivation,
//! so a session wears one persona across the sensor, the drain, and
//! the chat TUI. The label and the chip both route through
//! `agent_identity`; this crate is the glue that picks the right
//! constructor (user vs. cwd) per sender kind and appends the ADR-129
//! instance suffix.
//!
//! Every renderer takes a `&SnapshotCache`. The instance registry is a
//! file per cwd; the cache collapses lookups to one read per distinct
//! cwd for the lifetime of one pass (a sensor poll, a drain scan, a
//! chat render). Build it fresh per pass and never share across passes
//! — the registry can change between them.

use agent_identity::{ansi, Identity, TermCaps};
/// Re-exported so a renderer's crate needs only this dependency to
/// build the per-pass cache every function here takes.
pub use attend_instances::SnapshotCache;

/// Render a sender label from the wire `from`/`cwd` pair.
///
/// Claudes get `Nickname-instance (cwd_basename)` with the nickname in
/// their identity color. Humans get `user (cwd_basename)` styled the
/// same way — keyed on username, not cwd, so the same human shows up
/// consistently across projects. Unknown prefixes fall through
/// showing the raw `from` value, colored off its own hash.
///
/// Scope derivation deliberately differs from `attend-chat::chip::chip_for`:
/// that renderer falls back to the `project` field when `cwd` is
/// empty (because the chat TUI has room for a secondary line and
/// wants the best-effort label). This label is a single-line CLI
/// output where `(home)` is an acceptable empty-cwd marker, so we
/// keep the code simple and ignore `project`. Production signals
/// populate `cwd` either way — the divergence only manifests on
/// hand-crafted signals, which shouldn't be a hot path.
pub fn render_sender_label(from: &str, cwd: &str, caps: TermCaps, instances: &SnapshotCache) -> String {
    if let Some(sid) = from.strip_prefix("claude:") {
        let id = Identity::for_cwd(cwd, caps);
        // Instance suffix (ADR-129). Always rendered when present so
        // pattern matching on the display name is consistent — solo
        // and multi-session cwds both look the same.
        let primary = with_instance(id.nickname, cwd, sid, instances);
        compose(&primary, &id.cwd_basename, &id, caps)
    } else if let Some(rest) = from.strip_prefix("external:") {
        let username = rest.split('@').next().unwrap_or(rest);
        let scope = agent_identity::cwd_basename(cwd);
        let id = Identity::for_user(username, &scope, caps);
        compose(username, &id.cwd_basename, &id, caps)
    } else {
        let scope = agent_identity::cwd_basename(cwd);
        let id = Identity::for_user(from, &scope, caps);
        compose(from, &id.cwd_basename, &id, caps)
    }
}

/// Escape-free sender label for machine-carried text — the Monitor
/// event line, the ADR-172 drain injection, and piped (non-TTY)
/// output. Same derivation as [`render_sender_label`], zero ANSI:
/// `TermCaps::Mono` is NOT enough for these paths because Mono still
/// emits style bits (dim/reset) by design — that leak is issue #388.
pub fn render_sender_label_plain(from: &str, cwd: &str, instances: &SnapshotCache) -> String {
    // Caps only steer styling, which this path discards; Mono keeps
    // the identity derivation on its cheapest branch.
    let caps = TermCaps::Mono;
    if let Some(sid) = from.strip_prefix("claude:") {
        let id = Identity::for_cwd(cwd, caps);
        let primary = with_instance(id.nickname, cwd, sid, instances);
        format!("{primary} ({})", id.cwd_basename)
    } else if let Some(rest) = from.strip_prefix("external:") {
        let username = rest.split('@').next().unwrap_or(rest);
        let scope = agent_identity::cwd_basename(cwd);
        let id = Identity::for_user(username, &scope, caps);
        format!("{username} ({})", id.cwd_basename)
    } else {
        let scope = agent_identity::cwd_basename(cwd);
        let id = Identity::for_user(from, &scope, caps);
        format!("{from} ({})", id.cwd_basename)
    }
}

/// Compose `<nickname>-<instance>` for a claude session (ADR-129).
/// Falls back to the bare nickname when the registry has no entry —
/// only happens transiently before the session has registered, or
/// when the registry file is unreadable.
///
/// This is the one derivation of the persona's first line; the
/// sensor, the drain, and the attend-chat chip all call it, so a
/// session cannot wear one suffix on one surface and another
/// elsewhere. Reads through `instances`, which caches per-cwd
/// snapshots for one pass.
pub fn with_instance(nickname: &str, cwd: &str, session_id: &str, instances: &SnapshotCache) -> String {
    match instances.lookup(cwd, session_id) {
        Some(inst) => format!("{nickname}-{inst}"),
        None => nickname.to_string(),
    }
}

fn compose(primary: &str, secondary: &str, id: &Identity, caps: TermCaps) -> String {
    let coloured = ansi::wrap(primary, &id.palette, id.style, caps);
    format!("{coloured} \x1b[2m({})\x1b[0m", secondary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use attend_instances::Registry;

    /// A cache over an empty registry: no instance suffixes, no
    /// dependence on the test host's `~/.cache`.
    fn empty_cache() -> SnapshotCache {
        let dir = std::env::temp_dir().join(format!(
            "attend-identity-view-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("t")
        ));
        SnapshotCache::with_registry(Registry::with_base(dir))
    }

    #[test]
    fn claude_label_uses_nickname() {
        let label = render_sender_label("claude:abc", "/home/me/repo", TermCaps::Rich, &empty_cache());
        let expected = Identity::for_cwd("/home/me/repo", TermCaps::Rich);
        assert!(
            label.contains(expected.nickname),
            "label {label:?} should carry nickname {:?}",
            expected.nickname
        );
        assert!(label.contains("(repo)"), "label {label:?} missing cwd basename");
    }

    #[test]
    fn external_label_keeps_username() {
        let label = render_sender_label("external:aaron@kitty", "/home/aaron/Projects", TermCaps::Rich, &empty_cache());
        assert!(label.contains("aaron"));
        assert!(label.contains("(Projects)"));
    }

    #[test]
    fn unknown_sender_renders_without_panic() {
        let label = render_sender_label("weird-prefix:xyz", "/tmp", TermCaps::Rich, &empty_cache());
        assert!(label.contains("weird-prefix:xyz"));
    }

    #[test]
    fn mono_caps_produces_label_without_color() {
        let label = render_sender_label("claude:abc", "/home/me/repo", TermCaps::Mono, &empty_cache());
        // Mono path: no truecolor SGR, but style + reset still present.
        assert!(!label.contains("\x1b[38;2;"), "mono leaked color: {label:?}");
    }

    #[test]
    fn with_instance_appends_registered_suffix_and_falls_back_bare() {
        let dir = std::env::temp_dir().join(format!("attend-identity-view-reg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let registry = Registry::with_base(dir.clone());
        let inst = registry.register("/home/me/repo", "sess-1").unwrap();
        let cache = SnapshotCache::with_registry(Registry::with_base(dir.clone()));
        assert_eq!(with_instance("Nick", "/home/me/repo", "sess-1", &cache), format!("Nick-{inst}"));
        assert_eq!(with_instance("Nick", "/home/me/repo", "unregistered", &cache), "Nick");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn plain_label_is_escape_free_and_matches_styled_text() {
        let cache = empty_cache();
        let plain = render_sender_label_plain("claude:abc", "/home/me/repo", &cache);
        assert!(!plain.contains('\x1b'), "plain leaked ANSI: {plain:?}");
        let expected = Identity::for_cwd("/home/me/repo", TermCaps::Mono);
        assert_eq!(plain, format!("{} (repo)", expected.nickname));
    }
}
