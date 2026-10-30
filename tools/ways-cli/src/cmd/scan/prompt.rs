//! The prompt lane: `UserPromptSubmit`, and the queued-message lane that
//! feeds the same engine mid-turn (ADR-161).

use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::cmd::show::ContextBudget;
use crate::session;

use super::candidates::collect_with_off;
use super::{discard, gate};
use super::matching::{
    log_keyword_gated, log_near_miss, match_prompt, record_keyword_gated, record_near_miss, PromptMatch,
};
use super::order::{order_hits, Hit};
use super::scoring::{batch_embed_score, capture_show_way, EmbedScores};
use super::{
    body_map, decision, effective_thresholds, effective_thresholds_in_scan, eligible, emit_hook_context,
    has_shown_ancestor, late_interaction, mask_nonlinguistic, prompt_competitors, reduce, Lane, WayCandidate,
    BUDGET_PROMPT,
};

/// Match user prompt against ways and emit matched bodies for the agent.
///
/// Wired only from the `UserPromptSubmit` hook (`check-prompt.sh`), so the
/// envelope event name is hardcoded. The call routes through
/// `emit_hook_context`, which emits the canonical `hookSpecificOutput`
/// envelope for every event. If this is ever reused from another hook
/// event, just pass that event's name.
///
/// `response_context` (ADR-155 §3) is Claude's last response, stored raw by
/// the Stop hook. It feeds ONLY the embed input — never the regex lane — so
/// ways can trigger on what Claude was just reasoning about without response
/// tokens keyword-firing ways the user never mentioned. The ADR-130 reducer
/// weighs the prompt's and the response's sentences by salience within one
/// budget: a terse follow-up ("yes, do it") lets the response carry the
/// topic; a substantive prompt dominates on its own salience.
pub fn prompt(
    query: &str,
    session_id: &str,
    project: Option<&str>,
    response_context: Option<&str>,
    transcript: Option<&str>,
) -> Result<()> {
    // The hook's transcript_path names the invoking agent's transcript; the
    // firing path reads the model id (and the refire window) from it.
    crate::cmd::show::set_firing_transcript(transcript);
    // The last-scan marker belongs to the turn before; the scan below writes
    // this turn's, and a turn with no scan is left with none.
    session::clear_last_scan(session_id);
    // A user prompt starts a turn: bump the epoch.
    //
    // A Monitor notification that wakes an idle session also arrives as a
    // prompt, wrapped in a `<task-notification>` envelope. Its body is a
    // sensor line or a peer message, so it is not operator intent (ADR-161
    // scopes matching to operator text). Bump the epoch and skip the scan.
    if is_system_envelope(query) {
        session::bump_epoch(session_id);
        return Ok(());
    }
    scan_prompt_surface(query, session_id, project, response_context, true, "UserPromptSubmit")
}

/// ADR-161: the queued-message scan lane. A message the operator types while the
/// agent is working is queued (recorded in the transcript as a
/// `queue-operation`/`enqueue` entry) and never reaches `UserPromptSubmit`, so
/// the prompt lane never sees it. On PostToolUse, aggregate every enqueue newer
/// than the per-session scan mark into ONE surface — so a burst of short
/// fragments becomes a ≥2-chunk late-interaction surface instead of each lone
/// fragment falling to the single-vector fallback — match it through the same
/// engine as a prompt (without bumping the epoch: this is mid-turn, not a new
/// turn), then advance the mark.
pub fn messages(
    session_id: &str,
    project: Option<&str>,
    transcript: Option<&str>,
) -> Result<()> {
    let Some(path) = transcript else {
        return Ok(()); // PostToolUse always supplies transcript_path; nothing to do without it.
    };
    crate::cmd::show::set_firing_transcript(Some(path));
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // transcript not readable yet
    };

    let mark = session::read_queued_scan_mark(session_id);
    let scan = collect_queued(&content, mark.as_deref());

    // Advance the mark first: a failure mid-scan must not re-fire endlessly.
    // Skipped system envelopes still advance it (newest tracks all enqueues).
    if let Some(n) = scan.newest {
        session::write_queued_scan_mark(session_id, &n);
    }
    if scan.fragments.is_empty() {
        return Ok(());
    }

    // Aggregate the burst into one surface; lowercase to match the prompt lane's
    // keyword expectations (check-prompt.sh lowercases the prompt).
    let surface = scan.fragments.join("\n").to_lowercase();
    scan_prompt_surface(&surface, session_id, project, None, false, "PostToolUse")
}

