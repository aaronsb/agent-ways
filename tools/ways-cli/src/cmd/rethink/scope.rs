//! Project scoping for the replay — resolve which project(s) to replay and test
//! whether a stored event path belongs to the resolved scope.

use anyhow::{bail, Result};

use crate::util::detect_project_dir;

/// Resolve which project(s) to replay. `Ok(None)` means *every* project
/// (`--all`); `Ok(Some(path))` scopes to one. Defaults to the current project
/// and — the correctness fix (ADR-154 §4) — **fails loud** instead of silently
/// globalizing when the current project can't be detected.
pub(crate) fn resolve_project_scope(project: Option<&str>, all: bool) -> Result<Option<String>> {
    if all {
        return Ok(None);
    }
    if let Some(p) = project {
        return Ok(Some(p.to_string()));
    }
    match std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .or_else(detect_project_dir)
    {
        Some(p) => Ok(Some(p)),
        None => bail!(
            "couldn't detect the current project: CLAUDE_PROJECT_DIR is unset and no \
             .claude/settings.json or CLAUDE.md was found above the working directory. \
             Pass --project <path> to scope to a project, or --all to replay across every project."
        ),
    }
}

/// Whether an event's stored `project` path belongs to `scope`, compared as
/// normalized absolute paths — replacing the old loose `contains` substring test
/// that let unrelated projects (`/a/foo` vs `/a/foo-bar`) bleed together.
///
/// The comparison is exact (modulo trailing slash). On the primary path this is
/// right: `CLAUDE_PROJECT_DIR` is the scope at both write and read time, so the
/// strings match. On a *manual* run where scope falls back to `detect_project_dir`
/// (a symlink-resolved cwd), a session whose stored `project` was a logical or
/// symlinked path — or a subdirectory `$PWD` — won't match and is simply absent
/// from the list (not an error). Pass `--all` or an explicit `--project` to see it.
pub(crate) fn project_matches(stored: &str, scope: &str) -> bool {
    normalize_project_path(stored) == normalize_project_path(scope)
}

fn normalize_project_path(p: &str) -> String {
    p.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_matches_is_exact_not_substring() {
        // Exact path (and trailing-slash normalization) matches.
        assert!(project_matches("/home/a/proj", "/home/a/proj"));
        assert!(project_matches("/home/a/proj/", "/home/a/proj"));
        assert!(project_matches("/home/a/proj", "/home/a/proj/"));
        // The bug the fix closes: sibling / prefixed projects must NOT match,
        // which the old `contains` substring test wrongly conflated.
        assert!(!project_matches("/home/a/proj-2", "/home/a/proj"));
        assert!(!project_matches("/home/a/proj", "proj"));
        assert!(!project_matches("/home/a/other", "/home/a/proj"));
    }

    #[test]
    fn scope_all_is_none_and_explicit_wins() {
        // `--all` → every project, regardless of env.
        assert_eq!(resolve_project_scope(None, true).unwrap(), None);
        assert_eq!(resolve_project_scope(Some("/x"), true).unwrap(), None);
        // Explicit `--project` is honored without touching detection.
        assert_eq!(
            resolve_project_scope(Some("/home/a/proj"), false).unwrap(),
            Some("/home/a/proj".to_string())
        );
    }
}
