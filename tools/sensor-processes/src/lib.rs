use sensor_trait::{Focus, Sensor};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};

/// Exit status for the most recently completed build command, as reported
/// by a user wrapper writing `$XDG_STATE_HOME/attend/last-build-status`.
///
/// The sensor only observes `ps` diffs, so it can't read a process's exit
/// code directly — by the time we notice an exit, the process is already
/// gone. A small out-of-band marker file lets users opt in by wrapping
/// their build command; the sensor correlates an exit detection with the
/// most recent marker by command name and timestamp and enriches the
/// event with success/failure context.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildMarker {
    cmd: String,
    code: i32,
    ts: u64,
}

/// Correlate an exit event for `cmd` with the most recent marker. Returns
/// true when the marker is for the same command and its timestamp is
/// within `window_secs` of `now`. A stale or mismatched marker is silently
/// ignored — it represents some earlier build the sensor already handled.
fn marker_matches(marker: &BuildMarker, cmd: &str, now: u64, window_secs: u64) -> bool {
    marker.cmd == cmd && now.saturating_sub(marker.ts) <= window_secs
}

/// Parse a single-line marker file. Format: `cmd|code|unix_ts`. Any other
/// shape (extra fields, missing fields, non-numeric code) parses to None
/// — we'd rather silently ignore a malformed marker than surface a fake
/// status to the agent.
fn parse_marker(line: &str) -> Option<BuildMarker> {
    let line = line.trim();
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 3 {
        return None;
    }
    let cmd = parts[0].trim();
    if cmd.is_empty() {
        return None;
    }
    let code: i32 = parts[1].trim().parse().ok()?;
    let ts: u64 = parts[2].trim().parse().ok()?;
    Some(BuildMarker { cmd: cmd.to_string(), code, ts })
}

/// Location of the build-status marker. Honors `$XDG_STATE_HOME` first,
/// then falls back to `~/.local/state/attend/last-build-status` per the
/// XDG Base Directory Specification.
fn build_marker_path() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("attend").join("last-build-status")
}

/// Watches user session processes. Detects new/exited processes and
/// activity changes. Filters through focus to determine relevance.
pub struct ProcessSensor {
    /// Previous snapshot: app name → instance count
    prior: HashMap<String, u32>,
    /// First poll establishes baseline silently
    baseline_established: bool,
    /// Most recently parsed build-status marker (opt-in, from a user
    /// wrapper around their build command). None when no marker file
    /// exists or parsing failed.
    last_marker: Option<BuildMarker>,
    /// Marker file mtime on the previous read — lets us skip re-parsing
    /// when the file hasn't changed between polls.
    last_marker_mtime: Option<SystemTime>,
}

impl ProcessSensor {
    pub fn new() -> Self {
        Self {
            prior: HashMap::new(),
            baseline_established: false,
            last_marker: None,
            last_marker_mtime: None,
        }
    }

