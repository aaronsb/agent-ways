//! Project scoping for the replay — resolve which project(s) to replay and test
//! whether a stored event path belongs to the resolved scope.

use anyhow::{bail, Result};


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
    match crate::util::project_root() {
        Some(p) => Ok(Some(p)),
        None => bail!(
            "couldn't detect the current project: CLAUDE_PROJECT_DIR is unset and no \
             .claude/settings.json or CLAUDE.md was found above the working directory. \
             Pass --project <path> to scope to a project, or --all to replay across every project."
        ),
    }
}

/// Whether an event's stored `project` path belongs to `scope`: the project
/// itself or a path under it, such as an agent worktree
/// ([`ways_core::util::in_project`]). On a manual run where scope falls back
/// to `detect_project_dir` (a symlink-resolved cwd), a session whose stored
/// `project` was a logical or symlinked path won't match and is simply absent
/// from the list (not an error). Pass `--all` or an explicit `--project` to
/// see it.
pub(crate) fn project_matches(stored: &str, scope: &str) -> bool {
    ways_core::util::in_project(stored, scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_matches_the_project_and_what_is_under_it() {
        // Exact path (and trailing-slash normalization) matches.
        assert!(project_matches("/home/a/proj", "/home/a/proj"));
        assert!(project_matches("/home/a/proj/", "/home/a/proj"));
        assert!(project_matches("/home/a/proj", "/home/a/proj/"));
        // The bug the fix closes: sibling / prefixed projects must NOT match,
        // which the old `contains` substring test wrongly conflated.
        assert!(!project_matches("/home/a/proj-2", "/home/a/proj"));
        assert!(!project_matches("/home/a/proj", "proj"));
        assert!(!project_matches("/home/a/other", "/home/a/proj"));
        // An agent worktree under the project is part of it; the project is
        // not part of its worktree.
        assert!(project_matches("/home/a/proj/.claude/worktrees/agent-1", "/home/a/proj"));
        assert!(!project_matches("/home/a/proj", "/home/a/proj/.claude/worktrees/agent-1"));
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
