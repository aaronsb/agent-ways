use sensor_trait::{Focus, Sensor};
use std::collections::{HashMap, HashSet};
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

/// Default watch list — process names whose exits get enriched build-event
/// treatment (success/failure magnitude tiers, marker correlation). Covers
/// the common build and package tools out of the box; users working in
/// other toolchains (elixir, zig, custom scripts) override via
/// `sensors.processes.watch:` in attend config.
pub const DEFAULT_WATCH: &[&str] = &[
    "cargo", "rustc", "make", "cmake", "ninja",
    "gcc", "g++", "cc", "c++", "clang", "clang++",
    "go", "npm", "yarn", "pnpm", "tsc",
    "mvn", "gradle", "pip", "pip3",
];

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
/// XDG Base Directory Specification. Returns `None` when neither env
/// var is set — we refuse a world-writable `/tmp` fallback rather than
/// risk reading a spoofed marker on a multi-user machine.
fn build_marker_path() -> Option<PathBuf> {
    let base = if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/state")
    } else {
        return None;
    };
    Some(base.join("attend").join("last-build-status"))
}

/// Format a build-tool exit event, optionally enriched with success/
/// failure context from a marker. Pure function — the sensor's
/// `poll()` delegates here so the enrichment arms get direct unit
/// test coverage without needing a real filesystem.
///
/// The success case sits at 2.5 rather than the issue spec's 2.0 on
/// purpose: that's the same magnitude the sensor emits when no marker
/// is present, so opting into the wrapper never *lowers* an exit's
/// visibility. Failures stay at 3.5 to break through refractory gating.
fn format_build_exit(app: &str, marker: Option<&BuildMarker>, now: u64) -> (f64, String) {
    const MARKER_WINDOW_SECS: u64 = 60;
    let matched = marker.filter(|m| marker_matches(m, app, now, MARKER_WINDOW_SECS));
    match matched {
        Some(m) if m.code == 0 => (
            2.5,
            format!("{app} finished (success). Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps"),
        ),
        Some(m) => (
            3.5,
            format!("{app} finished (failure, code {}). Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps", m.code),
        ),
        None => (
            2.5,
            format!("{app} finished. Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps"),
        ),
    }
}

/// One row of `ps`: a process, its parent, and its command name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Proc {
    pid: u32,
    ppid: u32,
    comm: String,
}

/// Parse `ps -o pid=,ppid=,comm=` output. Rows that do not parse are
/// skipped. The command name is the rest of the row, so a name with a
/// space survives.
fn parse_ps(stdout: &str) -> Vec<Proc> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let comm = parts.collect::<Vec<_>>().join(" ");
            (!comm.is_empty()).then_some(Proc { pid, ppid, comm })
        })
        .collect()
}

/// How far up the parent chain [`tally`] looks for a watched tool. A build
/// tree is a few levels deep (cargo → rustc, make → cc → cc1); the cap
/// bounds the walk per process (ADR-137) and stops a pid cycle.
const MAX_ANCESTRY: usize = 16;

/// Count relevant processes by name, leaving out each one that runs under
/// a watched build tool. `rustc` under `cargo` or `cc` under `make` is part
/// of the parent's build, not an application of its own (ADR-113: the
/// sensor tracks application presence, not PID churn); the parent's exit
/// carries the build event. A watched tool run on its own still counts.
fn tally(procs: &[Proc], is_relevant: impl Fn(&str) -> bool, is_watched: impl Fn(&str) -> bool) -> HashMap<String, u32> {
    let by_pid: HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let under_watched = |p: &Proc| {
        let mut ppid = p.ppid;
        for _ in 0..MAX_ANCESTRY {
            let Some(parent) = by_pid.get(&ppid) else { return false };
            if is_watched(&parent.comm) {
                return true;
            }
            ppid = parent.ppid;
        }
        false
    };
    let mut apps: HashMap<String, u32> = HashMap::new();
    for p in procs {
        if is_relevant(&p.comm) && !under_watched(p) {
            *apps.entry(p.comm.clone()).or_insert(0) += 1;
        }
    }
    apps
}

