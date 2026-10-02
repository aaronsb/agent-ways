//! Locator for Claude Code's data on disk (ADR-504, Shared foundations).
//!
//! One copy of: the config directory (`~/.claude` or one given), its projects
//! and their transcripts, the project-path encoder ([`project_slug`]) and the
//! path a project directory was named from, transcript lookup by session id,
//! the name attend gives a project's tray ([`attend_key`]),
//! the session records under `sessions/`, and the usage and model a
//! transcript reports. Depends on the standard library and `serde_json` only,
//! so `attend` and the sensors use it without `ways-core`.
//!
//! This crate only reads. `ways projects cleanup` and `relocate` are the only
//! writers into the projects directory.

mod attend;
mod locate;
mod records;
mod slug;
pub mod usage;

pub use attend::{attend_key, attend_key_slug};
// transition read: removed by #701 (ADR-506)
pub use attend::{attend_tray_names, legacy_registry_name, legacy_tray_names};
pub use locate::{
    dir_belongs_to, find_project_dir_in, find_transcript_in, newest_transcript, prefix_candidates_in,
    project_dirs_in, resolve_project_path, transcripts_in,
};
pub use records::{parse_session_record, read_session_records, SessionRecord};
pub use slug::{project_slug, slug_matches, MAX_SLUG_LEN};

use std::path::{Path, PathBuf};

/// The user's home directory as the environment names it: `USERPROFILE`
/// first on Windows, else `HOME`. `None` when neither is set (an empty value
/// is unset), for a caller that must not invent one.
pub fn env_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(p) = std::env::var_os("USERPROFILE").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("HOME").filter(|p| !p.is_empty()).map(PathBuf::from)
}

/// The user's home directory: [`env_home_dir`], else `/tmp`.
pub fn home_dir() -> PathBuf {
    env_home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// `~/.claude.json`, Claude Code's per-user state file, which sits beside the
/// config directory rather than in it.
pub fn claude_json() -> PathBuf {
    home_dir().join(".claude.json")
}

/// A Claude Code config directory: `~/.claude`, or another one given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeDir {
    root: PathBuf,
}

impl ClaudeDir {
    /// The user's `~/.claude`.
    pub fn user() -> Self {
        Self::at(home_dir().join(".claude"))
    }

    /// The config directory at `root`.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `projects/`: one directory per project, holding its transcripts.
    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// `sessions/`: one record per running session.
    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    /// `history.jsonl`: every prompt typed, with its project.
    pub fn history_file(&self) -> PathBuf {
        self.root.join("history.jsonl")
    }

    /// Where `project`'s directory is named to be. It may not exist; see
    /// [`ClaudeDir::find_project_dir`] for one that does.
    pub fn project_dir(&self, project: &str) -> PathBuf {
        self.projects_dir().join(project_slug(project))
    }

    /// `project`'s directory, if it exists. See [`find_project_dir_in`].
    pub fn find_project_dir(&self, project: &str) -> Option<PathBuf> {
        find_project_dir_in(&self.projects_dir(), project)
    }

    /// A session's transcript. See [`find_transcript_in`].
    pub fn find_transcript(&self, project: Option<&str>, session_id: &str) -> Option<PathBuf> {
        find_transcript_in(&self.projects_dir(), project, session_id)
    }

    /// Every session record. See [`read_session_records`].
    pub fn session_records(&self) -> Vec<SessionRecord> {
        read_session_records(&self.sessions_dir())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    /// A temp directory removed on drop. Unique per process, tag and call.
    pub struct TempTree(PathBuf);

    impl TempTree {
        pub fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir()
                .join(format!("claude-sessions-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }

        pub fn path(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }

        pub fn dir(&self, rel: &str) {
            std::fs::create_dir_all(self.0.join(rel)).unwrap();
        }

        pub fn file(&self, rel: &str, content: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_under_a_given_root() {
        let c = ClaudeDir::at("/x/.claude");
        assert_eq!(c.projects_dir(), PathBuf::from("/x/.claude/projects"));
        assert_eq!(c.sessions_dir(), PathBuf::from("/x/.claude/sessions"));
        assert_eq!(c.history_file(), PathBuf::from("/x/.claude/history.jsonl"));
        assert_eq!(c.project_dir("/a/b_c"), PathBuf::from("/x/.claude/projects/-a-b-c"));
    }
}
