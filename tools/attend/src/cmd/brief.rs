// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend keepwarm brief`: the conditions the continuance judge reads at a
//! work boundary, as one JSON object of the shape `ways-agent-core`'s
//! `Conditions` defines. attend measures what it can see: the keepwarm
//! reading and ledger, the messages awaiting a reply, git, and the open
//! PRs through `gh`. The mod passes what only it sees on stdin, and attend
//! merges it. Every probe runs against one deadline, [`PROBE`] after the
//! start; a probe that fails or runs past it reports `"unknown"`, and the
//! verb exits 0 whatever the probes did.
//!
//! References:
//! - ADR-418 §10: the conditions row of the brief table.

use crate::cli::KeepwarmBriefArgs;
use crate::util::keepwarm_dir;
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the probes have, all together.
const PROBE: Duration = Duration::from_secs(2);
/// How long past the deadline a probe's thread is waited for, so a command
/// killed at the deadline reports through its own path.
const GRACE: Duration = Duration::from_millis(250);
/// The most stdin the brief reads.
const MAX_STDIN: u64 = 1024 * 1024;
/// The most a probe's command may print.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
/// The most commits, merges or PRs the brief lists, so the judge's brief
/// stays bounded. Commits and merges past it are counted in one last line.
const MAX_LISTED: usize = 50;

/// A measure, or `"unknown"` when its probe failed or timed out.
struct M<T>(Option<T>);

impl<T: Serialize> Serialize for M<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            Some(v) => v.serialize(s),
            None => s.serialize_str("unknown"),
        }
    }
}

#[derive(Serialize)]
struct OpenPr {
    number: u64,
    title: String,
    /// The checks rolled up to one state: `fail`, `pending`, `pass`, or
    /// `none` for a PR with no checks.
    checks: String,
}

#[derive(Serialize)]
struct Relays {
    n: u32,
    max: u32,
}

/// The brief's conditions, in the order `Conditions` declares them.
#[derive(Serialize)]
struct Conditions {
    fill_pct: M<f64>,
    fill_tokens: M<u64>,
    price_per_ping_usd: M<f64>,
    agents_in_flight: M<Vec<String>>,
    shells_in_flight: M<Vec<String>>,
    watchers_in_flight: M<Vec<String>>,
    /// Direct messages to peer sessions that await a reply. A question to
    /// the operator is not counted here but in `operator_questions`: the two
    /// come from one awaiting count, split by recipient, so they sum to it.
    threads_awaiting_reply: M<u64>,
    operator_questions: M<u64>,
    tasks_open: M<Vec<String>>,
    tasks_in_progress: M<Vec<String>>,
    git_status: M<String>,
    open_prs: M<Vec<OpenPr>>,
    minutes_since_operator_prompt: M<u64>,
    relays: M<Relays>,
    commits_since_wrap: M<Vec<String>>,
    merges_since_wrap: M<Vec<String>>,
    tasks_closed_since_wrap: M<Vec<String>>,
}

/// What the mod passes on stdin: a JSON object whose `agents_in_flight`,
/// `shells_in_flight`, `watchers_in_flight`, `tasks_open`,
/// `tasks_in_progress` and `tasks_closed_since_wrap` are lists of lines,
/// whose `window` is the model's whole window in tokens, and whose
/// `operator_prompt_at` is the operator's last prompt in epoch seconds. A
/// key attend measures itself is not read from it, and a key missing or of
/// another shape is not given.
#[derive(Default)]
struct FromMod {
    doc: Value,
}

impl FromMod {
    fn lines(&self, key: &str) -> M<Vec<String>> {
        let list = self.doc.get(key).and_then(Value::as_array);
        M(list.and_then(|l| l.iter().map(|v| v.as_str().map(str::to_string)).collect()))
    }

    fn window(&self) -> Option<u64> {
        self.doc.get("window").and_then(Value::as_u64)
    }

    fn operator_prompt_at(&self) -> Option<u64> {
        self.doc.get("operator_prompt_at").and_then(Value::as_u64)
    }
}

