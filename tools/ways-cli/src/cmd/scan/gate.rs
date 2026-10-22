//! The relevance gate on the prompt lane (ADR-196).
//!
//! After the matcher and the refire check, and before a fire is recorded, the
//! ways the lane would show are sent to the ways agent in one request with the
//! conversation's last turns. In enforce mode a way judged below the engine's
//! threshold is not shown and keeps its refire budget; in shadow mode every
//! verdict is logged and nothing is blocked. Ways with `pattern_strict` are not
//! judged. A request carries at most the profile's `max_candidates`, in the
//! matcher's order: judge latency grows with each candidate, and past the cap
//! the rest pass unjudged, except a way whose ancestor the judge blocked, which
//! is blocked with it and logged as a `block` with `reason: ancestor`, the
//! ancestor's id and its verdict's fields. A `pattern_strict` child of a
//! blocked parent is logged the same way, as kept out by the judge, since the
//! parent's block is what kept it out. Every verdict, cap and fallback is logged to
//! `events.jsonl`, and any failure fails open: the matcher's decision stands.
//! Each provider call is logged once more as `judge_call`, with its tokens
//! and cost, so spend is counted per call, not per way (#741). The gate also
//! hands every verdict and its own status back to the scan, which writes them
//! to the turn's decision record (ADR-701 §2).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ways_agent_core::cost::{CostSource, JudgeCall, Usage};
use ways_agent_core::engine;
use ways_agent_core::judge::{self, way_text, Candidate, Role, Turn};
use ways_agent_core::profile::{self, Mode, Provider, Settings, UserLayer};
use ways_agent_core::protocol::{JudgeAnswer, JudgeJob, JudgeRequest, Judged, Reply, Request, Verdict};

use crate::session::judge_jobs::{SessionEngine, Store, Waited};


/// Grace beyond the engine's deadline for the hook's read, so the agent's own
/// fallback reply arrives before the hook gives up. The agent waits for a
/// provider slot inside its deadline; starting an agent (up to 1.5 s, in the
/// client) comes before the read, so a cold start can take about
/// `timeout_ms` plus 3 s.
const READ_GRACE: Duration = Duration::from_millis(1500);

/// The least budget a Claude Code job is written with. `$.model.complete`
/// refuses a `timeoutMs` of 0, and a call given less than this cannot answer.
const MIN_JOB: Duration = Duration::from_millis(50);

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

/// One verdict of the judge, as its `way_judged` line logs it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Judgement {
    pub p_yes: f64,
    /// `pass`, `block` or `would_block`.
    pub verdict: &'static str,
}

/// How the gate ran on one scan.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) enum Status {
    /// Off: no engine and no key, or mode off.
    #[default]
    Off,
    /// On, with nothing to judge: no way is pending, or every pending way is
    /// `pattern_strict`.
    Idle,
    /// The judge answered.
    Judged { engine: String, model: String, judge_ms: u64 },
    /// The gate failed open; `reason` as its `gate_fallback` line logs it.
    Fallback { reason: String },
}

/// What the gate decided on one scan: the ways it blocks, every verdict, how
/// it ran, and the ways past the cap that went unjudged.
#[derive(Default)]
pub(super) struct Gate {
    pub blocked: Blocked,
    pub judgements: HashMap<String, Judgement>,
    pub status: Status,
    pub capped: Vec<String>,
}

impl Gate {
    fn fallback(reason: &str) -> Self {
        Gate { status: Status::Fallback { reason: reason.to_string() }, ..Gate::default() }
    }

    #[cfg(test)]
    fn ids(&self) -> std::collections::HashSet<String> {
        self.blocked.ids()
    }
}

/// The ways the gate blocked, each with the fields of its `way_judged` line.
#[derive(Default)]
pub(super) struct Blocked(HashMap<String, Vec<(String, String)>>);

