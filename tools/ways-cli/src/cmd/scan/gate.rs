//! The relevance gate on the prompt lane (ADR-196).
//!
//! After the matcher and the refire check, and before a fire is recorded, the
//! ways the lane would show are sent to the ways agent in one request with the
//! conversation's last turns. In enforce mode a way judged below the engine's
//! threshold is not shown and keeps its refire budget; in shadow mode every
//! verdict is logged and nothing is blocked. Ways with `pattern_strict` are not
//! judged. Every verdict and every fallback is logged to `events.jsonl`, and
//! any failure fails open: the matcher's decision stands.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use ways_agent::judge::{way_text, Candidate, Role, Turn};
use ways_agent::profile::{self, Mode, Settings};
use ways_agent::protocol::{JudgeRequest, Judged, Reply, Request};

use crate::session;

/// Grace beyond the engine's deadline for the hook's read: covers starting the
/// agent and waiting for a provider slot, both inside the agent's own deadline.
const READ_GRACE: Duration = Duration::from_millis(1500);

/// Where gate events go: `session::log_event` in the hook.
pub(super) type EventSink<'a> = &'a dyn Fn(&[(&str, &str)]);

/// A way the lane would show, as the gate needs it.
pub(super) struct Pending<'a> {
    pub id: &'a str,
    pub description: &'a str,
    pub pattern_strict: bool,
}

/// What the gate needs from the scan for its log lines, and where they go.
pub(super) struct LogContext<'a> {
    pub session_id: &'a str,
    pub project_dir: &'a str,
    pub scope: &'a str,
    pub hook_event: &'a str,
    pub sink: EventSink<'a>,
}

