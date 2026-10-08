//! scan/decision.rs — one decision record per prompt or task scan (ADR-701 §2).
//!
//! The unit of history is the turn. Each prompt-lane and task-lane scan writes
//! one record to `decisions.jsonl` holding its context once, its top
//! candidates with their scores, what became of each way the scan matched or
//! nearly matched, and how the relevance gate ran. The record absorbs the
//! `scan_candidates` event. The per-way events in `events.jsonl` are written as
//! before: they remain the debugging trail.
//!
//! A prompt scan writes its record even when nothing matched, so the record
//! count is a turn count on keyword-only installs too.

use serde_json::{Map, Value};
use std::collections::HashMap;

use super::candidate_log;
use super::gate;

/// The surface a scan read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Surface {
    Prompt,
    Task,
}

impl Surface {
    fn as_str(self) -> &'static str {
        match self {
            Surface::Prompt => "prompt",
            Surface::Task => "task",
        }
    }
}

/// What the record says about the scan before any way is decided.
pub(super) struct Context<'a> {
    pub surface: Surface,
    pub hook_event: &'a str,
    pub scope: &'a str,
    pub project: &'a str,
    pub session: &'a str,
    /// The agent's epoch counter as this scan saw it: the value `bump_epoch`
    /// returned when the scan bumped it, else the current one. The command
    /// and file lanes bump it on every tool call, so it is not a turn index;
    /// it orders a record against the agent's other epoch stamps.
    pub epoch: u64,
    /// True exactly when this scan bumped the epoch for a user prompt: a turn
    /// started. Turns are counted by records with `turn_start`, never by
    /// distinct epochs, and a later pull joins its turn through the
    /// `scan_id` the last-scan marker names, never through the epoch.
    pub turn_start: bool,
}

/// The decision record of one scan, built as the scan decides and written
/// once at its end.
pub(super) struct Record {
    head: Map<String, Value>,
    surface: Surface,
    session: String,
    scan_id: String,
    epoch: u64,
    lane: Option<&'static str>,
    sidecar: bool,
    /// The scan ranked on fused alias-plus-body scores (`matching.body_rank`).
    body_rank: Option<&'static str>,
    candidates: Vec<candidate_log::Candidate>,
    outcomes: Vec<Map<String, Value>>,
    judge: Option<Value>,
}

/// Where cosine and share come from: the single-vector rows over the whole
/// reduced surface, not the per-chunk late-interaction scores.
const BASIS: &str = "single";

impl Record {
    /// A record for a scan starting now, with the current agent and its token
    /// position read from the session's state.
    pub(super) fn begin(ctx: &Context<'_>) -> Self {
        Record::new(
            ctx,
            &scan_id(),
            &agent_fmt::when::now_utc_iso(),
            &crate::session::current_agent(),
            crate::session::get_token_position(ctx.session),
        )
    }

    /// A prompt-lane record with fixed context, for tests of what a lane records.
    #[cfg(test)]
    pub(super) fn for_test() -> Self {
        let ctx = Context { surface: Surface::Prompt, hook_event: "UserPromptSubmit", scope: "agent", project: "/p", session: "s", epoch: 1, turn_start: true };
        Record::new(&ctx, "test", "2026-10-05T00:00:00Z", "main", 0)
    }

    /// [`Record::begin`] with every read given.
    fn new(ctx: &Context<'_>, scan_id: &str, ts: &str, agent: &str, token_position: u64) -> Self {
        let epoch = ctx.epoch;
        let mut head = Map::new();
        head.insert("ts".into(), ts.into());
        head.insert("kind".into(), "scan".into());
        head.insert("scan_id".into(), scan_id.into());
        head.insert("session".into(), ctx.session.into());
        head.insert("agent".into(), agent.into());
        head.insert("epoch".into(), epoch.into());
        head.insert("turn_start".into(), ctx.turn_start.into());
        head.insert("token_position".into(), token_position.into());
        head.insert("surface".into(), ctx.surface.as_str().into());
        head.insert("hook_event".into(), ctx.hook_event.into());
        head.insert("scope".into(), ctx.scope.into());
        head.insert("project".into(), ctx.project.into());
        Record {
            head,
            surface: ctx.surface,
            session: ctx.session.to_string(),
            scan_id: scan_id.to_string(),
            epoch,
            lane: None,
            sidecar: false,
            body_rank: None,
            candidates: Vec::new(),
            outcomes: Vec::new(),
            judge: None,
        }
    }

    /// The id this record is written under, which a later pull joins to.
    pub(super) fn scan_id(&self) -> &str {
        &self.scan_id
    }

