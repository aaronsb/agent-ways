use sensor_trait::{Focus, Sensor};
use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;

/// Watches git state in the working directory.
/// Tracks uncommitted changes, branch divergence, and remote updates.
/// Reports application-level deltas, not individual file churn.
pub struct GitSensor {
    /// Previous snapshot
    prior: Option<GitSnapshot>,
    /// First poll establishes baseline silently
    baseline_established: bool,
    /// Why the last poll could not read the repository.
    failure: Option<String>,
}

#[derive(Clone, Debug)]
struct GitSnapshot {
    /// Current branch name
    branch: String,
    /// Number of uncommitted changed files (staged + unstaged)
    dirty_count: u32,
    /// Set of changed file paths (for delta detection)
    dirty_files: HashSet<String>,
    /// Commits ahead of upstream
    ahead: u32,
    /// Commits behind upstream
    behind: u32,
    /// HEAD commit short hash
    head: String,
}

impl GitSensor {
    pub fn new() -> Self {
        Self {
            prior: None,
            baseline_established: false,
            failure: None,
        }
    }

    fn snapshot(&self, focus: &Focus) -> Option<GitSnapshot> {
        let dir = &focus.working_dir;
        if dir.is_empty() {
            return None;
        }

        // Check if this is a git repo
        let branch = git_cmd(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let head = git_cmd(dir, &["rev-parse", "--short", "HEAD"])?;

        // Uncommitted changes
        let dirty_files = match git_cmd(dir, &["status", "--porcelain"]) {
            Some(out) => dirty_paths(&out),
            None => HashSet::new(),
        };
        let dirty_count = dirty_files.len() as u32;

        // Branch divergence from upstream
        let (ahead, behind) = match git_cmd(dir, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) {
            Some(counts) => {
                let parts: Vec<&str> = counts.split_whitespace().collect();
                if parts.len() == 2 {
                    (
                        parts[0].parse().unwrap_or(0),
                        parts[1].parse().unwrap_or(0),
                    )
                } else {
                    (0, 0)
                }
            }
            None => (0, 0), // No upstream configured
        };

        Some(GitSnapshot {
            branch,
            dirty_count,
            dirty_files,
            ahead,
            behind,
            head,
        })
    }
}

impl Default for GitSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl Sensor for GitSensor {
    fn name(&self) -> &str {
        "git"
    }

    fn description(&self) -> &str {
        "tracks dirty files, branch changes, and upstream divergence"
    }

    fn source(&self) -> String {
        concat!("attend@", env!("CARGO_PKG_VERSION")).to_string()
    }

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let current = match self.snapshot(focus) {
            Some(s) => s,
            None => {
                // Not a repository, unless git itself cannot run.
                if let Err(e) = Command::new("git").arg("--version").output() {
                    self.failure = Some(format!("cannot run git: {e}"));
                }
                return Vec::new();
            }
        };

        // First poll: establish baseline
        if !self.baseline_established {
            eprintln!(
                "[attend] git: baseline — {} @ {} ({} dirty, +{}/−{} vs upstream)",
                current.branch, current.head, current.dirty_count,
                current.ahead, current.behind,
            );
            self.prior = Some(current);
            self.baseline_established = true;
            return Vec::new();
        }

        let prior = match &self.prior {
            Some(p) => p,
            None => {
                self.prior = Some(current);
                return Vec::new();
            }
        };

        let mut observations = Vec::new();

        // Branch changed
        if current.branch != prior.branch {
            observations.push((3.0, format!(
                "branch changed: {} → {}", prior.branch, current.branch
            )));
        }

        // HEAD moved (new commits)
        if current.head != prior.head && current.branch == prior.branch {
            observations.push((2.0, format!(
                "new commits on {} (HEAD {} → {})",
                current.branch, prior.head, current.head
            )));
        }

        // Dirty file count changed significantly
        if current.dirty_count != prior.dirty_count {
            let diff = current.dirty_count as i32 - prior.dirty_count as i32;
            if diff > 0 {
                // Find which files are new
                let new_files: Vec<&String> = current.dirty_files
                    .iter()
                    .filter(|f| !prior.dirty_files.contains(*f))
                    .collect();
                if !new_files.is_empty() {
                    let display: Vec<&str> = new_files.iter()
                        .take(3)
                        .map(|s| s.as_str())
                        .collect();
                    let suffix = if new_files.len() > 3 {
                        format!(" (+{} more)", new_files.len() - 3)
                    } else {
                        String::new()
                    };
                    observations.push((1.5, format!(
                        "{} new dirty files: {}{}",
                        new_files.len(), display.join(", "), suffix
                    )));
                }
            } else if current.dirty_count == 0 && prior.dirty_count > 0 {
                observations.push((1.0, "working tree clean (changes committed or stashed)".to_string()));
            }
        }

        // Remote divergence changed
        if current.behind > prior.behind {
            let new_behind = current.behind - prior.behind;
            observations.push((2.5, format!(
                "{} new commits on upstream (now {} behind)",
                new_behind, current.behind
            )));
        }

        if current.ahead != prior.ahead && current.ahead > 0 {
            observations.push((1.0, format!(
                "{} commits ahead of upstream (unpushed)",
                current.ahead
            )));
        }

        self.prior = Some(current);
        observations
    }

    fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }

    fn emission_threshold(&self) -> f64 {
        2.0 // Need meaningful git state change before disclosing
    }

    fn base_interval(&self) -> Duration {
        Duration::from_secs(30) // Check every 30s at rest
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(10) // Don't poll git faster than 10s
    }

    fn decay_threshold(&self) -> u32 {
        4 // 4 quiet ticks before decaying
    }
}

/// The paths in `git status --porcelain` output. Each line is a two-column
/// status, a space, then the path, so the path starts at the fourth byte
/// whatever the status: ` M x`, `M  x` and `?? x` all name `x`.
fn dirty_paths(porcelain: &str) -> HashSet<String> {
    porcelain
        .lines()
        .filter_map(|line| line.get(3..))
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

/// Run a git command and return its stdout without the trailing newline,
/// or None on failure or empty output. The start is kept as printed:
/// porcelain status opens with a space for a change only in the work tree.
fn git_cmd(dir: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim_end().to_string();
    if stdout.trim_start().is_empty() {
        None
    } else {
        Some(stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn every_porcelain_status_shape_keeps_the_whole_path() {
        let out = " M src/app/mod.rs\nM  staged.rs\n?? new.rs\nMM both.rs";
        assert_eq!(dirty_paths(out), set(&["src/app/mod.rs", "staged.rs", "new.rs", "both.rs"]));
    }

    /// The first line of a real status keeps its leading space through
    /// `git_cmd`, so a file changed only in the work tree loses no letter.
    #[test]
    fn a_work_tree_change_on_the_first_line_keeps_its_first_letter() {
        let dir = std::env::temp_dir().join(format!("attend-git-sensor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_str().unwrap();
        let git = |args: &[&str]| {
            let ok = Command::new("git")
                .args(["-C", d, "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        std::fs::write(dir.join("tools.rs"), "a").unwrap();
        git(&["add", "tools.rs"]);
        git(&["commit", "-qm", "init"]);
        std::fs::write(dir.join("tools.rs"), "b").unwrap();
        let out = git_cmd(d, &["status", "--porcelain"]).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(out, " M tools.rs");
        assert_eq!(dirty_paths(&out), set(&["tools.rs"]));
    }
}
