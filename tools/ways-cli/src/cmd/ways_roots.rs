//! Where a machine's ways live (ADR-143): the shipped corpus, the user's own
//! root, and each project's `.claude/ways/`. `ways corpus` embeds what this
//! finds and the settings screens list it, so the two never disagree about
//! which ways a project has.

use std::path::{Path, PathBuf};

/// A project's own ways: `<project>/.claude/ways/`, when it exists. The
/// project is the invocation's root; nothing above it is searched.
pub fn project_ways(project: &Path) -> Option<PathBuf> {
    Some(project.join(".claude/ways")).filter(|p| p.is_dir())
}

/// The ways of a project Claude Code knows, found from a session's working
/// directory: the nearest `.claude/ways/` at or above it, below the home
/// directory. A session may have started in a subdirectory.
pub fn session_project_ways(dir: &Path) -> Option<PathBuf> {
    let home = crate::util::home_dir();
    let mut at = dir.to_path_buf();
    while at != Path::new("/") && at != home {
        if let Some(ways) = project_ways(&at) {
            return Some(ways);
        }
        at = at.parent()?.to_path_buf();
    }
    None
}

/// Every project Claude Code has a transcript directory for, with its ways
/// directory, as `(project path, ways dir)`. `progress` hears each encoded
/// name before it is resolved: resolving probes the filesystem, and an
/// unreachable mount stalls there.
pub fn known_project_ways(progress: &dyn Fn(&str)) -> Vec<(String, PathBuf)> {
    let root = ways_core::paths::transcripts_root();
    let Ok(entries) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let encoded = entry.file_name().to_string_lossy().to_string();
        progress(&encoded);
        let Some(project) = claude_sessions::resolve_project_path(&root, &encoded) else { continue };
        if let Some(ways) = session_project_ways(Path::new(&project)) {
            out.push((project, ways));
        }
    }
    out
}