    /// The scan's top candidates. `enabled` maps a corpus id to the way's bare
    /// id for every way that may compete; `sidecar` says whether body
    /// confirmation read the body sidecar. No lane ran: no candidates.
    pub(super) fn candidates(&mut self, scores: &super::scoring::EmbedScores, enabled: &HashMap<&str, &str>, sidecar: bool) {
        self.sidecar = sidecar;
        if let Some((lane, rows)) = candidate_log::scan_lane(scores) {
            self.lane = Some(lane);
            self.candidates = candidate_log::top_candidates(rows, enabled);
        }
    }

    /// Says the scan ranked on fused scores, so a reader of the log can tell
    /// them from alias cosines (ADR-701 §8).
    pub(super) fn body_rank(&mut self, mode: Option<&'static str>) {
        self.body_rank = mode;
    }

    /// A way that matched its pattern but was vetoed by the keyword floor.
    pub(super) fn keyword_gated(&mut self, way: &str, matched_span: &str, prob_en: Option<f64>, prob_multi: Option<f64>, floor: f64) {
        let mut o = outcome(way, "keyword_gated");
        o.insert("matched_span".into(), matched_span.into());
        put_prob(&mut o, "prob_en", prob_en);
        put_prob(&mut o, "prob_multi", prob_multi);
        o.insert("floor".into(), round(floor).into());
        self.outcomes.push(o);
    }

    /// A way that scored within the near-miss margin below its threshold.
    /// `shortfall` is the smallest `tau_s - probability` (the `margin` of the
    /// `way_nearmiss` event; renamed here so it is not read as a candidate's
    /// cosine margin).
    pub(super) fn near_miss(&mut self, way: &str, prob_en: Option<f64>, prob_multi: Option<f64>, tau_s: f64, shortfall: f64) {
        let mut o = outcome(way, "near_miss");
        put_prob(&mut o, "prob_en", prob_en);
        put_prob(&mut o, "prob_multi", prob_multi);
        o.insert("tau_s".into(), round(tau_s).into());
        o.insert("shortfall".into(), round(shortfall).into());
        self.outcomes.push(o);
    }

    /// A way the scan matched: its `rank` in admission order (1-based), the
    /// `channel` that fired it, its probability `p` when scored, and what
    /// became of it. A judge block carries the verdict that kept it out. An
    /// ancestor block names the ancestor and carries the ancestor's verdict as
    /// `ancestor_p_yes` and `ancestor_threshold`: the judge never saw this way.
    /// Any other judged way carries its `p_yes` and `verdict`.
    pub(super) fn hit(&mut self, way: &str, rank: usize, channel: &str, p: Option<f64>, result: &'static str, judge: Option<&gate::Gate>) {
        let mut o = outcome(way, result);
        o.insert("rank".into(), rank.into());
        o.insert("channel".into(), channel.into());
        put_prob(&mut o, "p", p);
        if let Some(g) = judge {
            match g.blocked.fields(way).filter(|_| matches!(result, "judge_block" | "ancestor_block")) {
                Some(fields) => {
                    let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                    let prefix = if result == "ancestor_block" { "ancestor_" } else { "" };
                    for k in ["p_yes", "threshold"] {
                        if let Some(v) = get(k).and_then(|v| v.parse::<f64>().ok()) {
                            o.insert(format!("{prefix}{k}"), round(v).into());
                        }
                    }
                    if let Some(m) = get("mode") {
                        o.insert("mode".into(), m.into());
                    }
                    if let Some(a) = get("ancestor") {
                        o.insert("ancestor".into(), a.into());
                    }
                }
                None => {
                    if let Some(j) = g.judgements.get(way) {
                        o.insert("p_yes".into(), round(j.p_yes).into());
                        o.insert("verdict".into(), j.verdict.into());
                    }
                }
            }
        }
        self.outcomes.push(o);
    }

    /// How the relevance gate ran on this scan.
    pub(super) fn judge(&mut self, g: &gate::Gate) {
        let mut j = Map::new();
        match &g.status {
            gate::Status::Off => {
                j.insert("status".into(), "off".into());
            }
            gate::Status::Idle => {
                j.insert("status".into(), "idle".into());
            }
            gate::Status::Judged { engine, model, judge_ms } => {
                j.insert("status".into(), "judged".into());
                j.insert("engine".into(), engine.as_str().into());
                j.insert("model".into(), model.as_str().into());
                j.insert("judge_ms".into(), (*judge_ms).into());
            }
            gate::Status::Fallback { reason } => {
                j.insert("status".into(), "fallback".into());
                j.insert("reason".into(), reason.as_str().into());
            }
        }
        if !g.capped.is_empty() {
            j.insert("capped".into(), g.capped.clone().into());
        }
        self.judge = Some(Value::Object(j));
    }

