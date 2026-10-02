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

use super::fsio::backup_file;
use super::rewrite::{
    merge_sessions_index, remove_if_unchanged, rewrite_claude_json, rewrite_jsonl, rewrite_sessions_index,
    superseded_remnant, ConfigStatus, IndexStatus, Rewrite, RewriteError,
};
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

// ── Paths ───────────────────────────────────────────────────────

/// Absolute, `~`-expanded, lexically normalized, no trailing separator.
///
/// A path starting with `/` is normalized on `/` as text, on every platform,
/// because that is how Claude Code records it. Any other absolute path (a
/// Windows `C:\x`) is normalized by its components. A relative path is
/// joined to the working directory first.
fn norm_path(p: &str, home: &str) -> String {
    let expanded = if p == "~" {
        home.to_string()
    } else if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        Path::new(home).join(rest).to_string_lossy().into_owned()
    } else {
        p.to_string()
    };
    let abs = if expanded.starts_with('/') || Path::new(&expanded).is_absolute() {
        expanded
    } else {
        std::env::current_dir().unwrap_or_default().join(&expanded).to_string_lossy().into_owned()
    };
    if abs.starts_with('/') {
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
        return format!("/{}", parts.join("/"));
    }
    let mut out = PathBuf::new();
    for c in Path::new(&abs).components() {
        match c {
            std::path::Component::CurDir => {}
            // `pop` never removes the root or a Windows prefix.
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
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
pub(super) fn shell_quote(s: &str) -> String {
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
/// place. Indexes are unioned rather than overwritten. On failure, the
/// names already moved come back with the error.
fn merge_into(old_dir: &Path, new_dir: &Path, out: &mut dyn Write) -> Result<(), (Vec<String>, std::io::Error)> {
    let mut done: Vec<String> = Vec::new();
    let fail = |done: &Vec<String>, e: std::io::Error| (done.clone(), e);
    fs::create_dir_all(new_dir).map_err(|e| fail(&done, e))?;
    let mut items: Vec<PathBuf> =
        fs::read_dir(old_dir).map_err(|e| fail(&done, e))?.flatten().map(|e| e.path()).collect();
    items.sort();
    let mut pending_idx = None;
    for item in items {
        let name = item.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        let label = name.to_string_lossy().into_owned();
        let target = new_dir.join(&name);
        if name == "sessions-index.json" && target.exists() {
            pending_idx = Some(item);
            continue;
        }
        if target.exists() {
            match superseded_remnant(&item, &target) {
                None => {
                    let _ = writeln!(out, "  skip {label} (already in target)");
                }
                Some(st) => {
                    let result = remove_if_unchanged(&item, st);
                    if result != "removed" {
                        let _ = writeln!(out, "  kept {label} ({result})");
                    }
                }
            }
            continue;
        }
        fs::rename(&item, &target).map_err(|e| fail(&done, e))?;
        done.push(label);
    }
    if let Some(idx) = pending_idx {
        let total =
            merge_sessions_index(&idx, &new_dir.join("sessions-index.json")).map_err(|e| fail(&done, e))?;
        fs::remove_file(&idx).map_err(|e| fail(&done, e))?;
        let _ = writeln!(out, "  merged sessions-index.json ({total} entries)");
    }
    if fs::read_dir(old_dir).map_err(|e| fail(&done, e))?.next().is_none() {
        fs::remove_dir(old_dir).map_err(|e| fail(&done, e))?;
    }
    Ok(())
}

/// Is the process `pid` running?
#[cfg(target_os = "linux")]
fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Is the process `pid` running?
#[cfg(all(unix, not(target_os = "linux")))]
fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Is the process `pid` running? A handle with query rights opens for a
/// live or recently exited process; the exit code tells them apart.
#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: the handle is checked before use and closed exactly once;
    // `code` outlives the call that writes it.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        ok && code == STILL_ACTIVE as u32
    }
}

/// What `relocate` will do, worked out without changing anything.
struct Plan {
    old: String,
    new: String,
    old_dir: PathBuf,
    new_dir: PathBuf,
    /// `old`'s directory is gone and `new`'s holds the history: an earlier
    /// run moved it and failed a later step. The remaining steps run.
    resume: bool,
    same_dir: bool,
    merging: bool,
    create_target: bool,
    transcripts: Vec<PathBuf>,
    /// Transcripts modified within [`LIVE_SESSION_WINDOW`].
    recent: usize,
    /// Running sessions whose cwd is at or under `old`.
    live_sessions: usize,
    config: ConfigStatus,
    history: PathBuf,
}

/// Work out and print the plan. `Ok(None)` is a refusal already printed.
fn plan(env: &Env, args: &RelocateArgs, out: &mut dyn Write) -> Result<Option<Plan>> {
    let old = norm_path(&args.old_path, &env.home);
    let new = norm_path(&args.new_path, &env.home);
    if old == new {
        writeln!(out, "  error old and new paths are identical: {old}")?;
        return Ok(None);
    }

    let projects = env.projects();
    let dir_name = |d: &Path| d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // OLD's directory is found by its slug prefix only when verified to be
    // OLD's; a sibling sharing the prefix is never taken. NEW normally has no
    // history yet, so an unverified sibling there is no ambiguity: NEW gets
    // the exact name Claude Code tries first.
    {
        let path = &old;
        let candidates = claude_sessions::prefix_candidates_in(&projects, path);
        if claude_sessions::find_project_dir_in(&projects, path).is_none() && !candidates.is_empty() {
            writeln!(
                out,
                "\n  error ambiguous: {} directories share the 200-character prefix of {path}, and none records that path:",
                candidates.len()
            )?;
            for c in candidates.iter().take(8) {
                writeln!(out, "    {}", dir_name(c))?;
            }
            writeln!(out)?;
            return Ok(None);
        }
    }
    let found_old = claude_sessions::find_project_dir_in(&projects, &old);
    let found_new = claude_sessions::find_project_dir_in(&projects, &new);
    let old_dir = found_old.clone().unwrap_or_else(|| projects.join(project_slug(&old)));
    let new_dir = found_new.clone().unwrap_or_else(|| projects.join(project_slug(&new)));
    let same_dir = old_dir == new_dir;
    let history = env.claude.history_file();

    // A rerun after a failed step: the directory already moved.
    let resume = found_old.is_none() && found_new.is_some() && {
        let pending_cwd = claude_sessions::transcripts_in(&new_dir)
            .iter()
            .any(|t| rewrite_jsonl(t, "cwd", &old, &new, false, false).is_ok_and(|r| r.changed > 0));
        let pending_hist = history.exists()
            && rewrite_jsonl(&history, "project", &old, &new, false, false).is_ok_and(|r| r.changed > 0);
        let pending_cfg = rewrite_claude_json(&env.claude_json, &old, &new, true, false) == ConfigStatus::Ok;
        pending_cwd || pending_hist || pending_cfg
    };

    if found_old.is_none() && !resume {
        writeln!(out, "\n  No session history for {old}")?;
        writeln!(out, "  expected {}\n", old_dir.display())?;
        let stem: String = old.rsplit('/').next().unwrap_or("").to_lowercase().chars().take(6).collect();
        if !stem.is_empty() {
            let near: Vec<String> = claude_sessions::project_dirs_in(&projects)
                .iter()
                .map(|d| dir_name(d))
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
        return Ok(None);
    }

    let work_dir = if resume { &new_dir } else { &old_dir };
    let transcripts = claude_sessions::transcripts_in(work_dir);

    writeln!(out, "\nRelocate project")?;
    writeln!(out, "  from  {old}   {}", dir_name(&old_dir))?;
    writeln!(out, "  to    {new}   {}", dir_name(&new_dir))?;
    writeln!(out)?;

    let new_path = Path::new(&new);
    if new_path.exists() && !new_path.is_dir() {
        writeln!(out, "  error {new} exists but is not a directory")?;
        return Ok(None);
    }
    // Nothing resumes into a missing working directory, so create it.
    let create_target = !new_path.is_dir();

    let mut warnings: Vec<String> = Vec::new();
    let recent = transcripts.iter().filter(|t| is_live(t, super::epoch_now())).count();
    if recent > 0 {
        warnings.push(format!(
            "{recent} transcript(s) modified in the last {LIVE_SESSION_WINDOW}s — a session may be live in this project"
        ));
    }
    let records = env.claude.session_records();
    let running: Vec<&claude_sessions::SessionRecord> = records.iter().filter(|r| pid_alive(r.pid)).collect();
    let live_sessions = running
        .iter()
        .filter(|r| {
            r.cwd.as_deref().is_some_and(|c| {
                c == old || c.strip_prefix(old.as_str()).is_some_and(|rest| rest.starts_with(['/', '\\']))
            })
        })
        .count();
    if live_sessions > 0 {
        warnings.push(format!(
            "{live_sessions} running Claude Code session(s) have their working directory in {old}; an idle session still appends to its transcript"
        ));
    }
    if !running.is_empty() {
        warnings.push(format!(
            "{} Claude Code session(s) are running; each rewrites ~/.claude.json from memory and can undo the key rename — exit them before relying on it",
            running.len()
        ));
    }

    let merging = !resume && new_dir.exists() && !same_dir;
    if merging && !args.merge {
        warnings.push(format!(
            "{} already exists — pass --merge to combine, or relocate that project away first",
            dir_name(&new_dir)
        ));
    }
    if create_target {
        writeln!(out, "  workdir     create {new} (does not exist yet)")?;
    }
    if resume {
        writeln!(out, "  directory   already moved to {} — resuming the remaining steps", dir_name(&new_dir))?;
    } else if same_dir {
        writeln!(out, "  directory   unchanged (both paths encode to the same slug)")?;
    } else if merging {
        writeln!(out, "  directory   merge into existing {}", dir_name(&new_dir))?;
    } else {
        writeln!(out, "  directory   rename → {}", dir_name(&new_dir))?;
    }

    let mut tally = Rewrite::default();
    if args.keep_transcript_cwd {
        writeln!(out, "  transcripts skipped (--keep-transcript-cwd)")?;
    } else {
        for t in &transcripts {
            if let Ok(r) = rewrite_jsonl(t, "cwd", &old, &new, false, false) {
                tally.changed += r.changed;
                tally.residual += r.residual;
                tally.nested += r.nested;
            }
        }
        writeln!(out, "  transcripts {} cwd field(s) across {} file(s)", tally.changed, transcripts.len())?;
    }

    let idx = work_dir.join("sessions-index.json");
    if idx.exists() {
        let (o, n) = (old_dir.to_string_lossy(), new_dir.to_string_lossy());
        match rewrite_sessions_index(&idx, &old, &new, &o, &n, false) {
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

    let hist_n = if history.exists() {
        rewrite_jsonl(&history, "project", &old, &new, false, false).map(|r| r.changed).unwrap_or(0)
    } else {
        0
    };
    writeln!(out, "  history     {hist_n} prompt(s) in history.jsonl")?;

    if tally.residual > 0 {
        writeln!(
            out,
            "\n  note {} line(s) still mention the old path in message text or tool output.",
            tally.residual
        )?;
        writeln!(out, "       Those are a record of what happened and are left as-is.")?;
    }
    if tally.nested > 0 {
        warnings.push(format!(
            "{} record(s) carry a nested cwd this command does not remap — Claude's transcript format may have changed; re-check before relying on the result",
            tally.nested
        ));
    }
    for w in &warnings {
        writeln!(out, "\n  warning {w}")?;
    }

    Ok(Some(Plan {
        old,
        new,
        old_dir,
        new_dir,
        resume,
        same_dir,
        merging,
        create_target,
        transcripts,
        recent,
        live_sessions,
        config,
        history,
    }))
}

/// Apply a plan. `Ok(false)` is a refusal or a failed step.
fn execute(env: &Env, args: &RelocateArgs, p: &Plan, out: &mut dyn Write) -> Result<bool> {
    let dir_name = |d: &Path| d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if (p.recent > 0 || p.live_sessions > 0) && !args.force {
        writeln!(out, "\n  Refusing to rewrite transcripts while a session may be live.")?;
        writeln!(out, "  Exit that session, or pass --force if you know it is idle.\n")?;
        return Ok(false);
    }
    if p.merging && !args.merge {
        writeln!(out, "\n  Refusing to overwrite an existing project directory.\n")?;
        return Ok(false);
    }
    if p.config == ConfigStatus::Conflict {
        // Proceeding would move the history while the old key points at nothing.
        writeln!(out, "\n  Refusing: ~/.claude.json already has an entry for {}.", p.new)?;
        writeln!(out, "  Pass --merge to supersede it with the relocated project.\n")?;
        return Ok(false);
    }

    // Move first: a rename is atomic and reversible, so if it fails nothing
    // else has been touched. A merge moves file by file; on failure it lists
    // what moved.
    writeln!(out)?;
    let mut failures: Vec<String> = Vec::new();
    if p.create_target {
        if let Err(e) = fs::create_dir_all(&p.new) {
            writeln!(out, "  error could not create {}: {e}\n", p.new)?;
            return Ok(false);
        }
        writeln!(out, "  created {}", p.new)?;
    }

    let moved: Vec<String> =
        p.transcripts.iter().filter_map(|t| t.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    let mut work_dir = if p.resume { p.new_dir.clone() } else { p.old_dir.clone() };
    if !p.resume && !p.same_dir {
        let result = if p.merging {
            merge_into(&p.old_dir, &p.new_dir, out)
        } else {
            fs::rename(&p.old_dir, &p.new_dir).map_err(|e| (Vec::new(), e))
        };
        if let Err((done, e)) = result {
            writeln!(out, "  error could not move project directory: {e}")?;
            if !done.is_empty() {
                writeln!(out, "  {} item(s) already moved into {}:", done.len(), dir_name(&p.new_dir))?;
                for d in done.iter().take(10) {
                    writeln!(out, "    {d}")?;
                }
                writeln!(out, "  Re-run the same command to finish the move.")?;
            }
            writeln!(out)?;
            return Ok(false);
        }
        work_dir = p.new_dir.clone();
        writeln!(out, "  moved {} → {}", dir_name(&p.old_dir), dir_name(&p.new_dir))?;
    }

    if !args.keep_transcript_cwd {
        let mut n = 0;
        for t in claude_sessions::transcripts_in(&work_dir) {
            // Guarded: an append from a session between the read and the
            // swap fails the step rather than being dropped.
            match rewrite_jsonl(&t, "cwd", &p.old, &p.new, true, true) {
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
        let (o, n) = (p.old_dir.to_string_lossy(), p.new_dir.to_string_lossy());
        match rewrite_sessions_index(&idx, &p.old, &p.new, &o, &n, true) {
            IndexStatus::Ok(n) => writeln!(out, "  rewrote {n} field(s) in sessions-index.json")?,
            IndexStatus::Unreadable(e) | IndexStatus::Failed(e) => {
                failures.push(format!("sessions-index.json: {e}"));
                writeln!(out, "  error sessions-index.json: {e}")?;
            }
        }
    }

    match rewrite_claude_json(&env.claude_json, &p.old, &p.new, args.merge, true) {
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

    if p.history.exists() {
        let result = backup_file(&p.history)
            .map_err(RewriteError::Io)
            .and_then(|_| rewrite_jsonl(&p.history, "project", &p.old, &p.new, true, true));
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

    if !p.resume {
        verify_move(&p.old_dir, &work_dir, &moved, &mut failures, out)?;
    }

    if !failures.is_empty() {
        writeln!(out, "\n  Relocation incomplete — {} step(s) failed:", failures.len())?;
        for f in &failures {
            writeln!(out, "    · {f}")?;
        }
        writeln!(out, "\n  The project directory has already moved. Re-run the same command")?;
        writeln!(out, "  to retry the remaining steps; it resumes from the moved directory.\n")?;
        return Ok(false);
    }

    writeln!(out, "\n  Done. Resume with:  cd {} && claude --resume", shell_quote(&p.new))?;
    if p.merging {
        writeln!(out, "  Merged into an existing project — reversing is not a single command.\n")?;
    } else {
        writeln!(
            out,
            "  To reverse: ways projects relocate {} {} --execute\n",
            shell_quote(&p.new),
            shell_quote(&p.old)
        )?;
    }
    Ok(true)
}

/// Run `relocate`: print the plan, then apply it with `--execute`.
/// `Ok(false)` is a refusal or a failed step.
pub(super) fn relocate(env: &Env, args: &RelocateArgs, out: &mut dyn Write) -> Result<bool> {
    let Some(plan) = plan(env, args, out)? else {
        return Ok(false);
    };
    if !args.execute {
        writeln!(out, "\n  Preview only — re-run with --execute to apply.\n")?;
        return Ok(true);
    }
    execute(env, args, &plan, out)
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
    #[cfg(windows)]
    fn norm_path_keeps_a_drive_path() {
        assert_eq!(norm_path(r"C:\a\.\b\..\c\", r"C:\Users\u"), r"C:\a\c");
        assert_eq!(norm_path(r"~\x", r"C:\Users\u"), r"C:\Users\u\x");
    }

    #[test]
    fn shell_quote_matches_shlex() {
        assert_eq!(shell_quote("/a/b-c_d.e"), "/a/b-c_d.e");
        assert_eq!(shell_quote("/a b"), "'/a b'");
        assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
    }

    #[test]
    fn this_process_is_alive() {
        assert!(pid_alive(std::process::id()));
    }
}
