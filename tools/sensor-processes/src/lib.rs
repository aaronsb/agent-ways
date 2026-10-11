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
    /// The process's start time in seconds, from `lstart`. `None` when
    /// `ps` cannot print `lstart`.
    start: Option<i64>,
    comm: String,
}

/// The `ps` arguments for the session user's processes, read with the
/// columns [`Columns::Lstart`] parses.
fn ps_args(user: &str) -> [&str; 4] {
    ["-u", user, "-o", "pid=,ppid=,lstart=,comm="]
}
/// The `ps` arguments for a `ps` that cannot print `lstart`, read with the
/// columns [`Columns::OwnedBy`] parses. Busybox's `ps` takes no `-u`, so
/// every process is listed with its owner, and [`parse_ps`] keeps the
/// session user's rows.
const PS_ARGS_NO_START: [&str; 2] = ["-o", "pid=,ppid=,user=,comm="];
/// The environment `ps` runs in. `LC_ALL=C` prints `lstart` as five fixed
/// words. `TZ=UTC0` prints it in UTC, so a change of the host's time zone
/// does not move it.
const PS_ENV: &[(&str, &str)] = &[("LC_ALL", "C"), ("TZ", "UTC0")];

/// How far apart two `lstart` reads of one process can be. `ps` derives
/// `lstart` from the wall clock at each read, so a rounding edge moves it
/// by a second, and a clock step moves it by the size of the step. A step
/// within this tolerance does not end a running tool. A larger step reads
/// as a restart, and the tool reports a finish: a known limit where only
/// `lstart` is read. On Linux a tool's start is read in clock ticks since
/// boot instead (see [`start_ticks`]), which no clock step moves.
const START_TOLERANCE_SECS: i64 = 2;

/// Is `now` the same run of a tool as `prior`? The start ticks are
/// compared exactly when both reads have them, otherwise the `lstart`
/// seconds within [`START_TOLERANCE_SECS`]. Without either, only the pid
/// and name tell runs apart, so any start matches.
fn same_start(prior: &Tool, now: &Tool) -> bool {
    match (prior.ticks, now.ticks, prior.start, now.start) {
        (Some(a), Some(b), _, _) => a == b,
        (_, _, Some(a), Some(b)) => (a - b).abs() <= START_TOLERANCE_SECS,
        _ => true,
    }
}

