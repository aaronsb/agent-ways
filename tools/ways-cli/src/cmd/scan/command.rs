//! The command (Bash) lane: `PreToolUse` on a shell command.

use anyhow::Result;
use std::collections::HashSet;

use crate::cmd::show::ContextBudget;
use crate::session;

use super::candidates::{check_when, collect_candidates, collect_checks};
use super::order::{order_hits, Hit};
use super::scoring::{batch_embed_score, capture_show_check, capture_show_way, EmbedScores};
use super::{
    check_semantic_score, effective_thresholds, effective_thresholds_in_scan, emit_hook_context,
    has_shown_ancestor, lookbehind, reduce, regex_matches, regex_span, WayCandidate, BUDGET_COMMAND,
};

// ── Command scan ────────────────────────────────────────────────

pub fn command(
    cmd: &str,
    description: Option<&str>,
    session_id: &str,
    project: Option<&str>,
    transcript: Option<&str>,
) -> Result<()> {
    crate::cmd::show::set_firing_transcript(transcript);
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(crate::util::project_dir);

    session::bump_epoch(session_id);
    let scope = session::detect_scope(session_id);
    let candidates = collect_candidates(&project_dir);

    let mut context = String::new();
    let mut budget = ContextBudget::hook();

    // One embed pass for the whole surface (ways and checks share it).
    // ADR-130: cap embed input. Heredoc bodies (gh pr create --body
    // "$(cat <<EOF…)"), curl -d JSON payloads, and similar argument-
    // body bash commands can run kilobytes long. The regex matchers
    // below see the full cmd; only the embed query is reduced.
    // ADR-160 stage 1 (contextualize): a bare command is an action, not intent.
    // Prepend the assistant's prose since the last human turn — the reasoning that
    // led to this tool call — so the semantic lane matches on intent. Fail-safe:
    // None → command + tool description only (unchanged). The reducer trims the
    // combined surface to the command budget by sentence salience.
    let lookbehind = lookbehind::intent(session_id);
    let query_for_embed = format!(
        "{} {} {}",
        lookbehind.as_deref().unwrap_or(""),
        cmd,
        description.unwrap_or("")
    );
    let reduced_for_embed = reduce::reduce_for_embed(&query_for_embed, BUDGET_COMMAND);
    let embed_matches = batch_embed_score(&reduced_for_embed);

    let hits = command_hits(&CommandSurface { cmd, description, session_id, scope: &scope, project_dir: &project_dir }, &candidates, &embed_matches);
    let mut shown: HashSet<String> = HashSet::new();
    for hit in &hits {
        let (channel, span, needs_parent) = &hit.payload;
        if *needs_parent && !has_shown_ancestor(&hit.id, &shown) {
            continue;
        }
        // A regex hit carries its span and no surface; a semantic hit carries
        // the embedded surface and no span.
        let surface = if span.is_some() { None } else { Some(reduced_for_embed.as_str()) };
        let out = capture_show_way(
            &hit.id,
            session_id,
            channel,
            hit.score,
            span.as_deref(),
            surface,
            Some(&mut budget),
        )
        .body;
        if !out.is_empty() {
            shown.insert(hit.id.clone());
            context.push_str(&out);
        }
    }

    // Check matching: commands regex + semantic scoring.
    let checks = collect_checks(&project_dir);

    for check in &checks {
        if !session::scope_matches(&check.scope, &scope) {
            continue;
        }
        if !check_when(&check.when_project, &check.when_file_exists, &project_dir) {
            continue;
        }

        let mut match_score: f64 = 0.0;

        if let Some(ref cmds_pattern) = check.commands {
            if regex_matches(cmds_pattern, cmd) {
                match_score = 3.0;
            }
        }

        if match_score == 0.0 && !check.description.is_empty() && !check.vocabulary.is_empty() {
            match_score = check_semantic_score(check, session_id, &embed_matches);
        }

        if match_score > 0.0 {
            let out = capture_show_check(&check.id, session_id, "bash", match_score, Some(&mut budget));
            if !out.is_empty() {
                context.push_str(&out);
            }
        }
    }

    if !context.is_empty() {
        emit_hook_context("PreToolUse", context.trim_end());
    }

    Ok(())
}