/// Result of selecting queued operator messages from a transcript: the operator
/// prose `fragments` to aggregate, and the `newest` enqueue timestamp seen
/// (which advances the scan mark even if every fragment was a filtered envelope).
struct QueuedScan {
    fragments: Vec<String>,
    newest: Option<String>,
}

/// Pure selection of queued mid-turn operator messages (ADR-161). Reads a
/// transcript's lines, keeps `queue-operation`/`enqueue` entries strictly newer
/// than `mark` (ISO-8601 sorts lexicographically, so a string compare is a time
/// compare), and separates genuine operator prose from harness envelopes that
/// ride the same queue.
fn collect_queued(content: &str, mark: Option<&str>) -> QueuedScan {
    let mut fragments: Vec<String> = Vec::new();
    let mut newest: Option<String> = None;

    for line in content.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("type").and_then(|x| x.as_str()) != Some("queue-operation")
            || v.get("operation").and_then(|x| x.as_str()) != Some("enqueue")
        {
            continue;
        }
        let Some(ts) = v.get("timestamp").and_then(|x| x.as_str()) else {
            continue;
        };
        if let Some(m) = mark {
            if ts <= m {
                continue;
            }
        }
        // Track the newest timestamp regardless of whether the content is kept,
        // so a skipped envelope still advances the mark past it.
        if newest.as_deref().is_none_or(|n| ts > n) {
            newest = Some(ts.to_string());
        }
        let msg = v
            .get("content")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if msg.is_empty() || is_system_envelope(msg) {
            continue;
        }
        fragments.push(msg.to_string());
    }

    QueuedScan { fragments, newest }
}

/// Content that is a harness-generated envelope, not operator prose — it
/// should not be matched as intent. Covers the harness's own tags and the
/// openings of attend's deliveries: the drain's header (ADR-172) and the
/// silence line every delivery now leads with (ADR-406 item 4).
pub(super) fn is_system_envelope(s: &str) -> bool {
    // The prompt lane hands over lowercased text and the queued lane raw
    // content; fold case here so neither caller carries the dependency.
    let t = s.trim_start().to_ascii_lowercase();
    t.starts_with("<task-notification")
        || t.starts_with("<system-reminder")
        || t.starts_with("<local-command")
        || t.starts_with("<command-")
        || t.starts_with("<persisted-output")
        || t.starts_with("[attend")
        // Every attend delivery, the drain's and the mod's, opens with this.
        || t.starts_with("silence is a valid reply to the messages below;")
        // A skill invocation arrives as the skill's own body. The operator
        // chose the skill; the body's vocabulary is the author's, and it fired
        // unrelated ways in four sibling sessions of one measured month.
        || t.starts_with("base directory for this skill:")
}