    /// The record as one JSON object.
    pub(super) fn to_json(&self) -> Value {
        let mut r = self.head.clone();
        r.insert("lane".into(), self.lane.map_or(Value::Null, Value::from));
        r.insert("basis".into(), BASIS.into());
        r.insert("sidecar".into(), self.sidecar.into());
        // Named only when on, so a record written with the flag off is unchanged.
        if let Some(mode) = self.body_rank {
            r.insert("body_rank".into(), mode.into());
        }
        r.insert("candidates".into(), candidate_log::candidates_json(&self.candidates));
        r.insert("outcomes".into(), Value::Array(self.outcomes.iter().cloned().map(Value::Object).collect()));
        if let Some(j) = &self.judge {
            r.insert("judge".into(), j.clone());
        }
        Value::Object(r)
    }

    /// Append the record to the decision log. A prompt-lane record also
    /// becomes the agent's last scan, which a pull later in the turn joins to.
    pub(super) fn write(self) {
        crate::session::log_decision(&self.to_json());
        if self.surface == Surface::Prompt {
            crate::session::write_last_scan(&self.session, &self.scan_id, self.epoch);
        }
    }
}

/// The result a matched way takes when the gate kept it out, or `None`.
pub(super) fn block_result(blocked: &gate::Blocked, way: &str) -> Option<&'static str> {
    let fields = blocked.fields(way)?;
    let by_ancestor = fields.iter().any(|(k, v)| k == "reason" && v == "ancestor");
    Some(if by_ancestor { "ancestor_block" } else { "judge_block" })
}

fn outcome(way: &str, result: &'static str) -> Map<String, Value> {
    let mut o = Map::new();
    o.insert("way".into(), way.into());
    o.insert("result".into(), result.into());
    o
}

fn put_prob(o: &mut Map<String, Value>, key: &str, p: Option<f64>) {
    if let Some(p) = p {
        o.insert(key.into(), round(p).into());
    }
}

