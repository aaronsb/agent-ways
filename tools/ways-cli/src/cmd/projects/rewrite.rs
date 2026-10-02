//! The record rewrites `relocate` applies: transcript `cwd`s and
//! `history.jsonl` projects, `sessions-index.json` paths and the
//! `~/.claude.json` key, plus the checks that decide whether a leftover file
//! may be dropped.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value};

use super::fsio::{backup_file, read_json, replace_atomic, stamp, tmp_sibling, write_json_atomic};

/// Records Claude Code writes to track session state rather than
/// conversation, each one regenerated on the next session. A file holding
/// nothing but these is a sidecar, not history.
///
/// Every entry must be regenerable, because a remnant is a divergent tail,
/// not a duplicate: sweeping it always discards a fork. `summary` is not in
/// the set: it carries compaction prose and the `leafUuid` `--resume` reads.
pub(super) const STATE_ONLY_TYPES: &[&str] =
    &["last-prompt", "ai-title", "mode", "permission-mode", "bridge-session", "agent-name"];

/// `value` remapped from `old` to `new` when it is `old` or lies under it.
/// True when `rest`, what follows an old path in a value, starts a path
/// component under it: `/`, or on Windows also `\`. On Unix a `\` is an
/// ordinary file-name character, so `/a/b\c` is not under `/a/b`.
pub(super) fn starts_component(rest: &str) -> bool {
    rest.starts_with('/') || (cfg!(windows) && rest.starts_with('\\'))
}

pub(super) fn remap(value: Option<&Value>, old: &str, new: &str) -> Option<String> {
    let v = value?.as_str()?;
    if v == old {
        Some(new.to_string())
    } else {
        v.strip_prefix(old).filter(|r| starts_component(r)).map(|r| format!("{new}{r}"))
    }
}

pub(super) fn path_matches(value: &Value, old: &str) -> bool {
    value.as_str().is_some_and(|v| v == old || v.strip_prefix(old).is_some_and(starts_component))
}

