// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The file lane: `PreToolUse` on a file path.

use anyhow::Result;

use crate::cmd::show::ContextBudget;
use crate::session;

use super::candidates::{check_when, collect_checks, collect_with_off};
use super::decision::Surface;
use super::order::{order_hits, Hit};
use super::scoring::{batch_embed_score, capture_show_check, capture_show_way};
use super::{check_semantic_score, discard, emit_hook_context, gate, judged, reduce, regex_matches, regex_span, WayCandidate, BUDGET_FILE};

// ── File scan ───────────────────────────────────────────────────

pub fn file(
    filepath: &str,
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
    let (candidates, off) = collect_with_off(&project_dir);

    let mut context = String::new();
    let mut budget = ContextBudget::hook();

    let hits = file_hits(&candidates, filepath, &scope, &project_dir);
    log_discarded(&off, filepath, &discard::LaneLog { lane: "file", scope: &scope, session: session_id, project: &project_dir });

    let checks = collect_checks(&project_dir);
    // ADR-130: filepaths are short by nature, but enforce the budget
    // uniformly across all hook surfaces for consistency.
    let reduced = reduce::reduce_for_embed(filepath, BUDGET_FILE);
    let embed_matches = batch_embed_score(&reduced);
    let mut matched_checks: Vec<(&WayCandidate, f64)> = Vec::new();
    for check in &checks {
        if !session::scope_matches(&check.scope, &scope) || !check_when(&check.when_project, &check.when_file_exists, &project_dir) {
            continue;
        }
        let mut match_score: f64 = 0.0;
        if let Some(ref files_pattern) = check.files {
            if regex_matches(files_pattern, filepath) {
                match_score = 3.0;
            }
        }
        if match_score == 0.0 && !check.description.is_empty() && !check.vocabulary.is_empty() {
            match_score = check_semantic_score(check, session_id, &embed_matches);
        }
        if match_score > 0.0 {
            matched_checks.push((check, match_score));
        }
    }

    // The ways the path fired, then the checks, through the judge before any
    // fire is recorded (scan/judged.rs).
    let scan = judged::Scan::begin(session_id);
    let by_id: std::collections::HashMap<&str, &WayCandidate> = candidates.iter().map(|w| (w.id.as_str(), w)).collect();
    let mut judge_candidates: Vec<judged::Candidate> =
        hits.iter().filter_map(|h| by_id.get(h.id.as_str())).map(|w| judged::declared(w, "file", &scan)).collect();
    judge_candidates.extend(matched_checks.iter().map(|(check, score)| judged::check_candidate(check, *score, "file")));
    let lane = judged::Lane {
        surface: Surface::File,
        hook_event: "PreToolUse".into(),
        scope: scope.clone(),
        project_dir: project_dir.clone(),
        session_id: session_id.to_string(),
        command: None,
    };
    let turns = || judged::tool_turns(session_id, &format!("About to edit: {filepath}"));
    judged::run(&lane, &scan, &judge_candidates, turns, gate::TOOL_WAIT, |c| {
        if let Some((check, score)) = judged::check_of(&c.key).and_then(|id| matched_checks.iter().find(|(w, _)| w.id == id)) {
            let out = capture_show_check(&check.id, session_id, "file", *score, Some(&mut budget));
            let delivered = !out.is_empty();
            context.push_str(&out);
            return if delivered { "fired" } else { "held" };
        }
        let Some(hit) = hits.iter().find(|h| h.id == c.key) else { return "error" };
        let shown = capture_show_way(&hit.id, session_id, "file", None, Some(hit.payload.as_str()), None, Some(&mut budget));
        context.push_str(&shown.body);
        shown.outcome.as_str()
    });

    if !context.is_empty() {
        emit_hook_context("PreToolUse", context.trim_end());
    }

    Ok(())
}

/// The file-lane hits among `candidates` for `filepath`, in admission order
/// (scan/order.rs): walk order is directory order, and it would decide what
/// the budget withholds. The payload is the matched span.
fn file_hits(candidates: &[WayCandidate], filepath: &str, scope: &str, project_dir: &str) -> Vec<Hit<String>> {
    let mut hits: Vec<Hit<String>> = Vec::new();
    for way in candidates {
        if !session::scope_matches(&way.scope, scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, project_dir) {
            continue;
        }
        if let Some(ref files_pattern) = way.files {
            if let Some(span) = regex_span(files_pattern, filepath) {
                hits.push(Hit::explicit(&way.id, files_pattern, &span, span.clone()));
            }
        }
    }
    order_hits(&mut hits);
    hits
}

/// The file lane's shadow pass: [`file_hits`] over the switched-off ways, each
/// logged as a discarded fire.
pub(super) fn log_discarded(off: &discard::Switched, filepath: &str, at: &discard::LaneLog<'_>) {
    for hit in file_hits(&off.ways, filepath, at.scope, at.project) {
        discard::log(off, at, &discard::Hit { way: &hit.id, trigger: "file", span: Some(&hit.payload), scores: None });
    }
}