fn scan_prompt_surface(
    query: &str,
    session_id: &str,
    project: Option<&str>,
    response_context: Option<&str>,
    bump_epoch: bool,
    hook_event: &str,
) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(crate::util::project_dir);

    let epoch = if bump_epoch { session::bump_epoch(session_id) } else { session::get_epoch(session_id) };

    let scope = session::detect_scope(session_id);
    // ADR-701 §2: the turn's decision record, written once at the end of the scan.
    let mut record = decision::Record::begin(&decision::Context {
        surface: decision::Surface::Prompt,
        hook_event,
        scope: &scope,
        project: &project_dir,
        session: session_id,
        epoch,
        turn_start: bump_epoch,
    });
    let (candidates, off) = collect_with_off(&project_dir);

    // ADR-130: cap embed input to the model's working window via the
    // sentence-salience reducer. Pattern/keyword matching downstream
    // operates on the masked full prompt (ADR-155 §2: URLs and fenced
    // code are not lexical intent) — only the embed signal sees the
    // reduced form, and it sees the unmasked prompt plus Claude's last
    // response (ADR-155 §3), competing on sentence salience.
    let embed_input = match response_context {
        Some(rc) if !rc.trim().is_empty() => format!("{query}\n{rc}"),
        _ => query.to_string(),
    };
    let reduced = reduce::reduce_for_embed(&embed_input, BUDGET_PROMPT);
    let masked = mask_nonlinguistic(query);

    let competitors = prompt_competitors(&candidates, &scope, &project_dir);
    let bodies = body_map(competitors.iter().copied());
    let body_rank = crate::config::global().body_rank;
    let (embed_matches, single_fused) = late_interaction::single_scores(&reduced, &bodies, body_rank);

    // ADR-160: the chunked late-interaction matcher IS the semantic matcher. It decides
    // the semantic channel over the reduced surface (chunk → softmax-share →
    // body-confirm), computed once here and consulted per way in match_prompt.
    // The single-vector calibrated gate is retained only as the fail-safe: when
    // the matcher can't run (surface too sparse to chunk, engine unavailable) it
    // returns None and match_prompt uses the single-vector scores. The keyword
    // gate and near-miss telemetry keep using the single-vector batch scores.
    let lane = Lane::Prompt { scope: &scope };
    let off_bodies = body_map(off.ways.iter().filter(|w| eligible(w, lane, &project_dir)));
    let verdicts = late_interaction::run(&reduced, &bodies, &off_bodies, crate::config::global().admission, body_rank);
    let fused = single_fused || verdicts.as_ref().is_some_and(|v| v.used_body_rank());

    // ADR-701 §2: record the top candidates with share and margin, from the
    // rows the scan already holds, and whether confirmation read the body
    // sidecar. Enabled ways only: `candidates` is already filtered by the
    // domain and per-way toggles.
    let enabled: std::collections::HashMap<&str, &str> =
        competitors.iter().map(|c| (c.corpus_id.as_str(), c.id.as_str())).collect();
    record.candidates(&embed_matches, &enabled, verdicts.as_ref().is_some_and(|v| v.used_sidecar()));
    record.body_rank(fused.then(|| body_rank.as_str()));

    // Prompt-only embed scores, computed lazily for gate re-checks (ADR-155
    // review): the shared embed vector mixes the response context in, which
    // can dilute a way's score below the gate floor even though the USER's
    // text carries the keyword — a terse "ship it" after an off-topic
    // response must not be vetoed by that response. Only turns where a hit
    // actually lands below the floor pay the second embed pass, and only
    // when response context contributed at all.
    let mut prompt_only_scores: Option<EmbedScores> = None;

    let mut context = String::new();
    let mut budget = ContextBudget::hook();
    // Fired ways are collected, then admitted in a fixed order (scan/order.rs).
    // Payload: (channel, matched span, fired only on this scan's parent boost).
    let mut hits: Vec<Hit<(String, Option<String>, bool)>> = Vec::new();
    // Ids fired so far in this scan. Candidates arrive in tree order, so a
    // parent is decided before its children and can boost them (see
    // `effective_thresholds_in_scan`).
    let mut fired_ids: HashSet<String> = HashSet::new();

    for way in &candidates {
        if !eligible(way, lane, &project_dir) {
            continue;
        }

        let (outcome, needs_parent) = prompt_outcome(
            way,
            &PromptSurface { body_rank, bodies: &bodies, query, masked: &masked, session_id, response_context, embed_matches: &embed_matches, verdicts: verdicts.as_ref() },
            &fired_ids,
            &mut prompt_only_scores,
        );

        match outcome {
            PromptMatch::Fired { channel, score, matched_span } => {
                fired_ids.insert(way.id.clone());
                let hit = match (&matched_span, way.pattern.as_deref()) {
                    (Some(span), Some(pat)) if score.is_none() => {
                        Hit::explicit(&way.id, pat, span, (channel, matched_span.clone(), needs_parent))
                    }
                    _ => Hit::scored(&way.id, score, (channel, matched_span, needs_parent)),
                };
                hits.push(hit);
            }
            PromptMatch::KeywordGated(kg) => {
                log_keyword_gated(way, &kg, "prompt", &scope, &project_dir, session_id);
                record_keyword_gated(&mut record, way, &kg);
            }
            PromptMatch::NearMiss(nm) => {
                log_near_miss(way, &nm, "prompt", &scope, &project_dir, session_id, query);
                record_near_miss(&mut record, way, &nm);
            }
            PromptMatch::NoMatch => {}
        }
    }

    discard::prompt_shaped(
        &off,
        |w| eligible(w, lane, &project_dir),
        &discard::PromptShadow {
            query,
            masked: &masked,
            session: session_id,
            scores: &embed_matches,
            recheck: prompt_only_scores.as_ref(),
            verdicts: verdicts.as_ref(),
            fired: Some(&fired_ids),
            enabled: &enabled,
        },
        &discard::LaneLog { lane: "prompt", scope: &scope, session: session_id, project: &project_dir },
    );

    order_hits(&mut hits);

    // ADR-196: the relevance gate judges the hits that would reach the agent
    // (those the refire curve still holds back are not sent) and returns the
    // ones it blocks. A blocked way is skipped before its fire is recorded, so
    // it keeps its refire budget.
    let by_id: std::collections::HashMap<&str, &WayCandidate> =
        candidates.iter().map(|w| (w.id.as_str(), w)).collect();
    let pending: Vec<gate::Pending<'_>> = hits
        .iter()
        .filter_map(|hit| by_id.get(hit.id.as_str()))
        .filter(|way| crate::cmd::show::would_fire(&way.id, session_id))
        .map(|way| gate::Pending { id: &way.id, description: &way.description, pattern_strict: way.pattern_strict })
        .collect();
    let gate_log = gate::LogContext {
        session_id,
        project_dir: &project_dir,
        scope: &scope,
        hook_event,
        sink: &session::log_event,
    };
    let mut gate = gate::apply(&pending, query, response_context, &gate_log);

    admit_hits(&hits, &mut gate, &pending, &gate_log, &mut record, |hit| {
        let (channel, matched_span, _) = &hit.payload;
        let shown = capture_show_way(
            &hit.id,
            session_id,
            channel,
            hit.score,
            matched_span.as_deref(),
            Some(reduced.as_str()),
            Some(&mut budget),
        );
        let delivered = !shown.body.is_empty();
        if delivered {
            context.push_str(&shown.body);
            context.push_str("\n\n");
            budget.charge("\n\n");
        }
        (shown.outcome, delivered)
    });
    record.write();

    if !context.is_empty() {
        emit_hook_context(hook_event, context.trim_end());
    }

    Ok(())
}