pub(crate) fn cmd_brief(ident: &attend_presence::session::SessionIdentity, args: &KeepwarmBriefArgs) {
    let deadline = Instant::now() + PROBE;
    let now = sensor_trait::epoch_secs();
    let project = PathBuf::from(&ident.origin_path);
    let session = ident.session_resolved.then(|| ident.session_id.clone());

    let stdin = probe(read_stdin);
    let reading = probe({
        let (session, project) = (session.clone(), ident.origin_path.clone());
        move || sensor_keepwarm::brief::read(&session?, &project, left(deadline))
    });
    let settings = probe({
        let (session, project) = (session.clone(), ident.origin_path.clone());
        move || super::keepwarm::policy_settings(&keepwarm_dir(), &session?, &project, now, left(deadline))
    });
    // One count, so the two fields cannot disagree.
    let awaiting = probe({
        let resolved = session.is_some();
        move || resolved.then(crate::verbs::awaiting::count)
    });
    let status = probe({
        let project = project.clone();
        move || git(&project, &["status", "--porcelain"], deadline)
    });
    let since = probe({
        let (project, handoff) = (project.clone(), args.handoff.clone());
        move || since_wrap(&project, handoff, deadline)
    });
    let prs = probe({
        let project = project.clone();
        move || open_prs(&project, deadline)
    });

    let from_mod = take(stdin, deadline).map(|doc| FromMod { doc }).unwrap_or_default();
    let ledger = session.as_deref().and_then(|sid| sensor_keepwarm::read_ledger(&keepwarm_dir(), sid));
    let max_relays = take(settings, deadline)
        .map(|doc| sensor_keepwarm::policy::Config::from_settings(Some(&doc)))
        .filter(|cfg| cfg.settings_ok)
        .map(|cfg| cfg.max_relays);
    let reading = take(reading, deadline);
    let kw = sensor_keepwarm::brief::measure(reading.as_ref(), from_mod.window(), ledger.as_ref(), from_mod.operator_prompt_at(), max_relays, now);
    let awaiting = take(awaiting, deadline);
    let (commits, merges) = take(since, deadline).unzip();
    // No record is a last Stop with nothing in flight; with no session
    // there is no record to read.
    let recorded = session.as_deref().map(|sid| sensor_keepwarm::background::read(&keepwarm_dir(), sid).unwrap_or_default());
    let InFlight { agents, shells, watchers } = in_flight(&from_mod, recorded);
    let conditions = Conditions {
        fill_pct: M(kw.fill_pct),
        fill_tokens: M(kw.fill_tokens),
        price_per_ping_usd: M(kw.price_per_ping_usd),
        agents_in_flight: agents,
        shells_in_flight: shells,
        watchers_in_flight: watchers,
        threads_awaiting_reply: M(awaiting.map(|a| (a.count - a.operator) as u64)),
        operator_questions: M(awaiting.map(|a| a.operator as u64)),
        tasks_open: from_mod.lines("tasks_open"),
        tasks_in_progress: from_mod.lines("tasks_in_progress"),
        git_status: M(take(status, deadline)),
        open_prs: M(take(prs, deadline)),
        minutes_since_operator_prompt: M(kw.minutes_since_operator_prompt),
        relays: M(kw.relays.map(|(n, max)| Relays { n, max })),
        commits_since_wrap: M(commits),
        merges_since_wrap: M(merges),
        tasks_closed_since_wrap: from_mod.lines("tasks_closed_since_wrap"),
    };
    super::print_json(&conditions);
}

struct InFlight {
    agents: M<Vec<String>>,
    shells: M<Vec<String>>,
    watchers: M<Vec<String>>,
}

/// The work in flight, by the check's rule: a list the mod gives of shells
/// or watchers wins over `recorded`, the last Stop's record, and the agents
/// the mod lists are merged with the ones the Stop recorded (`workflow:
/// <name>`, `subagent: …` and so on), the mod's first. A list is unknown
/// only where the mod gives none and there is no record to read.
///
/// References:
/// - ADR-418 item 5: agents, shells and watchers in flight are pending.
fn in_flight(from_mod: &FromMod, recorded: Option<sensor_keepwarm::background::Background>) -> InFlight {
    let (mod_agents, mod_shells, mod_watchers) =
        (from_mod.lines("agents_in_flight").0, from_mod.lines("shells_in_flight").0, from_mod.lines("watchers_in_flight").0);
    let Some(recorded) = recorded else {
        return InFlight { agents: M(mod_agents), shells: M(mod_shells), watchers: M(mod_watchers) };
    };
    let mut agents = mod_agents.unwrap_or_default();
    for a in recorded.agents {
        if !agents.contains(&a) {
            agents.push(a);
        }
    }
    InFlight {
        agents: M(Some(agents)),
        shells: M(Some(mod_shells.unwrap_or(recorded.shells))),
        watchers: M(Some(mod_watchers.unwrap_or(recorded.watchers))),
    }
}

/// The time left before `deadline`.
fn left(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

/// Run `f` on a thread of its own.
fn probe<T: Send + 'static>(f: impl FnOnce() -> Option<T> + Send + 'static) -> mpsc::Receiver<Option<T>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || tx.send(f()).ok());
    rx
}

