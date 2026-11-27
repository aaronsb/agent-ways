// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The judge step the tool-call lanes share (#1287).
//!
//! Each lane runs the same three stages on its candidates. The matcher has
//! already picked them. The read-only refire check ([`gate::refire_open`])
//! marks which would be shown now. The judge decides on those, against the
//! calling agent's recent context, before any fire is recorded; the lane
//! then shows what the judge did not block, and the fire is recorded only
//! for what was shown. A candidate the judge blocks leaves no refire state.
//!
//! In enforce mode the judge runs on the hot path, within the hook's budget.
//! In shadow mode nothing waits on it: the lane shows every candidate and
//! hands them, with what became of each, to a detached `ways judge detached`
//! process, which judges them and writes the verdicts to the decision log.
//! A share of the candidates the refire curve held (`gate.held_sample`) goes
//! to the same detached judge in either mode, logged as `held_refire`, so the
//! judge's accuracy on what refire holds can be measured. Nothing is shown
//! from a detached judgement.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use ways_agent_core::judge::{Role, Turn};
use ways_agent_core::profile::Mode;

use super::decision;
use super::gate::{self, Gate, Pending, Trigger};

/// A lane's candidate as the judge step reads it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Candidate {
    /// What the lane looks the verdict up by: the way id, or `check:<id>`
    /// for a check, whose id is its way's.
    pub key: String,
    pub description: String,
    pub trigger: Trigger,
    /// The channel the record names.
    pub channel: String,
    /// The matcher's probability, for a scored hit.
    pub p: Option<f64>,
    /// The read-only refire check passed: the lane would show it now.
    pub open: bool,
}

/// Where a lane runs, as its log lines and records name it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Lane {
    pub surface: decision::Surface,
    pub hook_event: String,
    pub scope: String,
    pub project_dir: String,
    pub session_id: String,
    /// The command lane's call as [`audit_command`] cuts it, which every
    /// judge line and decision outcome of the scan carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

impl Lane {
    fn context(&self, epoch: u64) -> decision::Context<'_> {
        decision::Context {
            surface: self.surface,
            hook_event: &self.hook_event,
            scope: &self.scope,
            project: &self.project_dir,
            session: &self.session_id,
            epoch,
            turn_start: false,
        }
    }

    fn log<'a>(&'a self, sink: gate::EventSink<'a>) -> gate::LogContext<'a> {
        gate::LogContext {
            lane: self.surface.as_str(),
            session_id: &self.session_id,
            project_dir: &self.project_dir,
            scope: &self.scope,
            hook_event: &self.hook_event,
            sink,
            command: self.command.as_deref(),
        }
    }
}

/// How the gate is set for this call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Config {
    /// No engine and no key, `gate.mode: off`, or an agent.yaml the gate
    /// cannot read.
    Off,
    /// Judge on the hot path.
    Enforce { held_sample: f64 },
    /// Show everything; judge off the hot path.
    Shadow { held_sample: f64 },
}

/// The gate's setting as agent.yaml and the key files give it.
pub(super) fn configured() -> Config {
    use ways_agent_core::engine::{read_gate, GateReading};
    match read_gate(&ways_agent_core::profile::user_layer_path(), |p| ways_agent_core::keys::locate_file(p).is_some(), false) {
        Ok(GateReading::Judges { user, settings }) => {
            let held_sample = user.held_sample();
            match settings.mode {
                Mode::Shadow => Config::Shadow { held_sample },
                Mode::Off => Config::Off,
                Mode::Enforce => Config::Enforce { held_sample },
            }
        }
        // A broken agent.yaml turns the gate off, so the lane fails open:
        // the matcher's decision stands and nothing goes to a provider. The
        // prompt lane logs why as `gate_fallback`; a tool lane, on every tool
        // call, stays as quiet as it was before it judged.
        Ok(GateReading::Off | GateReading::NoKey) | Err(_) => Config::Off,
    }
}

/// One scan's view of the gate, read once: the setting, and the token
/// position the refire check reads, read only while the gate is on. With the
/// gate off a lane does what it did before it judged: no refire check, no
/// context read, no decision record.
pub(super) struct Scan {
    pub config: Config,
    session_id: String,
    tick: Option<u64>,
}

impl Scan {
    pub(super) fn begin(session_id: &str) -> Scan {
        Scan::with(configured(), session_id)
    }

    fn with(config: Config, session_id: &str) -> Scan {
        let tick = (config != Config::Off).then(|| crate::session::get_token_position(session_id));
        Scan { config, session_id: session_id.to_string(), tick }
    }

    pub(super) fn is_on(&self) -> bool {
        self.tick.is_some()
    }

    /// The read-only refire check, at the scan's one token position. With
    /// the gate off every candidate reads open, and show decides alone.
    pub(super) fn open(&self, id: &str) -> bool {
        match self.tick {
            Some(t) => gate::refire_open(id, &self.session_id, t),
            None => true,
        }
    }
}

/// The judge step's result on one scan: what the gate decided, and what goes
/// to the detached judge once the lane has shown its ways.
pub(super) struct Judged {
    pub gate: Gate,
    pub turns: Vec<Turn>,
    /// Shadow mode: the open candidates go to the detached judge.
    detach_open: bool,
    /// The held candidates sampled for the detached judge.
    held: Vec<Candidate>,
    /// The session judges on the claude-code engine, read once per scan
    /// when anything would go to the detached judge: the record's judge
    /// status and whether a detached judge starts both follow it.
    unhosted: bool,
}

impl Judged {
    /// Whether the gate blocked the candidate `key`.
    pub(super) fn blocked(&self, key: &str) -> bool {
        self.gate.blocked.contains(key)
    }
}

/// The judge step on `candidates`, judged against `turns` within `wait`. A
/// call with no context at all (a state lane at session start) has nothing
/// to judge relevance against, and passes as with the gate off.
pub(super) fn judge(lane: &Lane, scan: &Scan, candidates: &[Candidate], turns: Vec<Turn>, wait: Duration) -> Judged {
    let sink = &crate::session::log_event;
    let log = lane.log(sink);
    let config = if turns.is_empty() || candidates.is_empty() { Config::Off } else { scan.config };
    judge_with(config, candidates, turns, || gate::unhosted_engine(&lane.session_id), |pending, turns| gate::apply(pending, turns, wait, &log))
}