impl Blocked {
    pub(super) fn contains(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    /// The fields of `id`'s `way_judged` block line, when it is blocked.
    pub(super) fn fields(&self, id: &str) -> Option<&[(String, String)]> {
        self.0.get(id).map(Vec::as_slice)
    }

    #[cfg(test)]
    fn ids(&self) -> std::collections::HashSet<String> {
        self.0.keys().cloned().collect()
    }

    /// Block `id` with its nearest blocked ancestor, logging a `way_judged`
    /// block that carries the ancestor's verdict, `reason: ancestor` and the
    /// ancestor the judge judged. The judge never saw `id`, so the line holds
    /// no per-call figures (`judge_ms`, `gate_ms`, `candidates`), and a reader
    /// counting the judge's own verdicts skips `reason: ancestor`. `false`
    /// when no ancestor of `id` is blocked, or `id` already is.
    pub(super) fn with_ancestor(&mut self, id: &str, log: &LogContext<'_>) -> bool {
        if self.contains(id) {
            return false;
        }
        let Some(fields) = self
            .0
            .iter()
            .filter(|(b, _)| super::order::is_proper_ancestor(b, id))
            .max_by_key(|(b, _)| b.len())
            .map(|(b, f)| {
                let mut f: Vec<(String, String)> =
                    f.iter().filter(|(k, _)| !matches!(k.as_str(), "judge_ms" | "gate_ms" | "candidates")).cloned().collect();
                if !f.iter().any(|(k, _)| k == "reason") {
                    f.push(("reason".into(), "ancestor".into()));
                    f.push(("ancestor".into(), b.clone()));
                }
                for (k, v) in &mut f {
                    if k == "way" {
                        *v = id.to_string();
                    }
                }
                f
            })
        else {
            return false;
        };
        (log.sink)(&fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>());
        self.0.insert(id.to_string(), fields);
        true
    }
}

/// What the gate decides. Nothing is blocked when the gate is off, in shadow
/// mode, or failed.
pub(super) fn apply(
    pending: &[Pending<'_>],
    prompt: &str,
    response_context: Option<&str>,
    log: &LogContext<'_>,
) -> Gate {
    use ways_agent_core::keys;
    let store = Store::for_session(log.session_id);
    // The agent reads the key file, never a hook's environment, so only a key
    // file turns the gate on: an ANTHROPIC_API_KEY set for Claude Code itself
    // must not send every prompt to an agent with no key to use.
    apply_from(&profile::user_layer_path(), |p| keys::locate_file(p).is_some(), store.as_ref(), pending, prompt, response_context, log, |req, timeout| {
        ways_agent_core::client::call(Request::Judge(req), timeout, true)
    })
}

/// [`apply`] with the agent file, the key check, the session's judge store
/// and the agent call injected, so a test can show which configurations
/// never reach a provider.
///
/// The session's engine is resolved at its first judge request and held
/// (ADR-708 §2). A key engine goes to the ways agent, named in the request;
/// `claude-code` goes to the session's judge host as a job ([`handoff`]);
/// none fails open with the resolution's `engine_unavailable` reason. No
/// request reaches both. A store of `None`, a session id that names no
/// directory, can have no host, so it never resolves to `claude-code`.
#[allow(clippy::too_many_arguments)]
fn apply_from(
    path: &std::path::Path,
    has_key: impl Fn(Provider) -> bool,
    store: Option<&Store>,
    pending: &[Pending<'_>],
    prompt: &str,
    response_context: Option<&str>,
    log: &LogContext<'_>,
    call: impl FnOnce(JudgeRequest, Duration) -> Result<Reply, String>,
) -> Gate {
    let begun = Instant::now();
    let (user, layer) = match settings(path, &has_key, log) {
        Ok(Some(found)) => found,
        Ok(None) => return Gate::default(),
        Err(reason) => return Gate::fallback(&reason),
    };
    let to_judge = pending.iter().filter(|p| !p.pattern_strict).count().min(layer.profile.max_candidates);
    if to_judge == 0 {
        return Gate { status: Status::Idle, ..Gate::default() };
    }
    let profiles = profile::profiles(&user);
    let resolve = |host: bool| {
        let resolution = engine::resolve_session(user.engine.as_deref(), &profiles, host, &has_key);
        let provider = resolution.engine.as_ref().and_then(|e| profiles.get(e)).map(|p| p.provider);
        SessionEngine { resolution, provider }
    };
    let session = match store.map(|s| s.resolve_once(resolve)) {
        Some(Ok(found)) => found,
        Some(Err(e)) => return fallback(&format!("resolution: {e}"), to_judge, log, "0"),
        None => resolve(false),
    };
    let Some(name) = session.resolution.engine else {
        // No key and no engine named: off, as before ADR-708.
        if session.resolution.reason == engine::NO_KEY {
            return Gate::default();
        }
        return fallback(&session.resolution.reason, to_judge, log, "0");
    };
    // The session's engine is a profile that agent.yaml no longer has.
    let Some(profile) = profiles.get(&name).cloned() else {
        return fallback(&format!("{}: {}", engine::CONFIG, profile::names_no_profile(&name)), to_judge, log, "0");
    };
    let settings = Settings { engine: name, engine_set: layer.engine_set, profile, mode: layer.mode };
    match (settings.profile.provider, store) {
        (Provider::ClaudeCode, Some(store)) => {
            run(pending, prompt, response_context, &settings, log, |req, _| handoff(store, &settings, req, begun))
        }
        // `resolve(false)` never names claude-code.
        (Provider::ClaudeCode, None) => unreachable!("a session with no store has no host"),
        _ => run(pending, prompt, response_context, &settings, log, |mut req, timeout| {
            req.engine = Some(settings.engine.clone());
            call(req, timeout)
        }),
    }
}

/// The gate's agent.yaml and the settings it resolves to. `None` when the
/// gate is off: no engine and no key, or mode off. A configuration error is
/// logged as `gate_fallback` and returned as its reason. An agent.yaml that
/// does not parse, or a bad `mode`, fails closed: the gate is off and
/// nothing is sent to a provider (ADR-503 addendum).
fn settings(
    path: &std::path::Path,
    has_key: impl Fn(Provider) -> bool,
    log: &LogContext<'_>,
) -> Result<Option<(UserLayer, Settings)>, String> {
    let read = profile::gate_layer(path).and_then(|user| Ok(profile::resolve(&user, has_key)?.map(|s| (user, s))));
    match read {
        Ok(s) => Ok(s.filter(|(_, s)| s.mode != Mode::Off)),
        Err(e) => {
            let reason = format!("config: {e:#}");
            // Logged for the tuning passes, and said on stderr, since a hook
            // shows nothing else: the gate is off until agent.yaml is fixed.
            (log.sink)(&[
                ("event", "gate_fallback"),
                ("reason", &reason),
                ("hook", log.hook_event),
                ("scope", log.scope),
                ("project", log.project_dir),
                ("session", log.session_id),
            ]);
            eprintln!("[ways] settings: {e:#}");
            Err(reason)
        }
    }
}

/// Judges `req` through the session's Claude Code host (ADR-708 §5): writes
/// the JSON-only request as a job, waits for the host's answer until the
/// gate's budget (`timeout_ms` from `begun`) plus [`READ_GRACE`] runs out,
/// and parses it with [`judge::parse_reply`]. The job's own `timeout_ms` is
/// what remains of the budget when it is written. Every failure comes back
/// as a fallback reply, carrying a call when the host took the job and so
/// may have spent one: `deadline` on silence, `invalid_reply: …` on a reply
/// that does not parse, `model_error: …` when the model call failed.
fn handoff(store: &Store, settings: &Settings, req: JudgeRequest, begun: Instant) -> Result<Reply, String> {
    let p = &settings.profile;
    let budget = Duration::from_millis(p.timeout_ms);
    let remaining = budget.saturating_sub(begun.elapsed());
    if remaining < MIN_JOB {
        return Ok(Reply::Fallback { reason: "deadline: before call".into(), latency_ms: 0, call: None });
    }
    let n = req.candidates.len();
    let turns = judge::render_turns(&req.turns, p.turns, p.max_turn_chars);
    let job = JudgeJob {
        model: p.model.clone(),
        system: judge::SYSTEM.to_string(),
        prompt: judge::render_json_prompt(&judge::render_prompt(&turns, &req.candidates)),
        max_tokens: judge::max_tokens(n),
        timeout_ms: remaining.as_millis() as u64,
    };
    let id = store.write_job(&job).map_err(|e| format!("job: {e}"))?;
    let written = Instant::now();
    let waited = store.wait_answer(&id, begun + budget + READ_GRACE);
    store.finish(&id);
    let latency_ms = written.elapsed().as_millis() as u64;
    // The spend is the account's usage: no price (ADR-708 §10).
    let call = |usage: Option<Usage>| JudgeCall {
        engine: settings.engine.clone(),
        provider: p.provider,
        model: p.model.clone(),
        candidates: n,
        usage,
        cost_usd: None,
        cost_source: CostSource::Unknown,
    };
    let JudgeAnswer { text, usage, reason } = match waited {
        Waited::Answered(a) => a,
        Waited::Silent { taken } => {
            return Ok(Reply::Fallback { reason: "deadline".into(), latency_ms, call: taken.then(|| Box::new(call(None))) })
        }
    };
    // A reason is a failed call whatever text came with it.
    let (Some(text), None) = (text, &reason) else {
        let reason = format!("model_error: {}", reason.as_deref().unwrap_or("no text"));
        return Ok(Reply::Fallback { reason, latency_ms, call: Some(Box::new(call(usage))) });
    };
    match judge::parse_reply(&text, n) {
        Ok(p_yes) => Ok(Reply::Judged(Judged {
            engine: settings.engine.clone(),
            provider: p.provider,
            model: p.model.clone(),
            mode: settings.mode,
            threshold: p.threshold,
            verdicts: req.candidates.iter().zip(p_yes).map(|(c, p_yes)| Verdict { id: c.id.clone(), p_yes }).collect(),
            latency_ms,
            call: Some(call(usage)),
        })),
        Err(e) => Ok(Reply::Fallback { reason: e.reason(), latency_ms, call: Some(Box::new(call(usage))) }),
    }
}

/// A judge block of `id` (p_yes 0.05), for tests of the scan's call site.
#[cfg(test)]
pub(super) fn test_blocked(id: &str, log: &LogContext<'_>) -> Blocked {
    test_gate(&[(id, 0.05)], Mode::Enforce, log).blocked
}

/// The gate as the judge's `verdicts` in `mode` leave it (threshold 0.3,
/// 800 ms), for tests of what the scan records.
#[cfg(test)]
pub(super) fn test_gate(verdicts: &[(&str, f64)], mode: Mode, log: &LogContext<'_>) -> Gate {
    use ways_agent_core::profile::Provider;
    use ways_agent_core::protocol::Verdict;
    let j = Judged {
        engine: "anthropic".into(),
        provider: Provider::Anthropic,
        model: "claude-haiku-4-5".into(),
        mode,
        threshold: 0.3,
        verdicts: verdicts.iter().map(|(id, p)| Verdict { id: id.to_string(), p_yes: *p }).collect(),
        latency_ms: 800,
        call: None,
    };
    decide(&j, log, "5")
}

/// The gate with the agent call injected, so tests can stand in an engine.
fn run(
    pending: &[Pending<'_>],
    prompt: &str,
    response_context: Option<&str>,
    settings: &Settings,
    log: &LogContext<'_>,
    call: impl FnOnce(JudgeRequest, Duration) -> Result<Reply, String>,
) -> Gate {
    let mut judged: Vec<&Pending<'_>> = pending.iter().filter(|p| !p.pattern_strict).collect();
    if judged.is_empty() {
        return Gate { status: Status::Idle, ..Gate::default() };
    }
    let cap = settings.profile.max_candidates;
    let mut unjudged: Vec<&str> = Vec::new();
    if judged.len() > cap {
        unjudged = judged.split_off(cap).iter().map(|p| p.id).collect();
        (log.sink)(&[
            ("event", "gate_capped"),
            ("judged", &cap.to_string()),
            ("unjudged", &unjudged.len().to_string()),
            ("ways", &unjudged.join(",")),
            ("hook", log.hook_event),
            ("scope", log.scope),
            ("project", log.project_dir),
            ("session", log.session_id),
        ]);
    }
    let request = JudgeRequest {
        session: log.session_id.to_string(),
        tool: "claude-code".to_string(),
        turns: turns(prompt, response_context),
        candidates: judged
            .iter()
            .map(|p| Candidate { id: p.id.to_string(), text: way_text(p.id, p.description) })
            .collect(),
        engine: None,
    };
    let begun = Instant::now();
    let reply = call(request, Duration::from_millis(settings.profile.timeout_ms) + READ_GRACE);
    let elapsed_ms = begun.elapsed().as_millis().to_string();
    let mut gate = match reply {
        Ok(Reply::Judged(j)) => {
            // An agent older than #741 reports no call; the call was still made.
            let call = j.call.clone().unwrap_or_else(|| JudgeCall::priced(&j.engine, &settings.profile, j.verdicts.len(), None));
            log_call(&call, None, log);
            let mut gate = decide(&j, log, &elapsed_ms);
            // A way's guidance presumes its parent's: an unjudged way under a
            // blocked ancestor goes with it.
            for id in &unjudged {
                gate.blocked.with_ancestor(id, log);
            }
            gate
        }
        Ok(Reply::Fallback { reason, call, .. }) => {
            // An agent older than #741 sends no call; its reason says whether
            // it reached the provider.
            // `deadline` alone is a provider timeout; `deadline: …` is the
            // agent running out of time before it called. A Claude Code host
            // always says: its fallback carries a call when it made one.
            let reached = settings.profile.provider != Provider::ClaudeCode
                && (reason == "deadline" || ["transport", "provider_", "answer"].iter().any(|p| reason.starts_with(p)));
            match call {
                Some(call) => log_call(&call, Some(&reason), log),
                None if reached => {
                    log_call(&JudgeCall::priced(&settings.engine, &settings.profile, judged.len(), None), Some(&reason), log)
                }
                None => {}
            }
            fallback(&reason, judged.len(), log, &elapsed_ms)
        }
        Ok(Reply::Error { message }) => fallback(&format!("agent_error: {message}"), judged.len(), log, &elapsed_ms),
        Ok(other) => fallback(&format!("unexpected_reply: {other:?}"), judged.len(), log, &elapsed_ms),
        Err(reason) => {
            // The hook stopped reading while the agent was waiting on the
            // provider (the agent answers within its own deadline otherwise):
            // count a call of unknown cost. An agent that dies mid-call
            // (`io`, `agent_closed`) logs nothing; the call is not known.
            if reason == "deadline" {
                log_call(&JudgeCall::priced(&settings.engine, &settings.profile, judged.len(), None), Some(&reason), log);
            }
            fallback(&reason, judged.len(), log, &elapsed_ms)
        }
    };
    gate.capped = unjudged.iter().map(|id| id.to_string()).collect();
    gate
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

/// Logs each verdict and returns them with the ways to block.
fn decide(j: &Judged, log: &LogContext<'_>, elapsed_ms: &str) -> Gate {
    let mut gate = Gate {
        status: Status::Judged { engine: j.engine.clone(), model: j.model.clone(), judge_ms: j.latency_ms },
        ..Gate::default()
    };
    let threshold = format!("{:.2}", j.threshold);
    for v in &j.verdicts {
        let pass = v.p_yes >= j.threshold;
        let verdict = match (pass, j.mode) {
            (true, _) => "pass",
            (false, Mode::Enforce) => "block",
            (false, _) => "would_block",
        };
        let agent = crate::session::current_agent();
        let fields: Vec<(String, String)> = [
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
            // The agent judged, as fires and checks record it (#818), so the
            // timeline marks that agent's row (#814).
            ("agent_id", &agent),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        (log.sink)(&fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>());
        gate.judgements.insert(v.id.clone(), Judgement { p_yes: v.p_yes, verdict });
        if verdict == "block" {
            gate.blocked.0.insert(v.id.clone(), fields);
        }
    }
    gate
}

/// Logs one provider call: `outcome` is `judged`, or `fallback` with the
/// fallback's reason. Token and cost fields are left out when unknown, so a
/// report never reads an unknown cost as zero.
fn log_call(call: &JudgeCall, fallback_reason: Option<&str>, log: &LogContext<'_>) {
    let candidates = call.candidates.to_string();
    let mut fields: Vec<(&str, String)> = vec![
        ("event", "judge_call".into()),
        ("outcome", if fallback_reason.is_some() { "fallback" } else { "judged" }.into()),
        ("engine", call.engine.clone()),
        ("provider", call.provider.as_str().into()),
        ("model", call.model.clone()),
        ("candidates", candidates),
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
    let tail = [("hook", log.hook_event), ("scope", log.scope), ("project", log.project_dir), ("session", log.session_id)];
    let mut line: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
    line.extend(tail);
    (log.sink)(&line);
}

/// Logs a fallback; nothing is blocked.
fn fallback(reason: &str, candidates: usize, log: &LogContext<'_>, elapsed_ms: &str) -> Gate {
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
    Gate::fallback(reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::judge_jobs::Taken;
    use std::collections::HashSet;

    fn settings(mode: Mode) -> Settings {
        let user = UserLayer { mode: Some(mode), ..Default::default() };
        profile::resolve(&user, |p| p == Provider::Anthropic).unwrap().unwrap()
    }

    /// The hooks whose scan runs this gate (the prompt lane and the queued
    /// messages it scans) must outlast the longest wait a profile allows: the
    /// deadline, the agent's start and the read grace. Claude Code kills a
    /// hook at its bound and drops its output, the fail-open record included.
    #[test]
    fn the_prompt_lane_hooks_outlast_the_longest_judge_wait() {
        let repo_settings = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into()),
        )
        .join("../../settings.json");
        let Ok(raw) = std::fs::read_to_string(&repo_settings) else { return }; // not a source checkout
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let wait = Duration::from_millis(profile::MAX_TIMEOUT_MS) + ways_agent_core::client::START_WAIT + READ_GRACE;
        let mut seen = HashSet::new();
        for entries in v["hooks"].as_object().unwrap().values() {
            for cmd in entries.as_array().unwrap().iter().flat_map(|e| e["hooks"].as_array().unwrap()) {
                let command = cmd["command"].as_str().unwrap_or("");
                let Some(script) = ["check-prompt.sh", "check-queued.sh"].into_iter().find(|s| command.ends_with(s)) else { continue };
                seen.insert(script);
                let bound = Duration::from_secs(cmd["timeout"].as_u64().unwrap_or(600));
                assert!(bound > wait, "{script}: a {bound:?} bound cannot hold a {wait:?} judge wait");
            }
        }
        assert_eq!(seen.len(), 2, "found both prompt-lane hooks: {seen:?}");
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
            call: None,
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
        assert_eq!(blocked.ids(), HashSet::from(["data/migrations".to_string()]));
        let events = events.borrow();
        let verdicts: Vec<_> = events.iter().filter(|e| field(e, "event") == "way_judged").collect();
        assert_eq!(verdicts.len(), 2);
        assert_eq!(field(verdicts[0], "verdict"), "pass");
        assert_eq!(field(verdicts[1], "verdict"), "block");
        assert_eq!(field(verdicts[1], "p_yes"), "0.050");
        // Each verdict names the agent judged (#814): main where the hook
        // named none. No test sets CLAUDE_AGENT_ID; a runner that does gets
        // its agent's key.
        let agent = if std::env::var_os("CLAUDE_AGENT_ID").is_none() { "main".to_string() } else { crate::session::current_agent() };
        assert!(verdicts.iter().all(|v| field(v, "agent_id") == agent), "{verdicts:?}");
    }

    fn calls_in(events: &[Vec<(String, String)>]) -> Vec<Vec<(String, String)>> {
        events.iter().filter(|e| field(e, "event") == "judge_call").cloned().collect()
    }

    fn calls(events: &Events) -> Vec<Vec<(String, String)>> {
        calls_in(&events.borrow())
    }

    #[test]
    fn a_judged_request_logs_one_priced_call() {
        use ways_agent_core::cost::Usage;
        let events = Events::default();
        let sink = recorder(&events);
        let s = settings(Mode::Enforce);
        run(&pending(), "q", None, &s, &log(&sink), |_, _| {
            let Reply::Judged(mut j) = judged(Mode::Enforce, &[("softwaredev/code/security/secrets", 0.9), ("data/migrations", 0.9)]) else { unreachable!() };
            let usage = Usage { input_tokens: 1000, output_tokens: 100, ..Default::default() };
            j.call = Some(JudgeCall::priced("anthropic", &s.profile, 2, Some(usage)));
            Ok(Reply::Judged(j))
        });
        let calls = calls(&events);
        assert_eq!(calls.len(), 1);
        let c = &calls[0];
        assert_eq!((field(c, "outcome"), field(c, "candidates"), field(c, "cost_source")), ("judged", "2", "price_table"));
        assert_eq!((field(c, "input_tokens"), field(c, "output_tokens")), ("1000", "100"));
        assert_eq!(field(c, "cost_usd"), "0.00150000");
        assert_eq!(field(c, "session"), "test-gate");
    }

    #[test]
    fn a_call_without_usage_is_logged_as_unknown_never_zero() {
        let s = settings(Mode::Enforce);
        let unpriced = JudgeCall::priced("anthropic", &s.profile, 2, None);
        let cases: Vec<(Result<Reply, String>, &str, &str)> = vec![
            // An agent older than #741 sends no call with its verdicts.
            (Ok(judged(Mode::Enforce, &[])), "judged", ""),
            (Ok(Reply::Fallback { reason: "deadline".into(), latency_ms: 2000, call: Some(Box::new(unpriced)) }), "fallback", "deadline"),
            // An agent older than #741 sends a reached-provider fallback without its call.
            (Ok(Reply::Fallback { reason: "transport: reset".into(), latency_ms: 300, call: None }), "fallback", "transport: reset"),
            // The hook gave up reading; the agent may still have called.
            (Err("deadline".into()), "fallback", "deadline"),
        ];
        for (reply, outcome, reason) in cases {
            let events = Events::default();
            let sink = recorder(&events);
            run(&pending(), "q", None, &s, &log(&sink), |_, _| reply);
            let calls = calls(&events);
            assert_eq!(calls.len(), 1, "{outcome} {reason}");
            let c = &calls[0];
            assert_eq!((field(c, "outcome"), field(c, "reason"), field(c, "cost_source")), (outcome, reason, "unknown"));
            assert!(c.iter().all(|(k, _)| k != "cost_usd" && k != "input_tokens"));
        }
    }

    #[test]
    fn no_call_is_logged_when_none_was_made() {
        let s = settings(Mode::Enforce);
        // `deadline: before call`: the agent ran out of time waiting for a
        // slot and never called the provider, so no call of unknown cost.
        for reply in [
            Err("agent_absent".to_string()),
            Ok(Reply::Fallback { reason: "no_key".into(), latency_ms: 0, call: None }),
            Ok(Reply::Fallback { reason: "deadline: before call".into(), latency_ms: 2000, call: None }),
        ] {
            let events = Events::default();
            let sink = recorder(&events);
            run(&pending(), "q", None, &s, &log(&sink), |_, _| reply);
            assert!(calls(&events).is_empty());
        }
    }

    #[test]
    fn shadow_blocks_nothing_and_logs_would_block() {
        let events = Events::default();
        let sink = recorder(&events);
        let blocked = run(&pending(), "q", None, &settings(Mode::Shadow), &log(&sink), |_, _| {
            Ok(judged(Mode::Shadow, &[("softwaredev/code/security/secrets", 0.1), ("data/migrations", 0.05)]))
        });
        assert!(blocked.ids().is_empty());
        let events = events.borrow();
        assert!(events.iter().filter(|e| field(e, "event") == "way_judged").all(|e| field(e, "verdict") == "would_block"));
    }

    #[test]
    fn every_failure_fails_open_and_logs_its_reason() {
        let s = settings(Mode::Enforce);
        let replies: Vec<(Result<Reply, String>, &str)> = vec![
            (Err("agent_absent".into()), "agent_absent"),
            (Err("deadline".into()), "deadline"),
            (Ok(Reply::Fallback { reason: "no_key".into(), latency_ms: 0, call: None }), "no_key"),
            (Ok(Reply::Error { message: "protocol 2 not served".into() }), "agent_error: protocol 2 not served"),
        ];
        for (reply, reason) in replies {
            let events = Events::default();
            let sink = recorder(&events);
            let blocked = run(&pending(), "q", None, &s, &log(&sink), |_, _| reply);
            assert!(blocked.ids().is_empty());
            let events = events.borrow();
            let fallback = events.iter().find(|e| field(e, "event") == "gate_fallback").unwrap();
            assert_eq!(field(fallback, "reason"), reason);
            assert_eq!(field(fallback, "candidates"), "2");
        }
    }

    #[test]
    fn past_the_cap_ways_pass_unjudged_and_the_cap_is_logged() {
        let ids: Vec<String> = (0..10).map(|i| format!("w/{i}")).collect();
        let many: Vec<Pending<'_>> =
            ids.iter().map(|id| Pending { id, description: "d", pattern_strict: false }).collect();
        let s = settings(Mode::Enforce);
        assert_eq!(s.profile.max_candidates, 8);
        let events = Events::default();
        let sink = recorder(&events);
        let blocked = run(&many, "q", None, &s, &log(&sink), |req, _| {
            let sent: Vec<&str> = req.candidates.iter().map(|c| c.id.as_str()).collect();
            assert_eq!(sent, ids[..8].iter().map(String::as_str).collect::<Vec<_>>());
            let low: Vec<(&str, f64)> = sent.iter().map(|id| (*id, 0.05)).collect();
            Ok(judged(Mode::Enforce, &low))
        });
        assert_eq!(blocked.ids(), ids[..8].iter().cloned().collect::<HashSet<_>>());
        let events = events.borrow();
        assert_eq!(field(&events[0], "event"), "gate_capped");
        assert_eq!(field(&events[0], "unjudged"), "2");
        assert_eq!(field(&events[0], "ways"), "w/8,w/9");
        assert_eq!(events.iter().filter(|e| field(e, "event") == "way_judged").count(), 8);
    }

    /// The scan's decision record reads every verdict, not only the blocks,
    /// and how the gate ran (ADR-701 §2).
    #[test]
    fn the_gate_returns_every_verdict_its_status_and_the_capped_ways() {
        let events = Events::default();
        let sink = recorder(&events);
        let g = run(&pending(), "q", None, &settings(Mode::Enforce), &log(&sink), |_, _| {
            Ok(judged(Mode::Enforce, &[("softwaredev/code/security/secrets", 0.92), ("data/migrations", 0.05)]))
        });
        assert_eq!(g.judgements["softwaredev/code/security/secrets"], Judgement { p_yes: 0.92, verdict: "pass" });
        assert_eq!(g.judgements["data/migrations"], Judgement { p_yes: 0.05, verdict: "block" });
        assert!(!g.judgements.contains_key("meta/strict"), "a strict way is never judged");
        assert_eq!(g.status, Status::Judged { engine: "anthropic".into(), model: "claude-haiku-4-5".into(), judge_ms: 800 });
        assert!(g.capped.is_empty());

        let ids: Vec<String> = (0..10).map(|i| format!("w/{i}")).collect();
        let g = run(&pending_ids(&ids, 0), "q", None, &settings(Mode::Enforce), &log(&sink), |_, _| Err("deadline".into()));
        assert_eq!(g.status, Status::Fallback { reason: "deadline".into() });
        assert_eq!(g.capped, ["w/8", "w/9"]);
        assert!(g.judgements.is_empty());

        let strict = vec![Pending { id: "a", description: "d", pattern_strict: true }];
        assert_eq!(run(&strict, "q", None, &settings(Mode::Enforce), &log(&sink), |_, _| panic!("no call")).status, Status::Idle);
    }

    fn pending_ids(ids: &[String], strict: usize) -> Vec<Pending<'_>> {
        ids.iter()
            .enumerate()
            .map(|(i, id)| Pending { id, description: "d", pattern_strict: i < strict })
            .collect()
    }

    #[test]
    fn an_unjudged_way_under_a_blocked_ancestor_is_blocked_with_it() {
        let mut ids: Vec<String> = (0..7).map(|i| format!("w/{i}")).collect();
        ids.extend(["p".to_string(), "p/c".to_string(), "q/c".to_string()]);
        let events = Events::default();
        let sink = recorder(&events);
        let blocked = run(&pending_ids(&ids, 0), "q", None, &settings(Mode::Enforce), &log(&sink), |req, _| {
            let v: Vec<(&str, f64)> =
                req.candidates.iter().map(|c| (c.id.as_str(), if c.id == "p" { 0.05 } else { 0.9 })).collect();
            Ok(judged(Mode::Enforce, &v))
        });
        assert_eq!(blocked.ids(), HashSet::from(["p".to_string(), "p/c".to_string()]));
        // The child's block is logged with its ancestor's verdict, so a
        // reader of the log counts it among what the judge kept out.
        let events = events.borrow();
        let child = events.iter().find(|e| field(e, "event") == "way_judged" && field(e, "way") == "p/c").expect("p/c logged");
        let got: Vec<&str> = ["verdict", "p_yes", "threshold", "mode", "engine", "model", "reason", "ancestor"].iter().map(|k| field(child, k)).collect();
        assert_eq!(got, ["block", "0.050", "0.30", "enforce", "anthropic", "claude-haiku-4-5", "ancestor", "p"]);
        assert!(events.iter().all(|e| field(e, "way") != "q/c"), "q/c passes unjudged and unlogged");
    }

    /// A way blocked with an ancestor that was itself blocked with one names
    /// the ancestor the judge judged; a way already blocked, or with no
    /// blocked ancestor, is left alone.
    #[test]
    fn with_ancestor_names_the_judged_ancestor_and_logs_once() {
        let events = Events::default();
        let sink = recorder(&events);
        let lc = log(&sink);
        let mut blocked = decide(
            &match judged(Mode::Enforce, &[("p", 0.05)]) { Reply::Judged(j) => j, _ => unreachable!() },
            &lc,
            "5",
        )
        .blocked;
        assert!(blocked.with_ancestor("p/c", &lc));
        assert!(blocked.with_ancestor("p/c/d", &lc));
        assert!(!blocked.with_ancestor("p/c", &lc), "already blocked");
        assert!(!blocked.with_ancestor("q", &lc), "no blocked ancestor");
        let events = events.borrow();
        assert_eq!(events.len(), 3);
        assert_eq!((field(&events[2], "way"), field(&events[2], "ancestor"), field(&events[2], "reason")), ("p/c/d", "p", "ancestor"));
        assert!(events[1..].iter().all(|e| field(e, "judge_ms").is_empty() && field(e, "candidates").is_empty()), "no per-call figures on an ancestor block");
    }

    #[test]
    fn strict_ways_do_not_count_against_the_cap_and_a_full_request_is_not_capped() {
        let ids: Vec<String> = (0..9).map(|i| format!("w/{i}")).collect();
        let events = Events::default();
        let sink = recorder(&events);
        run(&pending_ids(&ids, 1), "q", None, &settings(Mode::Enforce), &log(&sink), |req, _| {
            assert_eq!(req.candidates.len(), 8);
            Ok(judged(Mode::Enforce, &[]))
        });
        assert!(events.borrow().iter().all(|e| field(e, "event") != "gate_capped"));
    }

    #[test]
    fn a_fallback_after_the_cap_counts_the_candidates_sent() {
        let ids: Vec<String> = (0..12).map(|i| format!("w/{i}")).collect();
        let events = Events::default();
        let sink = recorder(&events);
        let blocked =
            run(&pending_ids(&ids, 0), "q", None, &settings(Mode::Enforce), &log(&sink), |_, _| Err("deadline".into()));
        assert!(blocked.ids().is_empty());
        let events = events.borrow();
        let fallback = events.iter().find(|e| field(e, "event") == "gate_fallback").unwrap();
        assert_eq!(field(fallback, "candidates"), "8");
        assert_eq!(calls_in(&events)[0].iter().find(|(k, _)| k == "candidates").unwrap().1, "8");
    }

    #[test]
    fn only_strict_ways_means_no_call_and_no_log() {
        let events = Events::default();
        let sink = recorder(&events);
        let strict = vec![Pending { id: "a", description: "d", pattern_strict: true }];
        let blocked = run(&strict, "q", None, &settings(Mode::Enforce), &log(&sink), |_, _| panic!("no call expected"));
        assert!(blocked.ids().is_empty());
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn turns_put_the_reply_before_the_prompt_and_drop_an_empty_reply() {
        assert_eq!(turns("p", Some("  ")).len(), 1);
        let t = turns("p", Some("r"));
        assert_eq!((t[0].role, t[1].role), (Role::Assistant, Role::User));
    }

    #[test]
    fn a_broken_agent_yaml_or_bad_mode_never_calls_the_provider() {
        // With a key present: the parent turned these off, the first build of
        // ADR-503 turned them back on. They fail closed and are logged.
        let dir = std::env::temp_dir().join(format!("ways-gate-closed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("agent.yaml");
        for (text, closed) in [
            ("mode: off\nengine: anthropic\nprofiles:\n  anthropic: [\n", true),
            ("mode: of\n", true),
            ("mode: off\n", false),
            ("mode: shadow\n", false),
        ] {
            std::fs::write(&path, text).unwrap();
            let events = Events::default();
            let sink = recorder(&events);
            let calls = std::cell::Cell::new(0);
            let gate = apply_from(&path, |_| true, None, &pending(), "add a secret", None, &log(&sink), |_, _| {
                calls.set(calls.get() + 1);
                Err("stub".into())
            });
            let config_fallback = matches!(&gate.status, Status::Fallback { reason } if reason.starts_with("config:"));
            assert_eq!(config_fallback, closed, "{text:?}: the record names the config fallback: {:?}", gate.status);
            if text == "mode: off\n" {
                assert_eq!(gate.status, Status::Off);
            }
            let fallback = events.borrow().iter().any(|e| {
                field(e, "event") == "gate_fallback"
                    && field(e, "reason").starts_with("config:")
                    && field(e, "session") == "test-gate"
                    && field(e, "hook") == "UserPromptSubmit"
                    && field(e, "project") == "/tmp"
            });
            if text.starts_with("mode: shadow") {
                assert_eq!(calls.get(), 1, "the control reaches the stub");
            } else {
                assert_eq!(calls.get(), 0, "{text:?} must never call the provider");
            }
            assert_eq!(fallback, closed, "{text:?}: {:?}", events.borrow());
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── ADR-708: the session's engine and the claude-code handoff ──

    /// A session's judge store and its agent.yaml, in a directory of their own.
    struct Session {
        base: std::path::PathBuf,
        store: Store,
        yaml: std::path::PathBuf,
    }

    impl Session {
        fn new(name: &str, yaml: &str) -> Session {
            let base = std::env::temp_dir().join(format!("ways-gate-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            let yaml_path = base.join("agent.yaml");
            std::fs::write(&yaml_path, yaml).unwrap();
            Session { store: Store::at(base.join("state"), base.join("run")), base, yaml: yaml_path }
        }

        /// The gate with `has_key` for every provider, and an agent call that
        /// records each request's engine and answers with `agent`.
        fn gate(&self, has_key: bool, events: &Events, agent: &std::cell::RefCell<Vec<Option<String>>>) -> Gate {
            let sink = recorder(events);
            apply_from(&self.yaml, |_| has_key, Some(&self.store), &pending(), "store the api key", Some("ok"), &log(&sink), |req, _| {
                agent.borrow_mut().push(req.engine.clone());
                Ok(judged(Mode::Enforce, &[("softwaredev/code/security/secrets", 0.9), ("data/migrations", 0.9)]))
            })
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// A host on a thread: it claims the session's next job and answers with
    /// what `reply` makes of it, as the mod pipes `$.model.complete`'s result
    /// to `ways judge answer`. `None` takes the job and stays silent.
    fn host(store: &Store, reply: impl FnOnce(&Taken) -> Option<JudgeAnswer> + Send + 'static) -> std::thread::JoinHandle<Option<Taken>> {
        let store = store.clone();
        std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(10);
            while Instant::now() < until {
                if let Some(t) = store.take().unwrap() {
                    if let Some(a) = reply(&t) {
                        store.answer(Some(&t.id), &a).unwrap();
                    }
                    return Some(t);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            None
        })
    }

    fn answer_text(text: &str) -> JudgeAnswer {
        let usage = ways_agent_core::cost::Usage { input_tokens: 1160, output_tokens: 117, ..Default::default() };
        JudgeAnswer { text: Some(text.to_string()), usage: Some(usage), reason: None }
    }

    const CLAUDE_CODE: &str = "engine: claude-code\nmode: enforce\n";

    #[test]
    fn a_host_answers_and_the_gate_blocks_and_admits_by_its_verdict() {
        let s = Session::new("host-verdict", CLAUDE_CODE);
        s.store.register_host().unwrap();
        let events = Events::default();
        let agent = Default::default();
        // g1 is the secrets way, g2 the migrations way; the strict way is
        // never sent.
        let h = host(&s.store, |_| {
            Some(answer_text(
                "```json\n{\"judgements\":[{\"id\":\"g1\",\"relevant\":true,\"confidence\":0.9},{\"id\":\"g2\",\"relevant\":false,\"confidence\":0.95}]}\n```",
            ))
        });
        let g = s.gate(false, &events, &agent);
        let taken = h.join().unwrap().expect("the host took a job");
        assert!(agent.borrow().is_empty(), "the agent is never asked");
        assert_eq!(g.ids(), HashSet::from(["data/migrations".to_string()]));
        assert!(
            matches!(&g.status, Status::Judged { engine, model, .. } if engine == "claude-code" && model == "claude-haiku-4-5"),
            "{:?}",
            g.status
        );
        // The job is the JSON-only request, under the remaining budget.
        assert_eq!(taken.job.model, "claude-haiku-4-5");
        assert_eq!(taken.job.system, ways_agent_core::judge::SYSTEM);
        assert!(taken.job.prompt.ends_with(ways_agent_core::judge::JSON_ONLY), "{}", taken.job.prompt);
        assert!(taken.job.prompt.contains("<guidance id=\"g2\">"));
        assert_eq!(taken.job.max_tokens, ways_agent_core::judge::max_tokens(2));
        assert!(taken.job.timeout_ms > 0 && taken.job.timeout_ms <= 2000, "{}", taken.job.timeout_ms);
        // One call, named for its engine, with the answer's usage and no price.
        let calls = calls(&events);
        assert_eq!(calls.len(), 1, "{calls:?}");
        let c = &calls[0];
        let got: Vec<&str> = ["outcome", "engine", "provider", "model", "candidates", "input_tokens", "output_tokens"].iter().map(|k| field(c, k)).collect();
        assert_eq!(got, ["judged", "claude-code", "claude-code", "claude-haiku-4-5", "2", "1160", "117"]);
        assert!(c.iter().all(|(k, _)| k != "cost_usd"), "no provider cost: {c:?}");
        let verdicts: Vec<_> = events.borrow().iter().filter(|e| field(e, "event") == "way_judged").map(|e| (field(e, "way").to_string(), field(e, "engine").to_string())).collect();
        assert_eq!(verdicts.len(), 2);
        assert!(verdicts.iter().all(|(_, e)| e == "claude-code"));
        assert_eq!(std::fs::read_dir(s.base.join("run/jobs")).unwrap().count(), 0, "the job is cleared");
    }

    #[test]
    fn the_jobs_timeout_is_what_remains_of_the_gates_budget() {
        let s = Session::new("budget", CLAUDE_CODE);
        s.store.register_host().unwrap();
        let events = Events::default();
        let sink = recorder(&events);
        let h = host(&s.store, |_| Some(answer_text("{\"judgements\":[]}")));
        // The gate's clock started 500 ms before the job was written.
        let settings = Settings { engine: "claude-code".into(), engine_set: true, profile: profile::shipped()["claude-code"].clone(), mode: Mode::Enforce };
        let begun = Instant::now() - Duration::from_millis(500);
        run(&pending(), "q", None, &settings, &log(&sink), |req, _| handoff(&s.store, &settings, req, begun));
        let taken = h.join().unwrap().unwrap();
        assert!(taken.job.timeout_ms <= 1500 && taken.job.timeout_ms > 1000, "{}", taken.job.timeout_ms);
        // Under the least budget a call can use, no job is written.
        let late = Instant::now() - Duration::from_millis(1970);
        let g = run(&pending(), "q", None, &settings, &log(&sink), |req, _| handoff(&s.store, &settings, req, late));
        assert_eq!(g.status, Status::Fallback { reason: "deadline: before call".into() });
        assert_eq!(s.store.take().unwrap(), None);
    }

    #[test]
    fn silence_fails_open_as_deadline() {
        let s = Session::new("silence", "engine: claude-code\nmode: enforce\nprofiles:\n  claude-code:\n    timeout_ms: 200\n");
        s.store.register_host().unwrap();
        // No host takes the job: nothing was spent, so no call is logged.
        let events = Events::default();
        let g = s.gate(false, &events, &Default::default());
        assert_eq!(g.status, Status::Fallback { reason: "deadline".into() });
        assert!(g.ids().is_empty());
        assert!(calls(&events).is_empty(), "an unclaimed job made no call");
        assert_eq!(field(events.borrow().iter().find(|e| field(e, "event") == "gate_fallback").unwrap(), "reason"), "deadline");
        // A host takes it and never answers: a call of unknown cost.
        let events = Events::default();
        let h = host(&s.store, |_| None);
        let g = s.gate(false, &events, &Default::default());
        h.join().unwrap().expect("taken");
        assert_eq!(g.status, Status::Fallback { reason: "deadline".into() });
        let calls = calls(&events);
        assert_eq!(calls.len(), 1);
        assert_eq!((field(&calls[0], "engine"), field(&calls[0], "reason"), field(&calls[0], "cost_source")), ("claude-code", "deadline", "unknown"));
    }

    #[test]
    fn a_garbage_reply_fails_open_as_invalid_reply() {
        let s = Session::new("garbage", CLAUDE_CODE);
        s.store.register_host().unwrap();
        let events = Events::default();
        let h = host(&s.store, |_| Some(answer_text("Both pieces of guidance look relevant to me.")));
        let g = s.gate(false, &events, &Default::default());
        h.join().unwrap().unwrap();
        let Status::Fallback { reason } = &g.status else { panic!("{:?}", g.status) };
        assert!(reason.starts_with("invalid_reply"), "{reason}");
        assert!(g.ids().is_empty());
        let calls = calls(&events);
        assert_eq!((calls.len(), field(&calls[0], "input_tokens")), (1, "1160"), "the spent call is logged with its usage");
        // A host whose model call failed sends its reason and no text.
        let h = host(&s.store, |_| Some(JudgeAnswer { reason: Some("api-error".into()), ..Default::default() }));
        let g = s.gate(false, &Events::default(), &Default::default());
        h.join().unwrap().unwrap();
        assert_eq!(g.status, Status::Fallback { reason: "model_error: api-error".into() });
        // A reason fails the call even when text came with it.
        let h = host(&s.store, |_| Some(JudgeAnswer { reason: Some("aborted".into()), ..answer_text("{\"judgements\":[]}") }));
        let g = s.gate(false, &Events::default(), &Default::default());
        h.join().unwrap().unwrap();
        assert_eq!(g.status, Status::Fallback { reason: "model_error: aborted".into() });
    }

    #[test]
    fn a_host_registered_after_resolution_leaves_the_session_on_its_key_engine() {
        let s = Session::new("late-host", CLAUDE_CODE);
        let agent = Default::default();
        let g = s.gate(true, &Events::default(), &agent);
        assert!(matches!(g.status, Status::Judged { .. }), "{:?}", g.status);
        // The agent judged with the session's engine, not agent.yaml's.
        assert_eq!(*agent.borrow(), [Some("anthropic".to_string())]);
        let r = s.store.resolution().unwrap();
        assert_eq!((r.resolution.engine.as_deref(), r.resolution.reason.as_str()), (Some("anthropic"), ways_agent_core::engine::NO_HOST));
        s.store.register_host().unwrap();
        s.gate(true, &Events::default(), &agent);
        assert_eq!(agent.borrow().len(), 2, "still the agent");
        assert!(!s.base.join("run/jobs").exists(), "no job was ever written");
    }

    #[test]
    fn a_key_engine_never_writes_a_job_and_claude_code_never_calls_the_agent() {
        let s = Session::new("one-engine", "engine: anthropic\nmode: enforce\n");
        s.store.register_host().unwrap();
        let agent = Default::default();
        s.gate(true, &Events::default(), &agent);
        assert_eq!(*agent.borrow(), [Some("anthropic".to_string())]);
        assert!(!s.base.join("run/jobs").exists());
        assert!(!s.store.resolution().unwrap().is_claude_code());
    }

    #[test]
    fn claude_code_with_no_host_and_no_key_fails_open_as_engine_unavailable() {
        let s = Session::new("unavailable", CLAUDE_CODE);
        let events = Events::default();
        let agent = Default::default();
        let g = s.gate(false, &events, &agent);
        let Status::Fallback { reason } = &g.status else { panic!("{:?}", g.status) };
        assert!(reason.starts_with("engine_unavailable: claude-code unavailable"), "{reason}");
        assert!(agent.borrow().is_empty());
        let events = events.borrow();
        let fb = events.iter().find(|e| field(e, "event") == "gate_fallback").expect("logged");
        assert_eq!((field(fb, "reason"), field(fb, "candidates")), (reason.as_str(), "2"));
    }

    #[test]
    fn nothing_to_judge_resolves_nothing() {
        let s = Session::new("idle", CLAUDE_CODE);
        let events = Events::default();
        let sink = recorder(&events);
        let strict = vec![Pending { id: "a", description: "d", pattern_strict: true }];
        let g = apply_from(&s.yaml, |_| true, Some(&s.store), &strict, "q", None, &log(&sink), |_, _| panic!("no call"));
        assert_eq!(g.status, Status::Idle);
        assert!(s.store.resolution().is_none(), "the first judge request resolves, not the first scan");
    }
}
