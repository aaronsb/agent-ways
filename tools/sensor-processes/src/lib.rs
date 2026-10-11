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
    /// The process's start time in seconds, from `lstart`. Compared within
    /// [`START_TOLERANCE_SECS`], so a pid reused by a later process reads
    /// as a different process. `None` when `ps` cannot print `lstart`.
    start: Option<i64>,
    comm: String,
}

/// The `ps` columns [`parse_ps`] reads. `lstart` is five words in the C
/// locale, which [`PS_ENV`] sets.
const PS_COLUMNS: &str = "pid=,ppid=,lstart=,comm=";
/// The columns for a `ps` that cannot print `lstart`, such as busybox's.
const PS_COLUMNS_NO_START: &str = "pid=,ppid=,comm=";
const PS_ENV: (&str, &str) = ("LC_ALL", "C");

/// How far apart two reads of one process's start time can be. `ps`
/// derives `lstart` from the wall clock at each read, so a clock step or
/// a rounding edge moves it by a second or two. The kernel hands a pid
/// out again only after the pid space wraps, which takes far longer.
const START_TOLERANCE_SECS: i64 = 2;

/// Do two start times name the same process? Without both, only the pid
/// and name tell processes apart, so any start matches.
fn same_start(a: Option<i64>, b: Option<i64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= START_TOLERANCE_SECS,
        _ => true,
    }
}

/// `lstart` words, such as `Sat Oct 10 22:18:30 2026`, as seconds since
/// the epoch, read as UTC. The local offset is the same in every read, so
/// a difference between two reads is exact.
fn parse_lstart(words: &[&str]) -> Option<i64> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let [_, month, day, time, year] = words else { return None };
    let month = MONTHS.iter().position(|m| m == month)? as i64 + 1;
    let day: i64 = day.parse().ok()?;
    let year: i64 = year.parse().ok()?;
    let mut hms = time.split(':').map(|n| n.parse::<i64>().ok());
    let (h, m, s) = (hms.next()??, hms.next()??, hms.next()??);
    if hms.next().is_some() {
        return None;
    }
    // Days from 1970-01-01 to the date, by Howard Hinnant's civil algorithm.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + m * 60 + s)
}

/// Parse `ps -o pid=,ppid=,lstart=,comm=` output, or with `with_start`
/// false, `ps -o pid=,ppid=,comm=` output. Rows that do not parse are
/// skipped. The command name is the rest of the row, so a name with a
/// space survives.
fn parse_ps(stdout: &str, with_start: bool) -> Vec<Proc> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let start = match with_start {
                true => Some(parse_lstart(&parts.by_ref().take(5).collect::<Vec<_>>())?),
                false => None,
            };
            let comm = parts.collect::<Vec<_>>().join(" ");
            (!comm.is_empty()).then_some(Proc { pid, ppid, start, comm })
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

/// Process names that a known tool gives itself, each with the name the
/// tool is tracked under. Node names its main thread, so its `comm` reads
/// `node-MainThread`. A new renamer is one entry here.
const ALIASES: &[(&str, &str)] = &[("node-MainThread", "node")];

/// npm subcommands that run a package, as `npx` does. npx itself runs
/// `npm exec`, and `x` is npm's alias for `exec`.
const NPM_EXEC: &[&str] = &["exec", "x"];

/// The name a process is tracked under. A process can rename itself, and
/// a few known tools do. An exact `comm` in [`ALIASES`] takes its alias.
/// npm sets its title to its command line, so a `comm` whose first word is
/// `npm` reads `npm test`, `npm run dev` or `npm exec @scope/server`: the
/// exec form is `npx`, which launches a package and is tracked as an app,
/// and every other form is `npm`, the build tool. Any other `comm` is kept
/// as it is, so `ssh-agent` stays `ssh-agent` and is never counted as `ssh`.
fn canonical_name(comm: &str) -> &str {
    if let Some((_, name)) = ALIASES.iter().find(|(alias, _)| *alias == comm) {
        return name;
    }
    let mut words = comm.split_whitespace();
    if words.next() == Some("npm") {
        return match words.next() {
            Some(sub) if NPM_EXEC.contains(&sub) => "npx",
            _ => "npm",
        };
    }
    comm
}

