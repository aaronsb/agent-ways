//! The task lane: a subagent or teammate dispatch (`PreToolUse` on the task
//! tool), whose matched ways are stashed for `SubagentStart` to claim.

use anyhow::Result;

use crate::session;

use super::candidates::collect_candidates;
use super::matching::{
    log_keyword_gated, log_near_miss, match_prompt, record_keyword_gated, record_near_miss, PromptMatch,
};
use super::order::{order_hits, Hit};
use super::{
    body_map, decision, effective_thresholds, eligible, late_interaction, mask_nonlinguistic, reduce, Lane,
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
    let candidates = collect_candidates(&project_dir);
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
    let bodies = body_map(candidates.iter().filter(|w| eligible(w, Lane::Task { teammate: is_teammate }, &project_dir)));
    let body_rank = crate::config::global().body_rank;
    let (embed_matches, single_fused) = late_interaction::single_scores(&reduced, &bodies, body_rank);
    let verdicts = late_interaction::run(&reduced, &bodies, crate::config::global().admission, body_rank);
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
    {
        let lane = Lane::Task { teammate: is_teammate };
        let enabled: std::collections::HashMap<&str, &str> = candidates
            .iter()
            .filter(|c| c.embeddable() && eligible(c, lane, &project_dir))
            .map(|c| (c.corpus_id.as_str(), c.id.as_str()))
            .collect();
        record.candidates(&embed_matches, &enabled, verdicts.as_ref().is_some_and(|v| v.used_sidecar()));
        record.body_rank(fused.then(|| body_rank.as_str()));
    }

    // Payload: channel. Ordered like the other lanes before the stash is written.
    let mut hits: Vec<Hit<String>> = Vec::new();

    for way in &candidates {
        if !eligible(way, Lane::Task { teammate: is_teammate }, &project_dir) {
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

    order_hits(&mut hits);
    let stashed = write_stash(&hits, record.scan_id(), session_id, is_teammate, team);
    let result = if stashed.is_ok() { "stashed" } else { "error" };
    for (i, hit) in hits.iter().enumerate() {
        record.hit(&hit.id, i + 1, &hit.payload, hit.score, result, None);
    }
    record.write();
    stashed
}

/// Write the task lane's matched ways to a stash file for SubagentStart to
/// claim, with the `scan_id` of the task scan's decision record, which
/// becomes the subagent's last scan. Nothing is written when nothing matched:
/// a stash nobody claims (a dispatch the operator denied) would be claimed by
/// the next subagent instead of its own.
fn write_stash(hits: &[Hit<String>], scan_id: &str, session_id: &str, is_teammate: bool, team: Option<&str>) -> Result<()> {
    let matched: Vec<(&str, &str)> = hits.iter().map(|h| (h.id.as_str(), h.payload.as_str())).collect();

    // Write stash file if any ways matched
    if !matched.is_empty() {
        let stash_dir = format!(
            "{}/{session_id}/subagent-stash",
            session::sessions_root()
        );
        std::fs::create_dir_all(&stash_dir)?;

        let ways: Vec<&str> = matched.iter().map(|(id, _)| *id).collect();
        let channels: Vec<&str> = matched.iter().map(|(_, ch)| *ch).collect();

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
    }

    Ok(())
}
