// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `ways judge host|take|answer`: the verbs a Claude Code judge host runs
//! (ADR-708 §3, §5). The ways mod registers as its session's host before the
//! session's first prompt, takes the job the prompt lane's gate writes, sends
//! it through `$.model.complete`, and pipes the result back. The files behind
//! the verbs are [`crate::session::judge_jobs`]'s; the host never reads them.
//!
//! Output and exit codes:
//! - `host` prints nothing and exits 0, again on every call.
//! - `take` prints one line, the job as JSON (its `id` and the `JudgeJob`
//!   fields) or `standby` when the session judges on another engine, and
//!   exits 0. With nothing to take by the deadline, or on SIGTERM, it prints
//!   nothing and exits 0.
//! - `answer --job <id>` reads the `JudgeAnswer` JSON on stdin for the job
//!   `take` printed, prints nothing and exits 0, also when the gate has
//!   stopped waiting and the answer is dropped.
//! - A session id that is not plain, or stdin that is not an answer, exits 2;
//!   a file that cannot be written exits 1.
//!
//! `continuance` is the continuance judge's verb, which the attend mod runs
//! at a work boundary; [`crate::cmd::judge_continuance`] has its output.

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Subcommand;
use ways_agent_core::profile;
use ways_agent_core::protocol::JudgeAnswer;

use crate::session::judge_jobs::{AnswerError, Store, POLL};

/// How long `take --wait` waits with no `--timeout-ms`. The host stops it
/// when the prompt's hooks settle, well before this.
const DEFAULT_WAIT_MS: u64 = 60_000;

#[derive(Subcommand)]
pub enum JudgeCommand {
    /// Register as the session's Claude Code judge host
    Host {
        /// The session id
        #[arg(long)]
        session: String,
    },
    /// Take the session's next judge job, or say `standby`
    Take {
        /// The session id
        #[arg(long)]
        session: String,
        /// Wait for a job, a standby resolution, the timeout or SIGTERM
        #[arg(long)]
        wait: bool,
        /// How long --wait waits, in milliseconds
        #[arg(long, default_value_t = DEFAULT_WAIT_MS)]
        timeout_ms: u64,
    },
    /// Store the answer on stdin for a job take printed
    Answer {
        /// The session id
        #[arg(long)]
        session: String,
        /// The job answered: the `id` take printed
        #[arg(long)]
        job: String,
    },
    /// Judge whether the session stops, compacts or continues, from the
    /// handoff and the conditions JSON on stdin
    Continuance {
        /// The session id
        #[arg(long)]
        session: String,
        /// The handoff the wrap wrote
        #[arg(long, required_unless_present = "parse")]
        handoff: Option<PathBuf>,
        /// Read the `$.model.complete` answer to a job on stdin, and print its verdict
        #[arg(long)]
        parse: bool,
        /// The caller hosts `$.model.complete`, so the claude-code engine
        /// runs whether or not the ways mod registered; the attend mod passes it
        #[arg(long)]
        host: bool,
    },
    /// Judge the candidates a lane handed off on stdin and log the verdicts
    /// (shadow mode, and the sample of held candidates); the lanes start it
    #[command(hide = true)]
    Detached,
}

pub fn run(command: JudgeCommand) -> Result<()> {
    match command {
        JudgeCommand::Host { session } => store(&session).register_host().context("registering the judge host"),
        JudgeCommand::Take { session, wait, timeout_ms } => {
            let store = store(&session);
            stop_on_sigterm();
            let configured = names_claude_code(&profile::user_layer_path());
            if let Some(line) = take(&store, configured, wait.then(|| Instant::now() + Duration::from_millis(timeout_ms)), stopped)? {
                println!("{line}");
            }
            Ok(())
        }
        JudgeCommand::Continuance { session, handoff, parse, host } => {
            crate::cmd::judge_continuance::run(&session, handoff.as_deref(), parse, host)
        }
        JudgeCommand::Detached => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).context("reading the job")?;
            crate::cmd::scan::run_detached(&input)
        }
        JudgeCommand::Answer { session, job } => {
            let store = store(&session);
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).context("reading the answer")?;
            let answer: JudgeAnswer = serde_json::from_str(&input).unwrap_or_else(|e| usage(&format!("not a judge answer: {e}")));
            match store.answer(&job, &answer) {
                Ok(()) | Err(AnswerError::NoJob(_)) => Ok(()),
                Err(AnswerError::Io(e)) => Err(e).context("writing the answer"),
            }
        }
    }
}

