//! Session metrics, git operations, and side-effectful display functions.

use std::path::Path;
use std::process::Command;

use crate::session;

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

    let bases = crate::paths::ways_roots(Some(Path::new(project_dir)));
    let mut total = 0u32;
    let mut fired = 0u32;
    // One count per sibling id, whatever number of roots carry a copy.
    for sib_id in sibling_ids(&bases, parent_path) {
        if session::resolve_way_file(&sib_id, project_dir).is_some() {
            total += 1;
            if session::way_is_shown(&sib_id, session_id) {
                fired += 1;
            }
        }
    }

    (total, fired)
}

/// The distinct sibling ids (`parent_path/<dir>`) found under `parent_path` in
/// any of `roots`.
fn sibling_ids(roots: &[std::path::PathBuf], parent_path: &str) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root.join(parent_path)) else { continue };
        for entry in entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            ids.insert(format!("{parent_path}/{}", entry.file_name().to_string_lossy()));
        }
    }
    ids
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

    // Only the native XDG projection (ADR-142) is nudged. The in-place clone,
    // ADR-140 subdirectory, fork and plugin layouts are legacy; an old cache
    // entry of those types renders nothing.
    if cached_type != "native" || behind == 0 {
        return String::new();
    }

    let repo = get("repo").unwrap_or_default();
    let repo_disp = if repo.is_empty() { "$XDG_DATA_HOME/agent-ways" } else { &repo };
    let mut out = String::from("\n");
    out.push_str(&format!("**⚠ agent-ways is {behind} commit(s) behind upstream.** Update the app source and reproject:\n"));
    out.push_str("`ways update`\n");
    out.push_str(&format!("`ways update` pulls `{repo_disp}`, refreshes the binaries, and reprojects `~/.claude`. A bare pull plus `make setup` skips existing binaries and leaves them stale.\n"));
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
    use super::{render_update_status, sibling_ids};

    #[test]
    fn a_sibling_in_two_roots_is_one_sibling() {
        let base = std::env::temp_dir().join(format!("ways-sibs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b) = (base.join("a"), base.join("b"));
        for d in [a.join("fx/shared/kid"), b.join("fx/shared/kid"), b.join("fx/shared/other")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let ids: Vec<String> = sibling_ids(&[a, b], "fx/shared").into_iter().collect();
        assert_eq!(ids, vec!["fx/shared/kid", "fx/shared/other"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn native_behind_advises_ways_update() {
        let out = render_update_status("type=native\nbehind=2\nrepo=/home/u/.local/share/agent-ways\n");
        assert!(out.contains("2 commit(s) behind"));
        assert!(out.contains("`ways update`"));
        assert!(!out.contains("make update"));
        assert!(!out.contains("sync-to-home"));
    }

    #[test]
    fn native_zero_behind_is_silent() {
        assert!(render_update_status("type=native\nbehind=0\nrepo=/x\n").is_empty());
        assert!(render_update_status("").is_empty());
    }

    #[test]
    fn legacy_topologies_are_silent() {
        for t in ["subdirectory", "clone", "fork", "renamed_clone", "plugin"] {
            let c = format!("type={t}\nbehind=3\nunsynced=true\nrepo=/x\n");
            assert!(render_update_status(&c).is_empty(), "{t} should render nothing");
        }
    }
}