    /// Refresh `self.last_marker` from the marker file if it has been
    /// touched since the last poll. Stale or missing files leave the
    /// previously parsed marker in place — the correlation window in
    /// `marker_matches` handles expiry.
    fn refresh_marker(&mut self) {
        let path = build_marker_path();
        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => return,
        };
        let mtime = match metadata.modified() {
            Ok(t) => t,
            Err(_) => return,
        };
        if self.last_marker_mtime == Some(mtime) {
            return;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Some(marker) = parse_marker(&content) {
                self.last_marker = Some(marker);
                self.last_marker_mtime = Some(mtime);
            }
        }
    }

    /// Snapshot the user's processes as app-name → instance-count.
    /// Tracks applications, not PIDs. Chrome going from 47 to 49
    /// renderer processes is not a state change. Chrome appearing
    /// or disappearing entirely is.
    fn snapshot(&self, focus: &Focus) -> HashMap<String, u32> {
        let mut apps: HashMap<String, u32> = HashMap::new();

        let output = Command::new("ps")
            .args(["--no-headers", "-u", &whoami(), "-o", "comm"])
            .output();

        let output = match output {
            Ok(o) => o,
            Err(_) => return apps,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let comm = line.trim().to_string();
            if !comm.is_empty() && Self::is_relevant(&comm, focus) {
                *apps.entry(comm).or_insert(0) += 1;
            }
        }

        apps
    }

    fn is_relevant(comm: &str, focus: &Focus) -> bool {
        // Filter out ambient noise: shells, coreutils, attend itself
        let noise = ["ps", "bash", "zsh", "sh", "fish", "attend",
                      "grep", "awk", "sed", "cat", "sleep", "tail", "head",
                      "wc", "sort", "uniq", "cut", "tr", "tee", "xargs",
                      "less", "more", "find", "ls", "cp", "mv", "rm",
                      "mkdir", "chmod", "chown", "date", "env",
                      "dbus-daemon", "dbus-broker", "systemd", "pipewire",
                      "pulseaudio", "wireplumber", "xdg-dbus-proxy",
                      "at-spi-bus-laun", "at-spi2-registr",
                      "gjs", "gsd-*", "gnome-*", "xdg-*"];
        if noise.iter().any(|n| {
            if let Some(prefix) = n.strip_suffix('*') {
                comm.starts_with(prefix)
            } else {
                comm == *n
            }
        }) {
            return false;
        }

        // Focus keywords boost relevance
        if focus.keywords.iter().any(|kw| comm.contains(kw)) {
            return true;
        }

        // Interesting application-level processes
        let interesting = [
            // Build tools
            "cargo", "rustc", "gcc", "g++", "make", "cmake", "ninja",
            // Runtimes (but not their subprocess churn)
            "node", "npm", "npx", "python", "python3", "ruby", "java", "go",
            // Editors
            "code", "nvim", "vim", "emacs", "helix",
            // Claude
            "claude",
            // Servers
            "postgres", "mysql", "redis-server", "nginx", "httpd",
            "docker", "podman", "containerd",
            // Browsers (top-level only — we track presence, not subprocess count)
            "firefox", "chromium", "chrome",
            // Network
            "ssh", "scp", "rsync", "curl", "wget",
            // Version control
            "git",
        ];
        interesting.contains(&comm)
    }
}

impl Default for ProcessSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl Sensor for ProcessSensor {
    fn name(&self) -> &str {
        "processes"
    }

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let current = self.snapshot(focus);
        self.refresh_marker();

        // First poll: establish baseline silently
        if !self.baseline_established {
            eprintln!("[attend] processes: baseline established ({} apps: {})",
                current.len(),
                current.keys().cloned().collect::<Vec<_>>().join(", "));
            self.prior = current;
            self.baseline_established = true;
            return Vec::new();
        }

        let mut observations = Vec::new();

        // New applications (not in prior at all)
        for app in current.keys() {
            if !self.prior.contains_key(app) {
                observations.push((2.0, format!("{app} started")));
            }
        }

        // Exited applications (were in prior, gone now)
        // Build tools: exact match on process name (comm field from /proc)
        let build_tools = [
            "cargo", "rustc", "make", "cmake", "ninja",
            "gcc", "g++", "cc", "c++", "clang", "clang++",
            "go", "npm", "yarn", "pnpm", "tsc",
            "mvn", "gradle", "pip", "pip3",
        ];
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Correlation window: a marker must be within this many seconds
        // of the exit detection to count. 60s covers the 30s default poll
        // interval plus slack for slow markers.
        const MARKER_WINDOW_SECS: u64 = 60;

        for app in self.prior.keys() {
            if !current.contains_key(app) {
                let is_build = build_tools.contains(&app.as_str());
                if is_build {
                    // Opt-in enrichment: if the user wrapped their build
                    // command and wrote a recent marker, we know the exit
                    // code. Failures get a louder magnitude so they break
                    // through refractory.
                    let status = self.last_marker.as_ref()
                        .filter(|m| marker_matches(m, app, now, MARKER_WINDOW_SECS));
                    let (magnitude, text) = match status {
                        Some(m) if m.code == 0 => (
                            2.5,
                            format!("{app} exited (success). Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps"),
                        ),
                        Some(m) => (
                            3.5,
                            format!("{app} exited (failure, code {}). Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps", m.code),
                        ),
                        None => (
                            2.5,
                            format!("{app} exited. Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps"),
                        ),
                    };
                    observations.push((magnitude, text));
                } else {
                    observations.push((2.0, format!("{app} exited")));
                }
            }
        }

