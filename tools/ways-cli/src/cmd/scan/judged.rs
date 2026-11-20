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
        }
    }
}

/// How the gate is set for this call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Config {
    /// No engine and no key, or `gate.mode: off`.
    Off,
    /// Judge on the hot path. A broken agent.yaml lands here too, so the
    /// gate logs why it falls back.
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
        // A broken agent.yaml fails closed: the gate is off. The prompt lane
        // logs why; a tool lane, on every tool call, stays as quiet as it
        // was before it judged.
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
    turns: Vec<Turn>,
    /// Shadow mode: the open candidates go to the detached judge.
    detach_open: bool,
    /// The held candidates sampled for the detached judge.
    held: Vec<Candidate>,
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
    judge_with(config, candidates, turns, |pending, turns| gate::apply(pending, turns, wait, &log))
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
    };
    let call = format!("Used the {tool} tool.");
    run(&lane, &scan, &candidates, || tool_turns(session_id, &call), gate::TOOL_WAIT, |c| show(&c.key));
}

/// [`judge`] with the setting and the gate given, so a test can stand in a
/// judge.
pub(super) fn judge_with(
    config: Config,
    candidates: &[Candidate],
    turns: Vec<Turn>,
    apply: impl FnOnce(&[Pending<'_>], Vec<Turn>) -> Gate,
) -> Judged {
    let (gate, detach_open, rate) = match config {
        Config::Off => return Judged { gate: Gate::default(), turns, detach_open: false, held: Vec::new() },
        Config::Shadow { held_sample } => {
            let any_open = candidates.iter().any(|c| c.open);
            let status = if any_open { gate::Status::Detached } else { gate::Status::Idle };
            (Gate { status, ..Gate::default() }, any_open, held_sample)
        }
        Config::Enforce { held_sample } => {
            let pending: Vec<Pending<'_>> = candidates
                .iter()
                .filter(|c| c.open)
                .map(|c| Pending { id: &c.key, description: &c.description, trigger: c.trigger })
                .collect();
            let gate = if pending.is_empty() { Gate { status: gate::Status::Idle, ..Gate::default() } } else { apply(&pending, turns.clone()) };
            (gate, false, held_sample)
        }
    };
    let held = candidates.iter().filter(|c| !c.open && sampled(rate, &c.key)).cloned().collect();
    Judged { gate, turns, detach_open, held }
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
    let mut record = decision::Record::begin_at(&lane.context(epoch), tick);
    for (i, s) in shown.iter().enumerate() {
        record.hit(&s.candidate.key, i + 1, &s.candidate.channel, s.candidate.p, s.result, Some(&judged.gate));
    }
    record.judge(&judged.gate);
    let scan_id = record.scan_id().to_string();
    record.write();
    detach(lane, judged, shown, &scan_id);
}

/// Hands what the detached judge should see, in shadow mode the shown
/// candidates and in either mode the sampled held ones, to a `ways judge
/// detached` process under `scan_id`, which nothing waits on.
pub(super) fn detach(lane: &Lane, judged: Judged, shown: &[Shown<'_>], scan_id: &str) {
    let scan_id = scan_id.to_string();
    let mut entries: Vec<Entry> = Vec::new();
    if judged.detach_open {
        entries.extend(shown.iter().filter(|s| s.candidate.open).map(|s| Entry { candidate: s.candidate.clone(), result: s.result.to_string() }));
    }
    entries.extend(judged.held.into_iter().map(|candidate| Entry { candidate, result: "held_refire".into() }));
    if entries.is_empty() {
        return;
    }
    spawn(&Job { lane: lane.clone(), scan_id, agent: crate::session::current_agent(), turns: judged.turns, entries });
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
}

/// Starts `ways judge detached` on `job`, in a session of its own so the
/// hook's end does not take it down, and returns without waiting.
fn spawn(job: &Job) {
    use std::process::{Command, Stdio};
    let Ok(exe) = std::env::current_exe() else { return };
    let Ok(text) = serde_json::to_string(job) else { return };
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
    if let Ok(mut child) = cmd.spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = std::io::Write::write_all(&mut stdin, text.as_bytes());
        }
    }
}

/// The longest a detached judge waits: it holds no hook, so the profile's
/// whole deadline applies.
const DETACHED_WAIT: Duration = Duration::from_secs(30);

/// `ways judge detached`: judges the job on stdin and writes its record. In
/// shadow mode the verdicts read `would_block`; a held candidate's verdict
/// is whatever the mode makes of it, and nothing is shown either way.
pub(crate) fn run_detached(text: &str) -> anyhow::Result<()> {
    let job: Job = serde_json::from_str(text)?;
    if job.agent != crate::session::MAIN_AGENT {
        std::env::set_var("CLAUDE_AGENT_ID", &job.agent);
    }
    std::env::set_var("CLAUDE_PROJECT_DIR", &job.lane.project_dir);
    let sink = &crate::session::log_event;
    let log = job.lane.log(sink);
    let pending: Vec<Pending<'_>> = job
        .entries
        .iter()
        .map(|e| Pending { id: &e.candidate.key, description: &e.candidate.description, trigger: e.candidate.trigger })
        .collect();
    let gate = gate::apply(&pending, job.turns.clone(), DETACHED_WAIT, &log);
    let epoch = crate::session::get_epoch(&job.lane.session_id);
    let mut record = decision::Record::detached(&job.lane.context(epoch), &job.scan_id);
    for (i, e) in job.entries.iter().enumerate() {
        record.hit(&e.candidate.key, i + 1, &e.candidate.channel, None, &e.result, Some(&gate));
    }
    record.judge(&gate);
    record.write();
    Ok(())
}

/// What a tool lane's judge reads: the calling agent's request, its last
/// human turn or its brief, then the call in hand as the agent's turn, the
/// one a profile of `turns: 1` reads. No narration (ADR-712).
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
    let judged = Judged { gate: Gate::default(), turns, detach_open: false, held };
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

/// The Bash call as the judge reads it: ADR-188's `About to run:` line with
/// the tool's description, then the command, cut to [`MAX_COMMAND_CHARS`] so
/// a long heredoc does not push the description and the prose out of the
/// profile's `max_turn_chars`.
pub(super) fn bash_call(command: &str, description: Option<&str>) -> String {
    let command = match command.char_indices().nth(MAX_COMMAND_CHARS) {
        Some((at, _)) => format!("{}…", &command[..at]),
        None => command.to_string(),
    };
    match description.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => format!("About to run: {d}\n$ {command}"),
        None => format!("About to run:\n$ {command}"),
    }
}

