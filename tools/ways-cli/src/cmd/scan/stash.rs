//! PreToolUse deferred-delivery stash (spike for issue #528).
//!
//! Claude Code 2.1.x never delivers PreToolUse `additionalContext` to the model
//! (upstream anthropics/claude-code#19432). The Bash and file lanes therefore
//! append what they would have printed here, and the next `UserPromptSubmit`
//! drains the file into the prompt lane's envelope — which IS delivered.
//!
//! Shape mirrors the Task lane (`scan task` writes `subagent-stash/`,
//! `inject-subagent.sh` claims it by rename). Differences: one JSONL file per
//! (session, agent) instead of one file per Task call, and the stash carries the
//! already-rendered body, because `show::way_scored` has already run — it
//! stamped the engagement state, markers, and `way_fired` at match time. The
//! drain side renders nothing and stamps nothing; it only moves text.

use std::io::Write;
use std::path::PathBuf;

use crate::session;

/// One deferred item: a way or check body, with the id and channel that fired it.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct Entry {
    pub ts: String,
    /// `"way"` or `"check"`.
    pub kind: String,
    pub id: String,
    pub trigger: String,
    pub body: String,
}

impl Entry {
    pub(super) fn way(id: &str, trigger: &str, body: String) -> Self {
        Self::new("way", id, trigger, body)
    }
    pub(super) fn check(id: &str, trigger: &str, body: String) -> Self {
        Self::new("check", id, trigger, body)
    }
    fn new(kind: &str, id: &str, trigger: &str, body: String) -> Self {
        Self {
            ts: chrono_utc_now(),
            kind: kind.to_string(),
            id: id.to_string(),
            trigger: trigger.to_string(),
            body,
        }
    }
}

fn path(session_id: &str) -> PathBuf {
    session::pretool_stash_path(session_id)
}

/// Append entries to the stash. O_APPEND, one JSON line per entry, so parallel
/// PreToolUse hooks (parallel tool calls) interleave at line granularity.
pub(super) fn push(session_id: &str, entries: &[Entry]) -> std::io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let p = path(session_id);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p)?;
    for e in entries {
        if let Ok(line) = serde_json::to_string(e) {
            writeln!(f, "{line}")?;
        }
    }
    Ok(())
}

/// Claim and drain the stash. The file is renamed first (atomic on one
/// filesystem) so a concurrent PreToolUse append lands in a fresh file rather
/// than in the one being read, and a concurrent second drain finds nothing.
pub(super) fn drain(session_id: &str) -> Vec<Entry> {
    let p = path(session_id);
    if !p.is_file() {
        return Vec::new();
    }
    let claimed = p.with_extension(format!("claimed.{}", std::process::id()));
    if std::fs::rename(&p, &claimed).is_err() {
        return Vec::new();
    }
    let content = std::fs::read_to_string(&claimed).unwrap_or_default();
    let _ = std::fs::remove_file(&claimed);
    content
        .lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .collect()
}

/// Render drained entries as one context block: provenance comment per item,
/// bodies separated by a blank line. Empty when there is nothing to deliver.
pub(super) fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    for e in entries {
        if e.body.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!(
            "<!-- ways: deferred from PreToolUse ({}:{}, {}) -->\n",
            e.kind, e.trigger, e.id
        ));
        out.push_str(e.body.trim_end());
    }
    out
}

fn chrono_utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_joins_bodies_with_provenance() {
        let entries = vec![
            Entry::way("a/b", "bash", "Body A".into()),
            Entry::check("c/d", "file", "Body B\n".into()),
            Entry::way("e/f", "bash", "   ".into()),
        ];
        let r = render(&entries);
        assert!(r.starts_with("<!-- ways: deferred from PreToolUse (way:bash, a/b) -->\nBody A"));
        assert!(r.contains("\n\n<!-- ways: deferred from PreToolUse (check:file, c/d) -->\nBody B"));
        assert!(!r.contains("e/f"));
        assert!(!r.ends_with('\n'));
    }

    #[test]
    fn push_then_drain_is_deliver_once() {
        let tmp = std::env::temp_dir().join(format!("ways-stash-test-{}", std::process::id()));
        std::env::set_var("XDG_RUNTIME_DIR", &tmp);
        let sid = "stash-test-session";
        push(sid, &[Entry::way("x/y", "bash", "hello".into())]).unwrap();
        push(sid, &[Entry::way("x/z", "bash", "world".into())]).unwrap();
        let first = drain(sid);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].id, "x/y");
        assert_eq!(first[1].id, "x/z");
        assert!(drain(sid).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
