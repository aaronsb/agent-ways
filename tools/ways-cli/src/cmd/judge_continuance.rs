// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `ways judge continuance`: the continuance request through the judge
//! interface. The attend mod runs it at a work boundary, with
//! the handoff's path and the measured conditions on stdin.
//!
//! The verb resolves the session's engine as the gate does: the engine the
//! session holds, else what it resolves to now. It never publishes a
//! resolution, so it does not move the session's disclosure engine. On that
//! engine's profile it reads the continuance judgement.
//!
//! Output, one line of JSON, and the exit code:
//! - On a key engine the ways agent makes the call, and the verb prints the
//!   verdict, `{"verdict": …, "handoff": …, "reason": …, "turns_left": …,
//!   "next_step": …}`.
//! - On `claude-code` it prints `{"job": <JudgeJob>}`, which the caller sends
//!   through `$.model.complete` and pipes, as a `JudgeAnswer`, to `--parse`.
//! - `--parse` reads the `JudgeAnswer` on stdin and prints the verdict.
//! - Any failure prints `{"fallback": "<reason>"}`, which the caller takes as
//!   doubt. A fallback exits 0.
//! - A session id that is not plain, or stdin that is not the conditions (or
//!   with `--parse`, an answer), exits 2.
//!
//! Each provider call is logged as a `judge_call` event with purpose
//! `continuance`, which `ways agent cost` sums with the disclosure calls.
//!
//! References:
//! - ADR-418 §10: the judge decides and never works.
//! - ADR-418 §14: on the Claude Code engine the call is `$.model.complete`.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use ways_agent_core::continuance::{self, Conditions, Verdict};
use ways_agent_core::cost::{JudgeCall, Usage};
use ways_agent_core::engine::Continuance;
use ways_agent_core::profile::{self, Provider};
use ways_agent_core::protocol::{ContinuanceRequest, JudgeAnswer, JudgeJob, Reply, Request};

use crate::session::judge_jobs::Store;

/// How long past the judgement's deadline the verb reads the agent's reply:
/// an agent start, then the gate's read grace.
const READ_GRACE: Duration = Duration::from_millis(1500);

/// What the verb prints.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Verdict(Verdict),
    Job(JudgeJob),
    Fallback(String),
}

impl Outcome {
    /// The one line of JSON printed for it.
    pub fn line(&self) -> String {
        match self {
            Outcome::Verdict(v) => serde_json::to_string(v),
            Outcome::Job(job) => serde_json::to_string(&json!({ "job": job })),
            Outcome::Fallback(reason) => serde_json::to_string(&json!({ "fallback": reason })),
        }
        .unwrap_or_else(|e| json!({ "fallback": format!("encode: {e}") }).to_string())
    }
}

/// Where an event goes: `session::log_event` outside tests.
pub type EventSink<'a> = &'a dyn Fn(&[(&str, &str)]);

/// Where a call's spend is logged.
pub struct SpendLog<'a> {
    pub session: &'a str,
    pub project: &'a str,
    pub sink: EventSink<'a>,
}

pub fn run(session: &str, handoff: Option<&Path>, parse: bool) -> Result<()> {
    let store = Store::for_session(session).unwrap_or_else(|| usage(&format!("not a session id: {session:?}")));
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).context("reading stdin")?;
    let held = store.resolution().map(|s| s.resolution);
    let judgement = continuance::for_session(
        &profile::user_layer_path(),
        held.as_ref(),
        |p| ways_agent_core::keys::locate_file(p).is_some(),
        store.host_registered(),
    );
    let project = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default();
    let log = SpendLog { session, project: &project, sink: &|fields| crate::session::log_event(fields) };
    let outcome = if parse {
        let answer: JudgeAnswer = serde_json::from_str(&input).unwrap_or_else(|e| usage(&format!("not a judge answer: {e}")));
        parse_answer(answer, judgement, &log)
    } else {
        let conditions: Conditions = serde_json::from_str(&input).unwrap_or_else(|e| usage(&format!("not the conditions: {e}")));
        let handoff = handoff.unwrap_or_else(|| usage("--handoff is required without --parse"));
        judge(session, handoff, conditions, judgement, |req, wait| ways_agent_core::client::call(req, wait, true), &log)
    };
    println!("{}", outcome.line());
    Ok(())
}

fn usage(message: &str) -> ! {
    eprintln!("ways judge continuance: {message}");
    std::process::exit(2)
}