/// The three stages on one lane's `candidates`, in the lane's order: the
/// judge on the ones the refire check left open, then `show` for each the
/// judge did not block, which records the fire and says what became of it.
/// The decision record and the detached judge follow ([`finish`]). With the
/// gate off, `show` runs for every candidate and nothing else happens;
/// `turns` is read only when the gate is on.
pub(super) fn run(
    lane: &Lane,
    scan: &Scan,
    candidates: &[Candidate],
    turns: impl FnOnce() -> Vec<Turn>,
    wait: Duration,
    mut show: impl FnMut(&Candidate) -> &'static str,
) {
    let Some(tick) = scan.tick else {
        for c in candidates {
            show(c);
        }
        return;
    };
    let judged = judge(lane, scan, candidates, turns(), wait);
    let shown: Vec<Shown<'_>> = candidates
        .iter()
        .map(|c| Shown { candidate: c, result: decision::block_result(&judged.gate.blocked, &c.key).unwrap_or_else(|| show(c)) })
        .collect();
    finish(lane, judged, &shown, tick);
}

/// The post-tool lane: the ways whose postchecks asked to fire on a
/// `tool` call, judged, then shown through `show`.
pub(crate) fn post_tool(session_id: &str, project_dir: &str, hook_event: &str, tool: &str, ids: Vec<String>, show: impl FnMut(&str) -> &'static str) {
    let mut show = show;
    if ids.is_empty() {
        return;
    }
    let scan = Scan::begin(session_id);
    // Descriptions are only read for the judge.
    let ways = if scan.is_on() { super::candidates::collect_candidates(project_dir) } else { Vec::new() };
    let description = |id: &str| ways.iter().find(|w| w.id == id).map(|w| w.description.clone()).unwrap_or_default();
    let candidates: Vec<Candidate> = ids
        .iter()
        .map(|id| Candidate {
            key: id.clone(),
            description: description(id),
            trigger: Trigger::Declared,
            channel: "postcheck".into(),
            p: None,
            open: scan.open(id),
        })
        .collect();
    let lane = Lane {
        surface: decision::Surface::PostTool,
        hook_event: hook_event.to_string(),
        scope: crate::session::detect_scope(session_id),
        project_dir: project_dir.to_string(),
        session_id: session_id.to_string(),
        command: None,
    };
    let call = format!("Used the {tool} tool.");
    run(&lane, &scan, &candidates, || tool_turns(session_id, &call), gate::TOOL_WAIT, |c| show(&c.key));
}