/// The session's store, or exit 2.
fn store(session: &str) -> Store {
    Store::for_session(session).unwrap_or_else(|| usage(&format!("not a session id: {session:?}")))
}

fn usage(message: &str) -> ! {
    eprintln!("ways judge: {message}");
    std::process::exit(2)
}

/// Whether agent.yaml at `path` has the gate judge with a `claude-code`
/// profile when a host is registered: on, and its engine one whose provider
/// is `claude-code`. A layer that fails closed turns the gate off, so no.
fn names_claude_code(path: &std::path::Path) -> bool {
    let Ok((user, findings)) = profile::UserLayer::load_with_findings(path) else { return false };
    if profile::fails_closed(&findings).is_some() || user.mode == Some(profile::Mode::Off) {
        return false;
    }
    user.engine.as_ref().and_then(|e| profile::profiles(&user).get(e).map(|p| p.provider)) == Some(profile::Provider::ClaudeCode)
}

/// What `take` prints: a claimed job as JSON, or `standby` when the session
/// will not judge inside Claude Code: it resolved to another engine, or it
/// holds no resolution and `configured` (agent.yaml names a claude-code
/// engine) is false. A session with no engine holds no resolution, so a
/// session on claude-code with no resolution yet is waited on. Looks once
/// with no `deadline`, else until it passes or `stop` says so.
///
/// A stop read just after a claim puts the job back and prints nothing:
/// the host that stopped this take no longer reads its output, and the
/// job's gate would otherwise wait for an answer to its deadline.
fn take(store: &Store, configured: bool, deadline: Option<Instant>, stop: impl Fn() -> bool) -> Result<Option<String>> {
    loop {
        if stop() {
            return Ok(None);
        }
        if let Some(job) = store.take().context("taking a judge job")? {
            if stop() {
                store.release(&job.id).context("putting back a judge job")?;
                return Ok(None);
            }
            return Ok(Some(serde_json::to_string(&job)?));
        }
        let claude_code = match store.resolution() {
            Some(r) => r.is_claude_code(),
            None => configured,
        };
        if !claude_code {
            return Ok(Some("standby".to_string()));
        }
        if deadline.is_none_or(|d| Instant::now() >= d) {
            return Ok(None);
        }
        std::thread::sleep(POLL);
    }
}

static STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn stopped() -> bool {
    STOPPED.load(std::sync::atomic::Ordering::SeqCst)
}