/// Judges the boundary: a job for the caller on `claude-code`, else the
/// verdict the agent answers through `call`, else a fallback naming why.
pub fn judge(
    session: &str,
    handoff: &Path,
    conditions: Conditions,
    judgement: Result<Continuance, String>,
    call: impl FnOnce(Request, Duration) -> Result<Reply, String>,
    log: &SpendLog<'_>,
) -> Outcome {
    let text = match std::fs::read_to_string(handoff) {
        Ok(t) => t,
        Err(e) => return Outcome::Fallback(format!("handoff: reading {}: {e}", handoff.display())),
    };
    let c = match judgement {
        Ok(c) => c,
        Err(reason) => return Outcome::Fallback(reason),
    };
    if c.provider == Provider::ClaudeCode {
        return Outcome::Job(JudgeJob {
            model: c.model,
            system: continuance::SYSTEM.to_string(),
            prompt: continuance::render_prompt(&text, &conditions),
            max_tokens: c.max_tokens as usize,
            timeout_ms: c.timeout_ms,
        });
    }
    let req = ContinuanceRequest { session: session.to_string(), engine: c.engine.clone(), handoff: text, conditions, deadline_ms: Some(c.timeout_ms) };
    let wait = Duration::from_millis(c.timeout_ms) + ways_agent_core::client::START_WAIT + READ_GRACE;
    match call(Request::Continuance(Box::new(req)), wait) {
        Ok(Reply::Continued(r)) => {
            if let Some(call) = &r.call {
                log_call(call, None, log);
            }
            Outcome::Verdict(r.verdict)
        }
        Ok(Reply::Fallback { reason, call, .. }) => {
            if let Some(call) = &call {
                log_call(call, Some(&reason), log);
            }
            Outcome::Fallback(reason)
        }
        // An agent built before the continuance request answers this.
        Ok(Reply::Error { message }) => Outcome::Fallback(format!("agent_error: {message}")),
        Ok(other) => Outcome::Fallback(format!("unexpected_reply: {other:?}")),
        Err(reason) => Outcome::Fallback(reason),
    }
}

/// The verdict in the caller's `$.model.complete` answer to a job, logged
/// as a call on the Claude Code account. A reason, no text, or a reply that
/// is not the closed verdict is a fallback.
pub fn parse_answer(answer: JudgeAnswer, judgement: Result<Continuance, String>, log: &SpendLog<'_>) -> Outcome {
    let (engine, model) = match &judgement {
        Ok(c) if c.provider == Provider::ClaudeCode => (c.engine.clone(), c.model.clone()),
        Ok(c) => (Provider::ClaudeCode.as_str().to_string(), c.model.clone()),
        Err(_) => (Provider::ClaudeCode.as_str().to_string(), "unknown".to_string()),
    };
    let account = |usage: Option<Usage>| continuance::account_call(&engine, Provider::ClaudeCode, &model, usage);
    let JudgeAnswer { text, usage, reason } = answer;
    // A reason is a failed call whatever text came with it.
    let (Some(text), None) = (text, &reason) else {
        let reason = format!("model_error: {}", reason.as_deref().unwrap_or("no text"));
        log_call(&account(usage), Some(&reason), log);
        return Outcome::Fallback(reason);
    };
    match continuance::parse_reply(&text) {
        Ok(v) => {
            log_call(&account(usage), None, log);
            Outcome::Verdict(v)
        }
        Err(e) => {
            log_call(&account(usage), Some(&e.reason()), log);
            Outcome::Fallback(e.reason())
        }
    }
}