        self.prior = current;
        observations
    }

    fn emission_threshold(&self) -> f64 {
        2.0  // Need at least 2 process events before disclosing
    }

    fn base_interval(&self) -> Duration {
        Duration::from_secs(30)  // Check every 30s at rest
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(5)   // Ramp up to 5s during activity
    }

    fn decay_threshold(&self) -> u32 {
        5  // 5 quiet ticks before decaying back
    }
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "nobody".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_marker_happy_path_success() {
        let m = parse_marker("cargo|0|1712983456").unwrap();
        assert_eq!(m.cmd, "cargo");
        assert_eq!(m.code, 0);
        assert_eq!(m.ts, 1712983456);
    }

    #[test]
    fn parse_marker_happy_path_failure() {
        let m = parse_marker("cargo|101|1712983456").unwrap();
        assert_eq!(m.code, 101);
    }

    #[test]
    fn parse_marker_trims_whitespace_and_trailing_newline() {
        let m = parse_marker("  cargo | 0 | 1712983456  \n").unwrap();
        assert_eq!(m.cmd, "cargo");
        assert_eq!(m.code, 0);
        assert_eq!(m.ts, 1712983456);
    }

    #[test]
    fn parse_marker_rejects_wrong_field_count() {
        assert!(parse_marker("cargo|0").is_none());
        assert!(parse_marker("cargo|0|1|extra").is_none());
        assert!(parse_marker("").is_none());
    }

    #[test]
    fn parse_marker_rejects_empty_cmd() {
        assert!(parse_marker("|0|1712983456").is_none());
    }

    #[test]
    fn parse_marker_rejects_non_numeric_code_or_ts() {
        assert!(parse_marker("cargo|oops|1712983456").is_none());
        assert!(parse_marker("cargo|0|yesterday").is_none());
    }

    #[test]
    fn parse_marker_accepts_negative_exit_code() {
        // POSIX exit codes are 0-255 but signal-killed processes use
        // 128+signo; the marker writer might legitimately pass a negative
        // number if they're forwarding a raw status. Don't reject it.
        let m = parse_marker("cargo|-1|1712983456").unwrap();
        assert_eq!(m.code, -1);
    }

    #[test]
    fn marker_matches_fresh_same_cmd() {
        let m = BuildMarker { cmd: "cargo".into(), code: 0, ts: 1000 };
        assert!(marker_matches(&m, "cargo", 1030, 60));
    }

    #[test]
    fn marker_matches_at_window_boundary() {
        let m = BuildMarker { cmd: "cargo".into(), code: 0, ts: 1000 };
        assert!(marker_matches(&m, "cargo", 1060, 60));
        assert!(!marker_matches(&m, "cargo", 1061, 60));
    }

    #[test]
    fn marker_matches_rejects_different_cmd() {
        let m = BuildMarker { cmd: "cargo".into(), code: 0, ts: 1000 };
        assert!(!marker_matches(&m, "rustc", 1030, 60));
    }

    #[test]
    fn marker_matches_rejects_stale() {
        let m = BuildMarker { cmd: "cargo".into(), code: 0, ts: 1000 };
        assert!(!marker_matches(&m, "cargo", 2000, 60));
    }

    #[test]
    fn marker_matches_handles_clock_skew_saturation() {
        // If the marker somehow reports a ts in the future (clock skew,
        // reordered writes), we shouldn't panic via integer underflow.
        // saturating_sub clamps to 0, which falls inside any window.
        let m = BuildMarker { cmd: "cargo".into(), code: 0, ts: 2000 };
        assert!(marker_matches(&m, "cargo", 1000, 60));
    }
}