/// SIGTERM, how the host stops a `take` it no longer needs, ends the wait at
/// its next look instead of killing the process mid-claim, and puts back a
/// job claimed as the stop landed.
#[cfg(unix)]
fn stop_on_sigterm() {
    extern "C" fn on_term(_: libc::c_int) {
        STOPPED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    // SAFETY: the handler only stores to an atomic, which is signal-safe.
    unsafe {
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
    }
}

#[cfg(not(unix))]
fn stop_on_sigterm() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::judge_jobs::{SessionEngine, Taken};
    use ways_agent_core::engine::{Resolution, EXPLICIT, NO_HOST};
    use ways_agent_core::profile::Provider;
    use ways_agent_core::protocol::JudgeJob;

    fn store(name: &str) -> (Store, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("ways-judge-cmd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        (Store::at(base.join("state"), base.join("run")), base)
    }

    fn resolved(s: &Store, engine: &str, provider: Provider, reason: &str) {
        s.resolve_once(|_| SessionEngine { resolution: Resolution { engine: Some(engine.into()), reason: reason.into() }, provider: Some(provider) })
            .unwrap();
    }

    #[test]
    fn take_says_standby_when_the_session_resolved_to_a_key_engine() {
        let (s, base) = store("standby");
        resolved(&s, "anthropic", Provider::Anthropic, NO_HOST);
        let begun = Instant::now();
        // The held resolution wins over what agent.yaml now names.
        assert_eq!(take(&s, true, Some(begun + Duration::from_secs(5)), || false).unwrap().as_deref(), Some("standby"));
        assert!(begun.elapsed() < Duration::from_secs(1), "without waiting out the timeout");
        std::fs::remove_dir_all(&base).ok();
    }

    /// A session with no engine holds no resolution: `take` reads agent.yaml
    /// and stands by unless it names a claude-code engine.
    #[test]
    fn take_says_standby_with_no_resolution_unless_agent_yaml_names_claude_code() {
        let (s, base) = store("unheld");
        assert_eq!(take(&s, false, Some(Instant::now() + Duration::from_secs(5)), || false).unwrap().as_deref(), Some("standby"));
        assert_eq!(take(&s, true, Some(Instant::now() + Duration::from_millis(50)), || false).unwrap(), None, "waits for the job");
        std::fs::create_dir_all(&base).unwrap();
        let yaml = base.join("agent.yaml");
        for (text, names) in [
            ("engine: claude-code\nmode: enforce\n", true),
            ("profiles:\n  cc-sonnet:\n    provider: claude-code\n    model: sonnet\nengine: cc-sonnet\n", true),
            ("engine: claude-code\nmode: off\n", false),
            ("engine: anthropic\n", false),
            ("", false),
            ("mode: of\nengine: claude-code\n", false),
        ] {
            std::fs::write(&yaml, text).unwrap();
            assert_eq!(names_claude_code(&yaml), names, "{text:?}");
        }
        assert!(!names_claude_code(&base.join("missing.yaml")));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn take_waits_for_the_job_the_gate_writes() {
        let (s, base) = store("waits");
        resolved(&s, "claude-code", Provider::ClaudeCode, EXPLICIT);
        let writer = s.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let job = JudgeJob { model: "m".into(), system: "s".into(), prompt: "p".into(), max_tokens: 96, timeout_ms: 1900 };
            writer.write_job(&job).unwrap();
        });
        let line = take(&s, false, Some(Instant::now() + Duration::from_secs(5)), || false).unwrap().expect("a job");
        let t: Taken = serde_json::from_str(&line).unwrap();
        assert_eq!((t.job.prompt.as_str(), t.job.timeout_ms), ("p", 1900));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn take_prints_nothing_at_the_timeout_or_when_stopped() {
        let (s, base) = store("quiet");
        // Unresolved on claude-code: the host keeps waiting, since the scan
        // may yet judge.
        assert_eq!(take(&s, true, Some(Instant::now() + Duration::from_millis(50)), || false).unwrap(), None);
        assert_eq!(take(&s, true, None, || false).unwrap(), None);
        assert_eq!(take(&s, true, Some(Instant::now() + Duration::from_secs(5)), || true).unwrap(), None);
        std::fs::remove_dir_all(&base).ok();
    }

    /// A stop that lands while the take claims: the claim is put back, so
    /// the next take claims the job instead of its gate waiting it out.
    #[test]
    fn take_stopped_while_claiming_puts_the_job_back() {
        let (s, base) = store("unclaim");
        resolved(&s, "claude-code", Provider::ClaudeCode, EXPLICIT);
        let job = JudgeJob { model: "m".into(), system: "s".into(), prompt: "p".into(), max_tokens: 96, timeout_ms: 1900 };
        let id = s.write_job(&job).unwrap();
        // The first look finds no stop; the stop lands during the claim.
        let looks = std::cell::Cell::new(0);
        let stop = || {
            looks.set(looks.get() + 1);
            looks.get() > 1
        };
        assert_eq!(take(&s, false, Some(Instant::now() + Duration::from_secs(5)), stop).unwrap(), None, "prints nothing");
        assert!(looks.get() > 1, "the stop was read after the claim");
        let again = s.take().unwrap().expect("the job is pending again");
        assert_eq!(again.id, id);
        std::fs::remove_dir_all(&base).ok();
    }
}