/// Logs one continuance call as the gate logs a disclosure call, with
/// purpose `continuance`. Token and cost fields are left out when unknown.
fn log_call(call: &JudgeCall, fallback_reason: Option<&str>, log: &SpendLog<'_>) {
    let mut fields: Vec<(&str, String)> = vec![
        ("event", "judge_call".into()),
        ("purpose", continuance::PURPOSE.into()),
        ("outcome", if fallback_reason.is_some() { "fallback" } else { "judged" }.into()),
        ("engine", call.engine.clone()),
        ("provider", call.provider.as_str().into()),
        ("model", call.model.clone()),
        ("cost_source", call.cost_source.as_str().into()),
    ];
    if let Some(reason) = fallback_reason {
        fields.push(("reason", reason.into()));
    }
    if let Some(u) = &call.usage {
        fields.push(("input_tokens", u.input_tokens.to_string()));
        fields.push(("output_tokens", u.output_tokens.to_string()));
        fields.push(("cache_read_tokens", u.cache_read_tokens.to_string()));
        fields.push(("cache_write_tokens", u.cache_write_tokens.to_string()));
    }
    if let Some(cost) = call.cost_usd {
        fields.push(("cost_usd", format!("{cost:.8}")));
    }
    fields.push(("project", log.project.into()));
    fields.push(("session", log.session.into()));
    (log.sink)(&fields.iter().map(|(k, v)| (*k, v.as_str())).collect::<Vec<_>>());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use ways_agent_core::continuance::{Decision, Sufficiency};
    use ways_agent_core::cost::CostSource;
    use ways_agent_core::protocol::Continued;

    const VALID: &str = r#"{"verdict":"continue","handoff":"sufficient","reason":"two tasks remain and the fill is low","turns_left":6,"next_step":"wire the verb into the boundary tool"}"#;

    fn conditions() -> Conditions {
        serde_json::from_str(include_str!("../../../ways-agent-core/tests/fixtures/conditions.json")).unwrap()
    }

    fn handoff(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ways-continuance-cmd-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("handoff.wrap.md");
        std::fs::write(&path, "<!-- wrap=w1 head=abc -->\nContinue work on the judge.\nDO NEXT: write the verb\n").unwrap();
        path
    }

    fn judgement(engine: &str, provider: Provider) -> Result<Continuance, String> {
        Ok(Continuance { engine: engine.into(), provider, model: "claude-haiku-5-5".into(), timeout_ms: 30_000, max_tokens: 4096 })
    }

    type Events = RefCell<Vec<Vec<(String, String)>>>;

    fn with_log<T>(f: impl FnOnce(&SpendLog<'_>) -> T) -> (T, Vec<Vec<(String, String)>>) {
        let events: Events = RefCell::new(Vec::new());
        let sink = |fields: &[(&str, &str)]| events.borrow_mut().push(fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        let out = f(&SpendLog { session: "s1", project: "/p", sink: &sink });
        (out, events.into_inner())
    }

    fn field<'a>(event: &'a [(String, String)], key: &str) -> Option<&'a str> {
        event.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn on_a_key_engine_the_stub_agent_answers_and_the_verb_prints_its_verdict() {
        let path = handoff("key");
        let verdict = continuance::parse_reply(VALID).unwrap();
        let base = ways_agent_core::profile::shipped()["anthropic"].clone();
        let c = judgement("anthropic", Provider::Anthropic).unwrap();
        let call = continuance::priced_call(&c, &base, Some(Usage { input_tokens: 4000, output_tokens: 100, ..Default::default() }));
        let (out, events) = with_log(|log| {
            judge("s1", &path, conditions(), judgement("anthropic", Provider::Anthropic), |req, wait| {
                let Request::Continuance(req) = req else { panic!("not a continuance request") };
                assert_eq!((req.engine.as_str(), req.deadline_ms, req.session.as_str()), ("anthropic", Some(30_000), "s1"));
                assert!(req.handoff.contains("DO NEXT: write the verb"));
                assert!(wait > Duration::from_secs(30), "waits past the call's deadline");
                Ok(Reply::Continued(Continued {
                    engine: "anthropic".into(),
                    provider: Provider::Anthropic,
                    model: "claude-haiku-5-5".into(),
                    verdict: verdict.clone(),
                    latency_ms: 900,
                    call: Some(call.clone()),
                }))
            }, log)
        });
        assert_eq!(out, Outcome::Verdict(verdict));
        let line: serde_json::Value = serde_json::from_str(&out.line()).unwrap();
        assert_eq!(line, serde_json::from_str::<serde_json::Value>(VALID).unwrap(), "exactly the five fields");
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!((field(e, "event"), field(e, "purpose"), field(e, "outcome")), (Some("judge_call"), Some("continuance"), Some("judged")));
        assert_eq!((field(e, "model"), field(e, "cost_source"), field(e, "session")), (Some("claude-haiku-5-5"), Some("price_table"), Some("s1")));
        // The spend reader takes it as a continuance call.
        let line = serde_json::to_string(&e.iter().map(|(k, v)| (k.clone(), v.clone())).chain([("ts".to_string(), "2026-11-27T09:00:00Z".to_string())]).collect::<std::collections::BTreeMap<_, _>>()).unwrap();
        let spent = ways_agent_core::spend::parse_line(&line).unwrap();
        assert_eq!(spent.purpose, "continuance");
        assert!(spent.cost_usd.is_some());
    }

    #[test]
    fn with_the_agent_unreachable_it_prints_a_fallback_naming_why() {
        let path = handoff("absent");
        for (reply, want) in [
            (Err("agent_absent".to_string()), "agent_absent"),
            (Err("agent_start: no socket after 1.5 s".to_string()), "agent_start: no socket after 1.5 s"),
            // An agent that predates the request cannot parse it.
            (Ok(Reply::Error { message: "bad request: unknown variant `continuance`".into() }), "agent_error: bad request: unknown variant `continuance`"),
            (Ok(Reply::Fallback { reason: "no_key".into(), latency_ms: 0, call: None }), "no_key"),
        ] {
            let (out, events) = with_log(|log| judge("s1", &path, conditions(), judgement("anthropic", Provider::Anthropic), |_, _| reply, log));
            assert_eq!(out, Outcome::Fallback(want.to_string()));
            assert_eq!(out.line(), json!({ "fallback": want }).to_string());
            assert!(events.is_empty(), "no call was made: {events:?}");
        }
        // A fallback after a call logs the call.
        let c = judgement("anthropic", Provider::Anthropic).unwrap();
        let base = ways_agent_core::profile::shipped()["anthropic"].clone();
        let call = Box::new(continuance::priced_call(&c, &base, None));
        let (out, events) = with_log(|log| {
            judge("s1", &path, conditions(), Ok(c.clone()), |_, _| Ok(Reply::Fallback { reason: "deadline".into(), latency_ms: 30_000, call: Some(call) }), log)
        });
        assert_eq!(out, Outcome::Fallback("deadline".into()));
        assert_eq!((field(&events[0], "outcome"), field(&events[0], "reason"), field(&events[0], "cost_source")), (Some("fallback"), Some("deadline"), Some("unknown")));
    }

    #[test]
    fn no_judgement_or_no_handoff_is_a_fallback_and_calls_nothing() {
        let path = handoff("none");
        let never = |_: Request, _: Duration| -> Result<Reply, String> { panic!("called the agent") };
        let (out, _) = with_log(|log| judge("s1", &path, conditions(), Err("no key".into()), never, log));
        assert_eq!(out, Outcome::Fallback("no key".into()));
        let missing = path.with_file_name("gone.md");
        let (out, _) = with_log(|log| judge("s1", &missing, conditions(), judgement("anthropic", Provider::Anthropic), never, log));
        assert!(matches!(&out, Outcome::Fallback(r) if r.starts_with("handoff: reading ")), "{out:?}");
    }

    #[test]
    fn on_claude_code_the_job_names_the_continuance_model_budget_and_deadline() {
        let path = handoff("cc");
        let never = |_: Request, _: Duration| -> Result<Reply, String> { panic!("called the agent") };
        let (out, events) = with_log(|log| judge("s1", &path, conditions(), judgement("claude-code", Provider::ClaudeCode), never, log));
        let Outcome::Job(job) = &out else { panic!("not a job: {out:?}") };
        assert_eq!((job.model.as_str(), job.max_tokens, job.timeout_ms), ("claude-haiku-5-5", 4096, 30_000));
        assert_eq!(job.system, continuance::SYSTEM);
        assert!(job.prompt.contains("```handoff\n<!-- wrap=w1 head=abc -->\nContinue work on the judge.\nDO NEXT: write the verb\n```"), "{}", job.prompt);
        assert!(!job.prompt.contains("record_judgements"));
        let line: serde_json::Value = serde_json::from_str(&out.line()).unwrap();
        assert_eq!(line["job"]["model"], "claude-haiku-5-5");
        assert_eq!(serde_json::from_value::<JudgeJob>(line["job"].clone()).unwrap(), *job);
        assert!(events.is_empty(), "no call yet");
    }

    #[test]
    fn parse_of_a_canned_answer_prints_the_verdict_and_logs_an_account_call() {
        let answer: JudgeAnswer = serde_json::from_value(json!({
            "text": format!("```json\n{VALID}\n```"),
            "usage": {"input_tokens": 4000, "output_tokens": 120, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}
        }))
        .unwrap();
        let (out, events) = with_log(|log| parse_answer(answer, judgement("claude-code", Provider::ClaudeCode), log));
        let Outcome::Verdict(v) = &out else { panic!("not a verdict: {out:?}") };
        assert_eq!((v.verdict, v.handoff, v.turns_left), (Decision::Continue, Sufficiency::Sufficient, 6));
        let e = &events[0];
        assert_eq!((field(e, "purpose"), field(e, "cost_source"), field(e, "engine"), field(e, "input_tokens")), (Some("continuance"), Some(CostSource::Account.as_str()), Some("claude-code"), Some("4000")));
        // Prose besides the verdict, a model error, or no text: a fallback.
        let prose = JudgeAnswer { text: Some(format!("I would continue.\n{VALID}")), ..Default::default() };
        let (out, events) = with_log(|log| parse_answer(prose, judgement("claude-code", Provider::ClaudeCode), log));
        assert!(matches!(&out, Outcome::Fallback(r) if r.starts_with("invalid_reply: ")), "{out:?}");
        assert_eq!((field(&events[0], "outcome"), field(&events[0], "cost_source")), (Some("fallback"), Some("unknown")));
        let failed = JudgeAnswer { reason: Some("api-error".into()), text: Some(VALID.into()), ..Default::default() };
        let (out, _) = with_log(|log| parse_answer(failed, Err("no key".into()), log));
        assert_eq!(out, Outcome::Fallback("model_error: api-error".into()));
        let (out, _) = with_log(|log| parse_answer(JudgeAnswer::default(), judgement("claude-code", Provider::ClaudeCode), log));
        assert_eq!(out, Outcome::Fallback("model_error: no text".into()));
    }
}