/// Does `field` hold a path at or under `old` anywhere below the top level?
/// Every cwd Claude Code writes today is top-level, which is what makes this
/// a targeted remap rather than a find-and-replace; a nested one is reported
/// as unhandled rather than counted as preserved history.
pub(super) fn has_nested(obj: &Value, field: &str, old: &str, top: bool) -> bool {
    match obj {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| (!top && k == field && path_matches(v, old)) || has_nested(v, field, old, false)),
        Value::Array(items) => items.iter().any(|v| has_nested(v, field, old, false)),
        _ => false,
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Rewrite {
    pub(super) changed: usize,
    /// Lines still mentioning the old path afterwards (message text, tool
    /// output), deliberately left as record.
    pub(super) residual: usize,
    /// Records carrying the field below the top level, which are not remapped.
    pub(super) nested: usize,
}

#[derive(Debug)]
pub(super) enum RewriteError {
    /// The file changed underneath the rewrite; swapping would drop records.
    Concurrent,
    Io(std::io::Error),
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RewriteError::Concurrent => write!(f, "changed while being rewritten"),
            RewriteError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<std::io::Error> for RewriteError {
    fn from(e: std::io::Error) -> Self {
        RewriteError::Io(e)
    }
}

/// Remap a top-level string `field` across a JSONL file. Lines that do not
/// mention `old` are copied byte for byte, so only affected records are
/// re-serialized, and line endings (CRLF included) are kept.
///
/// `guard_concurrent` re-checks the file just before the swap and refuses if
/// it changed, so an append from another session is never dropped.
pub(super) fn rewrite_jsonl(
    path: &Path,
    field: &str,
    old: &str,
    new: &str,
    apply: bool,
    guard_concurrent: bool,
) -> Result<Rewrite, RewriteError> {
    let before = if guard_concurrent { stamp(path) } else { None };
    let bytes = fs::read(path)?;
    let old_b = old.as_bytes();
    let mut r = Rewrite::default();
    let mut out: Vec<u8> = Vec::with_capacity(if apply { bytes.len() } else { 0 });

    for line in bytes.split_inclusive(|&b| b == b'\n') {
        if !contains(line, old_b) {
            out.extend_from_slice(if apply { line } else { &[] });
            continue;
        }
        let (body, term): (&[u8], &[u8]) = if line.ends_with(b"\r\n") {
            line.split_at(line.len() - 2)
        } else if line.ends_with(b"\n") {
            line.split_at(line.len() - 1)
        } else {
            (line, b"")
        };
        let parsed: Option<Value> = std::str::from_utf8(body).ok().and_then(|s| serde_json::from_str(s).ok());
        let mapped = parsed.as_ref().filter(|o| o.is_object()).and_then(|o| remap(o.get(field), old, new));
        let (mut obj, mapped) = match (parsed, mapped) {
            (Some(obj), Some(mapped)) => (obj, mapped),
            (parsed, _) => {
                r.residual += 1;
                if parsed.is_some_and(|o| has_nested(&o, field, old, true)) {
                    r.nested += 1;
                }
                if apply {
                    out.extend_from_slice(line);
                }
                continue;
            }
        };
        obj[field] = Value::String(mapped);
        r.changed += 1;
        if has_nested(&obj, field, old, true) {
            r.nested += 1;
        }
        let rendered = serde_json::to_string(&obj).map_err(std::io::Error::other)?;
        if rendered.contains(old) {
            r.residual += 1;
        }
        if apply {
            out.extend_from_slice(rendered.as_bytes());
            out.extend_from_slice(term);
        }
    }

    if apply && r.changed > 0 {
        let tmp = tmp_sibling(path);
        let result = (|| -> Result<(), RewriteError> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&out)?;
            // Durable before the swap: transcripts have no backup.
            f.sync_all()?;
            drop(f);
            if guard_concurrent && stamp(path) != before {
                return Err(RewriteError::Concurrent);
            }
            replace_atomic(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
    }
    Ok(r)
}

pub(super) fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Union two indexes by `sessionId`, the source winning. A merge that moved
/// transcripts but dropped their entries would lose their summaries.
pub(super) fn merge_sessions_index(src: &Path, dst: &Path) -> std::io::Result<usize> {
    let load = |p: &Path| read_json(p).filter(Value::is_object).unwrap_or_else(|| Value::Object(Map::new()));
    let (src_data, mut dst_data) = (load(src), load(dst));
    let mut order: Vec<Option<String>> = Vec::new();
    let mut entries: Vec<Value> = Vec::new();
    let all = dst_data.get("entries").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().chain(
        src_data.get("entries").and_then(Value::as_array).cloned().unwrap_or_default(),
    );
    for entry in all.filter(Value::is_object) {
        let sid = entry.get("sessionId").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        match sid.as_ref().and_then(|s| order.iter().position(|k| k.as_deref() == Some(s))) {
            Some(i) => entries[i] = entry,
            None => {
                order.push(sid);
                entries.push(entry);
            }
        }
    }
    let n = entries.len();
    dst_data["entries"] = Value::Array(entries);
    let pretty = fs::read_to_string(dst).map(|raw| raw.contains("\n  ")).unwrap_or(false);
    write_json_atomic(dst, &dst_data, pretty)?;
    Ok(n)
}

pub(super) enum IndexStatus {
    Ok(usize),
    Unreadable(String),
    Failed(String),
}

/// Remap `originalPath` and each entry's `projectPath` and `cwd`, plus
/// `fullPath`, which embeds the project directory.
pub(super) fn rewrite_sessions_index(idx: &Path, old: &str, new: &str, old_dir: &str, new_dir: &str, apply: bool) -> IndexStatus {
    let raw = match fs::read_to_string(idx) {
        Ok(r) => r,
        Err(e) => return IndexStatus::Unreadable(e.to_string()),
    };
    let mut data: Value = match serde_json::from_str(&raw) {
        Ok(d) => d,
        Err(e) => return IndexStatus::Unreadable(e.to_string()),
    };
    let mut n = 0;
    if let Some(m) = remap(data.get("originalPath"), old, new) {
        data["originalPath"] = Value::String(m);
        n += 1;
    }
    if let Some(entries) = data.get_mut("entries").and_then(Value::as_array_mut) {
        for entry in entries.iter_mut().filter(|e| e.is_object()) {
            for key in ["projectPath", "cwd"] {
                if let Some(m) = remap(entry.get(key), old, new) {
                    entry[key] = Value::String(m);
                    n += 1;
                }
            }
            if let Some(m) = remap(entry.get("fullPath"), old_dir, new_dir) {
                entry["fullPath"] = Value::String(m);
                n += 1;
            }
        }
    }
    if apply && n > 0 {
        if let Err(e) = backup_file(idx).and_then(|_| write_json_atomic(idx, &data, raw.contains("\n  "))) {
            return IndexStatus::Failed(e.to_string());
        }
    }
    IndexStatus::Ok(n)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ConfigStatus {
    Missing(Option<String>),
    Absent,
    Conflict,
    Changed,
    Ok,
}

/// Rename the project's key in `~/.claude.json`, keeping its position.
pub(super) fn rewrite_claude_json(path: &Path, old: &str, new: &str, merge: bool, apply: bool) -> ConfigStatus {
    if !path.exists() {
        return ConfigStatus::Missing(None);
    }
    // Stat before the read so the concurrency check brackets it.
    let before = stamp(path);
    let mut data: Value = match fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|r| {
        serde_json::from_str(&r).map_err(|e| e.to_string())
    }) {
        Ok(d) => d,
        Err(e) => return ConfigStatus::Missing(Some(e)),
    };
    let Some(projects) = data.get("projects").and_then(Value::as_object) else {
        return ConfigStatus::Absent;
    };
    if !projects.contains_key(old) {
        return ConfigStatus::Absent;
    }
    if projects.contains_key(new) && !merge {
        return ConfigStatus::Conflict;
    }
    let mut rebuilt = Map::new();
    for (k, v) in projects {
        if k == old {
            rebuilt.insert(new.to_string(), v.clone());
        } else if k != new {
            rebuilt.insert(k.clone(), v.clone());
        }
    }
    data["projects"] = Value::Object(rebuilt);
    if apply {
        // Shared file: another session may have rewritten it since the read.
        if stamp(path) != before {
            return ConfigStatus::Changed;
        }
        if let Err(e) = backup_file(path).and_then(|_| write_json_atomic(path, &data, true)) {
            return ConfigStatus::Missing(Some(e.to_string()));
        }
    }
    ConfigStatus::Ok
}

/// True only when every record is a recognized session-state record. An
/// allowlist: an unparseable line or an unfamiliar type returns false,
/// because the only caller uses this to decide a deletion.
pub(super) fn state_only(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else { return false };
    let text = String::from_utf8_lossy(&bytes);
    text.lines().filter(|l| !l.trim().is_empty()).all(|l| {
        serde_json::from_str::<Value>(l)
            .ok()
            .and_then(|v| v.get("type").and_then(Value::as_str).map(|t| STATE_ONLY_TYPES.contains(&t)))
            .unwrap_or(false)
    })
}

/// `src`'s (size, mtime) when it is a leftover whose content now lives at
/// `dst`: a live session writes state records after a move, recreating the
/// old path. Safe to drop only when the counterpart is strictly larger and
/// `src` holds nothing but regenerable state. The stamp is taken before the
/// contents are read, so the caller can tell whether it changed since.
pub(super) fn superseded_remnant(src: &Path, dst: &Path) -> Option<(u64, std::time::SystemTime)> {
    if src.is_symlink() || !dst.is_file() {
        return None;
    }
    let st = stamp(src)?;
    if st.0 >= fs::metadata(dst).ok()?.len() {
        return None;
    }
    state_only(src).then_some(st)
}

/// Unlink only if the file still matches `st`, taken before the check that
/// judged it removable.
pub(super) fn remove_if_unchanged(path: &Path, st: (u64, std::time::SystemTime)) -> String {
    if stamp(path) != Some(st) {
        return "changed".to_string();
    }
    match fs::remove_file(path) {
        Ok(()) => "removed".to_string(),
        Err(e) => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn a_backslash_is_not_a_separator_on_unix() {
        let v = Value::String("/a/b\\c".to_string());
        assert_eq!(remap(Some(&v), "/a/b", "/n"), None);
        assert!(!path_matches(&v, "/a/b"));
        assert!(!starts_component("\\c"));
    }

    #[test]
    fn remap_only_at_or_under_old() {
        let v = |s: &str| Value::String(s.to_string());
        assert_eq!(remap(Some(&v("/a/b")), "/a/b", "/n").as_deref(), Some("/n"));
        assert_eq!(remap(Some(&v("/a/b/c")), "/a/b", "/n").as_deref(), Some("/n/c"));
        assert_eq!(remap(Some(&v("/a/bc")), "/a/b", "/n"), None);
        assert_eq!(remap(None, "/a/b", "/n"), None);
    }

    #[test]
    fn rewrite_jsonl_keeps_untouched_lines_byte_for_byte() {
        let dir = std::env::temp_dir().join(format!("ways-reloc-jsonl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let content = concat!(
            "{\"type\":\"mode\",  \"x\":1}\r\n",
            "{\"cwd\":\"/old/p\",\"type\":\"user\",\"message\":\"in /old/p\"}\r\n",
            "{\"cwd\":\"/other\",\"note\":\"/old/p mentioned\"}\n",
            "{\"cwd\":\"/old/p/sub\",\"nested\":{\"cwd\":\"/old/p\"}}"
        );
        fs::write(&path, content).unwrap();
        let preview = rewrite_jsonl(&path, "cwd", "/old/p", "/new", false, false).unwrap();
        assert_eq!(preview, Rewrite { changed: 2, residual: 3, nested: 1 });
        assert_eq!(fs::read_to_string(&path).unwrap(), content);

        rewrite_jsonl(&path, "cwd", "/old/p", "/new", true, false).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = after.split_inclusive('\n').collect();
        assert_eq!(lines[0], "{\"type\":\"mode\",  \"x\":1}\r\n");
        assert_eq!(lines[1], "{\"cwd\":\"/new\",\"type\":\"user\",\"message\":\"in /old/p\"}\r\n");
        assert_eq!(lines[2], "{\"cwd\":\"/other\",\"note\":\"/old/p mentioned\"}\n");
        assert_eq!(lines[3], "{\"cwd\":\"/new/sub\",\"nested\":{\"cwd\":\"/old/p\"}}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn state_only_is_an_allowlist() {
        let dir = std::env::temp_dir().join(format!("ways-reloc-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.jsonl");
        fs::write(&a, "{\"type\":\"mode\"}\n\n{\"type\":\"ai-title\"}\n").unwrap();
        assert!(state_only(&a));
        fs::write(&a, "{\"type\":\"mode\"}\n{\"type\":\"summary\"}\n").unwrap();
        assert!(!state_only(&a));
        fs::write(&a, "{\"type\":\"mode\"}\nnot json\n").unwrap();
        assert!(!state_only(&a));
        fs::remove_dir_all(&dir).ok();
    }
}