/// A process's start time in clock ticks since boot, field 22 of
/// `/proc/<pid>/stat`. The kernel records it once, at the fork, so no
/// clock step or time zone change moves it, and a reused pid reads a new
/// value. `None` when the process is gone or the file does not parse.
#[cfg(target_os = "linux")]
fn start_ticks(pid: u32) -> Option<u64> {
    parse_stat_start_ticks(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// Outside Linux there is no `/proc/<pid>/stat`, and `lstart` alone
/// tells runs apart.
#[cfg(not(target_os = "linux"))]
fn start_ticks(_pid: u32) -> Option<u64> {
    None
}

/// Field 22 of a `/proc/<pid>/stat` line. Field 2 is the command name in
/// parentheses and can hold spaces and parentheses itself, so the fields
/// are counted from after the last `)`, where field 3 begins.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_stat_start_ticks(stat: &str) -> Option<u64> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(22 - 3)?.parse().ok()
}

/// `lstart` words, such as `Sat Oct 10 22:18:30 2026`, as seconds since
/// the epoch. `ps` runs with `TZ=UTC0` ([`PS_ENV`]), so the words are UTC
/// and are read as UTC.
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

/// The columns of one `ps` read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Columns<'a> {
    /// `pid=,ppid=,lstart=,comm=`, from a `ps` that already kept to the
    /// session user's processes.
    Lstart,
    /// `pid=,ppid=,user=,comm=`, every process with its owner. Only the
    /// rows this user owns are kept.
    OwnedBy(&'a str),
}

/// Does a `user` column name `user`? Busybox cuts the column to eight
/// characters, and procps prints a longer name as seven and a `+`, so a
/// longer name also matches those forms.
fn owner_matches(column: &str, user: &str) -> bool {
    if column == user {
        return true;
    }
    if user.chars().count() <= 8 {
        return false;
    }
    let cut: String = user.chars().take(8).collect();
    column == cut || column.strip_suffix('+').is_some_and(|head| !head.is_empty() && user.starts_with(head))
}

/// Parse `ps` output with `columns`. Rows that do not parse are skipped,
/// and with [`Columns::OwnedBy`], so are rows another user owns. The
/// command name is the rest of the row, so a name with a space survives.
fn parse_ps(stdout: &str, columns: Columns) -> Vec<Proc> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let start = match columns {
                Columns::Lstart => Some(parse_lstart(&parts.by_ref().take(5).collect::<Vec<_>>())?),
                Columns::OwnedBy(user) => {
                    if !owner_matches(parts.next()?, user) {
                        return None;
                    }
                    None
                }
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
fn ps_rows(output: std::io::Result<Output>, columns: Columns) -> Option<Vec<Proc>> {
    let output = output.ok().filter(|o| o.status.success())?;
    let rows = parse_ps(&String::from_utf8_lossy(&output.stdout), columns);
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
    /// The tool's start time from `lstart`, so a pid reused by a same-name
    /// process is told apart from the run that held it.
    start: Option<i64>,
    /// The tool's start in clock ticks since boot, on Linux. Compared
    /// before `start` when both reads have it (see [`same_start`]).
    ticks: Option<u64>,
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
                snap.tools.insert(p.pid, Tool { name: p.comm.clone(), start: p.start, ticks: None, hosts: HashSet::new() });
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
    /// Whether `ps` prints `lstart` and takes `-u`. Cleared, for the life
    /// of the sensor, when a read with them fails and one without them
    /// succeeds.
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

    /// Read `user`'s processes with `run`, which runs `ps` with the
    /// arguments it is given. A `ps` that cannot print `lstart` or take
    /// `-u`, such as busybox's, fails the first read; the sensor then
    /// reads every process with its owner from then on, keeps `user`'s
    /// rows, and says so once on stderr. Without `lstart`, a tool's run is
    /// told apart by its start ticks on Linux and by pid and name only
    /// elsewhere.
    fn read_ps(&mut self, user: &str, mut run: impl FnMut(&[&str]) -> std::io::Result<Output>) -> Option<Vec<Proc>> {
        if self.ps_has_lstart {
            if let Some(rows) = ps_rows(run(&ps_args(user)), Columns::Lstart) {
                return Some(rows);
            }
            let rows = ps_rows(run(&PS_ARGS_NO_START), Columns::OwnedBy(user))?;
            eprintln!("[attend] processes: ps cannot print lstart or take -u; reading every process with its owner");
            self.ps_has_lstart = false;
            return Some(rows);
        }
        ps_rows(run(&PS_ARGS_NO_START), Columns::OwnedBy(user))
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
    /// left out (see [`tally`]). `ticks` reads a pid's start ticks, and is
    /// asked only for the watched tools the poll tracks.
    fn observe(
        &mut self,
        rows: Option<Vec<Proc>>,
        ticks: impl Fn(u32) -> Option<u64>,
        focus: &Focus,
        now: u64,
    ) -> Vec<(f64, String)> {
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
        let mut current = tally(&rows, |raw, c| Self::is_relevant(raw, c, focus), |c| self.is_watched(c));
        for (pid, tool) in current.tools.iter_mut() {
            tool.ticks = ticks(*pid);
        }
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
            tools.get(pid).is_some_and(|t| self.prior_tools.get(pid).is_some_and(|p| p.name == t.name && same_start(p, t)))
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
        let rows = self.read_ps(&user, |args| Command::new("ps").args(args).envs(PS_ENV.iter().copied()).output());
        self.refresh_marker();
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.observe(rows, start_ticks, focus, now)
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
        Tool { name: name.into(), start: Some(T0_SECS), ticks: None, hosts: hosts.iter().map(|h| h.to_string()).collect() }
    }

    #[test]
    fn parse_ps_reads_pid_ppid_and_a_spaced_name() {
        let p = parse_ps(&format!(
            "  10     1 {T0} cargo\n  11    10 {T0} rustc\n bad row\n  13 10 Sat Oct 10 rustc\n  12 10 {T0} Web Content\n"
        ), Columns::Lstart);
        assert_eq!(p, procs(&[(10, 1, "cargo"), (11, 10, "rustc"), (12, 10, "Web Content")]));
    }

    #[test]
    fn parse_ps_reads_rows_owned_by_the_user() {
        let p = parse_ps("  10     1 me  cargo\n  11 1 root sshd\n  12 10 me Web Content\n bad row\n  13 1 me\n", Columns::OwnedBy("me"));
        let mut want = procs(&[(10, 1, "cargo"), (12, 10, "Web Content")]);
        want.iter_mut().for_each(|p| p.start = None);
        assert_eq!(p, want);
    }

    #[test]
    fn owner_matches_a_cut_user_column() {
        assert!(owner_matches("me", "me"));
        assert!(!owner_matches("mel", "me"));
        assert!(!owner_matches("me", "mel"));
        assert!(owner_matches("verylong", "verylongname"), "busybox cuts at eight");
        assert!(owner_matches("verylon+", "verylongname"), "procps marks a cut with +");
        assert!(!owner_matches("verylon+", "verylon"), "a name of eight or fewer matches only itself");
        assert!(!owner_matches("+", "verylongname"));
    }

    /// `ps` as busybox's parses its arguments: no `-u`, and `-o` with
    /// `pid`, `ppid`, `user` and `comm` but no `lstart`. Every process is
    /// listed, whoever owns it.
    fn busybox_ps(args: &[&str]) -> std::io::Result<Output> {
        const SUPPORTED: &[&str] = &["user", "group", "comm", "args", "pid", "ppid", "pgid", "etime", "nice", "rgroup", "ruser", "time", "tty", "vsz", "sid", "stat", "rss"];
        let mut columns: Vec<&str> = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match *arg {
                "-o" => columns.extend(args.next().map_or("", |v| v).split(',').map(|c| c.split('=').next().unwrap())),
                "-T" => {}
                _ => return ps_output(1, ""),
            }
        }
        if columns.iter().any(|c| !SUPPORTED.contains(c)) {
            return ps_output(1, "");
        }
        let table = [(1, 0, "root", "init"), (40, 1, "root", "sshd"), (50, 1, "me", "zsh"), (60, 50, "me", "cargo"), (70, 1, "other", "cargo")];
        let out: String = table
            .iter()
            .map(|&(pid, ppid, user, comm)| {
                let cell = |c: &str| match c {
                    "pid" => format!("{pid:>5}"),
                    "ppid" => format!("{ppid:>5}"),
                    "user" => format!("{user:<8}"),
                    "comm" => comm.to_string(),
                    _ => "?".to_string(),
                };
                columns.iter().map(|c| cell(c)).collect::<Vec<_>>().join(" ") + "\n"
            })
            .collect();
        ps_output(0, &out)
    }

    /// A `ps` that takes no `-u` and cannot print `lstart`, as busybox's,
    /// fails the first read; the sensor then reads every process with its
    /// owner and keeps the user's own rows.
    #[test]
    fn a_busybox_ps_falls_back_to_rows_owned_by_the_user() {
        let mut s = ProcessSensor::new();
        let mut asked: Vec<Vec<String>> = Vec::new();
        let mut run = |args: &[&str]| {
            asked.push(args.iter().map(|a| a.to_string()).collect());
            busybox_ps(args)
        };
        let rows = s.read_ps("me", &mut run).expect("the fallback read");
        assert_eq!(rows.iter().map(|p| (p.pid, p.start)).collect::<Vec<_>>(), vec![(50, None), (60, None)]);
        assert!(!s.ps_has_lstart);
        assert!(s.read_ps("me", &mut run).is_some());
        let fallback: Vec<String> = PS_ARGS_NO_START.iter().map(|a| a.to_string()).collect();
        assert_eq!(asked, vec![ps_args("me").iter().map(|a| a.to_string()).collect(), fallback.clone(), fallback]);
    }

    /// Without start times or start ticks, a tool is told apart by pid and
    /// name.
    #[test]
    fn without_start_times_a_tool_is_told_apart_by_pid_and_name() {
        let mut s = ProcessSensor::new();
        let focus = no_focus();
        let rows = |stdout: &str| Some(parse_ps(stdout, Columns::OwnedBy("me")));
        assert!(s.observe(rows("  1  0 me zsh\n  5  1 me nvim\n"), no_ticks, &focus, 0).is_empty(), "baseline");
        assert!(s.observe(rows("  1  0 me zsh\n  5  1 me nvim\n  10  1 me cargo\n"), no_ticks, &focus, 0).is_empty());
        assert!(s.observe(rows("  1  0 me zsh\n  5  1 me nvim\n  10  1 me cargo\n"), no_ticks, &focus, 0).is_empty(), "still running");
        assert_eq!(texts(&s.observe(rows("  1  0 me zsh\n  5  1 me nvim\n"), no_ticks, &focus, 0)), vec!["cargo finished"]);
    }

    /// A `ps` that prints `lstart` is read with it, and a read that fails
    /// both ways keeps it for the next poll.
    #[test]
    fn a_failed_read_keeps_lstart() {
        let mut s = ProcessSensor::new();
        assert!(s.read_ps("me", |_| ps_output(1, "")).is_none());
        assert!(s.ps_has_lstart);
        let rows = s.read_ps("me", |args| match args.contains(&"-u") {
            true => ps_output(0, &format!("  10  1 {T0} cargo\n")),
            false => ps_output(0, "  10  1 me cargo\n"),
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

    /// Where only `lstart` is read, a wall-clock step moves every start
    /// between reads. A step within two seconds leaves a running tool
    /// running; a larger one reads as a restart and reports a finish, a
    /// known limit outside Linux.
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

    #[test]
    fn ps_runs_in_the_c_locale_and_utc() {
        assert!(PS_ENV.contains(&("LC_ALL", "C")));
        assert!(PS_ENV.contains(&("TZ", "UTC0")));
    }

    #[test]
    fn parse_stat_start_ticks_reads_field_22() {
        let stat = "4242 (a (b) c) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10\n";
        assert_eq!(parse_stat_start_ticks(stat), Some(987_654));
        assert_eq!(parse_stat_start_ticks("4242 (cargo) S 1 2 3"), None, "too few fields");
        assert_eq!(parse_stat_start_ticks("4242 cargo S 1"), None, "no command name");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn start_ticks_reads_this_process() {
        let ticks = start_ticks(std::process::id());
        assert!(ticks.is_some());
        assert_eq!(start_ticks(std::process::id()), ticks);
    }

    /// One poll of a table where `cargo` (pid 10) started at `lstart` and
    /// at `ticks` clock ticks since boot, as a Linux host reads it.
    fn poll_with_ticks(s: &mut ProcessSensor, lstart: &str, ticks: u64) -> Vec<String> {
        let table = format!("  1  0 {lstart} zsh\n  5  1 {lstart} nvim\n  10  1 {lstart} cargo\n");
        let asked = std::cell::RefCell::new(Vec::new());
        let read = |pid: u32| {
            asked.borrow_mut().push(pid);
            (pid == 10).then_some(ticks)
        };
        let seen = texts(&s.observe(Some(parse_ps(&table, Columns::Lstart)), read, &no_focus(), 0));
        assert_eq!(asked.into_inner(), vec![10], "ticks are read for the watched tools only");
        seen
    }

    /// A time zone change or a clock step moves `lstart` but not the start
    /// ticks, so a running tool keeps running.
    #[test]
    fn start_ticks_survive_a_time_zone_change_and_a_clock_step() {
        let mut s = ProcessSensor::new();
        assert!(poll_with_ticks(&mut s, T0, 4242).is_empty(), "baseline");
        assert!(poll_with_ticks(&mut s, "Sat Oct 10 23:18:30 2026", 4242).is_empty(), "an hour on: a time zone change");
        assert!(poll_with_ticks(&mut s, "Sat Oct 10 23:19:15 2026", 4242).is_empty(), "45 s on: a clock step");
        assert!(poll_with_ticks(&mut s, "Sat Oct 10 21:00:00 2026", 4242).is_empty(), "a step back");
    }

    /// A pid reused by a new run of the same tool, started within the same
    /// second, reads new ticks and reports the finish.
    #[test]
    fn a_pid_reused_with_new_ticks_reports_a_finish() {
        let mut s = ProcessSensor::new();
        assert!(poll_with_ticks(&mut s, T0, 4242).is_empty(), "baseline");
        assert!(poll_with_ticks(&mut s, T0, 4242).is_empty(), "the same run");
        assert_eq!(poll_with_ticks(&mut s, T0, 4290), vec!["cargo finished"]);
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
        assert_eq!(ps_rows(ps_output(0, &format!("  10  1 {T0} cargo\n")), Columns::Lstart), Some(procs(&[(10, 1, "cargo")])));
        assert_eq!(ps_rows(ps_output(0, ""), Columns::Lstart), None, "zero rows");
        assert_eq!(ps_rows(ps_output(0, "garbage\n"), Columns::Lstart), None, "no row parsed");
        assert_eq!(ps_rows(ps_output(1, &format!("  10  1 {T0} cargo\n")), Columns::Lstart), None, "non-zero exit");
        assert_eq!(ps_rows(Err(std::io::Error::other("no ps")), Columns::Lstart), None, "did not start");
    }

    /// A poll whose `ps` read failed reports nothing, and the next good
    /// poll carries on from the state before it.
    #[test]
    fn a_failed_read_skips_the_poll() {
        let focus = no_focus();
        let idle = procs(&[ZSH, NVIM]);
        let running = procs(&[ZSH, NVIM, (10, 1, "npm"), (11, 10, "node")]);
        let mut s = ProcessSensor::new();
        assert!(s.observe(Some(idle), no_ticks, &focus, 0).is_empty(), "baseline");
        assert!(s.observe(Some(running.clone()), no_ticks, &focus, 0).is_empty());
        assert_eq!(texts(&s.observe(Some(running.clone()), no_ticks, &focus, 0)), vec!["node started"]);
        assert!(s.observe(None, no_ticks, &focus, 0).is_empty(), "the failed read");
        assert!(s.observe(Some(running), no_ticks, &focus, 0).is_empty(), "nothing restarted");
    }

    /// A sensor whose `ps` failed `failures` times in a row while npm was
    /// running, and then reads a table where npm is gone and docker runs.
    fn recover_after(failures: u32) -> Vec<String> {
        let focus = no_focus();
        let running = procs(&[ZSH, NVIM, (10, 1, "npm"), (11, 10, "node")]);
        let mut s = ProcessSensor::new();
        s.observe(Some(procs(&[ZSH, NVIM])), no_ticks, &focus, 0);
        s.observe(Some(running.clone()), no_ticks, &focus, 0);
        s.observe(Some(running), no_ticks, &focus, 0);
        for _ in 0..failures {
            assert!(s.observe(None, no_ticks, &focus, 0).is_empty());
        }
        let mut after = texts(&s.observe(Some(procs(&[ZSH, NVIM, (20, 1, "docker")])), no_ticks, &focus, 0));
        after.extend(texts(&s.observe(Some(procs(&[ZSH, NVIM, (20, 1, "docker")])), no_ticks, &focus, 0)));
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

    /// A start-ticks reader for a host with none, as outside Linux.
    fn no_ticks(_pid: u32) -> Option<u64> {
        None
    }

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

    /// A synthetic process table in the shape of
    /// `ps -u $USER -o pid=,ppid=,lstart=,comm=` on a desktop host: eight
    /// MCP servers launched with `npx` under `claude`, some with their
    /// `node-MainThread`, `ssh` beside `ssh-agent`, `cargo` under
    /// `cargo-watch`, `clang-format`, and an app whose `comm` has a space.
    /// The names, pids and times are invented.
    const HOST: &str = include_str!("../fixtures/ps-host.txt");

    #[test]
    fn a_host_table_names_each_process_as_itself() {
        let focus = no_focus();
        let s = ProcessSensor::new();
        let rows = parse_ps(HOST, Columns::Lstart);
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
        let t = tally(&parse_ps(LIVE_NPM, Columns::Lstart), |_, c| c != "bash", |c| s.is_watched(c));
        assert_eq!(t.apps, HashMap::from([("node".to_string(), 3), ("npx".to_string(), 1)]));
        assert_eq!(t.tools, HashMap::from([(3181672, tool("npm", &["node"])), (3181673, tool("npm", &["node"]))]));
    }

    #[test]
    fn a_live_npm_test_finishes_naming_node() {
        let s = ProcessSensor::new();
        let poll = |rows: &str| tally(&parse_ps(rows, Columns::Lstart), |_, c| c != "bash", |c| s.is_watched(c));
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