/// [`judge`] with the setting, the engine check and the gate given, so a
/// test can stand in a judge. `unhosted` is asked once, and only when
/// something would go to the detached judge: an open candidate in shadow
/// mode, or a sampled held one, read after the judge in enforce mode. Under
/// claude-code no detached judge will start; the record says so, and
/// [`detach`] follows the same reading.
pub(super) fn judge_with(
    config: Config,
    candidates: &[Candidate],
    turns: Vec<Turn>,
    unhosted: impl FnOnce() -> bool,
    apply: impl FnOnce(&[Pending<'_>], Vec<Turn>) -> Gate,
) -> Judged {
    let rate = match config {
        Config::Off => return Judged { gate: Gate::default(), turns, detach_open: false, held: Vec::new(), unhosted: false },
        Config::Shadow { held_sample } | Config::Enforce { held_sample } => held_sample,
    };
    let held: Vec<Candidate> = candidates.iter().filter(|c| !c.open && sampled(rate, &c.key)).cloned().collect();
    let (gate, detach_open, unhosted) = match config {
        Config::Off => unreachable!("returned above"),
        Config::Shadow { .. } => {
            let any_open = candidates.iter().any(|c| c.open);
            let unhosted = (any_open || !held.is_empty()) && unhosted();
            let status = match any_open {
                false => gate::Status::Idle,
                true if unhosted => gate::Status::DetachSkipped,
                true => gate::Status::Detached,
            };
            (Gate { status, ..Gate::default() }, any_open, unhosted)
        }
        Config::Enforce { .. } => {
            let pending: Vec<Pending<'_>> = candidates
                .iter()
                .filter(|c| c.open)
                .map(|c| Pending { id: &c.key, description: &c.description, trigger: c.trigger })
                .collect();
            let gate = if pending.is_empty() { Gate { status: gate::Status::Idle, ..Gate::default() } } else { apply(&pending, turns.clone()) };
            // Read once the judge ran, which resolves the session's engine.
            let unhosted = !held.is_empty() && unhosted();
            (gate, false, unhosted)
        }
    };
    Judged { gate, turns, detach_open, held, unhosted }
}

/// Whether `key` falls in a sample at `rate`: drawn from the clock, so each
/// call draws anew.
fn sampled(rate: f64, key: &str) -> bool {
    use std::hash::{Hash, Hasher};
    if rate <= 0.0 {
        return false;
    }
    if rate >= 1.0 {
        return true;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos().hash(&mut h);
    (h.finish() % 1_000_000) as f64 / 1_000_000.0 < rate
}

/// What became of one candidate in the lane.
pub(super) struct Shown<'a> {
    pub candidate: &'a Candidate,
    pub result: &'a str,
}

/// Ends the judge step once the lane has shown its ways: writes the scan's
/// decision record when it had a candidate, then [`detach`]es.
fn finish(lane: &Lane, judged: Judged, shown: &[Shown<'_>], tick: u64) {
    if shown.is_empty() {
        return;
    }
    let epoch = crate::session::get_epoch(&lane.session_id);
    let record = lane_record(decision::Record::begin_at(&lane.context(epoch), tick), lane, &judged, shown);
    let scan_id = record.scan_id().to_string();
    record.write();
    detach(lane, judged, shown, &scan_id);
}

/// `record` with each shown candidate's outcome, the command lane's call on
/// every outcome, and the judge's block.
fn lane_record(mut record: decision::Record, lane: &Lane, judged: &Judged, shown: &[Shown<'_>]) -> decision::Record {
    for (i, s) in shown.iter().enumerate() {
        record.hit(&s.candidate.key, i + 1, &s.candidate.channel, s.candidate.p, s.result, Some(&judged.gate));
    }
    if let Some(c) = &lane.command {
        record.command(c);
    }
    close_judge(&mut record, &judged.gate, &judged.turns);
    record
}

/// Writes the judge's block on a lane's record, the hook's own or the
/// detached judge's: its status, and whether its turns carried the calling
/// agent's request.
pub(super) fn close_judge(record: &mut decision::Record, gate: &Gate, turns: &[Turn]) {
    record.judge(gate);
    record.request(turns.iter().any(|t| t.role == Role::User));
}

/// Hands what the detached judge should see, in shadow mode the shown
/// candidates and in either mode the sampled held ones, to a `ways judge
/// detached` process under `scan_id`, which nothing waits on.
pub(super) fn detach(lane: &Lane, judged: Judged, shown: &[Shown<'_>], scan_id: &str) {
    let session = lane.session_id.clone();
    detach_via(
        lane,
        judged,
        shown,
        scan_id,
        || crate::session::first_mark(&session, "judge-detach-skipped"),
        &crate::session::log_event,
        start_detached,
    );
}

/// [`detach`] with the once-per-session marker, the log and the start
/// injected, so a test can see what would start. Whether one may start is
/// the engine `judged` was read under.
#[allow(clippy::too_many_arguments)]
fn detach_via(
    lane: &Lane,
    judged: Judged,
    shown: &[Shown<'_>],
    scan_id: &str,
    first_skip: impl FnOnce() -> bool,
    sink: gate::EventSink<'_>,
    start: impl FnOnce(&Lane, Job, gate::EventSink<'_>),
) {
    let mut entries: Vec<Entry> = Vec::new();
    if judged.detach_open {
        entries.extend(shown.iter().filter(|s| s.candidate.open).map(|s| Entry { candidate: s.candidate.clone(), result: s.result.to_string() }));
    }
    entries.extend(judged.held.into_iter().map(|candidate| Entry { candidate, result: "held_refire".into() }));
    if entries.is_empty() {
        return;
    }
    // Under claude-code a detached job could be claimed only by a later
    // event's take, and a take claims the oldest job first, so it would
    // queue ahead of a hot-path job and push that one past its claim window.
    // Nothing starts, and the session's log says so once.
    if judged.unhosted {
        if first_skip() {
            let log = lane.log(sink);
            (log.sink)(&[
                ("event", "judge_detach_skipped"),
                ("reason", DETACH_UNHOSTED),
                ("candidates", &entries.len().to_string()),
                ("lane", log.lane),
                ("session", log.session_id),
                ("agent_id", &crate::session::current_agent()),
            ]);
        }
        return;
    }
    let job = Job { lane: lane.clone(), scan_id: scan_id.to_string(), agent: crate::session::current_agent(), turns: judged.turns, entries, slot: Default::default() };
    start(lane, job, sink);
}

/// Why no detached judge starts under the claude-code engine, as
/// `judge_detach_skipped` logs it.
const DETACH_UNHOSTED: &str = "claude-code engine has no host outside a hook";

/// Claims a slot and starts `ways judge detached` on `job`.
fn start_detached(lane: &Lane, mut job: Job, sink: gate::EventSink<'_>) {
    let log = lane.log(sink);
    let Some(slot) = claim_slot(&slots_dir(), DETACHED_MAX) else {
        (log.sink)(&[
            ("event", "judge_detach_dropped"),
            ("reason", "cap"),
            ("cap", &DETACHED_MAX.to_string()),
            ("candidates", &job.entries.len().to_string()),
            ("lane", log.lane),
            ("session", log.session_id),
            ("agent_id", &crate::session::current_agent()),
        ]);
        return;
    };
    job.slot = slot.clone();
    let Ok(text) = serde_json::to_string(&job) else { return };
    let exe = match running_binary() {
        Ok(exe) => exe,
        Err(e) => {
            let _ = std::fs::remove_file(&slot);
            (log.sink)(&[("event", "judge_detach_failed"), ("reason", &format!("current_exe: {e}")), ("lane", log.lane), ("session", log.session_id)]);
            return;
        }
    };
    let _ = spawn_at(&exe, &text, &slot, log.sink);
}

/// The most detached judges running at once, across sessions. A job past it
/// is dropped and logged as `judge_detach_dropped`.
const DETACHED_MAX: usize = 4;

/// How old a slot file may be before it is taken to belong to a judge that
/// died: the detached judge's own deadline, with room.
const SLOT_STALE: Duration = Duration::from_secs(120);

/// Where the slot files live: one per detached judge in flight.
fn slots_dir() -> std::path::PathBuf {
    std::path::Path::new(&crate::session::sessions_root()).join("judge-detached")
}

/// Claims one of `cap` slot files in `dir`, created exclusively so two hooks
/// never take the same one, and reclaims a stale one. `None` when all are
/// held.
fn claim_slot(dir: &std::path::Path, cap: usize) -> Option<std::path::PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let create = |path: &std::path::Path| std::fs::OpenOptions::new().write(true).create_new(true).open(path).is_ok();
    for i in 0..cap {
        let path = dir.join(format!("slot-{i}"));
        if create(&path) || (is_stale(&path) && reclaim(&path) && create(&path)) {
            return Some(path);
        }
    }
    None
}

/// Whether the slot file at `path` is older than [`SLOT_STALE`].
fn is_stale(path: &std::path::Path) -> bool {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > SLOT_STALE)
}

/// Removes the slot file at `path` if it is still stale, under a lock on
/// the slot directory so that hooks which found it stale at once take it
/// back once: the first removes it and creates the slot again, and the
/// others then find it fresh. A hook creating a free slot takes no lock,
/// since only a reclaim removes a slot another process holds. A hook that
/// cannot take the lock within [`RECLAIM_WAIT`] leaves the slot.
fn reclaim(path: &std::path::Path) -> bool {
    let Some(dir) = path.parent() else { return false };
    let Ok(Some(_lock)) = agent_settings::writer::Lock::acquire_within(&dir.join("reclaim"), RECLAIM_WAIT) else { return false };
    is_stale(path) && std::fs::remove_file(path).is_ok()
}

/// The longest a hook waits for another hook's reclaim of a stale slot.
const RECLAIM_WAIT: Duration = Duration::from_millis(50);

/// The binary this process runs, for starting the detached judge. After an
/// update replaces it, Linux names the old one `… (deleted)`; the path
/// without that suffix is the new one.
fn running_binary() -> std::io::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    if exe.is_file() {
        return Ok(exe);
    }
    let text = exe.to_string_lossy();
    match text.strip_suffix(" (deleted)") {
        Some(live) if std::path::Path::new(live).is_file() => Ok(live.into()),
        _ => Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{} is gone", exe.display()))),
    }
}

/// One candidate handed to the detached judge, with what the lane did with it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    candidate: Candidate,
    result: String,
}

/// What a detached judge reads on its stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Job {
    lane: Lane,
    scan_id: String,
    agent: String,
    turns: Vec<Turn>,
    entries: Vec<Entry>,
    /// The slot file this judge holds, removed when it is done.
    slot: std::path::PathBuf,
}

/// Removes a detached judge's slot file however its run ends.
struct SlotGuard(std::path::PathBuf);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Starts `exe judge detached` with `text` on its stdin, in a session of its
/// own so the hook's end does not take it down, and returns once the job is
/// written or the child stops reading. The write runs on its own thread
/// through `agent_fmt::write_to_child`, so a child that exits without
/// reading ends it with an error, never with SIGPIPE, which the binary
/// restores to its default. A failure is logged as `judge_detach_failed`.
/// Once the child is spawned the slot is the child's, which gives it back
/// when it is done; a hook gives it back only when the spawn failed or the
/// child shut its stdin unread, since a child that never read its job never
/// holds the slot. A write that is still going at the bound leaves the slot
/// with the child, which may yet read the job; if it never does, the slot
/// goes stale and is reclaimed.
fn spawn_at(exe: &std::path::Path, text: &str, slot: &std::path::Path, sink: gate::EventSink<'_>) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.args(["judge", "detached"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid in the child before exec only detaches it from the
        // hook's session; it allocates nothing.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let log = |reason: &str| sink(&[("event", "judge_detach_failed"), ("reason", reason)]);
    let failed = |reason: String| {
        let _ = std::fs::remove_file(slot);
        log(&reason);
    };
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            failed(format!("spawn {}: {e}", exe.display()));
            return Err(e);
        }
    };
    let Some(mut stdin) = child.stdin.take() else { return Ok(()) };
    let bytes = text.as_bytes().to_vec();
    let (done, wrote) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(agent_fmt::write_to_child(&mut stdin, &bytes));
    });
    // The child reads its whole job before anything else, so this returns
    // at once; a child that never reads is not waited on past the bound.
    match wrote.recv_timeout(JOB_WRITE) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            failed(format!("write: {e}"));
            Err(e)
        }
        Err(_) => {
            log("write: timed out");
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "job write"))
        }
    }
}

