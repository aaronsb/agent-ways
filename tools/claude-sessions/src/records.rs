//! Claude Code's session records, `~/.claude/sessions/<pid>.json`.
//!
//! Claude Code writes one record per running session: its pid, session id and
//! working directory, among other fields. Read here with `serde_json`, so a
//! record written with whitespace or in another key order reads the same.

use std::path::{Path, PathBuf};

/// The fields of one session record that agent-ways uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub pid: u32,
    pub session_id: String,
    /// The session's working directory, when the record names one.
    pub cwd: Option<String>,
    /// When the session's process started (`startedAt`), in milliseconds
    /// since the epoch, when the record says.
    pub started_at_ms: Option<u64>,
    /// The process's start (`procStart`) as the kernel gives it on Linux:
    /// clock ticks after boot, `/proc/<pid>/stat` field 22. Claude Code
    /// writes it as a string; a number reads the same.
    pub proc_start: Option<u64>,
    /// The record file.
    pub path: PathBuf,
}

/// Parse one record. `None` unless it is a JSON object with a numeric `pid`
/// and a string `sessionId`.
pub fn parse_session_record(content: &str, path: &Path) -> Option<SessionRecord> {
    let v: serde_json::Value = serde_json::from_str(content).ok()?;
    let pid = u32::try_from(v.get("pid")?.as_u64()?).ok()?;
    let session_id = v.get("sessionId")?.as_str()?.to_string();
    let cwd = v.get("cwd").and_then(|c| c.as_str()).map(str::to_string);
    let started_at_ms = v.get("startedAt").and_then(serde_json::Value::as_u64);
    let proc_start = v.get("procStart").and_then(|s| s.as_u64().or_else(|| s.as_str()?.parse().ok()));
    Some(SessionRecord { pid, session_id, cwd, started_at_ms, proc_start, path: path.to_path_buf() })
}

/// Every readable record in `dir` (`*.json` only), sorted by file name.
/// Unreadable or malformed files are skipped.
pub fn read_session_records(dir: &Path) -> Vec<SessionRecord> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| parse_session_record(&std::fs::read_to_string(&p).ok()?, &p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempTree;

    #[test]
    fn reads_compact_and_pretty_records() {
        let t = TempTree::new("records");
        t.file("sessions/1.json", r#"{"pid":1,"sessionId":"a","cwd":"/p"}"#);
        t.file("sessions/2.json", "{\n  \"cwd\": \"/q\",\n  \"pid\": 2,\n  \"sessionId\": \"b\"\n}");
        t.file("sessions/3.json", r#"{"pid":3}"#);
        t.file("sessions/4.json", "not json");
        t.file("sessions/5.abc.key", r#"{"pid":5,"sessionId":"k"}"#);
        t.file("sessions/6.json", r#"{"pid":6,"sessionId":"c"}"#);
        let got = read_session_records(&t.path("sessions"));
        let ids: Vec<(u32, &str, Option<&str>)> =
            got.iter().map(|r| (r.pid, r.session_id.as_str(), r.cwd.as_deref())).collect();
        assert_eq!(ids, vec![(1, "a", Some("/p")), (2, "b", Some("/q")), (6, "c", None)]);
        assert!(got.iter().all(|r| r.started_at_ms.is_none()));
        let r = parse_session_record(r#"{"pid":7,"sessionId":"d","startedAt":1791774137947}"#, Path::new("7.json")).unwrap();
        assert_eq!(r.started_at_ms, Some(1_791_774_137_947));
        assert!(got.iter().all(|r| r.proc_start.is_none()) && r.proc_start.is_none());
        // `procStart` as Claude Code writes it, a string, and as a number.
        let r = parse_session_record(r#"{"pid":8,"sessionId":"e","procStart":"52880213"}"#, Path::new("8.json")).unwrap();
        assert_eq!(r.proc_start, Some(52_880_213));
        let r = parse_session_record(r#"{"pid":8,"sessionId":"e","procStart":52880213}"#, Path::new("8.json")).unwrap();
        assert_eq!(r.proc_start, Some(52_880_213));
        let r = parse_session_record(r#"{"pid":8,"sessionId":"e","procStart":"soon"}"#, Path::new("8.json")).unwrap();
        assert_eq!(r.proc_start, None);
    }

    #[test]
    fn missing_dir_reads_as_empty() {
        assert!(read_session_records(Path::new("/nonexistent/claude/sessions")).is_empty());
    }
}
