//! `ways projects relocate OLD NEW`: move a project's session history from
//! one working directory to another, so sessions started in OLD resume in
//! NEW. Previews by default; `--execute` applies.
//!
//! What moves: the project directory under `~/.claude/projects` (renamed,
//! or merged into an existing one with `--merge`), the top-level `cwd` of
//! each transcript record, the paths in `sessions-index.json`, the project's
//! key in `~/.claude.json`, and the `project` of each `history.jsonl` prompt.
//! Message text and tool output that mention the old path are left as the
//! record of what happened.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use claude_sessions::project_slug;
use serde_json::{Map, Value};

use super::Env;

#[derive(clap::Args)]
pub struct RelocateArgs {
    /// Current project directory (where the sessions were started)
    pub old_path: String,
    /// New project directory (where you want to resume)
    pub new_path: String,
    /// Apply the changes (default is preview)
    #[arg(long, conflicts_with = "dry_run")]
    pub execute: bool,
    /// Preview only (the default; accepted for symmetry with cleanup)
    #[arg(long)]
    pub dry_run: bool,
    /// Leave transcripts byte-identical; only move metadata
    #[arg(long)]
    pub keep_transcript_cwd: bool,
    /// Allow merging into an existing project at NEW_PATH
    #[arg(long)]
    pub merge: bool,
    /// Proceed even if a session looks live in this project
    #[arg(long)]
    pub force: bool,
}

/// A transcript touched this recently probably belongs to a live session,
/// which appends to the file about to be rewritten.
const LIVE_SESSION_WINDOW: u64 = 120;

/// Records Claude Code writes to track session state rather than
/// conversation, each one regenerated on the next session. A file holding
/// nothing but these is a sidecar, not history.
///
/// Every entry must be regenerable, because a remnant is a divergent tail,
/// not a duplicate: sweeping it always discards a fork. `summary` is not in
/// the set: it carries compaction prose and the `leafUuid` `--resume` reads.
const STATE_ONLY_TYPES: &[&str] =
    &["last-prompt", "ai-title", "mode", "permission-mode", "bridge-session", "agent-name"];

// ── Paths ───────────────────────────────────────────────────────

/// Absolute, `~`-expanded, lexically normalized, no trailing slash.
fn norm_path(p: &str, home: &str) -> String {
    let expanded = if p == "~" {
        home.to_string()
    } else if let Some(rest) = p.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        p.to_string()
    };
    let abs = if expanded.starts_with('/') {
        expanded
    } else {
        let cwd = std::env::current_dir().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{cwd}/{expanded}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in abs.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

/// `value` remapped from `old` to `new` when it is `old` or lies under it.
fn remap(value: Option<&Value>, old: &str, new: &str) -> Option<String> {
    let v = value?.as_str()?;
    if v == old {
        Some(new.to_string())
    } else {
        v.strip_prefix(old).filter(|r| r.starts_with('/')).map(|r| format!("{new}{r}"))
    }
}

fn path_matches(value: &Value, old: &str) -> bool {
    value.as_str().is_some_and(|v| v == old || v.strip_prefix(old).is_some_and(|r| r.starts_with('/')))
}

/// Does `field` hold a path at or under `old` anywhere below the top level?
/// Every cwd Claude Code writes today is top-level, which is what makes this
/// a targeted remap rather than a find-and-replace; a nested one is reported
/// as unhandled rather than counted as preserved history.
fn has_nested(obj: &Value, field: &str, old: &str, top: bool) -> bool {
    match obj {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| (!top && k == field && path_matches(v, old)) || has_nested(v, field, old, false)),
        Value::Array(items) => items.iter().any(|v| has_nested(v, field, old, false)),
        _ => false,
    }
}

// ── Files ───────────────────────────────────────────────────────