/// The longest a hook waits to hand the detached judge its job.
const JOB_WRITE: Duration = Duration::from_millis(500);

/// The longest a detached judge waits: it holds no hook, so the profile's
/// whole deadline applies.
const DETACHED_WAIT: Duration = Duration::from_secs(30);

/// `ways judge detached`: judges the job on stdin and writes its record. In
/// shadow mode the verdicts read `would_block`; a held candidate's verdict
/// is whatever the mode makes of it, and nothing is shown either way.
pub(crate) fn run_detached(text: &str) -> anyhow::Result<()> {
    let job: Job = serde_json::from_str(text)?;
    let _slot = SlotGuard(job.slot.clone());
    if job.agent != crate::session::MAIN_AGENT {
        std::env::set_var("CLAUDE_AGENT_ID", &job.agent);
    }
    std::env::set_var("CLAUDE_PROJECT_DIR", &job.lane.project_dir);
    // Every line this judge writes is marked detached, and a held
    // candidate's lines held_refire, so a reader never takes them for the
    // hook's own gate decisions (#1287).
    let held: std::collections::HashSet<&str> =
        job.entries.iter().filter(|e| e.result == "held_refire").map(|e| e.candidate.key.as_str()).collect();
    let marked = |fields: &[(&str, &str)]| {
        let mut line: Vec<(&str, &str)> = fields.to_vec();
        line.push(("detached", "true"));
        if fields.iter().any(|(k, v)| *k == "way" && held.contains(v)) {
            line.push(("held_refire", "true"));
        }
        crate::session::log_event(&line);
    };
    let log = job.lane.log(&marked);
    let pending: Vec<Pending<'_>> = job
        .entries
        .iter()
        .map(|e| Pending { id: &e.candidate.key, description: &e.candidate.description, trigger: e.candidate.trigger })
        .collect();
    let gate = gate::apply(&pending, job.turns.clone(), DETACHED_WAIT, &log);
    let epoch = crate::session::get_epoch(&job.lane.session_id);
    detached_record(&job, &gate, epoch).write();
    Ok(())
}

/// The detached judge's record of `job` under `gate`, marked detached and
/// keyed by the hook's `scan_id`.
fn detached_record(job: &Job, gate: &Gate, epoch: u64) -> decision::Record {
    let mut record = decision::Record::detached(&job.lane.context(epoch), &job.scan_id);
    for (i, e) in job.entries.iter().enumerate() {
        record.hit(&e.candidate.key, i + 1, &e.candidate.channel, None, &e.result, Some(gate));
    }
    if let Some(c) = &job.lane.command {
        record.command(c);
    }
    close_judge(&mut record, gate, &job.turns);
    record
}

/// What a tool lane's judge reads: the calling agent's request, its last
/// human turn or its brief, then the call in hand as the agent's turn, the
/// one a profile of `turns: 1` reads. The agent's narration is not sent.
///
/// References:
/// - ADR-712 §4: a tool lane's judge input is the request and the call.
pub(super) fn tool_turns(session_id: &str, call: &str) -> Vec<Turn> {
    with_call(super::lookbehind::recent_turns(session_id), call)
}

fn with_call(mut turns: Vec<Turn>, call: &str) -> Vec<Turn> {
    turns.push(Turn { role: Role::Assistant, text: call.to_string() });
    turns
}

/// What fired a prompt-shaped hit (the prompt and task lanes), from its
/// channel and the way's `pattern_strict`.
pub(super) fn matched_trigger(channel: &str, pattern_strict: bool) -> Trigger {
    if channel.starts_with("semantic") {
        Trigger::Semantic
    } else if pattern_strict {
        Trigger::Strict
    } else {
        Trigger::Keyword
    }
}