/// The most of a command the judge reads.
const MAX_COMMAND_CHARS: usize = 400;

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
        let lc = gate::LogContext { lane: "command", session_id: "s", project_dir: "/p", scope: "agent", hook_event: "PreToolUse", sink: &s };
        let j = judge_with(Config::Enforce { held_sample: 0.0 }, &cands, vec![], |pending, _| {
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
        let j = judge_with(Config::Shadow { held_sample: 1.0 }, &cands, vec![], |_, _| panic!("no judge on the hot path"));
        assert!(!j.blocked("a"));
        assert!(j.detach_open && j.gate.status == gate::Status::Detached);
        assert_eq!(j.held.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["b"], "every held one at rate 1");
    }

    #[test]
    fn off_judges_nothing() {
        let cands = [candidate("a", Trigger::Semantic, true)];
        let j = judge_with(Config::Off, &cands, vec![], |_, _| panic!("off calls no judge"));
        assert_eq!(j.gate.status, gate::Status::Off);
        assert!(!j.blocked("a") && j.held.is_empty() && !j.detach_open);
    }

    #[test]
    fn the_call_follows_the_request_and_no_narration_is_sent() {
        let call = bash_call("git commit -m x", Some("Commit the fix"));
        assert_eq!(call, "About to run: Commit the fix\n$ git commit -m x");
        let turns = with_call(vec![Turn { role: Role::User, text: "brief".into() }], &call);
        assert_eq!(turns, [Turn { role: Role::User, text: "brief".into() }, Turn { role: Role::Assistant, text: call.clone() }]);
        let alone = with_call(vec![], &call);
        assert_eq!(alone, [Turn { role: Role::Assistant, text: call }]);
        let long = bash_call(&"x".repeat(1000), None);
        assert!(long.chars().count() < 450 && long.ends_with('…'), "{long}");
    }

    #[test]
    fn sampling_at_the_ends_is_exact() {
        assert!(!sampled(0.0, "a") && sampled(1.0, "a"));
        assert!(!sampled(f64::NAN.max(0.0), "a"));
    }
}