/// A timestamped sibling copy, `<name>.bak-relocate-<UTC stamp>[.n]`.
fn backup_file(path: &Path) -> std::io::Result<PathBuf> {
    let secs = super::epoch_now();
    let (y, m, d) = ways_core::util::days_to_ymd(secs / 86_400);
    let t = secs % 86_400;
    let stamp = format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", t / 3600, t / 60 % 60, t % 60);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut dest = path.with_file_name(format!("{name}.bak-relocate-{stamp}"));
    let mut n = 0;
    // Same-second relocations must not clobber each other.
    while dest.exists() {
        n += 1;
        dest = path.with_file_name(format!("{name}.bak-relocate-{stamp}.{n}"));
    }
    fs::copy(path, &dest)?;
    if let Ok(meta) = fs::metadata(path) {
        if let Ok(mtime) = meta.modified() {
            let _ = fs::File::options().write(true).open(&dest).and_then(|f| f.set_modified(mtime));
        }
    }
    Ok(dest)
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{name}.relocate-tmp"))
}

/// Swap `tmp` into `path`, keeping `path`'s mtime and permissions: project
/// listings sort by mtime, and relocating is not activity.
fn replace_atomic(tmp: &Path, path: &Path) -> std::io::Result<()> {
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(tmp, meta.permissions())?;
        if let Ok(mtime) = meta.modified() {
            fs::File::options().write(true).open(tmp)?.set_modified(mtime)?;
        }
    }
    fs::rename(tmp, path)
}

