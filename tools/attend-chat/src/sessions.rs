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

use std::fs;
use std::path::PathBuf;

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

/// Enumerate sessions from the default location (`$HOME/.claude/sessions/`).
pub fn discover() -> Vec<DiscoveredSession> {
    let Ok(home) = std::env::var("HOME") else {
        return Vec::new();
    };
    let dir = PathBuf::from(home).join(".claude").join("sessions");
    discover_in(&dir)
}

/// Enumerate sessions from an arbitrary directory. Exists so tests
/// can drive the walk against a scratch dir without touching
/// `$HOME`.
pub fn discover_in(dir: &std::path::Path) -> Vec<DiscoveredSession> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(cwd) = extract_json_string(&content, "cwd") else {
            continue;
        };
        let Some(session_id) = extract_json_string(&content, "sessionId") else {
            continue;
        };
        out.push(DiscoveredSession { cwd, session_id });
    }
    out
}

/// Quick-and-dirty JSON string extractor. Byte-identical to
/// `sensor-peers/src/lib.rs::extract_json_string`; duplicated here
/// so attend-chat doesn't depend on sensor-peers for two fields of
/// a stable Claude Code file. If either copy changes (e.g., to
/// handle escape sequences), update both.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":\"", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tempdir_like() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "attend-chat-sessions-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
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
