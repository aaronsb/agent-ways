// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The task lane: a subagent or teammate dispatch (`PreToolUse` on the task
//! tool), whose matched ways are stashed for `SubagentStart` to claim.

use anyhow::Result;

use crate::session;

use super::candidates::collect_with_off;
use super::discard;
use super::matching::{
    log_keyword_gated, log_near_miss, match_prompt, record_keyword_gated, record_near_miss, PromptMatch,
};
use super::order::{order_hits, Hit};
use super::{
    body_map, decision, effective_thresholds, eligible, gate, judged, late_interaction, mask_nonlinguistic, reduce, Lane,
    BUDGET_TASK,
};

// ── Task scan (subagent/teammate stash) ────────────────────────

pub fn task(
    query: &str,
    session_id: &str,
    project: Option<&str>,
    team: Option<&str>,
) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(crate::util::project_dir);

    let is_teammate = team.is_some();
    let (candidates, off) = collect_with_off(&project_dir);
    let near_miss_margin = crate::config::global().near_miss_margin;
    let keyword_floor = crate::config::global().keyword_floor_probability;
    // Session scope for telemetry: the task channel is subagent unless a team
    // name marks it as a teammate dispatch.
    let task_scope = if is_teammate { "teammate" } else { "subagent" };

    // ADR-130: agent delegation prompts are the largest input class in
    // practice. Reduce to the model's window before embedding.
    let reduced = reduce::reduce_for_embed(query, BUDGET_TASK);
    let masked = mask_nonlinguistic(query);
    // ADR-160: the matcher is the semantic matcher on the task surface too;
    // single-vector is the fail-safe when it can't chunk (see scan::prompt).
    let lane = Lane::Task { teammate: is_teammate };
    let bodies = body_map(candidates.iter().filter(|w| eligible(w, lane, &project_dir)));
    let off_bodies = body_map(off.ways.iter().filter(|w| eligible(w, lane, &project_dir)));
    let body_rank = crate::config::global().body_rank;
    let (embed_matches, single_fused) = late_interaction::single_scores(&reduced, &bodies, body_rank);
    let verdicts = late_interaction::run(&reduced, &bodies, &off_bodies, crate::config::global().admission, body_rank);
    let fused = single_fused || verdicts.as_ref().is_some_and(|v| v.used_body_rank());
    // ADR-701 §2: the task lane writes a decision record too, its candidates
    // drawn from the ways eligible there. A dispatch is not a turn: the epoch
    // is the dispatching agent's, unbumped.
    let mut record = decision::Record::begin(&decision::Context {
        surface: decision::Surface::Task,
        hook_event: "PreToolUse",
        scope: task_scope,
        project: &project_dir,
        session: session_id,
        epoch: session::get_epoch(session_id),
        turn_start: false,
    });
    let enabled: std::collections::HashMap<&str, &str> = candidates
        .iter()
        .filter(|c| c.embeddable() && eligible(c, lane, &project_dir))
        .map(|c| (c.corpus_id.as_str(), c.id.as_str()))
        .collect();
    record.candidates(&embed_matches, &enabled, verdicts.as_ref().is_some_and(|v| v.used_sidecar()));
    record.body_rank(fused.then(|| body_rank.as_str()));

    // Payload: channel. Ordered like the other lanes before the stash is written.
    let mut hits: Vec<Hit<String>> = Vec::new();

    for way in &candidates {
        if !eligible(way, lane, &project_dir) {
            continue;
        }

        // Same strict semantics as the prompt surface: bypass mask + gate.
        let regex_text: &str = if way.pattern_strict { query } else { &masked };
        match match_prompt(
            regex_text,
            &way.pattern,
            way.pattern_strict,
            way.embeddable(),
            &way.corpus_id,
            effective_thresholds(way, session_id),
            &embed_matches,
            near_miss_margin,
            keyword_floor,
            verdicts.as_ref(),
        ) {
            PromptMatch::Fired { channel, score, matched_span } => {
                let hit = match (&matched_span, way.pattern.as_deref()) {
                    (Some(span), Some(pat)) if score.is_none() => {
                        Hit::explicit(&way.id, pat, span, channel)
                    }
                    _ => Hit::scored(&way.id, score, channel),
                };
                hits.push(hit);
            }
            PromptMatch::KeywordGated(kg) => {
                log_keyword_gated(way, &kg, "task", task_scope, &project_dir, session_id);
                record_keyword_gated(&mut record, way, &kg);
            }
            PromptMatch::NearMiss(nm) => {
                log_near_miss(way, &nm, "task", task_scope, &project_dir, session_id, query);
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
            recheck: None,
            verdicts: verdicts.as_ref(),
            boost: None,
            enabled: &enabled,
        },
        &discard::LaneLog { lane: "task", scope: task_scope, session: session_id, project: &project_dir },
    );

    order_hits(&mut hits);

    // The judge reads the dispatch against the brief the subagent will get,
    // the subagent's request. The subagent starts with no fires, so nothing
    // is held by refire; a blocked way is not stashed.
    let by_id: std::collections::HashMap<&str, &super::WayCandidate> = candidates.iter().map(|w| (w.id.as_str(), w)).collect();
    let judge_candidates: Vec<judged::Candidate> = hits
        .iter()
        .filter_map(|hit| {
            let way = by_id.get(hit.id.as_str())?;
            Some(judged::Candidate {
                key: hit.id.clone(),
                description: way.description.clone(),
                trigger: judged::matched_trigger(&hit.payload, way.pattern_strict),
                channel: hit.payload.clone(),
                p: hit.score,
                open: true,
            })
        })
        .collect();
    let lane = judged::Lane {
        surface: decision::Surface::Task,
        hook_event: "PreToolUse".into(),
        scope: task_scope.to_string(),
        project_dir: project_dir.clone(),
        session_id: session_id.to_string(),
        command: None,
    };
    let scan = judged::Scan::begin(session_id);
    let turns = vec![ways_agent_core::judge::Turn { role: ways_agent_core::judge::Role::User, text: query.to_string() }];
    let judged = judged::judge(&lane, &scan, &judge_candidates, turns, gate::TOOL_WAIT);
    let kept: Vec<Hit<String>> = hits
        .iter()
        .filter(|h| !judged.blocked(&h.id))
        .map(|h| Hit { id: h.id.clone(), score: h.score, specificity: h.specificity, payload: h.payload.clone() })
        .collect();

    let stashed = write_stash(&kept, record.scan_id(), session_id, is_teammate, team);
    let result = if stashed.is_ok() { "stashed" } else { "error" };
    let mut shown = Vec::new();
    for (i, hit) in hits.iter().enumerate() {
        let r = decision::block_result(&judged.gate.blocked, &hit.id).unwrap_or(result);
        record.hit(&hit.id, i + 1, &hit.payload, hit.score, r, Some(&judged.gate));
        if let Some(c) = judge_candidates.iter().find(|c| c.key == hit.id) {
            shown.push(judged::Shown { candidate: c, result: r });
        }
    }
    if scan.is_on() && !judge_candidates.is_empty() {
        judged::close_judge(&mut record, &judged.gate, &judged.turns);
    }
    let scan_id = record.scan_id().to_string();
    record.write();
    judged::detach(&lane, judged, &shown, &scan_id);
    stashed
}