/// What the prompt lane decides each way against: the surface, its embed scores
/// and the matcher's verdicts. Shared by the scan and the probe scorer.
pub(super) struct PromptSurface<'a> {
    /// Body-rank mode and the competing ways' bodies, so the keyword gate's
    /// prompt-only re-check fuses its scores as the primary pass did.
    pub(super) body_rank: crate::config::BodyRank,
    pub(super) bodies: &'a std::collections::HashMap<String, PathBuf>,
    pub(super) query: &'a str,
    pub(super) masked: &'a str,
    pub(super) session_id: &'a str,
    pub(super) response_context: Option<&'a str>,
    pub(super) embed_matches: &'a EmbedScores,
    pub(super) verdicts: Option<&'a late_interaction::Verdicts>,
}

/// One way's prompt-lane outcome and whether it fired only on this scan's
/// parent boost. `fired_ids` are the ways already fired in this scan (candidates
/// arrive in tree order). `prompt_only_scores` caches the second embed pass the
/// keyword gate's re-check may need.
pub(super) fn prompt_outcome(
    way: &WayCandidate,
    surface: &PromptSurface<'_>,
    fired_ids: &HashSet<String>,
    prompt_only_scores: &mut Option<EmbedScores>,
) -> (PromptMatch, bool) {
    let PromptSurface { body_rank, bodies, query, masked, session_id, response_context, embed_matches, verdicts } = *surface;
    let near_miss_margin = crate::config::global().near_miss_margin;
    let keyword_floor = crate::config::global().keyword_floor_probability;
    let response_contributed = response_context.is_some_and(|rc| !rc.trim().is_empty());

    // pattern_strict means "this exact text, always": it bypasses the
    // mask as well as the gate, so a strict pattern can target URL or
    // code-fence content the mask would otherwise hide (ADR-155 §2).
    let regex_text: &str = if way.pattern_strict { query } else { masked };
    let (thresholds, scan_boost) = effective_thresholds_in_scan(way, session_id, fired_ids);

    // Additive matching: pattern OR semantic
    let mut outcome = match_prompt(
        regex_text,
        &way.pattern,
        way.pattern_strict,
        way.embeddable(),
        &way.corpus_id,
        thresholds,
        embed_matches,
        near_miss_margin,
        keyword_floor,
        verdicts,
    );

    // Gate re-check against the prompt alone before accepting the veto.
    let mut used_prompt_only = false;
    if let PromptMatch::KeywordGated(_) = outcome {
        if response_contributed {
            used_prompt_only = true;
            let scores = prompt_only_scores.get_or_insert_with(|| {
                let reduced = reduce::reduce_for_embed(query, BUDGET_PROMPT);
                // Same ranking as the primary pass: fused where that pass was.
                if verdicts.is_none() {
                    late_interaction::single_scores(&reduced, bodies, body_rank).0
                } else {
                    batch_embed_score(&reduced)
                }
            });
            outcome = match_prompt(
                regex_text,
                &way.pattern,
                way.pattern_strict,
                way.embeddable(),
                &way.corpus_id,
                thresholds,
                scores,
                near_miss_margin,
                keyword_floor,
                verdicts,
            );
        }
    }

    // A fire that needed this scan's parent boost is shown only if that
    // parent is shown too: without it the child would not have fired.
    let needs_parent = scan_boost
        && matches!(outcome, PromptMatch::Fired { .. })
        && !matches!(
            match_prompt(
                regex_text,
                &way.pattern,
                way.pattern_strict,
                way.embeddable(),
                &way.corpus_id,
                effective_thresholds(way, session_id),
                match (used_prompt_only, prompt_only_scores.as_ref()) {
                    (true, Some(scores)) => scores,
                    _ => embed_matches,
                },
                near_miss_margin,
                keyword_floor,
                verdicts,
            ),
            PromptMatch::Fired { .. }
        );
    (outcome, needs_parent)
}

