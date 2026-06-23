//! Session metrics, git operations, and side-effectful display functions.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::session;
use super::helpers::home_dir;

/// Walk up the way ID path to compute tree depth, parent, and epoch distance.
pub(crate) fn compute_tree_metrics(
    way_id: &str,
    session_id: &str,
) -> (u32, Option<String>, Option<u64>, Option<u64>) {
    let mut depth = 0u32;
    let mut parent_id: Option<String> = None;
    let mut parent_epoch: Option<u64> = None;
    let mut epoch_from_parent: Option<u64> = None;
    let current_epoch = session::get_epoch(session_id);

    let mut path = way_id.to_string();
    while let Some(idx) = path.rfind('/') {
        path = path[..idx].to_string();
        if session::way_is_shown(&path, session_id) {
            depth += 1;
            if parent_id.is_none() {
                parent_id = Some(path.clone());
                let pe = session::get_way_epoch(&path, session_id);
                parent_epoch = Some(pe);
                epoch_from_parent = Some(current_epoch.saturating_sub(pe));
            }
        }
    }

    (depth, parent_id, parent_epoch, epoch_from_parent)
}

/// Count sibling ways (total and fired) under the same parent path.
pub(crate) fn count_siblings(way_id: &str, project_dir: &str, session_id: &str) -> (u32, u32) {
    let parent_path = match way_id.rfind('/') {
        Some(idx) => &way_id[..idx],
        None => return (0, 0),
    };

    let mut total = 0u32;
    let mut fired = 0u32;

    let bases = [
        PathBuf::from(project_dir).join(".claude/ways"),
        home_dir().join(".claude/hooks/ways"),
    ];

    for base in &bases {
        let parent_dir = base.join(parent_path);
        if !parent_dir.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&parent_dir) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                    continue;
                }
                let sib_name = entry.file_name().to_string_lossy().to_string();
                let sib_id = format!("{parent_path}/{sib_name}");
                // Check it has a way file
                if session::resolve_way_file(&sib_id, project_dir).is_some() {
                    total += 1;
                    if session::way_is_shown(&sib_id, session_id) {
                        fired += 1;
                    }
                }
            }
        }
    }

    (total, fired)
}

/// Get a human-readable version string from git describe.
pub(crate) fn git_version(repo: &Path) -> String {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-C", &repo.display().to_string(), "describe", "--tags", "--match", "v*", "--always", "--dirty"])
        .output();

    let raw = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => return "unknown".to_string(),
    };

    let (describe, is_dirty) = if raw.ends_with("-dirty") {
        (raw.trim_end_matches("-dirty"), true)
    } else {
        (raw.as_str(), false)
    };

    // Parse: "v0.1.0-29-ge0841be" or "v0.1.0" or "e0841be"
    let version = if let Some(caps) = parse_git_describe(describe) {
        if caps.distance > 0 {
            format!("{} + {} commits ({})", caps.tag, caps.distance, caps.hash)
        } else {
            format!("{} (release)", caps.tag)
        }
    } else if describe.starts_with('v') {
        format!("{describe} (release)")
    } else {
        describe.to_string()
    };

    if is_dirty {
        format!("{version} · dirty")
    } else {
        version
    }
}

pub(crate) struct GitDescribe {
    pub tag: String,
    pub distance: u32,
    pub hash: String,
}

pub(crate) fn parse_git_describe(s: &str) -> Option<GitDescribe> {
    // "v0.1.0-29-ge0841be"
    let last_dash = s.rfind('-')?;
    let hash = &s[last_dash + 1..];
    if !hash.starts_with('g') {
        return None;
    }
    let rest = &s[..last_dash];
    let second_dash = rest.rfind('-')?;
    let distance: u32 = rest[second_dash + 1..].parse().ok()?;
    let tag = &rest[..second_dash];
    Some(GitDescribe {
        tag: tag.to_string(),
        distance,
        hash: hash[1..].to_string(), // strip 'g' prefix
    })
}