/// Watches user session processes. Detects new/exited processes and
/// activity changes. Filters through focus to determine relevance.
pub struct ProcessSensor {
    /// Previous snapshot: app name → instance count
    prior: HashMap<String, u32>,
    /// Apps that appeared on the last poll and have not been announced.
    /// A start is announced only when the app is still there one poll
    /// later; an app gone by then was a short-lived run, and its start and
    /// exit both stay silent.
    pending_start: HashSet<String>,
    /// First poll establishes baseline silently
    baseline_established: bool,
    /// Most recently parsed build-status marker (opt-in, from a user
    /// wrapper around their build command). None when no marker file
    /// exists or parsing failed.
    last_marker: Option<BuildMarker>,
    /// Marker file mtime on the previous read — lets us skip re-parsing
    /// when the file hasn't changed between polls.
    last_marker_mtime: Option<SystemTime>,
    /// Exact process names whose exits get enriched build-event treatment.
    /// Resolved once at construction: either from the user's config or
    /// from `DEFAULT_WATCH`. Not merged — explicit-replace is the
    /// documented contract.
    watch: Vec<String>,
}

impl ProcessSensor {
    /// Build with the default watch list (`DEFAULT_WATCH`).
    pub fn new() -> Self {
        Self::with_watch(DEFAULT_WATCH.iter().map(|s| s.to_string()).collect())
    }

    /// Build with an explicit watch list. Passing an empty list disables
    /// build-event enrichment entirely — all process exits fall through
    /// to the plain `X exited` path.
    pub fn with_watch(watch: Vec<String>) -> Self {
        Self {
            prior: HashMap::new(),
            pending_start: HashSet::new(),
            baseline_established: false,
            last_marker: None,
            last_marker_mtime: None,
            watch,
        }
    }

    /// Is `name` one of the processes we enrich on exit?
    fn is_watched(&self, name: &str) -> bool {
        self.watch.iter().any(|w| w == name)
    }