/// Serialize to a temp sibling, fsync, then swap. Never truncates in place.
fn write_json_atomic(path: &Path, data: &Value, pretty: bool) -> std::io::Result<()> {
    let tmp = tmp_sibling(path);
    let result = (|| {
        let text = if pretty { serde_json::to_string_pretty(data) } else { serde_json::to_string(data) }
            .map_err(std::io::Error::other)?;
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        replace_atomic(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let m = fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Rewrite {
    changed: usize,
    /// Lines still mentioning the old path afterwards (message text, tool
    /// output), deliberately left as record.
    residual: usize,
    /// Records carrying the field below the top level, which are not remapped.
    nested: usize,
}

#[derive(Debug)]
enum RewriteError {
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
fn rewrite_jsonl(
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

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Union two indexes by `sessionId`, the source winning. A merge that moved
/// transcripts but dropped their entries would leave them looking orphaned,
/// and `hygiene` offers to delete those.
fn merge_sessions_index(src: &Path, dst: &Path) -> std::io::Result<usize> {
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

enum IndexStatus {
    Ok(usize),
    Unreadable(String),
    Failed(String),
}

/// Remap `originalPath` and each entry's `projectPath` and `cwd`, plus
/// `fullPath`, which embeds the project directory.
fn rewrite_sessions_index(idx: &Path, old: &str, new: &str, old_dir: &str, new_dir: &str, apply: bool) -> IndexStatus {
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
enum ConfigStatus {
    Missing(Option<String>),
    Absent,
    Conflict,
    Changed,
    Ok,
}

/// Rename the project's key in `~/.claude.json`, keeping its position.
fn rewrite_claude_json(path: &Path, old: &str, new: &str, merge: bool, apply: bool) -> ConfigStatus {
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
fn state_only(path: &Path) -> bool {
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
fn superseded_remnant(src: &Path, dst: &Path) -> Option<(u64, std::time::SystemTime)> {
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
fn remove_if_unchanged(path: &Path, st: (u64, std::time::SystemTime)) -> String {
    if stamp(path) != Some(st) {
        return "changed".to_string();
    }
    match fs::remove_file(path) {
        Ok(()) => "removed".to_string(),
        Err(e) => e.to_string(),
    }
}

/// Confirm this relocation landed, and account for anything left behind in
/// the old directory. Scoped to the files this command moved.
fn verify_move(
    old_dir: &Path,
    work_dir: &Path,
    moved: &[String],
    failures: &mut Vec<String>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let mut missing: Vec<&String> = moved.iter().filter(|n| !work_dir.join(n).exists()).collect();
    if !missing.is_empty() {
        missing.sort();
        let names: Vec<&str> = missing.iter().take(3).map(|s| s.as_str()).collect();
        failures.push(format!("{} transcript(s) did not arrive: {}", missing.len(), names.join(", ")));
        writeln!(out, "  error {} transcript(s) missing from the target", missing.len())?;
    }
    if !old_dir.exists() || old_dir == work_dir {
        return Ok(());
    }

    let old_name = old_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut items: Vec<PathBuf> = fs::read_dir(old_dir)?.flatten().map(|e| e.path()).collect();
    items.sort();
    let (mut swept, mut kept, mut stranded) = (0, Vec::new(), Vec::new());
    for item in items {
        let name = item.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if item.is_dir() {
            // Only succeeds when already empty.
            if fs::remove_dir(&item).is_err() {
                kept.push(format!("{name}/"));
            }
            continue;
        }
        let st = if moved.contains(&name) { superseded_remnant(&item, &work_dir.join(&name)) } else { None };
        match st {
            Some(st) => {
                let result = remove_if_unchanged(&item, st);
                if result == "removed" {
                    swept += 1;
                    continue;
                }
                kept.push(format!("{name} ({result})"));
            }
            None => kept.push(name.clone()),
        }
        if name.ends_with(".jsonl") {
            stranded.push(name);
        }
    }
    if swept > 0 {
        writeln!(out, "  swept {swept} superseded remnant(s) from {old_name}")?;
    }
    if !kept.is_empty() {
        writeln!(out, "  left behind {} item(s) in {old_name}:", kept.len())?;
        for k in kept.iter().take(5) {
            writeln!(out, "    {k}")?;
        }
        if kept.len() > 5 {
            writeln!(out, "    … and {} more", kept.len() - 5)?;
        }
    }
    if !stranded.is_empty() {
        // A transcript still under the old name did not relocate: a failed
        // move, not a note.
        stranded.sort();
        let names: Vec<&str> = stranded.iter().take(3).map(|s| s.as_str()).collect();
        failures.push(format!("{} transcript(s) stranded in {old_name}: {}", stranded.len(), names.join(", ")));
    }
    if fs::remove_dir(old_dir).is_ok() {
        writeln!(out, "  removed empty {old_name}")?;
    }
    Ok(())
}

/// POSIX shell quoting, as Python's `shlex.quote`.
fn shell_quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

fn is_live(path: &Path, now: u64) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            now.saturating_sub(secs) < LIVE_SESSION_WINDOW
        })
        .unwrap_or(false)
}

/// Move `old_dir`'s contents into an existing `new_dir`. A name collision is
/// decided here: a superseded remnant is dropped, anything else is kept in
/// place. Indexes are unioned rather than overwritten.
fn merge_into(old_dir: &Path, new_dir: &Path, out: &mut dyn Write) -> std::io::Result<()> {
    fs::create_dir_all(new_dir)?;
    let mut items: Vec<PathBuf> = fs::read_dir(old_dir)?.flatten().map(|e| e.path()).collect();
    items.sort();
    let mut pending_idx = None;
    for item in items {
        let name = item.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        let target = new_dir.join(&name);
        if name == "sessions-index.json" && target.exists() {
            pending_idx = Some(item);
            continue;
        }
        if target.exists() {
            match superseded_remnant(&item, &target) {
                None => writeln!(out, "  skip {} (already in target)", name.to_string_lossy())?,
                Some(st) => {
                    let result = remove_if_unchanged(&item, st);
                    if result != "removed" {
                        writeln!(out, "  kept {} ({result})", name.to_string_lossy())?;
                    }
                }
            }
            continue;
        }
        fs::rename(&item, &target)?;
    }
    if let Some(idx) = pending_idx {
        let total = merge_sessions_index(&idx, &new_dir.join("sessions-index.json"))?;
        fs::remove_file(&idx)?;
        writeln!(out, "  merged sessions-index.json ({total} entries)")?;
    }
    if fs::read_dir(old_dir)?.next().is_none() {
        fs::remove_dir(old_dir)?;
    }
    Ok(())
}

/// Run `relocate`. `Ok(false)` is a refusal or a failed step.
pub(super) fn relocate(env: &Env, args: &RelocateArgs, out: &mut dyn Write) -> Result<bool> {
    let apply = args.execute;
    let old = norm_path(&args.old_path, &env.home);
    let new = norm_path(&args.new_path, &env.home);
    if old == new {
        writeln!(out, "  error old and new paths are identical: {old}")?;
        return Ok(false);
    }

    let projects = env.projects();
    let (old_slug, new_slug) = (project_slug(&old), project_slug(&new));
    let old_dir = claude_sessions::find_project_dir_in(&projects, &old).unwrap_or_else(|| projects.join(&old_slug));
    let new_dir = claude_sessions::find_project_dir_in(&projects, &new).unwrap_or_else(|| projects.join(&new_slug));
    let same_dir = old_dir == new_dir;

    if !old_dir.is_dir() {
        writeln!(out, "\n  No session history for {old}")?;
        writeln!(out, "  expected {}\n", old_dir.display())?;
        let stem: String = old.rsplit('/').next().unwrap_or("").to_lowercase().chars().take(6).collect();
        if !stem.is_empty() {
            let near: Vec<String> = claude_sessions::project_dirs_in(&projects)
                .iter()
                .filter_map(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
                .filter(|n| n.to_lowercase().contains(&stem))
                .take(8)
                .collect();
            if !near.is_empty() {
                writeln!(out, "  Similar project directories:")?;
                for n in near {
                    writeln!(out, "    {n}")?;
                }
                writeln!(out)?;
            }
        }
        return Ok(false);
    }

    let transcripts = claude_sessions::transcripts_in(&old_dir);
    let dir_name = |d: &Path| d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    // ── Plan ────────────────────────────────────────────────────
    writeln!(out, "\nRelocate project")?;
    writeln!(out, "  from  {old}   {}", dir_name(&old_dir))?;
    writeln!(out, "  to    {new}   {}", dir_name(&new_dir))?;
    writeln!(out)?;

    let mut warnings: Vec<String> = Vec::new();
    let new_path = Path::new(&new);
    if new_path.exists() && !new_path.is_dir() {
        writeln!(out, "  error {new} exists but is not a directory")?;
        return Ok(false);
    }
    // Nothing resumes into a missing working directory, so create it.
    let create_target = !new_path.is_dir();

    let live = transcripts.iter().filter(|t| is_live(t, super::epoch_now())).count();
    if live > 0 {
        warnings.push(format!(
            "{live} transcript(s) modified in the last {LIVE_SESSION_WINDOW}s — a session may be live in this project"
        ));
    }
    let merging = new_dir.exists() && !same_dir;
    if merging && !args.merge {
        warnings.push(format!(
            "{} already exists — pass --merge to combine, or relocate that project away first",
            dir_name(&new_dir)
        ));
    }
    if create_target {
        writeln!(out, "  workdir     create {new} (does not exist yet)")?;
    }
    if same_dir {
        writeln!(out, "  directory   unchanged (both paths encode to the same slug)")?;
    } else if merging {
        writeln!(out, "  directory   merge into existing {}", dir_name(&new_dir))?;
    } else {
        writeln!(out, "  directory   rename → {}", dir_name(&new_dir))?;
    }

    let mut plan = Rewrite::default();
    if args.keep_transcript_cwd {
        writeln!(out, "  transcripts skipped (--keep-transcript-cwd)")?;
    } else {
        for t in &transcripts {
            if let Ok(r) = rewrite_jsonl(t, "cwd", &old, &new, false, false) {
                plan.changed += r.changed;
                plan.residual += r.residual;
                plan.nested += r.nested;
            }
        }
        writeln!(out, "  transcripts {} cwd field(s) across {} file(s)", plan.changed, transcripts.len())?;
    }

    let (old_dir_s, new_target) = (old_dir.to_string_lossy().into_owned(), new_dir.to_string_lossy().into_owned());
    let idx = old_dir.join("sessions-index.json");
    if idx.exists() {
        match rewrite_sessions_index(&idx, &old, &new, &old_dir_s, &new_target, false) {
            IndexStatus::Ok(n) => writeln!(out, "  index       {n} field(s) in sessions-index.json")?,
            IndexStatus::Unreadable(e) | IndexStatus::Failed(e) => {
                warnings.push(format!(
                    "sessions-index.json is unreadable ({e}) — it will be left alone; Claude Code rebuilds it"
                ));
                writeln!(out, "  index       unreadable")?;
            }
        }
    } else {
        writeln!(out, "  index       none")?;
    }

    let config = rewrite_claude_json(&env.claude_json, &old, &new, args.merge, false);
    let label = match &config {
        ConfigStatus::Ok => "projects key renamed".to_string(),
        ConfigStatus::Absent => "no entry for this path".to_string(),
        ConfigStatus::Conflict => format!("entry for {new} already exists — pass --merge to supersede it"),
        ConfigStatus::Missing(Some(e)) => format!("unreadable ({e})"),
        ConfigStatus::Missing(None) | ConfigStatus::Changed => "not found".to_string(),
    };
    writeln!(out, "  config      ~/.claude.json: {label}")?;

    let history = env.claude.history_file();
    let hist_n = if history.exists() {
        rewrite_jsonl(&history, "project", &old, &new, false, false).map(|r| r.changed).unwrap_or(0)
    } else {
        0
    };
    writeln!(out, "  history     {hist_n} prompt(s) in history.jsonl")?;

    if plan.residual > 0 {
        writeln!(
            out,
            "\n  note {} line(s) still mention the old path in message text or tool output.",
            plan.residual
        )?;
        writeln!(out, "       Those are a record of what happened and are left as-is.")?;
    }
    if plan.nested > 0 {
        warnings.push(format!(
            "{} record(s) carry a nested cwd this command does not remap — Claude's transcript format may have changed; re-check before relying on the result",
            plan.nested
        ));
    }
    for w in &warnings {
        writeln!(out, "\n  warning {w}")?;
    }

    if !apply {
        writeln!(out, "\n  Preview only — re-run with --execute to apply.\n")?;
        return Ok(true);
    }
    if live > 0 && !args.force {
        writeln!(out, "\n  Refusing to rewrite transcripts while a session may be live.")?;
        writeln!(out, "  Exit that session, or pass --force if you know it is idle.\n")?;
        return Ok(false);
    }
    if merging && !args.merge {
        writeln!(out, "\n  Refusing to overwrite an existing project directory.\n")?;
        return Ok(false);
    }
    if config == ConfigStatus::Conflict {
        // Proceeding would move the history while the old key points at nothing.
        writeln!(out, "\n  Refusing: ~/.claude.json already has an entry for {new}.")?;
        writeln!(out, "  Pass --merge to supersede it with the relocated project.\n")?;
        return Ok(false);
    }

    // ── Execute ─────────────────────────────────────────────────
    // Move first: a rename is atomic and reversible, so if it fails nothing
    // else has been touched.
    writeln!(out)?;
    let mut failures: Vec<String> = Vec::new();
    if create_target {
        if let Err(e) = fs::create_dir_all(&new) {
            writeln!(out, "  error could not create {new}: {e}\n")?;
            return Ok(false);
        }
        writeln!(out, "  created {new}")?;
    }

    let moved: Vec<String> =
        transcripts.iter().filter_map(|t| t.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    let mut work_dir = old_dir.clone();
    if !same_dir {
        let result = if merging { merge_into(&old_dir, &new_dir, out) } else { fs::rename(&old_dir, &new_dir) };
        if let Err(e) = result {
            writeln!(out, "  error could not move project directory: {e}\n")?;
            return Ok(false);
        }
        work_dir = new_dir.clone();
        writeln!(out, "  moved {} → {}", dir_name(&old_dir), dir_name(&new_dir))?;
    }

    if !args.keep_transcript_cwd {
        let mut n = 0;
        for t in claude_sessions::transcripts_in(&work_dir) {
            match rewrite_jsonl(&t, "cwd", &old, &new, true, false) {
                Ok(r) => n += r.changed,
                Err(e) => {
                    let name = dir_name(&t);
                    failures.push(format!("{name}: {e}"));
                    writeln!(out, "  error {name}: {e}")?;
                }
            }
        }
        writeln!(out, "  rewrote {n} cwd field(s) in transcripts")?;
    }

    let idx = work_dir.join("sessions-index.json");
    if idx.exists() {
        match rewrite_sessions_index(&idx, &old, &new, &old_dir_s, &new_target, true) {
            IndexStatus::Ok(n) => writeln!(out, "  rewrote {n} field(s) in sessions-index.json")?,
            IndexStatus::Unreadable(e) | IndexStatus::Failed(e) => {
                failures.push(format!("sessions-index.json: {e}"));
                writeln!(out, "  error sessions-index.json: {e}")?;
            }
        }
    }

    match rewrite_claude_json(&env.claude_json, &old, &new, args.merge, true) {
        ConfigStatus::Ok => writeln!(out, "  renamed projects key in ~/.claude.json (backed up)")?,
        ConfigStatus::Changed => {
            failures.push("~/.claude.json changed while relocating — left untouched".to_string());
            writeln!(out, "  error ~/.claude.json changed underneath us — left untouched")?;
        }
        ConfigStatus::Missing(Some(e)) => {
            failures.push(format!("~/.claude.json: {e}"));
            writeln!(out, "  error ~/.claude.json: {e}")?;
        }
        _ => {}
    }

    if history.exists() {
        let result = backup_file(&history)
            .map_err(RewriteError::Io)
            .and_then(|_| rewrite_jsonl(&history, "project", &old, &new, true, true));
        match result {
            Ok(r) => writeln!(out, "  rewrote {} prompt(s) in history.jsonl (backed up)", r.changed)?,
            Err(RewriteError::Concurrent) => {
                failures.push("history.jsonl was appended to while relocating — left untouched".to_string());
                writeln!(out, "  error history.jsonl changed underneath us — left untouched")?;
            }
            Err(e) => {
                failures.push(format!("history.jsonl: {e}"));
                writeln!(out, "  error history.jsonl: {e}")?;
            }
        }
    }

    verify_move(&old_dir, &work_dir, &moved, &mut failures, out)?;

    if !failures.is_empty() {
        writeln!(out, "\n  Relocation incomplete — {} step(s) failed:", failures.len())?;
        for f in &failures {
            writeln!(out, "    · {f}")?;
        }
        writeln!(out, "\n  The project directory has already moved. Re-run the same command")?;
        writeln!(out, "  to retry the remaining steps; completed steps are no-ops.\n")?;
        return Ok(false);
    }

    writeln!(out, "\n  Done. Resume with:  cd {} && claude --resume", shell_quote(&new))?;
    if merging {
        writeln!(out, "  Merged into an existing project — reversing is not a single command.\n")?;
    } else {
        writeln!(
            out,
            "  To reverse: ways projects relocate {} {} --execute\n",
            shell_quote(&new),
            shell_quote(&old)
        )?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_path_expands_and_normalizes() {
        assert_eq!(norm_path("~/a/./b/../c/", "/home/u"), "/home/u/a/c");
        assert_eq!(norm_path("~", "/home/u"), "/home/u");
        assert_eq!(norm_path("/", "/home/u"), "/");
        assert_eq!(norm_path("/x//y/", "/home/u"), "/x/y");
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
    fn shell_quote_matches_shlex() {
        assert_eq!(shell_quote("/a/b-c_d.e"), "/a/b-c_d.e");
        assert_eq!(shell_quote("/a b"), "'/a b'");
        assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
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
