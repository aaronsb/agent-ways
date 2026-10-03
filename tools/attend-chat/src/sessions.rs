//! Seed the identity registry from `~/.claude/sessions/*.json`.
//!
//! Without this, the TUI only knows about agents who have emitted a
//! signal that's still in its buffer — so a fresh launch shows an
//! empty `@` legend and `@Nickname` routing fails even for claudes
//! that are visibly alive in `attend peers`. Walking the sessions
//! directory gives us every claude cwd Claude Code has checked in
//! for recently, which is the ground truth for "who exists".
//!
//! We deliberately don't filter by liveness (no `ps` call per entry).
//! A stale session file is cheap legend clutter; a missing live
//! agent is a broken Tab-completion and a silent send-failure. The
//! tradeoff favors permissiveness.
//!
//! This is read-only. Session files are owned by Claude Code.


/// Minimal view of a claude session file — only the fields the TUI
/// needs to produce an `Identity`. We purposely don't carry `pid`
/// (we're not checking liveness in this PR).
///
/// `session_id` is carried even though `KnownIdentity` doesn't use it
/// yet — ADR-124's group-membership glyph lookup wants to match a
/// seeded peer's session UUID against `_groups.yaml` members. The
/// field exists now so the seed path is forward-compatible; the
/// consumer lands in a follow-up PR.
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    pub cwd: String,
    pub session_id: String,
}

/// Enumerate sessions from the default location (`~/.claude/sessions/`).
pub fn discover() -> Vec<DiscoveredSession> {
    discover_in(&claude_sessions::ClaudeDir::user().sessions_dir())
}

/// Enumerate sessions from an arbitrary directory. Exists so tests
/// can drive the walk against a scratch dir without touching
/// `$HOME`. Records are read by `claude_sessions`; one without a cwd
/// is skipped.
pub fn discover_in(dir: &std::path::Path) -> Vec<DiscoveredSession> {
    claude_sessions::read_session_records(dir)
        .into_iter()
        .filter_map(|r| {
            // Identity root, not live cwd (#394): a session mid-worktree
            // must seed the chip registry at the tray it actually scans,
            // or @-completion routes a DM to a tray nobody reads.
            let cwd = attend_presence::session::normalize_origin(&r.cwd?);
            Some(DiscoveredSession { cwd, session_id: r.session_id })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tempdir_like() -> std::path::PathBuf {
        crate::test_dir::unique("sessions")
    }

    fn write_session(dir: &std::path::Path, id: &str, cwd: &str) {
        let p = dir.join(format!("{}.json", id));
        let body = format!(
            r#"{{"sessionId":"{id}","cwd":"{cwd}","pid":12345,"model":"x"}}"#
        );
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn discover_reads_all_session_cwds() {
        let dir = tempdir_like();
        write_session(&dir, "sess-a", "/home/me/proj-a");
        write_session(&dir, "sess-b", "/home/me/proj-b");
        let mut found = discover_in(&dir);
        found.sort_by(|a, b| a.cwd.cmp(&b.cwd));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].cwd, "/home/me/proj-a");
        assert_eq!(found[1].cwd, "/home/me/proj-b");
    }

    #[test]
    fn discover_empty_if_dir_missing() {
        let dir = tempdir_like().join("nope"); // never created
        assert!(discover_in(&dir).is_empty());
    }

    #[test]
    fn discover_skips_non_json() {
        let dir = tempdir_like();
        write_session(&dir, "sess-a", "/home/me/proj");
        let mut f = fs::File::create(dir.join("notes.txt")).unwrap();
        f.write_all(b"nope").unwrap();
        assert_eq!(discover_in(&dir).len(), 1);
    }

    #[test]
    fn discover_skips_malformed_json() {
        // A session file missing `cwd` is silently dropped — we're
        // best-effort and don't want one bad file to mask the rest.
        let dir = tempdir_like();
        let p = dir.join("broken.json");
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(br#"{"sessionId":"x","pid":1}"#).unwrap();
        assert!(discover_in(&dir).is_empty());
    }

    #[test]
    fn discover_carries_session_id() {
        // Session IDs drive the per-chip group glyph lookup; the
        // seed path must carry them through or discovery becomes
        // legend-only (no membership render for pre-seeded peers).
        let dir = tempdir_like();
        write_session(&dir, "sess-xyz", "/home/me/proj");
        let found = discover_in(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].session_id, "sess-xyz");
    }
}