fn round(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// A scan id unique across processes: the clock in nanoseconds and the pid.
fn scan_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:x}-{:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(surface: Surface) -> Context<'static> {
        Context { surface, hook_event: "UserPromptSubmit", scope: "agent", project: "/p", session: "s", epoch: 7, turn_start: true }
    }

    fn record(surface: Surface) -> Record {
        Record::new(&ctx(surface), "abc-1", "2026-10-05T00:00:00Z", "main", 1234)
    }

    type Events = std::cell::RefCell<Vec<Vec<(String, String)>>>;

    fn gate_of(verdicts: &[(&str, f64)], mode: ways_agent_core::profile::Mode) -> gate::Gate {
        let events = Events::default();
        let sink = |f: &[(&str, &str)]| events.borrow_mut().push(f.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        let lc = gate::LogContext { session_id: "s", project_dir: "/p", scope: "agent", hook_event: "UserPromptSubmit", sink: &sink };
        gate::test_gate(verdicts, mode, &lc)
    }

    #[test]
    fn an_empty_scan_still_has_its_context_and_empty_lists() {
        let v = record(Surface::Prompt).to_json();
        assert_eq!(v["kind"], "scan");
        assert_eq!(v["scan_id"], "abc-1");
        assert_eq!((v["epoch"].as_u64(), v["token_position"].as_u64()), (Some(7), Some(1234)));
        assert_eq!(v["turn_start"], true);
        assert_eq!((v["surface"].as_str(), v["agent"].as_str()), (Some("prompt"), Some("main")));
        assert!(v["lane"].is_null(), "no lane ran");
        assert_eq!(v["basis"], "single");
        assert_eq!(v["candidates"], json!([]));
        assert_eq!(v["outcomes"], json!([]));
        assert!(v.get("judge").is_none(), "the gate never ran");
    }

    #[test]
    fn each_outcome_kind_carries_its_own_fields() {
        let mut r = record(Surface::Prompt);
        r.keyword_gated("a", "refactor", Some(0.05), None, 0.15);
        r.near_miss("b", Some(0.42), Some(0.4), 0.5, 0.08);
        r.hit("c", 1, "keyword", None, "fired", None);
        r.hit("d", 2, "semantic:embedding:en", Some(0.61234), "held_refire", None);
        let v = r.to_json();
        let o = v["outcomes"].as_array().unwrap();
        assert_eq!(o[0], json!({"way": "a", "result": "keyword_gated", "matched_span": "refactor", "prob_en": 0.05, "floor": 0.15}));
        assert_eq!(o[1], json!({"way": "b", "result": "near_miss", "prob_en": 0.42, "prob_multi": 0.4, "tau_s": 0.5, "shortfall": 0.08}));
        assert_eq!(o[2], json!({"way": "c", "result": "fired", "rank": 1, "channel": "keyword"}));
        assert_eq!(o[3], json!({"way": "d", "result": "held_refire", "rank": 2, "channel": "semantic:embedding:en", "p": 0.6123}));
    }

    #[test]
    fn a_judge_pass_rides_on_the_fire_and_a_block_names_its_verdict() {
        use ways_agent_core::profile::Mode;
        let g = gate_of(&[("pass/way", 0.9), ("block/way", 0.05)], Mode::Enforce);
        let mut r = record(Surface::Prompt);
        r.hit("pass/way", 1, "keyword", None, "fired", Some(&g));
        let blocked = block_result(&g.blocked, "block/way").unwrap();
        assert_eq!(blocked, "judge_block");
        r.hit("block/way", 2, "keyword", None, blocked, Some(&g));
        r.judge(&g);
        let v = r.to_json();
        let o = v["outcomes"].as_array().unwrap();
        assert_eq!(o[0]["result"], "fired");
        assert_eq!((o[0]["p_yes"].as_f64(), o[0]["verdict"].as_str()), (Some(0.9), Some("pass")));
        assert_eq!(o[1]["result"], "judge_block");
        assert_eq!((o[1]["p_yes"].as_f64(), o[1]["threshold"].as_f64(), o[1]["mode"].as_str()), (Some(0.05), Some(0.3), Some("enforce")));
        assert_eq!(v["judge"], json!({"status": "judged", "engine": "anthropic", "model": "claude-haiku-4-5", "judge_ms": 800}));
    }

    #[test]
    fn a_shadow_verdict_rides_on_the_fire_it_did_not_stop() {
        use ways_agent_core::profile::Mode;
        let g = gate_of(&[("w", 0.05)], Mode::Shadow);
        assert_eq!(block_result(&g.blocked, "w"), None, "shadow blocks nothing");
        let mut r = record(Surface::Prompt);
        r.hit("w", 1, "keyword", None, "fired", Some(&g));
        let o = &r.to_json()["outcomes"][0];
        assert_eq!((o["result"].as_str(), o["verdict"].as_str()), (Some("fired"), Some("would_block")));
    }

    #[test]
    fn an_ancestor_block_names_the_ancestor() {
        use ways_agent_core::profile::Mode;
        let events = Events::default();
        let sink = |f: &[(&str, &str)]| events.borrow_mut().push(f.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        let lc = gate::LogContext { session_id: "s", project_dir: "/p", scope: "agent", hook_event: "UserPromptSubmit", sink: &sink };
        let mut g = gate::test_gate(&[("p", 0.05)], Mode::Enforce, &lc);
        assert!(g.blocked.with_ancestor("p/c", &lc));
        assert_eq!(block_result(&g.blocked, "p/c"), Some("ancestor_block"));
        let mut r = record(Surface::Prompt);
        r.hit("p/c", 2, "keyword", None, "ancestor_block", Some(&g));
        let o = &r.to_json()["outcomes"][0];
        assert_eq!(o["ancestor"], "p");
        assert_eq!((o["ancestor_p_yes"].as_f64(), o["ancestor_threshold"].as_f64()), (Some(0.05), Some(0.3)));
        assert!(o.get("p_yes").is_none() && o.get("threshold").is_none(), "the judge never saw p/c: {o}");
    }

    #[test]
    fn the_gate_status_is_named_with_its_reason_or_cap() {
        let mut r = record(Surface::Prompt);
        r.judge(&gate::Gate { status: gate::Status::Fallback { reason: "deadline".into() }, capped: vec!["w/9".into()], ..Default::default() });
        assert_eq!(r.to_json()["judge"], json!({"status": "fallback", "reason": "deadline", "capped": ["w/9"]}));
        r.judge(&gate::Gate::default());
        assert_eq!(r.to_json()["judge"], json!({"status": "off"}));
    }

    #[test]
    fn a_task_record_says_task() {
        let mut r = Record::new(&Context { turn_start: false, epoch: 1, ..ctx(Surface::Task) }, "x", "t", "main", 0);
        r.hit("w", 1, "keyword", None, "stashed", None);
        let v = r.to_json();
        assert_eq!((v["surface"].as_str(), v["turn_start"].as_bool()), (Some("task"), Some(false)));
        assert_eq!(v["outcomes"][0]["result"], "stashed");
    }

    #[test]
    fn scan_ids_differ_between_calls() {
        let (a, b) = (scan_id(), scan_id());
        assert!(a.ends_with(&format!("-{:x}", std::process::id())));
        assert_ne!(a, b);
    }
}
