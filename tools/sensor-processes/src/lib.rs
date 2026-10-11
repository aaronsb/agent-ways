use sensor_trait::{Focus, Sensor};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Command, Output};
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
/// failure context from a marker and with the hosted apps that exited
/// with the tool. Pure function — the sensor's `poll()` delegates here so
/// the enrichment arms get direct unit test coverage without needing a
/// real filesystem.
///
/// The success case sits at 2.5 rather than the issue spec's 2.0 on
/// purpose: that's the same magnitude the sensor emits when no marker
/// is present, so opting into the wrapper never *lowers* an exit's
/// visibility. Failures stay at 3.5 to break through refractory gating.
fn format_build_exit(app: &str, marker: Option<&BuildMarker>, now: u64, hosted_exits: &[String]) -> (f64, String) {
    const MARKER_WINDOW_SECS: u64 = 60;
    let matched = marker.filter(|m| marker_matches(m, app, now, MARKER_WINDOW_SECS));
    let (mag, status) = match matched {
        Some(m) if m.code == 0 => (2.5, Some("success".to_string())),
        Some(m) => (3.5, Some(format!("failure, code {}", m.code))),
        None => (2.5, None),
    };
    let exits = (!hosted_exits.is_empty()).then(|| format!("{} exited", hosted_exits.join(", ")));
    let notes: Vec<String> = status.into_iter().chain(exits).collect();
    let notes = if notes.is_empty() { String::new() } else { format!(" ({})", notes.join("; ")) };
    (
        mag,
        format!("{app} finished{notes}. Use `ways show attend build-complete --session $CLAUDE_SESSION_ID` for next steps"),
    )
}

/// One row of `ps`: a process, its parent, its start time, and its
/// command name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Proc {
    pid: u32,
    ppid: u32,
    /// `lstart` as `ps` prints it, such as `Sat Oct 10 22:18:30 2026`.
    /// Only compared for equality, so a pid reused by a later process
    /// reads as a different process.
    start: String,
    comm: String,
}

/// The `ps` columns [`parse_ps`] reads. `lstart` is five words in the C
/// locale, which [`PS_ENV`] sets.
const PS_COLUMNS: &str = "pid=,ppid=,lstart=,comm=";
const PS_ENV: (&str, &str) = ("LC_ALL", "C");

/// Parse `ps -o pid=,ppid=,lstart=,comm=` output. Rows that do not parse
/// are skipped. The command name is the rest of the row, so a name with a
/// space survives.
fn parse_ps(stdout: &str) -> Vec<Proc> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let start: Vec<&str> = parts.by_ref().take(5).collect();
            let comm = parts.collect::<Vec<_>>().join(" ");
            (start.len() == 5 && !comm.is_empty()).then(|| Proc { pid, ppid, start: start.join(" "), comm })
        })
        .collect()
}

/// Apps the sensor tracks besides the watched tools: application-level
/// processes whose presence is worth an event.
const TRACKED: &[&str] = &[
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

/// The name a process is tracked under. A process can rename itself, and
/// some known tools do: npm sets its title to the command line, so its
/// `comm` reads `npm test` or `npm run dev`, and node names its main thread,
/// so its `comm` reads `node-MainThread`. The first word of `comm`, and
/// that word without a trailing `-<suffix>`, are tried after `comm` itself,
/// and the first of the three that `known` accepts is the name. A process
/// none of them matches keeps its `comm` as it is, so `Web Content` or
/// `slack-mcp-linux` is not cut down to a name it does not have.
fn canonical_name(comm: &str, known: impl Fn(&str) -> bool) -> String {
    let word = comm.split_whitespace().next().unwrap_or(comm);
    let base = word.rsplit_once('-').map_or(word, |(base, _)| base);
    [comm, word, base].into_iter().find(|n| known(n)).unwrap_or(comm).to_string()
}

/// The rows of a `ps` run, or `None` when it could not be read: `ps` did
/// not start, exited non-zero, or listed no process. The user running the
/// sensor always has at least the sensor itself, so zero rows is a failed
/// read, never an empty session.
fn ps_rows(output: std::io::Result<Output>) -> Option<Vec<Proc>> {
    let output = output.ok().filter(|o| o.status.success())?;
    let rows = parse_ps(&String::from_utf8_lossy(&output.stdout));
    (!rows.is_empty()).then_some(rows)
}

/// How far up the parent chain [`tally`] looks for a watched tool. A build
/// tree is a few levels deep (cargo → rustc, make → cc → cc1); the cap
/// bounds the walk per process (ADR-137) and stops a pid cycle.
const MAX_ANCESTRY: usize = 16;

/// A watched tool at the top of a build, such as one `npm test` run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tool {
    name: String,
    /// The tool's start time, so a pid reused by a same-name process is
    /// told apart from the run that held it.
    start: String,
    /// Apps (not themselves watched) running under this tool, such as
    /// `node` under `npm run dev` or `docker` under `make up`.
    hosts: HashSet<String>,
}