/// A prompt-lane hit's payload: the channel that fired it, its matched span,
/// and whether it fired only on this scan's parent boost.
type PromptPayload = (String, Option<String>, bool);

/// Admit the prompt lane's hits in order and record what became of each in
/// the turn's decision record (ADR-701 §2), then how the gate ran. A way the
/// gate blocked is skipped. A way that fired only on a parent shown in this
/// scan is withheld when that parent was not shown. Every other way goes to
/// `show`, which returns what `way_scored` did with it and whether its body
/// reached the context.
fn admit_hits(
    hits: &[Hit<PromptPayload>],
    gate: &mut gate::Gate,
    pending: &[gate::Pending<'_>],
    gate_log: &gate::LogContext<'_>,
    record: &mut decision::Record,
    mut show: impl FnMut(&Hit<PromptPayload>) -> (crate::cmd::show::ShowOutcome, bool),
) {
    let mut shown: HashSet<String> = HashSet::new();
    for (i, hit) in hits.iter().enumerate() {
        let (channel, _, needs_parent) = &hit.payload;
        let result = if let Some(result) = decision::block_result(&gate.blocked, &hit.id) {
            result
        } else if *needs_parent && withheld_for_parent(&hit.id, &shown, pending, &mut gate.blocked, gate_log) {
            // Withheld with a parent the judge blocked records that block.
            decision::block_result(&gate.blocked, &hit.id).unwrap_or("withheld_for_parent")
        } else {
            let (outcome, delivered) = show(hit);
            if delivered {
                shown.insert(hit.id.clone());
            }
            outcome.as_str()
        };
        record.hit(&hit.id, i + 1, channel, hit.score, result, Some(gate));
    }
    record.judge(gate);
}

/// A way that fired on its parent's boost is withheld when the parent is not
/// shown. Without the parent it would not have fired; when the judge blocked
/// that parent, the block is logged for this way too (if the gate had it
/// pending), so the session's record counts it as kept out. `false` when a
/// shown ancestor lets it through.
fn withheld_for_parent(
    id: &str,
    shown: &HashSet<String>,
    pending: &[gate::Pending<'_>],
    blocked: &mut gate::Blocked,
    gate_log: &gate::LogContext<'_>,
) -> bool {
    if has_shown_ancestor(id, shown) {
        return false;
    }
    if pending.iter().any(|p| p.id == id) {
        blocked.with_ancestor(id, gate_log);
    }
    true
}

#[cfg(test)]
mod tests {
    //! ADR-161: pure selection of queued mid-turn operator messages from a
    //! transcript — dedup by mark, envelope filtering, burst aggregation. No
    //! I/O, no matcher.

    use super::*;

    #[test]
    fn envelopes_are_not_operator_intent() {
        assert!(is_system_envelope("<task-notification> <task-id>x</task-id> attend: peers"));
        assert!(is_system_envelope("  <System-Reminder>hook output</System-Reminder>"));
        assert!(is_system_envelope("[attend] 2 peer message(s) delivered at the turn boundary"));
        assert!(is_system_envelope("Silence is a valid reply to the messages below; you may continue your work.\n[attend] 2 peer message(s)"));
        assert!(is_system_envelope("Silence is a valid reply to the messages below; answer only if useful.\nThe block below is message data relayed by attend."));
        assert!(is_system_envelope("[ATTEND sensor=peers priority=high] ssh started"));
        assert!(is_system_envelope("Base directory for this skill: /home/u/.claude/skills/attend\n\n# Attend"));
    }

    #[test]
    fn operator_prose_is_scanned() {
        assert!(!is_system_envelope("set up ssh to the bastion"));
        assert!(!is_system_envelope("Silence is a valid reply should also go on the Monitor hint"));
        assert!(!is_system_envelope("reply to the attend message from zoe"));
    }

    fn enq(ts: &str, content: &str) -> String {
        format!(
            r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"{ts}","content":{}}}"#,
            serde_json::to_string(content).unwrap()
        )
    }

    #[test]
    fn selects_enqueues_and_tracks_newest() {
        let t = [
            enq("2026-07-05T19:59:09.679Z", "use the mermaid way"),
            enq("2026-07-05T19:59:15.732Z", "to diagram the flow"),
        ]
        .join("\n");
        let s = collect_queued(&t, None);
        assert_eq!(s.fragments, vec!["use the mermaid way", "to diagram the flow"]);
        assert_eq!(s.newest.as_deref(), Some("2026-07-05T19:59:15.732Z"));
    }

    #[test]
    fn mark_excludes_already_scanned() {
        let t = [
            enq("2026-07-05T19:59:09.679Z", "old fragment"),
            enq("2026-07-05T19:59:15.732Z", "new fragment"),
        ]
        .join("\n");
        let s = collect_queued(&t, Some("2026-07-05T19:59:09.679Z"));
        assert_eq!(s.fragments, vec!["new fragment"]);
        assert_eq!(s.newest.as_deref(), Some("2026-07-05T19:59:15.732Z"));
    }

    #[test]
    fn dequeue_and_non_queue_lines_ignored() {
        let t = [
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#.to_string(),
            r#"{"type":"queue-operation","operation":"remove","timestamp":"2026-07-05T20:00:00Z"}"#.to_string(),
            enq("2026-07-05T20:00:01Z", "real message"),
            "not json at all".to_string(),
        ]
        .join("\n");
        let s = collect_queued(&t, None);
        assert_eq!(s.fragments, vec!["real message"]);
        assert_eq!(s.newest.as_deref(), Some("2026-07-05T20:00:01Z"));
    }

    #[test]
    fn system_envelope_filtered_but_still_advances_mark() {
        // A completed-agent notification rides the same queue; it must not be
        // matched as operator intent, yet the mark must move past it so it is
        // not re-examined forever.
        let t = [
            enq("2026-07-05T20:16:30Z", "<task-notification>\n<task-id>abc</task-id>\n</task-notification>"),
        ]
        .join("\n");
        let s = collect_queued(&t, None);
        assert!(s.fragments.is_empty());
        assert_eq!(s.newest.as_deref(), Some("2026-07-05T20:16:30Z"));
    }

    #[test]
    fn empty_and_no_matches_are_none() {
        assert!(collect_queued("", None).newest.is_none());
        let only_old = enq("2026-07-05T10:00:00Z", "old");
        let s = collect_queued(&only_old, Some("2026-07-05T11:00:00Z"));
        assert!(s.fragments.is_empty());
        assert!(s.newest.is_none());
    }


    /// The `needs_parent` call site: the judge blocks the parent, the child
    /// was pending (strict, so never judged), and it is withheld with exactly
    /// one ancestor block and no per-call figures. A child the gate never
    /// had pending logs nothing.
    #[test]
    fn needs_parent_child_of_a_blocked_parent_logs_one_ancestor_block() {
        use std::cell::RefCell;
        let events: RefCell<Vec<Vec<(String, String)>>> = RefCell::default();
        let sink = |f: &[(&str, &str)]| {
            events.borrow_mut().push(f.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
        };
        let lc = gate::LogContext { session_id: "s", project_dir: "/tmp", scope: "agent", hook_event: "UserPromptSubmit", sink: &sink };
        let mut blocked = gate::test_blocked("p", &lc);
        let pending = vec![
            gate::Pending { id: "p", description: "", pattern_strict: false },
            gate::Pending { id: "p/c", description: "", pattern_strict: true },
        ];
        let shown = HashSet::new();
        assert!(withheld_for_parent("p/c", &shown, &pending, &mut blocked, &lc));
        assert!(withheld_for_parent("p/c", &shown, &pending, &mut blocked, &lc), "withheld again, logged once");
        assert!(withheld_for_parent("p/held", &shown, &pending, &mut blocked, &lc), "not pending: withheld, unlogged");
        let shown_p: HashSet<String> = ["p".to_string()].into_iter().collect();
        assert!(!withheld_for_parent("p/c", &shown_p, &pending, &mut blocked, &lc), "shown parent lets it through");

        let events = events.borrow();
        let get = |e: &Vec<(String, String)>, k: &str| e.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
        let child: Vec<_> = events.iter().filter(|e| get(e, "event") == "way_judged" && get(e, "way") == "p/c").collect();
        assert_eq!(child.len(), 1);
        assert_eq!((get(child[0], "verdict"), get(child[0], "reason"), get(child[0], "ancestor")), ("block".into(), "ancestor".into(), "p".into()));
        for k in ["judge_ms", "gate_ms", "candidates"] {
            assert_eq!(get(child[0], k), "", "{k}");
        }
        assert!(events.iter().all(|e| get(e, "way") != "p/held"));
    }

    // ── ADR-701 §2: what the prompt lane records for each hit ──

    use crate::cmd::show::ShowOutcome;

    fn hit(id: &str, needs_parent: bool) -> Hit<PromptPayload> {
        Hit::scored(id, None, ("keyword".to_string(), None, needs_parent))
    }

    /// Run [`admit_hits`] over `hits` with `gate`, every shown way firing, and
    /// return the record's outcomes keyed by way, its judge block, and the
    /// ways `show` was asked for.
    fn admit(hits: &[Hit<PromptPayload>], mut gate: gate::Gate) -> (serde_json::Value, Vec<String>) {
        let sink = |_: &[(&str, &str)]| {};
        let lc = gate::LogContext { session_id: "s", project_dir: "/tmp", scope: "agent", hook_event: "UserPromptSubmit", sink: &sink };
        let mut record = decision::Record::for_test();
        let mut asked = Vec::new();
        admit_hits(hits, &mut gate, &[], &lc, &mut record, |h| {
            asked.push(h.id.clone());
            (ShowOutcome::Fired, true)
        });
        (record.to_json(), asked)
    }

    fn gate_of(verdicts: &[(&str, f64)], mode: ways_agent_core::profile::Mode) -> gate::Gate {
        let sink = |_: &[(&str, &str)]| {};
        let lc = gate::LogContext { session_id: "s", project_dir: "/tmp", scope: "agent", hook_event: "UserPromptSubmit", sink: &sink };
        gate::test_gate(verdicts, mode, &lc)
    }

    fn outcome<'v>(record: &'v serde_json::Value, way: &str) -> &'v serde_json::Value {
        record["outcomes"].as_array().unwrap().iter().find(|o| o["way"] == way).unwrap_or_else(|| panic!("no outcome for {way}: {record}"))
    }

    #[test]
    fn an_enforced_block_is_recorded_and_never_shown_and_a_pass_carries_its_verdict() {
        let (r, asked) = admit(&[hit("a", false), hit("b", false)], gate_of(&[("a", 0.9), ("b", 0.05)], ways_agent_core::profile::Mode::Enforce));
        assert_eq!(asked, ["a"], "a blocked way is never shown");
        let a = outcome(&r, "a");
        assert_eq!((a["result"].as_str(), a["p_yes"].as_f64(), a["verdict"].as_str(), a["rank"].as_u64()), (Some("fired"), Some(0.9), Some("pass"), Some(1)));
        let b = outcome(&r, "b");
        assert_eq!((b["result"].as_str(), b["p_yes"].as_f64(), b["threshold"].as_f64(), b["mode"].as_str()), (Some("judge_block"), Some(0.05), Some(0.3), Some("enforce")));
        assert_eq!(r["judge"]["status"], "judged");
    }

    #[test]
    fn a_shadow_would_block_is_shown_and_recorded_with_its_verdict() {
        let (r, asked) = admit(&[hit("a", false)], gate_of(&[("a", 0.05)], ways_agent_core::profile::Mode::Shadow));
        assert_eq!(asked, ["a"]);
        let a = outcome(&r, "a");
        assert_eq!((a["result"].as_str(), a["verdict"].as_str()), (Some("fired"), Some("would_block")));
    }

    #[test]
    fn a_gate_fallback_shows_every_way_and_names_the_fallback() {
        let g = gate::Gate { status: gate::Status::Fallback { reason: "deadline".into() }, ..Default::default() };
        let (r, asked) = admit(&[hit("a", false), hit("b", false)], g);
        assert_eq!(asked, ["a", "b"]);
        assert!(["a", "b"].iter().all(|w| outcome(&r, w)["result"] == "fired" && outcome(&r, w).get("verdict").is_none()));
        assert_eq!(r["judge"], serde_json::json!({"status": "fallback", "reason": "deadline"}));
    }

    #[test]
    fn a_child_without_its_parent_is_withheld_and_with_it_is_shown() {
        let (r, asked) = admit(&[hit("p/c", true)], gate::Gate::default());
        assert!(asked.is_empty());
        assert_eq!(outcome(&r, "p/c")["result"], "withheld_for_parent");

        let (r, asked) = admit(&[hit("p", false), hit("p/c", true)], gate::Gate::default());
        assert_eq!(asked, ["p", "p/c"]);
        assert_eq!(outcome(&r, "p/c")["result"], "fired");
        assert_eq!(r["judge"]["status"], "off");
    }
}