/// Print update availability status from the cached state file.
pub(crate) fn update_status_text() -> String {
    // Path MUST match check-config-updates.sh (the writer). Unix keys by uid
    // under /tmp; Windows uses the per-user LOCALAPPDATA base (no uid namespace —
    // LOCALAPPDATA is already per-user and `id -u` / getuid disagree there).
    #[cfg(not(windows))]
    let cache_file = {
        let uid = unsafe { libc_getuid() };
        format!("/tmp/.claude-config-update-state-{uid}")
    };
    #[cfg(windows)]
    let cache_file = format!(
        "{}/.claude-config-update-state",
        std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned())
    );
    let content = match std::fs::read_to_string(&cache_file) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    render_update_status(&content)
}

/// Render the update-availability message from the cache file's content.
/// Pure (no IO) so every install-type branch is unit-testable.
pub(crate) fn render_update_status(content: &str) -> String {
    let get = |key: &str| -> Option<String> {
        content
            .lines()
            .find(|l| l.starts_with(&format!("{key}=")))
            .map(|l| l[key.len() + 1..].to_string())
    };

    let cached_type = get("type").unwrap_or_default();
    let behind: u32 = get("behind").and_then(|s| s.parse().ok()).unwrap_or(0);
    let has_upstream = get("has_upstream").unwrap_or_default() == "true";
    let upstream_repo = "aaronsb/agent-ways";

    // Subdirectory topology (ADR-140) has two independent nudge conditions: behind
    // upstream, and "pulled but not synced" (repo HEAD moved past the last
    // projection). It is the one type that can need a nudge while behind == 0.
    if cached_type == "subdirectory" {
        let unsynced = get("unsynced").unwrap_or_default() == "true";
        if behind == 0 && !unsynced {
            return String::new();
        }
        let repo = get("repo").unwrap_or_default();
        let repo_disp = if repo.is_empty() { "<repo>" } else { &repo };
        let mut out = String::from("\n");
        if behind > 0 {
            out.push_str(&format!("**⚠ agent-ways is {behind} commit(s) behind upstream (subdirectory install).** Pull, then project into ~/.claude:\n"));
            out.push_str(&format!("`cd \"{repo_disp}\" && git pull && make sync-to-home`\n"));
        } else {
            out.push_str("**⚠ agent-ways was pulled but not synced into ~/.claude.** Re-project the latest commit:\n");
            out.push_str(&format!("`cd \"{repo_disp}\" && make sync-to-home`\n"));
        }
        return out;
    }

    if behind == 0 {
        return String::new();
    }

    let mut out = String::from("\n");
    match cached_type.as_str() {
        "clone" => {
            out.push_str(&format!("**⚠ agent-ways is {behind} commit(s) behind — run `cd ~/.claude && make update`.**\n"));
            out.push_str("`make update` pulls, rebuilds the binaries, and reinstalls. Don't use a bare `git pull`: it leaves stale binaries and aborts on machine-local config edits (settings.json) — `make update` handles both.\n");
        }
        "fork" | "renamed_clone" => {
            if has_upstream {
                out.push_str(&format!("**⚠ agent-ways is behind {upstream_repo}.** Sync upstream, then rebuild:\n"));
                out.push_str("`cd ~/.claude && git fetch upstream && git merge upstream/main && make update-binaries && make install`\n");
            } else {
                out.push_str(&format!("**⚠ agent-ways is behind {upstream_repo}.** Add upstream, then sync + rebuild:\n"));
                out.push_str(&format!("`git -C ~/.claude remote add upstream https://github.com/{upstream_repo}`\n"));
                out.push_str("`cd ~/.claude && git fetch upstream && git merge upstream/main && make update-binaries && make install`\n");
            }
        }
        "plugin" => {
            let installed = get("installed").unwrap_or_default();
            let latest = get("latest").unwrap_or_default();
            out.push_str(&format!("**Plugin update available (v{installed} -> v{latest}).** Run: `/plugin update disciplined-methodology`\n"));
        }
        _ => {}
    }
    out
}

