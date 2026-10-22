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
//! - `answer` reads the `JudgeAnswer` JSON on stdin, prints nothing and exits
//!   0, also when the gate has stopped waiting and the answer is dropped.
//! - A session id that is not plain, or stdin that is not an answer, exits 2;
//!   a file that cannot be written exits 1.

use std::io::Read;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Subcommand;
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
    /// Store the answer on stdin for the session's claimed job
    Answer {
        /// The session id
        #[arg(long)]
        session: String,
        /// The job answered: the `id` take printed (default: the newest claimed job)
        #[arg(long)]
        job: Option<String>,
    },
}

pub fn run(command: JudgeCommand) -> Result<()> {
    match command {
        JudgeCommand::Host { session } => store(&session).register_host().context("registering the judge host"),
        JudgeCommand::Take { session, wait, timeout_ms } => {
            let store = store(&session);
            stop_on_sigterm();
            if let Some(line) = take(&store, wait.then(|| Instant::now() + Duration::from_millis(timeout_ms)), stopped)? {
                println!("{line}");
            }
            Ok(())
        }
        JudgeCommand::Answer { session, job } => {
            let store = store(&session);
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).context("reading the answer")?;
            let answer: JudgeAnswer = serde_json::from_str(&input).unwrap_or_else(|e| usage(&format!("not a judge answer: {e}")));
            match store.answer(job.as_deref(), &answer) {
                Ok(_) | Err(AnswerError::NoJob(_)) => Ok(()),
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

/// What `take` prints: a claimed job as JSON, or `standby` once the session
/// resolved to an engine other than `claude-code`. Looks once with no
/// `deadline`, else until it passes or `stop` says so.
fn take(store: &Store, deadline: Option<Instant>, stop: impl Fn() -> bool) -> Result<Option<String>> {
    loop {
        if stop() {
            return Ok(None);
        }
        if let Some(job) = store.take().context("taking a judge job")? {
            return Ok(Some(serde_json::to_string(&job)?));
        }
        if store.resolution().is_some_and(|r| !r.is_claude_code()) {
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
/// its next look instead of killing the process mid-claim.
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
        assert_eq!(take(&s, Some(begun + Duration::from_secs(5)), || false).unwrap().as_deref(), Some("standby"));
        assert!(begun.elapsed() < Duration::from_secs(1), "without waiting out the timeout");
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
        let line = take(&s, Some(Instant::now() + Duration::from_secs(5)), || false).unwrap().expect("a job");
        let t: Taken = serde_json::from_str(&line).unwrap();
        assert_eq!((t.job.prompt.as_str(), t.job.timeout_ms), ("p", 1900));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn take_prints_nothing_at_the_timeout_or_when_stopped() {
        let (s, base) = store("quiet");
        // Unresolved: the host keeps waiting, since the scan may yet judge.
        assert_eq!(take(&s, Some(Instant::now() + Duration::from_millis(50)), || false).unwrap(), None);
        assert_eq!(take(&s, None, || false).unwrap(), None);
        assert_eq!(take(&s, Some(Instant::now() + Duration::from_secs(5)), || true).unwrap(), None);
        std::fs::remove_dir_all(&base).ok();
    }
}