/// What the Bash lane matches a command against.
pub(super) struct CommandSurface<'a> {
    pub(super) cmd: &'a str,
    pub(super) description: Option<&'a str>,
    pub(super) session_id: &'a str,
    pub(super) scope: &'a str,
    pub(super) project_dir: &'a str,
}

/// The Bash lane's way hits for one command, in admission order: the command
/// and description regexes, then the semantic lane on the tool description.
/// Shared by the scan and the probe scorer.
pub(super) fn command_hits(
    surface: &CommandSurface<'_>,
    candidates: &[WayCandidate],
    embed_matches: &EmbedScores,
) -> Vec<Hit<(&'static str, Option<String>, bool)>> {
    let CommandSurface { cmd, description, session_id, scope, project_dir } = *surface;
    // Way matching: commands regex + pattern regex + semantic (ADR-155 §4).
    // Hits are collected, then admitted in a fixed order (scan/order.rs).
    // Payload: (channel, matched span, fired only on this scan's parent boost).
    let mut hits: Vec<Hit<(&'static str, Option<String>, bool)>> = Vec::new();
    let mut fired_ids: HashSet<String> = HashSet::new();
    for way in candidates {
        if !session::scope_matches(&way.scope, scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, project_dir) {
            continue;
        }

        // Commands regex first, then the description pattern — capture the span
        // of whichever matched (ADR-153 §3), with the pattern that matched it.
        let matched = way
            .commands
            .as_deref()
            .and_then(|p| regex_span(p, cmd).map(|s| (p, s)))
            .or_else(|| match (description, way.pattern.as_deref()) {
                // Pattern compiles case-insensitively (ADR-157), so match the
                // description in its original case for a truer captured span.
                (Some(desc), Some(pat)) => regex_span(pat, desc).map(|s| (pat, s)),
                _ => None,
            });

        if let Some((pat, span)) = matched {
            fired_ids.insert(way.id.clone());
            hits.push(Hit::explicit(&way.id, pat, &span, ("bash", Some(span.clone()), false)));
            continue;
        }

        // Semantic lane at the bash surface (ADR-155 §4): the tool
        // `description` is Claude's own natural-language statement of intent,
        // scored with the same per-way thresholds as the prompt surface,
        // against embeddings this event already computed for checks. State-
        // triggered ways are excluded, mirroring the task surface — their
        // trigger is a condition, not a topic. No near-miss logging here:
        // bash events are the highest-volume surface, and the tuning stream
        // (ADR-134) is fed by the prompt/task surfaces.
        if way.trigger.is_some() {
            continue;
        }
        let (t, scan_boost) = effective_thresholds_in_scan(way, session_id, &fired_ids);
        let prob_en = embed_matches.prob_en(&way.corpus_id, way.embeddable());
        let prob_multi = embed_matches.prob_multi(&way.corpus_id, way.embeddable());
        // `semantic:` prefix keeps every consumer that special-cases semantic
        // channels (drill-down's "no recoverable term" note, span handling)
        // treating this lane correctly.
        let fired = if prob_en.is_some_and(|p| p >= t.semantic) {
            Some(("semantic:bash:en", prob_en))
        } else if prob_multi.is_some_and(|p| p >= t.semantic) {
            Some(("semantic:bash:multi", prob_multi))
        } else {
            None
        };
        if let Some((channel, score)) = fired {
            // Fired only on this scan's parent boost: shown only with that parent.
            let base = effective_thresholds(way, session_id).semantic;
            let needs_parent = scan_boost && !score.is_some_and(|p| p >= base);
            fired_ids.insert(way.id.clone());
            hits.push(Hit::scored(&way.id, score, (channel, None, needs_parent)));
        }
    }
    order_hits(&mut hits);
    hits
}
