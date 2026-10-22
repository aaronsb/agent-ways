//! The file lane: `PreToolUse` on a file path.

use anyhow::Result;

use crate::cmd::show::ContextBudget;
use crate::session;

use super::candidates::{check_when, collect_candidates, collect_checks};
use super::order::{order_hits, Hit};
use super::scoring::{batch_embed_score, capture_show_check, capture_show_way};
use super::{check_semantic_score, emit_hook_context, reduce, regex_matches, regex_span, BUDGET_FILE};

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
    let candidates = collect_candidates(&project_dir);

    let mut context = String::new();
    let mut budget = ContextBudget::hook();

    // Collect the hits, then admit them in a fixed order (scan/order.rs):
    // walk order is directory order, and it decides what the budget withholds.
    let mut hits: Vec<Hit<String>> = Vec::new();
    for way in &candidates {
        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, &project_dir) {
            continue;
        }

        if let Some(ref files_pattern) = way.files {
            if let Some(span) = regex_span(files_pattern, filepath) {
                hits.push(Hit::explicit(&way.id, files_pattern, &span, span.clone()));
            }
        }
    }
    order_hits(&mut hits);
    for hit in &hits {
        let out = capture_show_way(&hit.id, session_id, "file", None, Some(hit.payload.as_str()), None, Some(&mut budget)).body;
        if !out.is_empty() {
            context.push_str(&out);
        }
    }

    let checks = collect_checks(&project_dir);
    // ADR-130: filepaths are short by nature, but enforce the budget
    // uniformly across all hook surfaces for consistency.
    let reduced = reduce::reduce_for_embed(filepath, BUDGET_FILE);
    let embed_matches = batch_embed_score(&reduced);

    for check in &checks {
        if !session::scope_matches(&check.scope, &scope) {
            continue;
        }
        if !check_when(&check.when_project, &check.when_file_exists, &project_dir) {
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
            let out = capture_show_check(&check.id, session_id, "file", match_score, Some(&mut budget));
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