/// The rows of a `ps` run, or `None` when it could not be read: `ps` did
/// not start, exited non-zero, or listed no process. The user running the
/// sensor always has at least the sensor itself, so zero rows is a failed
/// read, never an empty session.
fn ps_rows(output: std::io::Result<Output>, with_start: bool) -> Option<Vec<Proc>> {
    let output = output.ok().filter(|o| o.status.success())?;
    let rows = parse_ps(&String::from_utf8_lossy(&output.stdout), with_start);
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
    start: Option<i64>,
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
/// process is first given its [`canonical_name`]. `is_relevant` is given
/// the `comm` as `ps` printed it and the canonical name, in that order.
fn tally(procs: &[Proc], is_relevant: impl Fn(&str, &str) -> bool, is_watched: impl Fn(&str) -> bool) -> Snapshot {
    let raw: Vec<&str> = procs.iter().map(|p| p.comm.as_str()).collect();
    let procs: Vec<Proc> = procs.iter().map(|p| Proc { comm: canonical_name(&p.comm).to_string(), ..p.clone() }).collect();
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
    for (p, _) in procs.iter().zip(raw).filter(|(p, raw)| is_watched(&p.comm) || is_relevant(raw, &p.comm)) {
        let above = watched_above(p);
        if is_watched(&p.comm) {
            if above.is_empty() {
                snap.tools.insert(p.pid, Tool { name: p.comm.clone(), start: p.start, hosts: HashSet::new() });
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
    /// Whether `ps` prints `lstart`. Cleared, for the life of the sensor,
    /// when a read with it fails and one without it succeeds.
    ps_has_lstart: bool,
    /// `ps` reads that have failed in a row.
    failed_reads: u32,
}

/// Failed `ps` reads in a row after which the recovering poll establishes
/// a new baseline rather than diffing against the state before the outage.
const REBASELINE_AFTER: u32 = 6;

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
            ps_has_lstart: true,
            failed_reads: 0,
        }
    }

    /// Is `name` one of the processes we enrich on exit?
    fn is_watched(&self, name: &str) -> bool {
        self.watch.iter().any(|w| w == name)
    }

    /// Read the process table with `run`, which runs `ps -o <columns>`.
    /// A `ps` that cannot print `lstart`, such as busybox's, fails the
    /// first read; the sensor then reads without it from then on, tells
    /// processes apart by pid and name only, and says so once on stderr.
    fn read_ps(&mut self, mut run: impl FnMut(&str) -> std::io::Result<Output>) -> Option<Vec<Proc>> {
        if self.ps_has_lstart {
            if let Some(rows) = ps_rows(run(PS_COLUMNS), true) {
                return Some(rows);
            }
            let rows = ps_rows(run(PS_COLUMNS_NO_START), false)?;
            eprintln!("[attend] processes: ps cannot print lstart; telling processes apart by pid and name only");
            self.ps_has_lstart = false;
            return Some(rows);
        }
        ps_rows(run(PS_COLUMNS_NO_START), false)
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
    /// report every app and tool as gone. The first failed read and the
    /// recovery are each said once on stderr. After [`REBASELINE_AFTER`]
    /// failed reads in a row, the prior state is too old to diff, so the
    /// recovering poll establishes a new baseline silently instead of
    /// reporting everything that changed during the outage. Apps are
    /// tracked by name, not by pid, so Chrome going from 47 to 49 renderers
    /// is not a change, and a watched tool under another watched tool is
    /// left out (see [`tally`]).
    fn observe(&mut self, rows: Option<Vec<Proc>>, focus: &Focus, now: u64) -> Vec<(f64, String)> {
        let Some(rows) = rows else {
            self.failed_reads += 1;
            if self.failed_reads == 1 {
                eprintln!("[attend] processes: ps read failed; skipping polls until it reads again");
            }
            return Vec::new();
        };
        let failed = std::mem::take(&mut self.failed_reads);
        if failed > 0 {
            eprintln!("[attend] processes: ps reads again after {failed} failed poll(s)");
        }
        let current = tally(&rows, |raw, c| Self::is_relevant(raw, c, focus), |c| self.is_watched(c));
        if !self.baseline_established || failed >= REBASELINE_AFTER {
            eprintln!("[attend] processes: baseline established ({} apps: {})",
                current.apps.len(),
                current.apps.keys().cloned().collect::<Vec<_>>().join(", "));
            self.prior = current.apps;
            self.prior_tools = current.tools;
            self.pending_start.clear();
            self.held_exits.clear();
            self.baseline_established = true;
            return Vec::new();
        }
        self.diff(current, now)
    }

    /// The events between the previous snapshot and `current`, given the
    /// current marker and time. A watched tool never announces its start,
    /// and its exit is always `<tool> finished`. A hosted app whose exit is
    /// seen in the same poll as its tool's, or one poll before, is named in
    /// that one event, as in `npm finished (node exited)`, and only in the
    /// first such event when several of its tools finish together. Any other app
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
            tools.get(pid).is_some_and(|t| self.prior_tools.get(pid).is_some_and(|p| p.name == t.name && same_start(p.start, t.start)))
        };
        let mut finished: Vec<(&u32, &Tool)> = self.prior_tools.iter().filter(|(pid, _)| !alive(pid)).collect();
        finished.sort_by_key(|(pid, _)| **pid);
        // Each exit is named by one tool only: of the finished tools that
        // hosted it, the one with the lowest pid.
        let mut named = vec![false; exits.len()];
        for (pid, tool) in finished {
            let mut with: Vec<String> = Vec::new();
            for (i, exit) in exits.iter().enumerate() {
                if !named[i] && exit.hosts.contains(pid) {
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

    /// Is a process worth events? `raw` is its `comm` as `ps` printed it
    /// and `name` its [`canonical_name`]. Noise and the tracked list are
    /// read by `name`. A focus keyword matches either, so a keyword finds
    /// a process by the title it gave itself or by the name it is tracked
    /// under.
    fn is_relevant(raw: &str, name: &str, focus: &Focus) -> bool {
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
                name.starts_with(prefix)
            } else {
                name == *n
            }
        }) {
            return false;
        }

        // Focus keywords boost relevance
        if focus.keywords.iter().any(|kw| raw.contains(kw) || name.contains(kw)) {
            return true;
        }

        TRACKED.contains(&name)
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
        let user = whoami();
        let rows = self.read_ps(|columns| Command::new("ps").args(["-u", &user, "-o", columns]).env(PS_ENV.0, PS_ENV.1).output());
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
    /// [`T0`] in seconds since the epoch, read as UTC.
    const T0_SECS: i64 = 1_791_670_710;

    fn procs(rows: &[(u32, u32, &str)]) -> Vec<Proc> {
        rows.iter().map(|&(pid, ppid, comm)| Proc { pid, ppid, start: Some(T0_SECS), comm: comm.into() }).collect()
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
        tally(&procs(rows), |_, c| c != "zsh" && c != "sh", |c| s.is_watched(c))
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
        Tool { name: name.into(), start: Some(T0_SECS), hosts: hosts.iter().map(|h| h.to_string()).collect() }
    }

    #[test]
    fn parse_ps_reads_pid_ppid_and_a_spaced_name() {
        let p = parse_ps(&format!(
            "  10     1 {T0} cargo\n  11    10 {T0} rustc\n bad row\n  13 10 Sat Oct 10 rustc\n  12 10 {T0} Web Content\n"
        ), true);
        assert_eq!(p, procs(&[(10, 1, "cargo"), (11, 10, "rustc"), (12, 10, "Web Content")]));
    }

    #[test]
    fn parse_ps_reads_rows_without_a_start_time() {
        let p = parse_ps("  10     1 cargo\n  12 10 Web Content\n bad row\n", false);
        let mut want = procs(&[(10, 1, "cargo"), (12, 10, "Web Content")]);
        want.iter_mut().for_each(|p| p.start = None);
        assert_eq!(p, want);
    }

    /// A `ps` that cannot print `lstart`, as busybox's cannot, fails the
    /// first read; the sensor reads without it from then on.
    #[test]
    fn a_ps_without_lstart_falls_back_to_pid_and_name() {
        let mut s = ProcessSensor::new();
        let mut asked: Vec<String> = Vec::new();
        let mut run = |columns: &str| {
            asked.push(columns.to_string());
            match columns {
                PS_COLUMNS => ps_output(1, ""),
                _ => ps_output(0, "  1  0 zsh\n  10  1 cargo\n"),
            }
        };
        let rows = s.read_ps(&mut run).expect("the fallback read");
        assert_eq!(rows.iter().map(|p| (p.pid, p.start)).collect::<Vec<_>>(), vec![(1, None), (10, None)]);
        assert!(!s.ps_has_lstart);
        assert!(s.read_ps(&mut run).is_some());
        assert_eq!(asked, vec![PS_COLUMNS, PS_COLUMNS_NO_START, PS_COLUMNS_NO_START]);

        // Without start times, a tool is told apart by pid and name.
        let focus = no_focus();
        let rows = |stdout: &str| Some(parse_ps(stdout, false));
        assert!(s.observe(rows("  1  0 zsh\n  5  1 nvim\n"), &focus, 0).is_empty(), "baseline");
        assert!(s.observe(rows("  1  0 zsh\n  5  1 nvim\n  10  1 cargo\n"), &focus, 0).is_empty());
        assert!(s.observe(rows("  1  0 zsh\n  5  1 nvim\n  10  1 cargo\n"), &focus, 0).is_empty(), "still running");
        assert_eq!(texts(&s.observe(rows("  1  0 zsh\n  5  1 nvim\n"), &focus, 0)), vec!["cargo finished"]);
    }

    /// A `ps` that prints `lstart` is read with it, and a read that fails
    /// both ways keeps it for the next poll.
    #[test]
    fn a_failed_read_keeps_lstart() {
        let mut s = ProcessSensor::new();
        assert!(s.read_ps(|_| ps_output(1, "")).is_none());
        assert!(s.ps_has_lstart);
        let rows = s.read_ps(|columns| match columns {
            PS_COLUMNS => ps_output(0, &format!("  10  1 {T0} cargo\n")),
            _ => ps_output(0, "  10  1 cargo\n"),
        });
        assert_eq!(rows, Some(procs(&[(10, 1, "cargo")])));
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
        assert_eq!(tally(&p, |_, _| true, |_| false).apps.len(), 2);
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

    /// Two npm runs finishing in the same poll, with node gone from under
    /// both: the one node exit is named once, by the first run.
    #[test]
    fn a_hosted_exit_is_named_by_one_tool() {
        let both = [ZSH, NVIM, (10, 1, "npm"), (11, 10, "node"), (20, 1, "npm"), (21, 20, "node")];
        let mut s = sensor_after(&[ZSH, NVIM]);
        for _ in 0..3 {
            s.diff(see(&both), 0);
        }
        assert_eq!(texts(&s.diff(see(&[ZSH, NVIM]), 0)), vec!["npm finished", "npm finished (node exited)"]);
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
        second[2].start = Some(T0_SECS + 32);
        let second = tally(&second, |_, c| c != "zsh", |c| s.is_watched(c));
        assert_eq!(texts(&s.diff(second, 0)), vec!["cargo finished"]);
    }

    #[test]
    fn parse_lstart_reads_seconds_since_the_epoch() {
        let secs = |s: &str| parse_lstart(&s.split_whitespace().collect::<Vec<_>>());
        assert_eq!(secs(T0), Some(T0_SECS));
        assert_eq!(secs("Thu Jan  1 00:00:00 1970"), Some(0));
        assert_eq!(secs("Wed Dec 31 23:59:59 1969"), Some(-1));
        assert_eq!(secs("Thu Feb 29 23:59:59 2024"), Some(1_709_251_199), "leap day");
        assert_eq!(secs("Sat Foo 10 22:18:30 2026"), None);
        assert_eq!(secs("Sat Oct 10 22:18 2026"), None);
        assert_eq!(secs("Sat Oct 10 22:18:30:01 2026"), None);
        assert_eq!(secs("Sat Oct 10 22:18:30"), None);
    }

    /// A wall-clock step moves every `lstart` a little between reads; no
    /// running tool reads as finished for it.
    #[test]
    fn a_clock_step_does_not_finish_a_running_tool() {
        let rows = [ZSH, NVIM, (10, 1, "cargo")];
        for step in [-2, -1, 1, 2] {
            let mut s = sensor_after(&rows);
            let mut stepped = procs(&rows);
            stepped[2].start = Some(T0_SECS + step);
            assert!(s.diff(tally(&stepped, |_, c| c != "zsh", |c| s.is_watched(c)), 0).is_empty(), "step {step}");
        }
        let mut s = sensor_after(&rows);
        let mut later = procs(&rows);
        later[2].start = Some(T0_SECS + 3);
        assert_eq!(texts(&s.diff(tally(&later, |_, c| c != "zsh", |c| s.is_watched(c)), 0)), vec!["cargo finished"]);
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
        assert_eq!(ps_rows(ps_output(0, &format!("  10  1 {T0} cargo\n")), true), Some(procs(&[(10, 1, "cargo")])));
        assert_eq!(ps_rows(ps_output(0, ""), true), None, "zero rows");
        assert_eq!(ps_rows(ps_output(0, "garbage\n"), true), None, "no row parsed");
        assert_eq!(ps_rows(ps_output(1, &format!("  10  1 {T0} cargo\n")), true), None, "non-zero exit");
        assert_eq!(ps_rows(Err(std::io::Error::other("no ps")), true), None, "did not start");
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

    /// A sensor whose `ps` failed `failures` times in a row while npm was
    /// running, and then reads a table where npm is gone and docker runs.
    fn recover_after(failures: u32) -> Vec<String> {
        let focus = no_focus();
        let running = procs(&[ZSH, NVIM, (10, 1, "npm"), (11, 10, "node")]);
        let mut s = ProcessSensor::new();
        s.observe(Some(procs(&[ZSH, NVIM])), &focus, 0);
        s.observe(Some(running.clone()), &focus, 0);
        s.observe(Some(running), &focus, 0);
        for _ in 0..failures {
            assert!(s.observe(None, &focus, 0).is_empty());
        }
        let mut after = texts(&s.observe(Some(procs(&[ZSH, NVIM, (20, 1, "docker")])), &focus, 0));
        after.extend(texts(&s.observe(Some(procs(&[ZSH, NVIM, (20, 1, "docker")])), &focus, 0)));
        assert_eq!(s.failed_reads, 0);
        after.sort();
        after
    }

    /// A short outage is diffed across: what changed during it is reported.
    #[test]
    fn a_short_ps_outage_reports_what_changed() {
        assert_eq!(recover_after(REBASELINE_AFTER - 1), vec!["docker started", "npm finished (node exited)"]);
    }

    /// A long outage re-baselines on recovery: no burst of stale events.
    #[test]
    fn a_long_ps_outage_rebaselines_silently() {
        assert!(recover_after(REBASELINE_AFTER).is_empty());
        assert!(recover_after(50).is_empty());
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
            tally(&procs(rows), |raw, c| ProcessSensor::is_relevant(raw, c, &focus), |c| s.is_watched(c))
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

    #[test]
    fn a_focus_keyword_matches_the_raw_comm_or_the_name() {
        let focus = Focus { keywords: vec!["vite".into()], ..no_focus() };
        assert!(ProcessSensor::is_relevant("npm exec vite", "devsrv", &focus), "raw comm");
        assert!(ProcessSensor::is_relevant("devsrv-Main", "vite", &focus), "canonical name");
        assert!(!ProcessSensor::is_relevant("devsrv-Main", "devsrv", &focus));
        assert!(!ProcessSensor::is_relevant("vite", "zsh", &focus), "noise is read by name");
        let t = tally(&procs(&[(10, 1, "npm exec vite")]), |raw, name| raw == "npm exec vite" && name == "npx", |_| false);
        assert_eq!(t.apps, apps(&["npx"]), "tally passes the raw comm and the name");
    }

    /// A user's own watch list is tracked the same way.
    #[test]
    fn a_configured_watched_tool_is_tracked() {
        let s = ProcessSensor::with_watch(vec!["mix".into()]);
        let focus = no_focus();
        let t = tally(&procs(&[ZSH, (10, 1, "mix")]), |raw, c| ProcessSensor::is_relevant(raw, c, &focus), |c| s.is_watched(c));
        assert_eq!(t.tools, HashMap::from([(10, tool("mix", &[]))]));
    }

    #[test]
    fn canonical_name_reads_npm_and_node_titles() {
        assert_eq!(canonical_name("npm test"), "npm");
        assert_eq!(canonical_name("npm run dev"), "npm");
        assert_eq!(canonical_name("npm install"), "npm");
        assert_eq!(canonical_name("npm ci"), "npm");
        assert_eq!(canonical_name("npm"), "npm");
        assert_eq!(canonical_name("npm exec node -"), "npx");
        assert_eq!(canonical_name("npm exec @scope"), "npx", "comm cut at 15 characters");
        assert_eq!(canonical_name("npm x cowsay"), "npx");
        assert_eq!(canonical_name("node-MainThread"), "node");
        assert_eq!(canonical_name("cargo"), "cargo");
        assert_eq!(canonical_name("g++"), "g++");
        assert_eq!(canonical_name("Web Content"), "Web Content");
        assert_eq!(canonical_name("slack-mcp-linux"), "slack-mcp-linux");
    }

    /// A tool's companion binary keeps its own name: it is not the tool.
    #[test]
    fn canonical_name_keeps_a_companion_binary_whole() {
        for comm in [
            "ssh-agent", "cargo-watch", "clang-format", "pip-compile", "docker-proxy",
            "containerd-shim", "node-gyp", "python3-config", "rustc-wrapper", "gcc-ar",
        ] {
            assert_eq!(canonical_name(comm), comm);
        }
        let t = see(&[ZSH, (10, 1, "cargo-watch"), (11, 1, "ssh-agent"), (12, 1, "gcc-ar"), (13, 1, "ssh")]);
        assert!(t.tools.is_empty(), "{:?}", t.tools);
        assert_eq!(t.apps.get("ssh"), Some(&1));
    }

    /// An MCP server launched with `npx` under `claude` is an app, not a
    /// build: it gives no tool, so it never reports `npm finished`.
    #[test]
    fn an_npm_exec_under_claude_is_no_tool() {
        let focus = no_focus();
        let s = ProcessSensor::new();
        let rows = [ZSH, (10, 1, "claude"), (11, 10, "npm exec @foo/mcp"), (12, 11, "node-MainThread")];
        let t = tally(&procs(&rows), |raw, c| ProcessSensor::is_relevant(raw, c, &focus), |c| s.is_watched(c));
        assert!(t.tools.is_empty(), "{:?}", t.tools);
        assert_eq!(t.apps, HashMap::from([("claude".into(), 1), ("npx".into(), 1), ("node".into(), 1)]));
    }

    /// This host's process table, captured with
    /// `LC_ALL=C ps -u $USER -o pid=,ppid=,lstart=,comm=` while eight MCP
    /// servers launched with `npx` ran under `claude`. The npm scope in
    /// their titles is redacted to `@scope`.
    const LIVE_HOST: &str = include_str!("../fixtures/ps-live-host.txt");

    #[test]
    fn the_live_host_table_names_each_process_as_itself() {
        let focus = no_focus();
        let s = ProcessSensor::new();
        let rows = parse_ps(LIVE_HOST, true);
        let comms = |name: &str| rows.iter().filter(|p| p.comm == name).count();
        assert_eq!(comms("npm exec @scope"), 8, "the fixture holds the MCP servers");
        assert!(comms("ssh-agent") > 0, "the fixture holds ssh-agent");
        let t = tally(&rows, |raw, c| ProcessSensor::is_relevant(raw, c, &focus), |c| s.is_watched(c));
        assert!(t.tools.values().all(|tool| tool.name != "npm"), "{:?}", t.tools);
        assert_eq!(t.apps.get("npx"), Some(&8));
        assert_eq!(t.apps.get("ssh").copied().unwrap_or(0) as usize, comms("ssh"), "ssh-agent is not ssh");
        for p in rows.iter().filter(|p| p.comm == "cargo-watch") {
            assert!(!t.tools.contains_key(&p.pid), "cargo-watch is no tool");
        }
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
        let t = tally(&parse_ps(LIVE_NPM, true), |_, c| c != "bash", |c| s.is_watched(c));
        assert_eq!(t.apps, HashMap::from([("node".to_string(), 3), ("npx".to_string(), 1)]));
        assert_eq!(t.tools, HashMap::from([(3181672, tool("npm", &["node"])), (3181673, tool("npm", &["node"]))]));
    }

    #[test]
    fn a_live_npm_test_finishes_naming_node() {
        let s = ProcessSensor::new();
        let poll = |rows: &str| tally(&parse_ps(rows, true), |_, c| c != "bash", |c| s.is_watched(c));
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