/// Hands a sample of the candidates the refire curve held on a lane that
/// judges on its own path (the prompt lane) to the detached judge, under
/// that scan's `scan_id`.
pub(super) fn detach_held(lane: &Lane, held: Vec<Candidate>, turns: Vec<Turn>, scan_id: &str) {
    if held.is_empty() {
        return;
    }
    let rate = match configured() {
        Config::Off => return,
        Config::Enforce { held_sample } | Config::Shadow { held_sample } => held_sample,
    };
    let held: Vec<Candidate> = held.into_iter().filter(|c| sampled(rate, &c.key)).collect();
    let unhosted = !held.is_empty() && gate::unhosted_engine(&lane.session_id);
    let judged = Judged { gate: Gate::default(), turns, detach_open: false, held, unhosted };
    detach(lane, judged, &[], scan_id);
}

/// The prefix that keeps a check's key apart from its way's: a check has its
/// way's id.
const CHECK: &str = "check:";

/// A check that matched, as the judge step reads it. A check has no refire
/// curve of its own (its decay is in `show::check_within`), so it is always
/// open; a `score` of 3.0 is a regex match.
pub(super) fn check_candidate(check: &super::WayCandidate, score: f64, channel: &str) -> Candidate {
    let regex = score >= 3.0;
    Candidate {
        key: format!("{CHECK}{}", check.id),
        description: check.description.clone(),
        trigger: if regex { Trigger::Declared } else { Trigger::Semantic },
        channel: channel.to_string(),
        p: (!regex).then_some(score),
        open: true,
    }
}

/// The check id a [`check_candidate`] key names, or `None` for a way's.
pub(super) fn check_of(key: &str) -> Option<&str> {
    key.strip_prefix(CHECK)
}

/// A declared trigger's candidate: a way fired by `files:`, `tool_name:` or a
/// state trigger, marked by the scan's read-only refire check.
pub(super) fn declared(way: &super::WayCandidate, channel: &str, scan: &Scan) -> Candidate {
    Candidate {
        key: way.id.clone(),
        description: way.description.clone(),
        trigger: Trigger::Declared,
        channel: channel.to_string(),
        p: None,
        open: scan.open(&way.id),
    }
}

/// The Bash call as the judge reads it: the described block, an `About to
/// run:` line with the tool's description and then one line per unit of
/// the call, each segment with what its tldr page says it does, then the
/// command itself, cut to [`MAX_COMMAND_CHARS`] so a long heredoc does not
/// push the rest out of the profile's `max_turn_chars`. `segments` is the
/// call's parse and `summaries` the tldr data; with none loaded, each
/// segment reads as its program and subcommand alone.
///
/// References:
/// - ADR-188 item 3c: the judge reads the ordered described block.
/// - ADR-707: the block measured, and the judge reading it.
pub(super) fn bash_call(command: &str, description: Option<&str>, segments: &[command_describer::Segment], summaries: &command_describer::Summaries) -> String {
    let block = command_describer::described_block(description.unwrap_or(""), segments, summaries);
    format!("{}\n$ {}", block.trim_end(), cut(command, MAX_COMMAND_CHARS))
}

/// The first `max` characters of `text`, with `…` when it was cut.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// The most of a command the judge reads.
const MAX_COMMAND_CHARS: usize = 400;

/// The tldr summaries the judge's described block reads, from the app's
/// `catalog/commands/tldr/summaries.tsv`; none when the file is missing.
pub(super) fn tldr_summaries() -> command_describer::Summaries {
    command_describer::Summaries::load(&ways_core::paths::tldr_summaries()).unwrap_or_default()
}

/// The most of a command a judge line or decision outcome records.
const AUDIT_COMMAND_CHARS: usize = 80;