/// Write the ways the judge kept for a dispatch to a stash file for
/// SubagentStart to claim, with the `scan_id` of the task scan's decision
/// record, which becomes the subagent's last scan. The caller writes one stash
/// for every dispatch, with `ways: []` when nothing matched or the judge
/// blocked every match. SubagentStart claims the oldest stash, so a dispatch
/// that left none would hand its subagent the next dispatch's ways and scan id.
fn write_stash(kept: &[Hit<String>], scan_id: &str, session_id: &str, is_teammate: bool, team: Option<&str>) -> Result<()> {
    let stash_dir = format!("{}/{session_id}/subagent-stash", session::sessions_root());
    std::fs::create_dir_all(&stash_dir)?;
    let ways: Vec<&str> = kept.iter().map(|h| h.id.as_str()).collect();
    let channels: Vec<&str> = kept.iter().map(|h| h.payload.as_str()).collect();
    let stash = serde_json::json!({
        "ways": ways,
        "channels": channels,
        "is_teammate": is_teammate,
        "team_name": team.unwrap_or(""),
        "scan_id": scan_id,
    });
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Published whole: SubagentStart may claim the stash the moment it
    // appears, and the writer's temporary name does not end in `.json`.
    let stash_file = format!("{stash_dir}/{timestamp}.json");
    agent_settings::writer::write_atomic(std::path::Path::new(&stash_file), stash.to_string())?;
    Ok(())
}