/// The ids the gate blocks. Empty when the gate is off, in shadow mode, or
/// failed.
pub(super) fn apply(
    pending: &[Pending<'_>],
    prompt: &str,
    response_context: Option<&str>,
    log: &LogContext<'_>,
) -> HashSet<String> {
    let Some(settings) = settings() else { return HashSet::new() };
    run(pending, prompt, response_context, &settings, log, |req, timeout| {
        ways_agent::client::call(Request::Judge(req), timeout, true)
    })
}

/// The gate's settings, read from key locations alone. `None` when the gate is
/// off: no engine and no key, mode off, or a configuration error (logged).
fn settings() -> Option<Settings> {
    use ways_agent::keys;
    let user = match profile::UserLayer::load(&profile::user_layer_path()) {
        Ok(u) => u,
        Err(e) => {
            session::log_event(&[("event", "gate_fallback"), ("reason", &format!("config: {e:#}"))]);
            return None;
        }
    };
    match profile::resolve(&user, |p| keys::locate(p).is_some()) {
        Ok(Some(s)) if s.mode != Mode::Off => Some(s),
        Ok(_) => None,
        Err(e) => {
            session::log_event(&[("event", "gate_fallback"), ("reason", &format!("config: {e:#}"))]);
            None
        }
    }
}

/// The gate with the agent call injected, so tests can stand in an engine.
fn run(
    pending: &[Pending<'_>],
    prompt: &str,
    response_context: Option<&str>,
    settings: &Settings,
    log: &LogContext<'_>,
    call: impl FnOnce(JudgeRequest, Duration) -> Result<Reply, String>,
) -> HashSet<String> {
    let judged: Vec<&Pending<'_>> = pending.iter().filter(|p| !p.pattern_strict).collect();
    if judged.is_empty() {
        return HashSet::new();
    }
    let request = JudgeRequest {
        session: log.session_id.to_string(),
        tool: "claude-code".to_string(),
        turns: turns(prompt, response_context),
        candidates: judged
            .iter()
            .map(|p| Candidate { id: p.id.to_string(), text: way_text(p.id, p.description) })
            .collect(),
    };
    let begun = Instant::now();
    let reply = call(request, Duration::from_millis(settings.profile.timeout_ms) + READ_GRACE);
    let elapsed_ms = begun.elapsed().as_millis().to_string();
    match reply {
        Ok(Reply::Judged(j)) => decide(&j, log, &elapsed_ms),
        Ok(Reply::Fallback { reason, .. }) => fallback(&reason, judged.len(), log, &elapsed_ms),
        Ok(Reply::Error { message }) => fallback(&format!("agent_error: {message}"), judged.len(), log, &elapsed_ms),
        Ok(other) => fallback(&format!("unexpected_reply: {other:?}"), judged.len(), log, &elapsed_ms),
        Err(reason) => fallback(&reason, judged.len(), log, &elapsed_ms),
    }
}

/// The turns the judge reads: Claude's last reply, then the prompt. Profiles
/// that ask for more than two turns get these two.
fn turns(prompt: &str, response_context: Option<&str>) -> Vec<Turn> {
    let mut turns = Vec::with_capacity(2);
    if let Some(rc) = response_context.map(str::trim).filter(|r| !r.is_empty()) {
        turns.push(Turn { role: Role::Assistant, text: rc.to_string() });
    }
    turns.push(Turn { role: Role::User, text: prompt.to_string() });
    turns
}

/// Logs each verdict and returns the ids to block.
fn decide(j: &Judged, log: &LogContext<'_>, elapsed_ms: &str) -> HashSet<String> {
    let mut blocked = HashSet::new();
    let threshold = format!("{:.2}", j.threshold);
    for v in &j.verdicts {
        let pass = v.p_yes >= j.threshold;
        let verdict = match (pass, j.mode) {
            (true, _) => "pass",
            (false, Mode::Enforce) => "block",
            (false, _) => "would_block",
        };
        if verdict == "block" {
            blocked.insert(v.id.clone());
        }
        (log.sink)(&[
            ("event", "way_judged"),
            ("way", &v.id),
            ("p_yes", &format!("{:.3}", v.p_yes)),
            ("threshold", &threshold),
            ("verdict", verdict),
            ("mode", j.mode.as_str()),
            ("engine", &j.engine),
            ("model", &j.model),
            ("judge_ms", &j.latency_ms.to_string()),
            ("gate_ms", elapsed_ms),
            ("candidates", &j.verdicts.len().to_string()),
            ("hook", log.hook_event),
            ("scope", log.scope),
            ("project", log.project_dir),
            ("session", log.session_id),
        ]);
    }
    blocked
}

/// Logs a fallback; nothing is blocked.
fn fallback(reason: &str, candidates: usize, log: &LogContext<'_>, elapsed_ms: &str) -> HashSet<String> {
    (log.sink)(&[
        ("event", "gate_fallback"),
        ("reason", reason),
        ("gate_ms", elapsed_ms),
        ("candidates", &candidates.to_string()),
        ("hook", log.hook_event),
        ("scope", log.scope),
        ("project", log.project_dir),
        ("session", log.session_id),
    ]);
    HashSet::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ways_agent::profile::{Provider, UserLayer};
    use ways_agent::protocol::Verdict;

    fn settings(mode: Mode) -> Settings {
        let user = UserLayer { mode: Some(mode), ..Default::default() };
        profile::resolve(&user, |p| p == Provider::Anthropic).unwrap().unwrap()
    }

    type Events = std::cell::RefCell<Vec<Vec<(String, String)>>>;

    fn recorder(events: &Events) -> impl Fn(&[(&str, &str)]) + '_ {
        move |fields| events.borrow_mut().push(fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    fn log(sink: EventSink<'_>) -> LogContext<'_> {
        LogContext { session_id: "test-gate", project_dir: "/tmp", scope: "agent", hook_event: "UserPromptSubmit", sink }
    }

    fn field<'e>(event: &'e [(String, String)], key: &str) -> &'e str {
        event.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    fn judged(mode: Mode, verdicts: &[(&str, f64)]) -> Reply {
        Reply::Judged(Judged {
            engine: "anthropic".into(),
            provider: Provider::Anthropic,
            model: "claude-haiku-4-5".into(),
            mode,
            threshold: 0.3,
            verdicts: verdicts.iter().map(|(id, p)| Verdict { id: id.to_string(), p_yes: *p }).collect(),
            latency_ms: 800,
        })
    }

    fn pending() -> Vec<Pending<'static>> {
        vec![
            Pending { id: "softwaredev/code/security/secrets", description: "Keep secrets out of source.", pattern_strict: false },
            Pending { id: "data/migrations", description: "Schema migrations.", pattern_strict: false },
            Pending { id: "meta/strict", description: "Always.", pattern_strict: true },
        ]
    }

    #[test]
    fn enforce_blocks_below_threshold_logs_each_verdict_and_skips_strict_ways() {
        let events = Events::default();
        let sink = recorder(&events);
        let mut seen = Vec::new();
        let blocked = run(&pending(), "store the api key", Some("ok"), &settings(Mode::Enforce), &log(&sink), |req, timeout| {
            seen = req.candidates.iter().map(|c| c.id.clone()).collect();
            assert_eq!(req.turns.len(), 2);
            assert_eq!(req.turns[1].role, Role::User);
            assert!(req.candidates[0].text.starts_with("softwaredev › code › security › secrets\n"));
            assert!(timeout > Duration::from_millis(2000));
            Ok(judged(Mode::Enforce, &[("softwaredev/code/security/secrets", 0.92), ("data/migrations", 0.05)]))
        });
        assert_eq!(seen, vec!["softwaredev/code/security/secrets", "data/migrations"]);
        assert_eq!(blocked, HashSet::from(["data/migrations".to_string()]));
        let events = events.borrow();
        assert_eq!(events.len(), 2);
        assert_eq!(field(&events[0], "verdict"), "pass");
        assert_eq!(field(&events[1], "verdict"), "block");
        assert_eq!(field(&events[1], "p_yes"), "0.050");
    }

    #[test]
    fn shadow_blocks_nothing_and_logs_would_block() {
        let events = Events::default();
        let sink = recorder(&events);
        let blocked = run(&pending(), "q", None, &settings(Mode::Shadow), &log(&sink), |_, _| {
            Ok(judged(Mode::Shadow, &[("softwaredev/code/security/secrets", 0.1), ("data/migrations", 0.05)]))
        });
        assert!(blocked.is_empty());
        assert!(events.borrow().iter().all(|e| field(e, "verdict") == "would_block"));
    }

    #[test]
    fn every_failure_fails_open_and_logs_its_reason() {
        let s = settings(Mode::Enforce);
        let replies: Vec<(Result<Reply, String>, &str)> = vec![
            (Err("agent_absent".into()), "agent_absent"),
            (Err("deadline".into()), "deadline"),
            (Ok(Reply::Fallback { reason: "no_key".into(), latency_ms: 0 }), "no_key"),
            (Ok(Reply::Error { message: "protocol 2 not served".into() }), "agent_error: protocol 2 not served"),
        ];
        for (reply, reason) in replies {
            let events = Events::default();
            let sink = recorder(&events);
            let blocked = run(&pending(), "q", None, &s, &log(&sink), |_, _| reply);
            assert!(blocked.is_empty());
            let events = events.borrow();
            assert_eq!(field(&events[0], "event"), "gate_fallback");
            assert_eq!(field(&events[0], "reason"), reason);
            assert_eq!(field(&events[0], "candidates"), "2");
        }
    }

    #[test]
    fn only_strict_ways_means_no_call_and_no_log() {
        let events = Events::default();
        let sink = recorder(&events);
        let strict = vec![Pending { id: "a", description: "d", pattern_strict: true }];
        let blocked = run(&strict, "q", None, &settings(Mode::Enforce), &log(&sink), |_, _| panic!("no call expected"));
        assert!(blocked.is_empty());
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn turns_put_the_reply_before_the_prompt_and_drop_an_empty_reply() {
        assert_eq!(turns("p", Some("  ")).len(), 1);
        let t = turns("p", Some("r"));
        assert_eq!((t[0].role, t[1].role), (Role::Assistant, Role::User));
    }
}