/// A command as the judge's log lines and decision outcomes record it, so a
/// misfire can be traced to the call: its first [`AUDIT_COMMAND_CHARS`]
/// characters, with the value of every shell assignment (`GH_TOKEN=…`,
/// `export KEY=…`), of every flag named for a credential (`--token`,
/// `--password`, `--api-key`, …) and of an `Authorization:` or bearer
/// header taken out first. Nothing else is changed.
pub(super) fn audit_command(command: &str) -> String {
    use std::sync::OnceLock;
    static SECRETS: OnceLock<[regex::Regex; 3]> = OnceLock::new();
    let [assignment, flag, header] = SECRETS.get_or_init(|| {
        [
            regex::Regex::new(r#"(^|[\s;&|(`])([A-Za-z_][A-Za-z0-9_]*=)("[^"]*"|'[^']*'|[^\s;&|)`]*)"#).expect("static regex"),
            regex::Regex::new(r#"(?i)(--?(?:[a-z0-9]+[_-])*(?:token|secret|password|passwd|api[_-]?key|auth|credentials?)(?:[_-][a-z0-9_-]*)?)(=|\s+)("[^"]*"|'[^']*'|[^\s;&|)`-][^\s;&|)`]*)"#)
                .expect("static regex"),
            regex::Regex::new(r#"(?i)((?:authorization|x-api-key|api-key)\s*:\s*(?:(?:bearer|basic|token)\s+)?|bearer\s+)([^\s"';&|)`]+)"#)
                .expect("static regex"),
        ]
    });
    let text = assignment.replace_all(command, "$1$2…");
    let text = flag.replace_all(&text, "$1$2…");
    let text = header.replace_all(&text, "$1…");
    match text.char_indices().nth(AUDIT_COMMAND_CHARS) {
        Some((at, _)) => text[..at].to_string(),
        None => text.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ways_agent_core::profile::Mode;

    fn candidate(key: &str, trigger: Trigger, open: bool) -> Candidate {
        Candidate { key: key.into(), description: format!("about {key}"), trigger, channel: "bash".into(), p: None, open }
    }

    fn sink() -> impl Fn(&[(&str, &str)]) {
        |_: &[(&str, &str)]| {}
    }

    /// A semantic candidate on the Bash lane reaches the judge with the
    /// declared ones, and an enforced block is reported for it.
    #[test]
    fn enforce_sends_every_open_candidate_and_reports_the_block() {
        let cands = [
            candidate("sem/way", Trigger::Semantic, true),
            candidate("cmd/way", Trigger::Declared, true),
            candidate("held/way", Trigger::Semantic, false),
        ];
        let mut sent = Vec::new();
        let s = sink();
        let lc = gate::LogContext { lane: "command", session_id: "s", project_dir: "/p", scope: "agent", hook_event: "PreToolUse", sink: &s, command: None };
        let j = judge_with(Config::Enforce { held_sample: 0.0 }, &cands, vec![], || false, |pending, _| {
            sent = pending.iter().map(|p| p.id.to_string()).collect();
            gate::test_gate(&[("sem/way", 0.05), ("cmd/way", 0.9)], Mode::Enforce, &lc)
        });
        assert_eq!(sent, ["sem/way", "cmd/way"], "the held one is not sent");
        assert!(j.blocked("sem/way") && !j.blocked("cmd/way"));
        assert!(j.held.is_empty(), "sampling at 0 holds nothing back for the record");
    }

    /// Shadow mode blocks nothing and calls no judge on the hot path.
    #[test]
    fn shadow_blocks_nothing_and_hands_the_open_candidates_on() {
        let cands = [candidate("a", Trigger::Semantic, true), candidate("b", Trigger::Declared, false)];
        let j = judge_with(Config::Shadow { held_sample: 1.0 }, &cands, vec![], || false, |_, _| panic!("no judge on the hot path"));
        assert!(!j.blocked("a"));
        assert!(j.detach_open && j.gate.status == gate::Status::Detached);
        assert_eq!(j.held.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["b"], "every held one at rate 1");
    }

    /// Under claude-code no host runs outside a hook, so a held sample and a
    /// shadow call start no detached judge, and the skip is logged once per
    /// session. A key engine still starts one.
    #[test]
    fn under_claude_code_no_detached_judge_starts_and_the_skip_is_logged_once() {
        let lane = Lane {
            surface: decision::Surface::Command,
            hook_event: "PreToolUse".into(),
            scope: "agent".into(),
            project_dir: "/p".into(),
            session_id: "s".into(),
            command: None,
        };
        let held = [candidate("held/way", Trigger::Semantic, false)];
        let open = [candidate("open/way", Trigger::Declared, true)];
        let run = |unhosted: bool| {
            let started = std::cell::RefCell::new(Vec::<usize>::new());
            let events = std::cell::RefCell::new(Vec::<Vec<(String, String)>>::new());
            let first = std::cell::Cell::new(true);
            let sink = |f: &[(&str, &str)]| events.borrow_mut().push(f.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
            let held_call = judge_with(Config::Enforce { held_sample: 1.0 }, &held, vec![], || unhosted, |_, _| panic!("nothing open to judge"));
            let shadow_call = judge_with(Config::Shadow { held_sample: 0.0 }, &open, vec![], || unhosted, |_, _| panic!("no judge on the hot path"));
            let shown = [Shown { candidate: &open[0], result: "shown" }];
            for (j, s) in [(held_call, &[][..]), (shadow_call, &shown[..])] {
                detach_via(&lane, j, s, "scan", || first.replace(false), &sink, |_, job, _| {
                    started.borrow_mut().push(job.entries.len())
                });
            }
            let skips: Vec<String> = events
                .borrow()
                .iter()
                .filter(|e| e.iter().any(|(k, v)| k == "event" && v == "judge_detach_skipped"))
                .map(|e| e.iter().find(|(k, _)| k == "reason").map(|(_, v)| v.clone()).unwrap_or_default())
                .collect();
            (started.into_inner(), skips)
        };
        let (started, skips) = run(true);
        assert!(started.is_empty(), "no detached judge starts under claude-code: {started:?}");
        assert_eq!(skips, ["claude-code engine has no host outside a hook"], "logged once for the session");
        // The control: a key engine starts both.
        let (started, skips) = run(false);
        assert_eq!(started, [1, 1]);
        assert!(skips.is_empty());
    }

    /// Shadow mode under claude-code shows every candidate and starts no
    /// detached judge, so the record's judge status says no verdicts follow:
    /// not `detached`, not judged, not a fallback.
    #[test]
    fn shadow_under_claude_code_records_detach_skipped() {
        let cands = [candidate("a", Trigger::Semantic, true)];
        let j = judge_with(Config::Shadow { held_sample: 0.0 }, &cands, vec![], || true, |_, _| panic!("no judge on the hot path"));
        assert!(!j.blocked("a"));
        assert_eq!(j.gate.status, gate::Status::DetachSkipped);
        // With nothing open there is nothing to skip, and the engine is not asked.
        let held = [candidate("h", Trigger::Semantic, false)];
        let j = judge_with(Config::Shadow { held_sample: 0.0 }, &held, vec![], || panic!("not asked"), |_, _| panic!("no judge"));
        assert_eq!(j.gate.status, gate::Status::Idle);
    }

    /// The engine is read once per scan: a record that says no detached
    /// judge follows starts none, even when a second reading would differ.
    #[test]
    fn the_detach_follows_the_engine_the_record_was_written_under() {
        let lane = Lane {
            surface: decision::Surface::Command,
            hook_event: "PreToolUse".into(),
            scope: "agent".into(),
            project_dir: "/p".into(),
            session_id: "s".into(),
            command: None,
        };
        let open = [candidate("open/way", Trigger::Declared, true)];
        let shown = [Shown { candidate: &open[0], result: "shown" }];
        let readings = std::cell::Cell::new(0);
        let read = || {
            readings.set(readings.get() + 1);
            readings.get() == 1
        };
        let j = judge_with(Config::Shadow { held_sample: 0.0 }, &open, vec![], read, |_, _| panic!("no judge on the hot path"));
        assert_eq!(j.gate.status, gate::Status::DetachSkipped);
        let started = std::cell::Cell::new(0);
        detach_via(&lane, j, &shown, "scan", || false, &|_| {}, |_, _, _| started.set(started.get() + 1));
        assert_eq!(started.get(), 0, "the record says none starts");
        assert_eq!(readings.get(), 1, "the engine was read once");
    }

    /// The hook's record and the detached judge's both say whether the
    /// judge read the agent's request.
    #[test]
    fn both_records_say_whether_the_request_was_read() {
        let lane = Lane {
            surface: decision::Surface::Command,
            hook_event: "PreToolUse".into(),
            scope: "agent".into(),
            project_dir: "/p".into(),
            session_id: "s".into(),
            command: None,
        };
        let gate = Gate { status: gate::Status::Judged { engine: "e".into(), model: "m".into(), judge_ms: 1 }, ..Gate::default() };
        let call = Turn { role: Role::Assistant, text: "About to run: x".into() };
        let brief = Turn { role: Role::User, text: "brief".into() };
        for (turns, want) in [(vec![brief, call.clone()], "found"), (vec![call], "none")] {
            let mut hook = decision::Record::begin_at(&lane.context(0), 1);
            close_judge(&mut hook, &gate, &turns);
            assert_eq!(hook.to_json()["judge"]["request"], want);
            let job: Job = serde_json::from_value(serde_json::json!({
                "lane": serde_json::to_value(&lane).unwrap(),
                "scan_id": "scan",
                "agent": "main",
                "turns": serde_json::to_value(&turns).unwrap(),
                "entries": [],
                "slot": "/nonexistent",
            }))
            .unwrap();
            assert_eq!(detached_record(&job, &gate, 0).to_json()["judge"]["request"], want);
        }
    }

    #[test]
    fn off_judges_nothing() {
        let cands = [candidate("a", Trigger::Semantic, true)];
        let j = judge_with(Config::Off, &cands, vec![], || false, |_, _| panic!("off calls no judge"));
        assert_eq!(j.gate.status, gate::Status::Off);
        assert!(!j.blocked("a") && j.held.is_empty() && !j.detach_open);
    }

    fn bash(cmd: &str, description: Option<&str>, summaries: &command_describer::Summaries) -> String {
        bash_call(cmd, description, &command_describer::parse(cmd), summaries)
    }

    #[test]
    fn the_call_follows_the_request_and_no_narration_is_sent() {
        let tldr = command_describer::Summaries::parse("git-commit\tCommit files to the repository.\n");
        let call = bash("git commit -m x", Some("Commit the fix"), &tldr);
        assert_eq!(call, "About to run: Commit the fix\ngit commit (Commit files to the repository)\n$ git commit -m x");
        let turns = with_call(vec![Turn { role: Role::User, text: "brief".into() }], &call);
        assert_eq!(turns, [Turn { role: Role::User, text: "brief".into() }, Turn { role: Role::Assistant, text: call.clone() }]);
        let alone = with_call(vec![], &call);
        assert_eq!(alone, [Turn { role: Role::Assistant, text: call }]);
        let long = bash(&"x".repeat(1000), None, &tldr);
        assert!(long.chars().count() < 1450 && long.ends_with('…'), "{long}");
        assert!(long.lines().last().unwrap().chars().count() < 410, "the command line is cut: {long}");
    }

    /// The judge reads what each segment of the call does, in order, with
    /// the command and its description beside it.
    ///
    /// References:
    /// - ADR-188 item 3c: the judge reads the described block.
    #[test]
    fn the_judge_reads_the_described_block_beside_the_command() {
        let tldr = command_describer::Summaries::parse(
            "git-branch\tMain Git command for working with branches.\ngrep\tFind patterns in files using regular expressions.\n",
        );
        let call = bash("cd repo && git branch -D foo | grep x", Some("Delete the merged branch"), &tldr);
        assert_eq!(
            call,
            "About to run: Delete the merged branch\n\
             git branch (Main Git command for working with branches) | grep (Find patterns in files using regular expressions)\n\
             $ cd repo && git branch -D foo | grep x"
        );
        // With no tldr data each segment reads as its program and subcommand.
        let bare = bash("git show abc --stat", None, &command_describer::Summaries::default());
        assert_eq!(bare, "About to run: \ngit show\n$ git show abc --stat");
    }

    /// #1372: each outcome row of a command-lane record carries the call's
    /// first 80 characters, and a judged one the judge's critique when it
    /// gave one; a semantic candidate the judge never saw names why.
    #[test]
    fn a_command_outcome_row_carries_the_command_and_the_critique() {
        let cmd = format!("GH_TOKEN=ghp_x git branch -D foo && {}", "echo y; ".repeat(20));
        let lane = Lane {
            surface: decision::Surface::Command,
            hook_event: "PreToolUse".into(),
            scope: "agent".into(),
            project_dir: "/p".into(),
            session_id: "s".into(),
            command: Some(audit_command(&cmd)),
        };
        let mut gate = Gate { status: gate::Status::Judged { engine: "claude-code".into(), model: "m".into(), judge_ms: 900 }, ..Gate::default() };
        let critique = Some("the call deletes a branch; this way is about history rewrites".to_string());
        gate.judgements.insert("history/way".into(), gate::Judgement { p_yes: 0.85, verdict: "pass", critique });
        gate.judgements.insert("quiet/way".into(), gate::Judgement { p_yes: 0.9, verdict: "pass", critique: None });
        gate.unjudged.insert("late/way".into(), "overflow".into());
        let cands = [
            candidate("history/way", Trigger::Semantic, true),
            candidate("quiet/way", Trigger::Declared, true),
            candidate("late/way", Trigger::Declared, true),
        ];
        let shown: Vec<Shown<'_>> = cands.iter().map(|c| Shown { candidate: c, result: "fired" }).collect();
        let judged = Judged { gate, turns: vec![], detach_open: false, held: vec![], unhosted: false };
        let v = lane_record(decision::Record::for_test(), &lane, &judged, &shown).to_json();
        let rows = v["outcomes"].as_array().unwrap();
        let want = "GH_TOKEN=… git branch -D foo && echo y; echo y; echo y; echo y; echo y; echo y; ";
        assert_eq!(want.chars().count(), 80);
        assert!(rows.iter().all(|o| o["command"] == want), "{v:#}");
        assert_eq!(rows[0]["critique"], "the call deletes a branch; this way is about history rewrites");
        assert!(rows[1].get("critique").is_none(), "no critique given, none recorded");
        assert_eq!(rows[2]["unjudged"], "overflow");
        // A lane with no command records none.
        let quiet = Lane { command: None, ..lane };
        let v = lane_record(decision::Record::for_test(), &quiet, &judged, &shown).to_json();
        assert!(v["outcomes"].as_array().unwrap().iter().all(|o| o.get("command").is_none()));
    }

    /// A judge line and decision outcome record the call's first 80
    /// characters, with no assignment value, credential flag or auth header.
    #[test]
    fn the_audit_command_keeps_the_text_and_drops_secret_values() {
        assert_eq!(audit_command("git branch -D foo"), "git branch -D foo");
        let long = format!("cargo test -p ways {}", "x".repeat(200));
        assert_eq!(audit_command(&long).chars().count(), 80);
        assert!(audit_command(&long).starts_with("cargo test -p ways xx"));
        assert_eq!(audit_command("GH_TOKEN=ghp_abc123 gh pr create"), "GH_TOKEN=… gh pr create");
        assert_eq!(audit_command("cd x && export API_KEY='s3 cr3t' && make"), "cd x && export API_KEY=… && make");
        assert_eq!(audit_command("tool --token abc --password=hunter2 -v"), "tool --token … --password=… -v");
        assert_eq!(audit_command(r#"curl -H "Authorization: Bearer abc.def" https://x"#), r#"curl -H "Authorization: Bearer …" https://x"#);
        assert_eq!(audit_command("curl -H 'X-Api-Key: k1' u"), "curl -H 'X-Api-Key: …' u");
        assert_eq!(audit_command("git log --format=%H --author=aaron"), "git log --format=%H --author=aaron", "a plain flag value stays");
        assert_eq!(audit_command("gh auth login --with-token --github-token=t1"), "gh auth login --with-token --github-token=…");
    }

    type Lines = std::cell::RefCell<Vec<String>>;

    fn recorder(lines: &Lines) -> impl Fn(&[(&str, &str)]) + '_ {
        move |f: &[(&str, &str)]| lines.borrow_mut().push(f.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "))
    }

    fn slot_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ways-detached-slots-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// A binary that cannot be started is logged, not passed over in
    /// silence, and gives its slot back.
    #[test]
    fn a_spawn_that_fails_is_logged_and_frees_its_slot() {
        let dir = slot_dir("fail");
        let lines = Lines::default();
        let sink = recorder(&lines);
        let slot = claim_slot(&dir, 1).expect("a free slot");
        let r = spawn_at(std::path::Path::new("/nonexistent/ways"), "{}", &slot, &sink);
        assert!(r.is_err());
        assert!(lines.borrow().iter().any(|l| l.contains("event=judge_detach_failed") && l.contains("reason=")), "{:?}", lines.borrow());
        assert!(!slot.exists(), "the slot is given back");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A child that exits without reading its stdin ends the write with an
    /// error, never a signal or a hang, even for a job larger than a pipe:
    /// the failure is logged and the slot is given back.
    #[cfg(unix)]
    #[test]
    fn a_child_that_does_not_read_its_job_does_not_hold_the_hook() {
        let dir = slot_dir("noread");
        let lines = Lines::default();
        let sink = recorder(&lines);
        let slot = claim_slot(&dir, 1).unwrap();
        let big = "x".repeat(1 << 20);
        let begun = std::time::Instant::now();
        // macOS ships `true` only at /usr/bin; older Linux layouts only at /bin.
        let truth = ["/usr/bin/true", "/bin/true"]
            .into_iter()
            .map(std::path::Path::new)
            .find(|p| p.exists())
            .expect("a `true` binary at /usr/bin or /bin");
        let r = spawn_at(truth, &big, &slot, &sink);
        assert!(begun.elapsed() < Duration::from_secs(3), "{:?}", begun.elapsed());
        assert!(r.is_err(), "the write fails");
        assert!(lines.borrow().iter().any(|l| l.contains("event=judge_detach_failed") && l.contains("reason=write")), "{:?}", lines.borrow());
        assert!(!slot.exists(), "the slot is given back");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Detached judges are capped: past the cap no slot is given, and a slot
    /// a dead judge left behind is taken back once it is stale.
    #[test]
    fn detached_judges_are_capped_by_slots() {
        let dir = slot_dir("cap");
        let a = claim_slot(&dir, 2).unwrap();
        let b = claim_slot(&dir, 2).unwrap();
        assert_ne!(a, b);
        assert!(claim_slot(&dir, 2).is_none(), "past the cap");
        std::fs::remove_file(&a).unwrap();
        assert!(claim_slot(&dir, 2).is_some(), "a freed slot is reused");
        let old = std::time::SystemTime::now() - SLOT_STALE - Duration::from_secs(1);
        std::fs::File::options().write(true).open(&b).unwrap().set_modified(old).unwrap();
        assert!(claim_slot(&dir, 2).is_some(), "a stale slot is reclaimed");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Hooks that find the same slot stale at once take it back once: one
    /// of them gets it and the rest find it held.
    #[test]
    fn a_stale_slot_is_reclaimed_by_one_hook() {
        let dir = slot_dir("race");
        let old = std::time::SystemTime::now() - SLOT_STALE - Duration::from_secs(1);
        for round in 0..100 {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let slot = dir.join("slot-0");
            std::fs::write(&slot, "").unwrap();
            std::fs::File::options().write(true).open(&slot).unwrap().set_modified(old).unwrap();
            let start = std::sync::Arc::new(std::sync::Barrier::new(8));
            let won: usize = (0..8)
                .map(|_| {
                    let (dir, start) = (dir.clone(), start.clone());
                    std::thread::spawn(move || {
                        start.wait();
                        claim_slot(&dir, 1).is_some()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| usize::from(h.join().unwrap()))
                .sum();
            assert_eq!(won, 1, "round {round}: one slot, {won} holders");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A child that is alive but slow to read holds its slot once it is
    /// spawned: a hook that stops waiting on the write logs it and leaves
    /// the slot for the child to give back.
    #[cfg(unix)]
    #[test]
    fn a_write_that_times_out_leaves_the_slot_to_the_child() {
        use std::os::unix::fs::PermissionsExt;
        let dir = slot_dir("slow");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("slow-reader");
        std::fs::write(&exe, "#!/bin/sh\nsleep 3\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let lines = Lines::default();
        let sink = recorder(&lines);
        let slot = claim_slot(&dir, 1).unwrap();
        let r = spawn_at(&exe, &"x".repeat(1 << 20), &slot, &sink);
        assert!(r.is_err(), "the write did not finish");
        assert!(lines.borrow().iter().any(|l| l.contains("reason=write: timed out")), "{:?}", lines.borrow());
        assert!(slot.exists(), "the slot belongs to the child");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sampling_at_the_ends_is_exact() {
        assert!(!sampled(0.0, "a") && sampled(1.0, "a"));
        assert!(!sampled(f64::NAN.max(0.0), "a"));
    }
}