    /// Refresh `self.last_marker` from the marker file if it has been
    /// touched since the last poll. Stale or missing files leave the
    /// previously parsed marker in place — the correlation window in
    /// `marker_matches` handles expiry.
    fn refresh_marker(&mut self) {
        let path = match build_marker_path() {
            Some(p) => p,
            None => return,
        };
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
        // Record the mtime unconditionally so a persistently malformed
        // marker isn't re-parsed on every tick — the gate is "has this
        // file changed since we last looked," not "has it parsed."
        self.last_marker_mtime = Some(mtime);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Some(marker) = parse_marker(&content) {
                self.last_marker = Some(marker);
            }
        }
    }

    /// Snapshot the user's processes as app-name → instance-count.
    /// Tracks applications, not PIDs. Chrome going from 47 to 49
    /// renderer processes is not a state change. Chrome appearing
    /// or disappearing entirely is. Children of a watched build tool are
    /// left out (see [`tally`]).
    fn snapshot(&self, focus: &Focus) -> HashMap<String, u32> {
        let Ok(output) = Command::new("ps").args(["-u", &whoami(), "-o", "pid=,ppid=,comm="]).output() else {
            return HashMap::new();
        };
        let procs = parse_ps(&String::from_utf8_lossy(&output.stdout));
        tally(&procs, |c| Self::is_relevant(c, focus), |c| self.is_watched(c))
    }

    /// The events between the previous snapshot and `current`, given the
    /// current marker and time. A watched build tool never announces its
    /// start; its exit is the one build event. Any other app announces its
    /// start once it has lasted a poll, and an app that came and went
    /// within one poll produces nothing.
    fn diff(&mut self, current: HashMap<String, u32>, now: u64) -> Vec<(f64, String)> {
        let mut observations = Vec::new();
        for app in current.keys() {
            if self.is_watched(app) || (self.prior.contains_key(app) && !self.pending_start.contains(app)) {
                continue;
            }
            if self.pending_start.remove(app) {
                observations.push((2.0, format!("{app} started")));
            } else {
                self.pending_start.insert(app.clone());
            }
        }
        for app in self.prior.keys() {
            if current.contains_key(app) {
                continue;
            }
            if self.is_watched(app) {
                observations.push(format_build_exit(app, self.last_marker.as_ref(), now));
            } else if !self.pending_start.remove(app) {
                observations.push((2.0, format!("{app} exited")));
            }
        }
        self.prior = current;
        observations
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

    sensor_trait::sensor_metadata!();

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

        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.diff(current, now)
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

    // ── format_build_exit ──────────────────────────────────────────────
    //
    // Covers the three arms of the enrichment table directly, so the
    // fallback path (no marker) is tested by something other than the
    // filesystem-touching refresh_marker.

    #[test]
    fn format_build_exit_no_marker_falls_back_to_legacy_text() {
        let (mag, text) = format_build_exit("cargo", None, 1000);
        assert_eq!(mag, 2.5);
        assert!(text.starts_with("cargo finished. "));
        assert!(!text.contains("success"));
        assert!(!text.contains("failure"));
    }

    #[test]
    fn format_build_exit_marker_mismatch_falls_back_to_legacy_text() {
        // Stale marker (outside the 60s window) must not enrich — the
        // filter inside format_build_exit is the only gate.
        let m = BuildMarker { cmd: "cargo".into(), code: 0, ts: 100 };
        let (mag, text) = format_build_exit("cargo", Some(&m), 1000);
        assert_eq!(mag, 2.5);
        assert!(!text.contains("success"));
    }

    #[test]
    fn format_build_exit_marker_wrong_cmd_falls_back_to_legacy_text() {
        // Marker for a different command must not tag this exit.
        let m = BuildMarker { cmd: "rustc".into(), code: 0, ts: 1000 };
        let (mag, text) = format_build_exit("cargo", Some(&m), 1010);
        assert_eq!(mag, 2.5);
        assert!(!text.contains("success"));
    }

    #[test]
    fn format_build_exit_matching_success_marker() {
        let m = BuildMarker { cmd: "cargo".into(), code: 0, ts: 1000 };
        let (mag, text) = format_build_exit("cargo", Some(&m), 1010);
        assert_eq!(mag, 2.5);
        assert!(text.contains("(success)"));
    }

    #[test]
    fn format_build_exit_matching_failure_marker_is_louder() {
        let m = BuildMarker { cmd: "cargo".into(), code: 101, ts: 1000 };
        let (mag, text) = format_build_exit("cargo", Some(&m), 1010);
        assert_eq!(mag, 3.5);
        assert!(text.contains("(failure, code 101)"));
    }

    #[test]
    fn format_build_exit_non_zero_even_if_tiny_is_failure() {
        // Any non-zero code is a failure — don't assume 1-255.
        let m = BuildMarker { cmd: "cargo".into(), code: 1, ts: 1000 };
        let (mag, _) = format_build_exit("cargo", Some(&m), 1010);
        assert_eq!(mag, 3.5);
    }

    // ── watch list ─────────────────────────────────────────────────────

    #[test]
    fn new_uses_default_watch_list() {
        let s = ProcessSensor::new();
        // Spot-check the defaults that appear in docs and old hardcoded list.
        assert!(s.is_watched("cargo"));
        assert!(s.is_watched("rustc"));
        assert!(s.is_watched("go"));
        assert!(s.is_watched("npm"));
        // And something that's *not* in the default list — a Rust dev
        // shouldn't accidentally start getting enriched exits for python.
        assert!(!s.is_watched("python"));
        assert!(!s.is_watched("mix"));
    }

    #[test]
    fn with_watch_replaces_defaults() {
        // Explicit-replace contract: passing `watch:` drops the defaults.
        // An elixir dev who lists only `mix` should NOT still see cargo
        // enriched — their config is the single source of truth.
        let s = ProcessSensor::with_watch(vec!["mix".into(), "zig".into()]);
        assert!(s.is_watched("mix"));
        assert!(s.is_watched("zig"));
        assert!(!s.is_watched("cargo"));
        assert!(!s.is_watched("rustc"));
    }

    #[test]
    fn with_watch_empty_disables_enrichment() {
        // An empty watch list is a documented way to turn off build-event
        // enrichment entirely — every process exit becomes plain 2.0.
        let s = ProcessSensor::with_watch(Vec::new());
        assert!(!s.is_watched("cargo"));
        assert!(!s.is_watched("anything"));
    }

    #[test]
    fn default_watch_const_matches_expected_members() {
        // Locks in the set so we notice at review time if someone silently
        // adds or removes a default. New defaults are fine; this just
        // forces the discussion.
        assert_eq!(DEFAULT_WATCH.len(), 20);
        assert!(DEFAULT_WATCH.contains(&"cargo"));
        assert!(DEFAULT_WATCH.contains(&"pip3"));
    }

    // ── child churn and start/exit pairs ───────────────────────────────

    fn procs(rows: &[(u32, u32, &str)]) -> Vec<Proc> {
        rows.iter().map(|&(pid, ppid, comm)| Proc { pid, ppid, comm: comm.into() }).collect()
    }

    fn apps(names: &[&str]) -> HashMap<String, u32> {
        names.iter().map(|n| (n.to_string(), 1)).collect()
    }

    fn texts(obs: &[(f64, String)]) -> Vec<String> {
        let mut t: Vec<String> = obs.iter().map(|(_, s)| s.split('.').next().unwrap().to_string()).collect();
        t.sort();
        t
    }

    #[test]
    fn parse_ps_reads_pid_ppid_and_a_spaced_name() {
        let p = parse_ps("  10     1 cargo\n  11    10 rustc\n bad row\n  12 10 Web Content\n");
        assert_eq!(p, procs(&[(10, 1, "cargo"), (11, 10, "rustc"), (12, 10, "Web Content")]));
    }

    #[test]
    fn tally_drops_a_build_tools_children() {
        let s = ProcessSensor::new();
        // zsh → cargo → rustc ×2, and cc under make under make.
        let p = procs(&[
            (1, 0, "zsh"),
            (10, 1, "cargo"),
            (11, 10, "rustc"),
            (12, 10, "rustc"),
            (20, 1, "make"),
            (21, 20, "make"),
            (22, 21, "gcc"),
            (30, 1, "nvim"),
        ]);
        let t = tally(&p, |c| c != "zsh", |c| s.is_watched(c));
        assert_eq!(t, HashMap::from([("cargo".into(), 1), ("make".into(), 1), ("nvim".into(), 1)]));
    }

    #[test]
    fn tally_counts_a_watched_tool_run_on_its_own() {
        let s = ProcessSensor::new();
        let p = procs(&[(1, 0, "zsh"), (5, 1, "rustc")]);
        assert_eq!(tally(&p, |c| c != "zsh", |c| s.is_watched(c)), apps(&["rustc"]));
    }

    #[test]
    fn tally_survives_a_parent_cycle() {
        let p = procs(&[(5, 6, "git"), (6, 5, "ssh")]);
        assert_eq!(tally(&p, |_| true, |_| false).len(), 2);
    }

    #[test]
    fn a_build_emits_one_finished_event_and_no_start() {
        let mut s = ProcessSensor::new();
        s.prior = apps(&["nvim"]);
        assert!(s.diff(apps(&["nvim", "cargo"]), 0).is_empty(), "no start for a build tool");
        assert!(s.diff(apps(&["nvim", "cargo"]), 0).is_empty(), "nor a deferred one");
        assert_eq!(texts(&s.diff(apps(&["nvim"]), 0)), vec!["cargo finished"]);
    }

    #[test]
    fn a_short_lived_app_is_silent() {
        let mut s = ProcessSensor::new();
        s.prior = apps(&["nvim"]);
        assert!(s.diff(apps(&["nvim", "git"]), 0).is_empty());
        assert!(s.diff(apps(&["nvim"]), 0).is_empty(), "git came and went within a poll");
    }

    #[test]
    fn a_lasting_app_starts_one_poll_late_and_exits() {
        let mut s = ProcessSensor::new();
        s.prior = apps(&["nvim"]);
        assert!(s.diff(apps(&["nvim", "docker"]), 0).is_empty());
        assert_eq!(texts(&s.diff(apps(&["nvim", "docker"]), 0)), vec!["docker started"]);
        assert!(s.diff(apps(&["nvim", "docker"]), 0).is_empty());
        assert_eq!(texts(&s.diff(apps(&["docker"]), 0)), vec!["nvim exited"]);
        assert_eq!(texts(&s.diff(apps(&[]), 0)), vec!["docker exited"]);
    }
}