/// One poll's view of the session's processes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Snapshot {
    /// App name → instance count, for apps that are not watched tools.
    apps: HashMap<String, u32>,
    /// Watched tools not under another watched tool, by pid, so two
    /// concurrent `npm` runs are two tools.
    tools: HashMap<u32, Tool>,
}

/// An app's exit, with the pids of the watched tools it ran under.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HeldExit {
    app: String,
    hosts: Vec<u32>,
}

/// Sort relevant processes into apps and tools. A watched tool is always
/// relevant, whatever `is_relevant` says of it. A watched tool running
/// under another watched tool is left out: `rustc` under `cargo` or `cc`
/// under `make` is part of the parent's build, not an application of its
/// own (ADR-113: the sensor tracks application presence, not PID churn),
/// and the parent's exit carries the build event. Any other app stays
/// visible wherever it runs, so `node` under `npm` or `docker` under
/// `make` is counted, and recorded as hosted by the tool above it. Every
/// process is first given its [`canonical_name`], against the watched
/// tools and [`TRACKED`].
fn tally(procs: &[Proc], is_relevant: impl Fn(&str) -> bool, is_watched: impl Fn(&str) -> bool) -> Snapshot {
    let known = |n: &str| is_watched(n) || TRACKED.contains(&n);
    let procs: Vec<Proc> = procs.iter().map(|p| Proc { comm: canonical_name(&p.comm, known), ..p.clone() }).collect();
    let by_pid: HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let watched_above = |p: &Proc| {
        let mut tools = Vec::new();
        let mut ppid = p.ppid;
        for _ in 0..MAX_ANCESTRY {
            let Some(parent) = by_pid.get(&ppid) else { break };
            if is_watched(&parent.comm) {
                tools.push(parent.pid);
            }
            ppid = parent.ppid;
        }
        tools
    };
    let mut snap = Snapshot::default();
    let mut hosted: HashMap<u32, HashSet<String>> = HashMap::new();
    for p in procs.iter().filter(|p| is_watched(&p.comm) || is_relevant(&p.comm)) {
        let above = watched_above(p);
        if is_watched(&p.comm) {
            if above.is_empty() {
                snap.tools.insert(p.pid, Tool { name: p.comm.clone(), start: p.start.clone(), hosts: HashSet::new() });
            }
            continue;
        }
        *snap.apps.entry(p.comm.clone()).or_insert(0) += 1;
        for pid in above {
            hosted.entry(pid).or_default().insert(p.comm.clone());
        }
    }
    for (pid, apps) in hosted {
        if let Some(tool) = snap.tools.get_mut(&pid) {
            tool.hosts = apps;
        }
    }
    snap
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
    /// Previous snapshot's watched tools, by pid.
    prior_tools: HashMap<u32, Tool>,
    /// Apps that exited on the last poll while a tool that hosted them was
    /// still running. Each is held one poll, so a tool exiting right after
    /// names it in its own event; otherwise it is reported on its own.
    held_exits: Vec<HeldExit>,
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
            prior_tools: HashMap::new(),
            held_exits: Vec::new(),
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

    /// One poll over the process table `rows`: the first establishes the
    /// baseline silently, and each later one is diffed against the last.
    /// `None` means the table could not be read, and the poll is skipped
    /// with the prior state kept: an empty read diffed as a snapshot would
    /// report every app and tool as gone. Apps are tracked by name, not by
    /// pid, so Chrome going from 47 to 49 renderers is not a change, and a
    /// watched tool under another watched tool is left out (see [`tally`]).
    fn observe(&mut self, rows: Option<Vec<Proc>>, focus: &Focus, now: u64) -> Vec<(f64, String)> {
        let Some(rows) = rows else { return Vec::new() };
        let current = tally(&rows, |c| Self::is_relevant(c, focus), |c| self.is_watched(c));
        if !self.baseline_established {
            eprintln!("[attend] processes: baseline established ({} apps: {})",
                current.apps.len(),
                current.apps.keys().cloned().collect::<Vec<_>>().join(", "));
            self.prior = current.apps;
            self.prior_tools = current.tools;
            self.baseline_established = true;
            return Vec::new();
        }
        self.diff(current, now)
    }

    /// The events between the previous snapshot and `current`, given the
    /// current marker and time. A watched tool never announces its start,
    /// and its exit is always `<tool> finished`. A hosted app whose exit is
    /// seen in the same poll as its tool's, or one poll before, is named in
    /// that one event, as in `npm finished (node exited)`. Any other app
    /// announces its start once it has lasted a poll, and an app that came
    /// and went within one poll produces nothing.
    fn diff(&mut self, current: Snapshot, now: u64) -> Vec<(f64, String)> {
        let Snapshot { apps: current, tools } = current;
        let mut observations = Vec::new();
        for app in current.keys() {
            if self.prior.contains_key(app) && !self.pending_start.contains(app) {
                continue;
            }
            if self.pending_start.remove(app) {
                observations.push((2.0, format!("{app} started")));
            } else {
                self.pending_start.insert(app.clone());
            }
        }
        // App exits seen this poll, each with the pids of the tools it ran
        // under, after the ones held from the last poll.
        let held = std::mem::take(&mut self.held_exits);
        let held_count = held.len();
        let mut exits = held;
        let mut fresh: Vec<HeldExit> = Vec::new();
        for app in self.prior.keys() {
            if !current.contains_key(app) && !self.pending_start.remove(app) {
                let hosts = self.prior_tools.iter().filter(|(_, t)| t.hosts.contains(app)).map(|(pid, _)| *pid).collect();
                fresh.push(HeldExit { app: app.clone(), hosts });
            }
        }
        fresh.sort_by(|a, b| a.app.cmp(&b.app));
        exits.extend(fresh);
        let alive = |pid: &u32| {
            tools.get(pid).is_some_and(|t| self.prior_tools.get(pid).is_some_and(|p| (&p.name, &p.start) == (&t.name, &t.start)))
        };
        let mut finished: Vec<(&u32, &Tool)> = self.prior_tools.iter().filter(|(pid, _)| !alive(pid)).collect();
        finished.sort_by_key(|(pid, _)| **pid);
        let mut named = vec![false; exits.len()];
        for (pid, tool) in finished {
            let mut with: Vec<String> = Vec::new();
            for (i, exit) in exits.iter().enumerate() {
                if exit.hosts.contains(pid) {
                    named[i] = true;
                    if !with.contains(&exit.app) {
                        with.push(exit.app.clone());
                    }
                }
            }
            observations.push(format_build_exit(&tool.name, self.last_marker.as_ref(), now, &with));
        }
        for (i, exit) in exits.into_iter().enumerate() {
            if named[i] {
                continue;
            }
            if i >= held_count && exit.hosts.iter().any(alive) {
                self.held_exits.push(exit);
            } else {
                observations.push((2.0, format!("{} exited", exit.app)));
            }
        }
        self.prior = current;
        self.prior_tools = tools;
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

        TRACKED.contains(&comm)
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
        let rows = ps_rows(Command::new("ps").args(["-u", &whoami(), "-o", PS_COLUMNS]).env(PS_ENV.0, PS_ENV.1).output());
        self.refresh_marker();
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.observe(rows, focus, now)
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
        let (mag, text) = format_build_exit("cargo", None, 1000, &[]);
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
        let (mag, text) = format_build_exit("cargo", Some(&m), 1000, &[]);
        assert_eq!(mag, 2.5);
        assert!(!text.contains("success"));
    }

    #[test]
    fn format_build_exit_marker_wrong_cmd_falls_back_to_legacy_text() {
        // Marker for a different command must not tag this exit.
        let m = BuildMarker { cmd: "rustc".into(), code: 0, ts: 1000 };
        let (mag, text) = format_build_exit("cargo", Some(&m), 1010, &[]);
        assert_eq!(mag, 2.5);
        assert!(!text.contains("success"));
    }

    #[test]
    fn format_build_exit_matching_success_marker() {
        let m = BuildMarker { cmd: "cargo".into(), code: 0, ts: 1000 };
        let (mag, text) = format_build_exit("cargo", Some(&m), 1010, &[]);
        assert_eq!(mag, 2.5);
        assert!(text.contains("(success)"));
    }

    #[test]
    fn format_build_exit_matching_failure_marker_is_louder() {
        let m = BuildMarker { cmd: "cargo".into(), code: 101, ts: 1000 };
        let (mag, text) = format_build_exit("cargo", Some(&m), 1010, &[]);
        assert_eq!(mag, 3.5);
        assert!(text.contains("(failure, code 101)"));
    }

    #[test]
    fn format_build_exit_non_zero_even_if_tiny_is_failure() {
        // Any non-zero code is a failure — don't assume 1-255.
        let m = BuildMarker { cmd: "cargo".into(), code: 1, ts: 1000 };
        let (mag, _) = format_build_exit("cargo", Some(&m), 1010, &[]);
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

    /// The start time every test process has, unless a test says otherwise.
    const T0: &str = "Sat Oct 10 22:18:30 2026";

    fn procs(rows: &[(u32, u32, &str)]) -> Vec<Proc> {
        rows.iter().map(|&(pid, ppid, comm)| Proc { pid, ppid, start: T0.into(), comm: comm.into() }).collect()
    }

    fn apps(names: &[&str]) -> HashMap<String, u32> {
        names.iter().map(|n| (n.to_string(), 1)).collect()
    }

    fn snap(names: &[&str]) -> Snapshot {
        Snapshot { apps: apps(names), tools: HashMap::new() }
    }

    fn texts(obs: &[(f64, String)]) -> Vec<String> {
        let mut t: Vec<String> = obs.iter().map(|(_, s)| s.split('.').next().unwrap().to_string()).collect();
        t.sort();
        t
    }

    /// A poll of the process table `rows`, with shells left out as noise.
    fn see(rows: &[(u32, u32, &'static str)]) -> Snapshot {
        let s = ProcessSensor::new();
        tally(&procs(rows), |c| c != "zsh" && c != "sh", |c| s.is_watched(c))
    }

    /// A sensor whose baseline poll saw `rows`.
    fn sensor_after(rows: &[(u32, u32, &'static str)]) -> ProcessSensor {
        let mut s = ProcessSensor::new();
        let base = see(rows);
        s.prior = base.apps;
        s.prior_tools = base.tools;
        s
    }

    fn tool(name: &str, hosts: &[&str]) -> Tool {
        Tool { name: name.into(), start: T0.into(), hosts: hosts.iter().map(|h| h.to_string()).collect() }
    }

    #[test]
    fn parse_ps_reads_pid_ppid_and_a_spaced_name() {
        let p = parse_ps(&format!(
            "  10     1 {T0} cargo\n  11    10 {T0} rustc\n bad row\n  13 10 Sat Oct 10 rustc\n  12 10 {T0} Web Content\n"
        ));
        assert_eq!(p, procs(&[(10, 1, "cargo"), (11, 10, "rustc"), (12, 10, "Web Content")]));
    }

    #[test]
    fn tally_drops_a_build_tools_children() {
        // zsh → cargo → rustc ×2, and gcc under make under make.
        let t = see(&[
            (1, 0, "zsh"),
            (10, 1, "cargo"),
            (11, 10, "rustc"),
            (12, 10, "rustc"),
            (20, 1, "make"),
            (21, 20, "make"),
            (22, 21, "gcc"),
            (30, 1, "nvim"),
        ]);
        assert_eq!(t.apps, apps(&["nvim"]));
        assert_eq!(t.tools, HashMap::from([(10, tool("cargo", &[])), (20, tool("make", &[]))]));
    }

    #[test]
    fn tally_counts_a_watched_tool_run_on_its_own() {
        let t = see(&[(1, 0, "zsh"), (5, 1, "rustc")]);
        assert_eq!(t.tools, HashMap::from([(5, tool("rustc", &[]))]));
    }

    #[test]
    fn tally_survives_a_parent_cycle() {
        let p = procs(&[(5, 6, "git"), (6, 5, "ssh")]);
        assert_eq!(tally(&p, |_| true, |_| false).apps.len(), 2);
    }

    /// An app launched by a watched tool is still an app: `node` under
    /// `npm run dev` and `docker` under `make up` stay visible, and each
    /// tool records what it hosts.
    #[test]
    fn tally_keeps_an_app_under_a_watched_tool() {
        let t = see(&[(1, 0, "zsh"), (10, 1, "npm"), (11, 10, "sh"), (12, 11, "node"), (20, 1, "make"), (21, 20, "docker")]);
        assert_eq!(t.apps, apps(&["node", "docker"]));
        assert_eq!(t.tools, HashMap::from([(10, tool("npm", &["node"])), (20, tool("make", &["docker"]))]));
    }

    const NVIM: (u32, u32, &str) = (5, 1, "nvim");
    const ZSH: (u32, u32, &str) = (1, 0, "zsh");

    /// `npm test` running for 60 seconds, with node lasting several polls,
    /// still ends in `npm finished`.
    #[test]
    fn a_long_npm_test_still_finishes() {
        let mut s = sensor_after(&[ZSH, NVIM]);
        let running = [ZSH, NVIM, (10, 1, "npm"), (11, 10, "node")];
        assert!(s.diff(see(&running), 0).is_empty());
        assert_eq!(texts(&s.diff(see(&running), 0)), vec!["node started"]);
        for _ in 0..10 {
            assert!(s.diff(see(&running), 0).is_empty());
        }
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["npm finished (node exited)"]);
    }

    /// The same run with an unrelated `node` already in the baseline: the
    /// name never disappears, and npm's exit is still reported.
    #[test]
    fn a_long_npm_test_finishes_beside_an_unrelated_node() {
        let other = (50, 1, "node");
        let mut s = sensor_after(&[ZSH, NVIM, other]);
        let running = [ZSH, NVIM, other, (10, 1, "npm"), (11, 10, "node")];
        for _ in 0..6 {
            assert!(s.diff(see(&running), 0).is_empty());
        }
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM, other]), 0)), vec!["npm finished"]);
    }

    /// `npm run dev` and `npm test` side by side: each npm's exit is its
    /// own event, and only the run whose node exited names it.
    #[test]
    fn concurrent_npm_runs_finish_separately() {
        let dev = [(10, 1, "npm"), (11, 10, "node")];
        let test = [(20, 1, "npm"), (21, 20, "node")];
        let both: Vec<_> = [ZSH, NVIM].into_iter().chain(dev).chain(test).collect();
        let only_dev: Vec<_> = [ZSH, NVIM].into_iter().chain(dev).collect();
        let only_test: Vec<_> = [ZSH, NVIM].into_iter().chain(test).collect();

        // The test run ends first; the dev server keeps node present.
        let mut s = sensor_after(&[ZSH, NVIM]);
        for _ in 0..4 {
            s.diff(see(&both), 0);
        }
        assert_eq!(texts(&s.diff(see(&only_dev), 0)), vec!["npm finished"]);
        assert!(s.diff(see(&only_dev), 0).is_empty());
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["npm finished (node exited)"]);

        // The dev server ends first; the test run's node is still there.
        let mut s = sensor_after(&[ZSH, NVIM]);
        for _ in 0..4 {
            s.diff(see(&both), 0);
        }
        assert_eq!(texts(&s.diff(see(&only_test), 0)), vec!["npm finished"]);
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["npm finished (node exited)"]);
    }

    /// A server dying under `npm run dev` gives one event, whether npm
    /// exits in the same poll or lingers one poll.
    #[test]
    fn a_server_dying_under_npm_run_dev_is_one_event() {
        let running = [ZSH, NVIM, (10, 1, "npm"), (11, 10, "node")];
        let mut s = sensor_after(&[ZSH, NVIM]);
        for _ in 0..3 {
            s.diff(see(&running), 0);
        }
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["npm finished (node exited)"]);

        let mut s = sensor_after(&[ZSH, NVIM]);
        for _ in 0..3 {
            s.diff(see(&running), 0);
        }
        assert!(s.diff(see(&[ZSH, NVIM, (10, 1, "npm")]), 0).is_empty(), "node's exit waits a poll for npm");
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["npm finished (node exited)"]);
    }

    /// A tool that exits between polls, its pid taken by a new run of the
    /// same tool, still reports its finish: the start time differs.
    #[test]
    fn a_reused_pid_does_not_hide_a_finish() {
        let mut s = sensor_after(&[ZSH, NVIM]);
        let first = [ZSH, NVIM, (10, 1, "cargo")];
        assert!(s.diff(see(&first), 0).is_empty());
        assert!(s.diff(see(&first), 0).is_empty(), "the same run, still going");
        let mut second = procs(&first);
        second[2].start = "Sat Oct 10 22:19:02 2026".into();
        let second = tally(&second, |c| c != "zsh", |c| s.is_watched(c));
        assert_eq!(texts(&s.diff(second, 0)), vec!["cargo finished"]);
    }

    /// An app that exits while its tool keeps running is reported one poll
    /// later, on its own.
    #[test]
    fn an_app_exiting_under_a_running_tool_is_reported_a_poll_later() {
        let mut s = sensor_after(&[ZSH, NVIM]);
        let up = [ZSH, NVIM, (20, 1, "make"), (21, 20, "docker")];
        let make = [ZSH, NVIM, (20, 1, "make")];
        for _ in 0..3 {
            s.diff(see(&up), 0);
        }
        assert!(s.diff(see(&make), 0).is_empty());
        assert_eq!(texts(&s.diff(see(&make), 0)), vec!["docker exited"]);
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["make finished"]);
    }

    /// `npm test` whose node lasts less than a poll is still a build: its
    /// exit is `npm finished`, and node's start and exit stay silent.
    #[test]
    fn a_short_npm_run_still_finishes() {
        let mut s = sensor_after(&[ZSH, NVIM]);
        assert!(s.diff(see(&[ZSH, NVIM, (10, 1, "npm"), (11, 10, "node")]), 0).is_empty());
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["npm finished"]);
    }

    /// rustc processes coming and going under cargo produce nothing; the
    /// build is one `cargo finished`.
    #[test]
    fn rustc_under_cargo_stays_silent() {
        let mut s = sensor_after(&[ZSH, NVIM]);
        assert!(s.diff(see(&[ZSH, NVIM, (10, 1, "cargo")]), 0).is_empty(), "no start for a build tool");
        for pid in 11..16 {
            assert!(s.diff(see(&[ZSH, NVIM, (10, 1, "cargo"), (pid, 10, "rustc")]), 0).is_empty());
            assert!(s.diff(see(&[ZSH, NVIM, (10, 1, "cargo")]), 0).is_empty());
        }
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["cargo finished"]);
    }

    #[test]
    fn format_build_exit_names_hosted_exits_after_the_status() {
        let m = BuildMarker { cmd: "npm".into(), code: 1, ts: 1000 };
        let (mag, text) = format_build_exit("npm", Some(&m), 1010, &["node".into()]);
        assert_eq!(mag, 3.5);
        assert!(text.starts_with("npm finished (failure, code 1; node exited). "), "{text}");
    }

    #[test]
    fn a_short_lived_app_is_silent() {
        let mut s = ProcessSensor::new();
        s.prior = apps(&["nvim"]);
        assert!(s.diff(snap(&["nvim", "git"]), 0).is_empty());
        assert!(s.diff(snap(&["nvim"]), 0).is_empty(), "git came and went within a poll");
    }

    // ── a failed read ──────────────────────────────────────────────────

    fn ps_output(code: i32, stdout: &str) -> std::io::Result<Output> {
        use std::os::unix::process::ExitStatusExt;
        Ok(Output { status: std::process::ExitStatus::from_raw(code << 8), stdout: stdout.into(), stderr: Vec::new() })
    }

    #[test]
    fn ps_rows_is_none_for_a_failed_or_empty_read() {
        assert_eq!(ps_rows(ps_output(0, &format!("  10  1 {T0} cargo\n"))), Some(procs(&[(10, 1, "cargo")])));
        assert_eq!(ps_rows(ps_output(0, "")), None, "zero rows");
        assert_eq!(ps_rows(ps_output(0, "garbage\n")), None, "no row parsed");
        assert_eq!(ps_rows(ps_output(1, &format!("  10  1 {T0} cargo\n"))), None, "non-zero exit");
        assert_eq!(ps_rows(Err(std::io::Error::other("no ps"))), None, "did not start");
    }

    /// A poll whose `ps` read failed reports nothing, and the next good
    /// poll carries on from the state before it.
    #[test]
    fn a_failed_read_skips_the_poll() {
        let focus = no_focus();
        let idle = procs(&[ZSH, NVIM]);
        let running = procs(&[ZSH, NVIM, (10, 1, "npm"), (11, 10, "node")]);
        let mut s = ProcessSensor::new();
        assert!(s.observe(Some(idle), &focus, 0).is_empty(), "baseline");
        assert!(s.observe(Some(running.clone()), &focus, 0).is_empty());
        assert_eq!(texts(&s.observe(Some(running.clone()), &focus, 0)), vec!["node started"]);
        assert!(s.observe(None, &focus, 0).is_empty(), "the failed read");
        assert!(s.observe(Some(running), &focus, 0).is_empty(), "nothing restarted");
    }

    // ── names as the host reports them ─────────────────────────────────

    fn no_focus() -> Focus {
        Focus { description: String::new(), working_dir: String::new(), keywords: Vec::new() }
    }

    /// Every default watched tool is seen, though most (`yarn`, `tsc`,
    /// `clang`, ...) are not on the tracked list: `yarn build` running
    /// `tsc` is one yarn tool, and its exit reports.
    #[test]
    fn every_watched_tool_is_tracked() {
        let s = ProcessSensor::new();
        let focus = no_focus();
        let poll = |rows: &[(u32, u32, &'static str)]| {
            tally(&procs(rows), |c| ProcessSensor::is_relevant(c, &focus), |c| s.is_watched(c))
        };
        for (i, name) in DEFAULT_WATCH.iter().enumerate() {
            let pid = 100 + i as u32;
            assert_eq!(poll(&[ZSH, (pid, 1, name)]).tools, HashMap::from([(pid, tool(name, &[]))]), "{name}");
        }
        let build = [ZSH, NVIM, (10, 1, "yarn"), (11, 10, "node"), (12, 11, "tsc")];
        assert_eq!(poll(&build).tools, HashMap::from([(10, tool("yarn", &["node"]))]));
        let mut sensor = ProcessSensor::new();
        let base = poll(&[ZSH, NVIM]);
        sensor.prior = base.apps;
        sensor.prior_tools = base.tools;
        assert!(sensor.diff(poll(&build), 0).is_empty());
        assert_eq!(texts(&sensor.diff(poll(&[ZSH, NVIM]), 0)), vec!["yarn finished"]);
    }

    /// A user's own watch list is tracked the same way.
    #[test]
    fn a_configured_watched_tool_is_tracked() {
        let s = ProcessSensor::with_watch(vec!["mix".into()]);
        let focus = no_focus();
        let t = tally(&procs(&[ZSH, (10, 1, "mix")]), |c| ProcessSensor::is_relevant(c, &focus), |c| s.is_watched(c));
        assert_eq!(t.tools, HashMap::from([(10, tool("mix", &[]))]));
    }

    #[test]
    fn canonical_name_finds_the_tool_inside_a_renamed_process() {
        let s = ProcessSensor::new();
        let known = |n: &str| s.is_watched(n) || TRACKED.contains(&n);
        assert_eq!(canonical_name("npm test", known), "npm");
        assert_eq!(canonical_name("npm run dev", known), "npm");
        assert_eq!(canonical_name("npm exec node -", known), "npm");
        assert_eq!(canonical_name("node-MainThread", known), "node");
        assert_eq!(canonical_name("cargo", known), "cargo");
        assert_eq!(canonical_name("g++", known), "g++");
        // Names that reduce to nothing known stay whole.
        assert_eq!(canonical_name("Web Content", known), "Web Content");
        assert_eq!(canonical_name("slack-mcp-linux", known), "slack-mcp-linux");
    }

    /// Rows from `ps -o pid=,ppid=,lstart=,comm=` during a real `npm test`,
    /// `npm run dev` and `npm exec`, on node v26.10.0 and npm 12.2.0.
    const LIVE_NPM: &str = "\
3181657 3181656 Sat Oct 10 22:18:30 2026 bash
3181672 3181657 Sat Oct 10 22:18:30 2026 npm test
3181673 3181657 Sat Oct 10 22:18:30 2026 npm run dev
3181674 3181657 Sat Oct 10 22:18:30 2026 npm exec node -
3181706 3181673 Sat Oct 10 22:18:30 2026 node-MainThread
3181707 3181672 Sat Oct 10 22:18:30 2026 node-MainThread
3181721 3181674 Sat Oct 10 22:18:30 2026 node-MainThread
";

    #[test]
    fn tally_sees_npm_and_node_as_this_host_names_them() {
        let s = ProcessSensor::new();
        let t = tally(&parse_ps(LIVE_NPM), |c| c != "bash", |c| s.is_watched(c));
        assert_eq!(t.apps, HashMap::from([("node".to_string(), 3)]));
        assert_eq!(
            t.tools,
            HashMap::from([
                (3181672, tool("npm", &["node"])),
                (3181673, tool("npm", &["node"])),
                (3181674, tool("npm", &["node"])),
            ])
        );
    }

    #[test]
    fn a_live_npm_test_finishes_naming_node() {
        let s = ProcessSensor::new();
        let poll = |rows: &str| tally(&parse_ps(rows), |c| c != "bash", |c| s.is_watched(c));
        let lines: Vec<&str> = LIVE_NPM.lines().collect();
        let idle = format!("{}\n", lines[0]);
        let test_run = format!("{}\n{}\n{}\n", lines[0], lines[1], lines[5]);
        let mut sensor = ProcessSensor::new();
        let base = poll(&idle);
        sensor.prior = base.apps;
        sensor.prior_tools = base.tools;
        assert!(sensor.diff(poll(&test_run), 0).is_empty());
        assert_eq!(texts(&sensor.diff(poll(&test_run), 0)), vec!["node started"]);
        assert_eq!(texts(&sensor.diff(poll(&idle), 0)), vec!["npm finished (node exited)"]);
    }

    #[test]
    fn a_lasting_app_starts_one_poll_late_and_exits() {
        let mut s = ProcessSensor::new();
        s.prior = apps(&["nvim"]);
        assert!(s.diff(snap(&["nvim", "docker"]), 0).is_empty());
        assert_eq!(texts(&s.diff(snap(&["nvim", "docker"]), 0)), vec!["docker started"]);
        assert!(s.diff(snap(&["nvim", "docker"]), 0).is_empty());
        assert_eq!(texts(&s.diff(snap(&["docker"]), 0)), vec!["nvim exited"]);
        assert_eq!(texts(&s.diff(snap(&[]), 0)), vec!["docker exited"]);
    }
}