/// A probe's measure, or `None` when it has none by the deadline.
fn take<T>(rx: mpsc::Receiver<Option<T>>, deadline: Instant) -> Option<T> {
    rx.recv_timeout(left(deadline) + GRACE).ok().flatten()
}

/// The JSON object on stdin, when stdin is no terminal and holds one.
fn read_stdin() -> Option<Value> {
    use std::io::{IsTerminal, Read};
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut text = String::new();
    stdin.take(MAX_STDIN).read_to_string(&mut text).ok()?;
    serde_json::from_str::<Value>(&text).ok().filter(Value::is_object)
}

/// `program args` run in `dir`, its stdout when it exits 0 by `deadline`.
/// It runs in a process group of its own, which is killed at the deadline,
/// so nothing it started outlives the probe holding its stdout. It starts
/// through [`sensor_keepwarm::spawn_probe`], so its pipe never leaks into a
/// sibling probe's child.
fn run(program: &str, args: &[&str], dir: &Path, env: &[(&str, &str)], deadline: Instant) -> Option<String> {
    use std::io::Read;
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = sensor_keepwarm::spawn_probe(&mut cmd).ok()?;
    let out = child.stdout.take()?;
    let read = probe(move || {
        let mut buf = Vec::new();
        out.take(MAX_OUTPUT + 1).read_to_end(&mut buf).ok().map(|_| buf)
    });
    let kill = |child: &mut std::process::Child| {
        #[cfg(unix)]
        // SAFETY: kill(2) takes no pointers; a negative pid names the
        // process group the child leads, which process_group(0) made, and
        // the child is not yet reaped, so the group id is still its.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        child.kill().ok();
        child.wait().ok();
    };
    let Some(buf) = read.recv_timeout(left(deadline)).ok().flatten() else {
        kill(&mut child);
        return None;
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                kill(&mut child);
                return None;
            }
        }
    };
    if !status.success() || buf.len() as u64 > MAX_OUTPUT {
        return None;
    }
    String::from_utf8(buf).ok()
}

/// `git args` in the repository at `project`, read-only: it takes no
/// optional lock and asks for nothing.
fn git(project: &Path, args: &[&str], deadline: Instant) -> Option<String> {
    let dir = project.to_string_lossy();
    let argv: Vec<&str> = ["-C", dir.as_ref()].into_iter().chain(args.iter().copied()).collect();
    run("git", &argv, project, &[("GIT_OPTIONAL_LOCKS", "0"), ("GIT_TERMINAL_PROMPT", "0")], deadline)
}

/// The head the handoff names: the second word of its first line,
/// `<date> <short HEAD> wrap=<id>`, when it is a commit's hex.
fn handoff_head(text: &str) -> Option<&str> {
    let head = text.lines().next()?.split_whitespace().nth(1)?;
    ((4..=40).contains(&head.len()) && head.chars().all(|c| c.is_ascii_hexdigit())).then_some(head)
}

/// The commits since the handoff's head, and the merges among them, one
/// `<short> <subject>` line each, newest first, at most [`MAX_LISTED`] of
/// each and then `… N more`. `handoff` is the file, by default the
/// repository's `.claude/handoff.wrap.md`.
fn since_wrap(project: &Path, handoff: Option<PathBuf>, deadline: Instant) -> Option<(Vec<String>, Vec<String>)> {
    let path = match handoff {
        Some(p) => p,
        None => PathBuf::from(git(project, &["rev-parse", "--show-toplevel"], deadline)?.trim()).join(ways_core::handoff::FILE),
    };
    let text = std::fs::read_to_string(path).ok()?;
    let range = format!("{}..HEAD", handoff_head(&text)?);
    let max = format!("--max-count={MAX_LISTED}");
    let log = |merges: &[&str]| {
        let args: Vec<&str> = ["log", "--format=%h %s", max.as_str()].iter().chain(merges).chain(&[range.as_str(), "--"]).copied().collect();
        let mut lines: Vec<String> = git(project, &args, deadline)?.lines().map(str::to_string).collect();
        if lines.len() == MAX_LISTED {
            let args: Vec<&str> = ["rev-list", "--count"].iter().chain(merges).chain(&[range.as_str(), "--"]).copied().collect();
            let total: usize = git(project, &args, deadline)?.trim().parse().ok()?;
            if total > MAX_LISTED {
                lines.push(format!("… {} more", total - MAX_LISTED));
            }
        }
        Some(lines)
    };
    Some((log(&[])?, log(&["--merges"])?))
}

