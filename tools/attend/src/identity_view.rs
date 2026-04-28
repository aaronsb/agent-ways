//! Shared helpers that turn a signal's sender into an identity-styled
//! label for attend's terminal output.
//!
//! attend-chat has its own renderer (targeting iocraft), and attend
//! renders to raw ANSI via `agent_fmt::Table`. We keep the derivation
//! identical by routing both sides through `agent_identity` — this
//! module is just the glue that picks the right constructor (user vs.
//! cwd) per sender kind.

use agent_identity::{ansi, Identity, TermCaps};

/// Render a sender label from the wire `from`/`cwd` pair.
///
/// Claudes get `Nickname (cwd_basename)` with the nickname in their
/// identity color. Humans get `user (cwd_basename)` styled the same
/// way — keyed on username, not cwd, so the same human shows up
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
pub(crate) fn render_sender_label(from: &str, cwd: &str, caps: TermCaps) -> String {
    if let Some(sid) = from.strip_prefix("claude:") {
        let id = Identity::for_cwd(cwd, caps);
        // Instance suffix (ADR-129). Always rendered when present so
        // pattern matching on the display name is consistent — solo
        // and multi-session cwds both look the same.
        let primary = with_instance(id.nickname, cwd, sid);
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

/// Compose `<nickname>-<instance>` for a claude session. Falls back to
/// the bare nickname when the registry has no entry — only happens
/// transiently before the session has registered, or when the
/// registry file is unreadable.
fn with_instance(nickname: &str, cwd: &str, session_id: &str) -> String {
    match attend_instances::Registry::new().lookup(cwd, session_id) {
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

    #[test]
    fn claude_label_uses_nickname() {
        let label = render_sender_label("claude:abc", "/home/me/repo", TermCaps::Rich);
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
        let label = render_sender_label("external:aaron@kitty", "/home/aaron/Projects", TermCaps::Rich);
        assert!(label.contains("aaron"));
        assert!(label.contains("(Projects)"));
    }

    #[test]
    fn unknown_sender_renders_without_panic() {
        let label = render_sender_label("weird-prefix:xyz", "/tmp", TermCaps::Rich);
        assert!(label.contains("weird-prefix:xyz"));
    }

    #[test]
    fn mono_caps_produces_label_without_color() {
        let label = render_sender_label("claude:abc", "/home/me/repo", TermCaps::Mono);
        // Mono path: no truecolor SGR, but style + reset still present.
        assert!(!label.contains("\x1b[38;2;"), "mono leaked color: {label:?}");
    }
}