/// Return dirty file status from git.
pub(crate) fn dirty_status_text(claude_dir: &Path) -> String {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-C", &claude_dir.display().to_string(), "status", "--short"])
        .output();

    let files: Vec<String> = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.split_whitespace().last().unwrap_or("").to_string())
                .collect()
        }
        _ => return String::new(),
    };

    if files.is_empty() {
        return String::new();
    }

    let count = files.len();
    let mut out = String::from("\n");
    if count >= 4 {
        out.push_str(&format!("**Uncommitted local changes ({count} files)** — not tracked by git.\n"));
        out.push_str("Other sessions won't see these. Commit to keep, or discard to match remote.\n");
    } else {
        let s = if count != 1 { "s" } else { "" };
        out.push_str(&format!("**Uncommitted local changes ({count} file{s}):**\n"));
    }

    let max_show = 5;
    for f in files.iter().take(max_show) {
        out.push_str(&format!("- `{f}`\n"));
    }
    if count > max_show {
        out.push_str(&format!("- ... and {} more\n", count - max_show));
    }
    if count < 4 {
        out.push_str("\n_Run `git -C ~/.claude status` to review._\n");
    }
    out
}

/// Get uid without pulling in libc crate. Only needed off Windows, where the
/// config-update cache is keyed by uid (Windows uses the per-user LOCALAPPDATA
/// base instead — see `update_status_text`).
#[cfg(not(windows))]
pub(crate) unsafe fn libc_getuid() -> u32 {
    #[cfg(unix)]
    unsafe {
        extern "C" {
            fn getuid() -> u32;
        }
        getuid()
    }
    #[cfg(not(unix))]
    0
}

#[cfg(test)]
mod tests {
    use super::render_update_status;

    #[test]
    fn subdirectory_behind_nudges_pull_then_sync() {
        let out = render_update_status(
            "type=subdirectory\nbehind=3\nrepo=/home/u/.claude/directory\nunsynced=false\n",
        );
        assert!(out.contains("3 commit(s) behind"));
        assert!(out.contains("cd \"/home/u/.claude/directory\" && git pull && make sync-to-home"));
    }

    #[test]
    fn subdirectory_unsynced_nudges_sync_even_when_not_behind() {
        let out = render_update_status(
            "type=subdirectory\nbehind=0\nrepo=/home/u/.claude/directory\nunsynced=true\n",
        );
        assert!(out.contains("pulled but not synced"));
        assert!(out.contains("cd \"/home/u/.claude/directory\" && make sync-to-home"));
        // not the behind message
        assert!(!out.contains("git pull"));
    }

    #[test]
    fn subdirectory_behind_and_unsynced_prefers_the_pull_then_sync_nudge() {
        // Both flags live: `git pull && make sync-to-home` resolves both, so the
        // behind message (which includes the pull) takes precedence.
        let out = render_update_status(
            "type=subdirectory\nbehind=2\nrepo=/home/u/.claude/directory\nunsynced=true\n",
        );
        assert!(out.contains("2 commit(s) behind"));
        assert!(out.contains("git pull && make sync-to-home"));
        assert!(!out.contains("pulled but not synced"));
    }

    #[test]
    fn subdirectory_repo_path_with_spaces_is_quoted() {
        let out = render_update_status(
            "type=subdirectory\nbehind=1\nrepo=/home/My User/.claude/dir\nunsynced=false\n",
        );
        assert!(out.contains("cd \"/home/My User/.claude/dir\" && git pull && make sync-to-home"));
    }

    #[test]
    fn subdirectory_clean_is_silent() {
        let out = render_update_status("type=subdirectory\nbehind=0\nrepo=/x\nunsynced=false\n");
        assert!(out.is_empty());
    }

    #[test]
    fn subdirectory_missing_repo_falls_back_to_placeholder() {
        let out = render_update_status("type=subdirectory\nbehind=1\nunsynced=false\n");
        assert!(out.contains("cd \"<repo>\" && git pull && make sync-to-home"));
    }

    #[test]
    fn clone_behind_still_advises_make_update() {
        let out = render_update_status("type=clone\nbehind=2\n");
        assert!(out.contains("2 commit(s) behind"));
        assert!(out.contains("make update"));
    }

    #[test]
    fn non_subdirectory_zero_behind_is_silent() {
        assert!(render_update_status("type=clone\nbehind=0\n").is_empty());
        assert!(render_update_status("").is_empty());
    }
}