/// The open PRs whose head is the current branch, in the repository `gh`
/// finds from `project`. None on a detached head.
fn open_prs(project: &Path, deadline: Instant) -> Option<Vec<OpenPr>> {
    let branch = git(project, &["rev-parse", "--abbrev-ref", "HEAD"], deadline)?;
    let branch = branch.trim();
    if branch == "HEAD" {
        return Some(Vec::new());
    }
    let env = [("GH_PROMPT_DISABLED", "1"), ("GH_NO_UPDATE_NOTIFIER", "1"), ("NO_COLOR", "1")];
    let limit = MAX_LISTED.to_string();
    let args = ["pr", "list", "--head", branch, "--state", "open", "--json", "number,title,statusCheckRollup", "--limit", &limit];
    let out = run("gh", &args, project, &env, deadline)?;
    let list: Vec<Value> = serde_json::from_str(&out).ok()?;
    list.iter()
        .map(|pr| {
            Some(OpenPr {
                number: pr.get("number")?.as_u64()?,
                title: pr.get("title")?.as_str()?.to_string(),
                checks: checks(pr.get("statusCheckRollup").and_then(Value::as_array).map_or(&[][..], Vec::as_slice)).to_string(),
            })
        })
        .collect()
}

/// A PR's `statusCheckRollup` as one state: `fail` when any check failed,
/// else `pending` when any has not finished, else `pass`; `none` with no
/// checks. A check run carries `status` and `conclusion`, a commit status
/// `state`; a state this does not know counts as pending.
fn checks(rollup: &[Value]) -> &'static str {
    if rollup.is_empty() {
        return "none";
    }
    let word = |c: &Value, k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default().to_ascii_uppercase();
    let each = rollup.iter().map(|c| {
        let state = match c.get("__typename").and_then(Value::as_str) {
            Some("StatusContext") => word(c, "state"),
            _ if word(c, "status") != "COMPLETED" => return "pending",
            _ => word(c, "conclusion"),
        };
        match state.as_str() {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => "pass",
            "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE" | "STALE" => "fail",
            _ => "pending",
        }
    });
    let states: Vec<&str> = each.collect();
    ["fail", "pending"].into_iter().find(|s| states.contains(s)).unwrap_or("pass")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_is_the_second_word_of_the_first_line() {
        assert_eq!(handoff_head("2026-11-27 e6bbfbdd wrap=20261127T000000Z-abcdef12\nContinue"), Some("e6bbfbdd"));
        assert_eq!(handoff_head("e6bbfbdd 2026-11-27 wrap=x"), None);
        assert_eq!(handoff_head("<!-- wrap=w1 head=abc -->"), None);
        assert_eq!(handoff_head(""), None);
    }

    /// With no session there is no record to read: what the mod gives
    /// stands, and what it leaves out is unknown, not empty.
    #[test]
    fn without_a_record_to_read_the_work_the_mod_leaves_out_is_unknown() {
        let json = |m: &M<Vec<String>>| serde_json::to_value(m).unwrap();
        let from_mod = FromMod { doc: serde_json::json!({"shells_in_flight": ["cargo test"]}) };
        let f = in_flight(&from_mod, None);
        assert_eq!((json(&f.agents), json(&f.shells), json(&f.watchers)), ("unknown".into(), serde_json::json!(["cargo test"]), "unknown".into()));
        let recorded = sensor_keepwarm::background::Background { agents: vec!["workflow: wave3".into()], watchers: vec!["watch CI".into()], ..Default::default() };
        let f = in_flight(&from_mod, Some(recorded));
        assert_eq!(
            (json(&f.agents), json(&f.shells), json(&f.watchers)),
            (serde_json::json!(["workflow: wave3"]), serde_json::json!(["cargo test"]), serde_json::json!(["watch CI"]))
        );
    }

    #[test]
    fn checks_roll_up_to_one_state() {
        let run = |status: &str, conclusion: &str| serde_json::json!({"__typename": "CheckRun", "status": status, "conclusion": conclusion});
        let ctx = |state: &str| serde_json::json!({"__typename": "StatusContext", "state": state});
        assert_eq!(checks(&[]), "none");
        assert_eq!(checks(&[run("COMPLETED", "SUCCESS"), ctx("SUCCESS")]), "pass");
        assert_eq!(checks(&[run("QUEUED", ""), ctx("SUCCESS")]), "pending");
        assert_eq!(checks(&[run("QUEUED", ""), ctx("ERROR")]), "fail");
        assert_eq!(checks(&[ctx("EXPECTED")]), "pending");
    }
}
