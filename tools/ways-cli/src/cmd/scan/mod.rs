//! Scan ways and output matched content — replaces hook scan loops.
//!
//! Combines file walking, frontmatter extraction, matching (pattern + semantic),
//! scope/precondition gating, parent-threshold lowering, and show (display).

pub(crate) mod candidates;
mod reduce;
mod scoring;
mod state;
pub(crate) use scoring::batch_embed_score;

// Per-hook embed-query budgets (approximate tokens). MiniLM's window
// is 128 position embeddings; we budget ~85% of that. The reducer
// passes inputs through unchanged when they already fit; long inputs
// collapse to top-salience sentences within budget. The approximate
// tokenizer here (whitespace + char-budget max) over-counts vs
// MiniLM's WordPiece, so real tokens land safely under 128 even at
// the higher budgets. See ADR-130.
const BUDGET_PROMPT: usize = 110;
const BUDGET_TASK: usize = 110;
const BUDGET_COMMAND: usize = 75;
const BUDGET_FILE: usize = 30;
pub use state::state;

use anyhow::Result;
use regex::Regex;
use std::path::PathBuf;

use crate::session;

use candidates::{check_when, collect_candidates, collect_checks};
use scoring::{capture_show_check, capture_show_way, default_project, EmbedScores};

pub(crate) struct WayCandidate {
    pub id: String,
    /// Namespaced id used solely for the embedding-corpus lookup. Equals `id`
    /// for global ways; for project ways it is `{project_key}/{id}`, matching
    /// how `ways corpus` namespaces project entries. Session markers, show, and
    /// parent-boost all use the bare `id`, not this.
    pub corpus_id: String,
    pub path: PathBuf,
    pub pattern: Option<String>,
    pub commands: Option<String>,
    pub files: Option<String>,
    pub description: String,
    pub vocabulary: String,
    /// Context-threshold percentage (only meaningful for trigger: context-threshold).
    pub threshold: f64,
    /// Per-way cosine-similarity threshold. When absent, uses config default.
    /// Parent-boost (ADR-125) multiplies this at match time if any ancestor has fired.
    pub embed_threshold: Option<f64>,
    pub scope: String,
    pub when_project: Option<String>,
    pub when_file_exists: Option<String>,
    pub trigger: Option<String>,
    pub repeat: bool,
    pub trigger_path: Option<String>,
}

// ── Prompt scan ─────────────────────────────────────────────────

/// Match user prompt against ways and emit matched bodies for the agent.
///
/// Wired only from the `UserPromptSubmit` hook (`check-prompt.sh`), so the
/// envelope event name is hardcoded. The call routes through the canonical
/// `hookSpecificOutput` default branch of `emit_hook_context`. If this is
/// ever reused from another hook event, just pass that event's name —
/// `SessionStart` and `PreToolUse` are the only events that take the
/// legacy top-level `additionalContext` envelope.
pub fn prompt(query: &str, session_id: &str, project: Option<&str>) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(default_project);

    // Bump epoch
    session::bump_epoch(session_id);

    let scope = session::detect_scope(session_id);
    let candidates = collect_candidates(&project_dir);
    let near_miss_margin = crate::config::global().near_miss_margin;

    // ADR-130: cap embed input to the model's working window via the
    // sentence-salience reducer. Pattern/keyword matching downstream
    // still operates on `query` (the full prompt) — only the embed
    // signal sees the reduced form.
    let reduced = reduce::reduce_for_embed(query, BUDGET_PROMPT);
    let embed_matches = batch_embed_score(&reduced);

    let mut context = String::new();

    for way in &candidates {
        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, &project_dir) {
            continue;
        }

        // Additive matching: pattern OR semantic
        match match_prompt(
            query,
            &way.pattern,
            &way.corpus_id,
            effective_thresholds(way, session_id),
            &embed_matches,
            near_miss_margin,
        ) {
            PromptMatch::Fired { channel, score, matched_span } => {
                let out = capture_show_way(&way.id, session_id, &channel, score, matched_span.as_deref());
                if !out.is_empty() {
                    context.push_str(&out);
                    context.push_str("\n\n");
                }
            }
            PromptMatch::NearMiss(nm) => {
                log_near_miss(way, &nm, "prompt", &scope, &project_dir, session_id, query);
            }
            PromptMatch::NoMatch => {}
        }
    }

    if !context.is_empty() {
        emit_hook_context("UserPromptSubmit", context.trim_end());
    }

    Ok(())
}

// ── Task scan (subagent/teammate stash) ────────────────────────

pub fn task(
    query: &str,
    session_id: &str,
    project: Option<&str>,
    team: Option<&str>,
) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(default_project);

    let is_teammate = team.is_some();
    let candidates = collect_candidates(&project_dir);
    let near_miss_margin = crate::config::global().near_miss_margin;
    // Session scope for telemetry: the task channel is subagent unless a team
    // name marks it as a teammate dispatch.
    let task_scope = if is_teammate { "teammate" } else { "subagent" };

    // ADR-130: agent delegation prompts are the largest input class in
    // practice. Reduce to the model's window before embedding.
    let reduced = reduce::reduce_for_embed(query, BUDGET_TASK);
    let embed_matches = batch_embed_score(&reduced);

    let mut matched: Vec<(String, String)> = Vec::new(); // (way_id, channel)

    for way in &candidates {
        // Must have subagent or teammate scope
        let scope = &way.scope;
        if is_teammate {
            if !scope.contains("subagent") && !scope.contains("teammate") {
                continue;
            }
        } else if !scope.contains("subagent") {
            continue;
        }

        // Skip state-triggered ways
        if way.trigger.is_some() {
            continue;
        }

        if !check_when(&way.when_project, &way.when_file_exists, &project_dir) {
            continue;
        }

        match match_prompt(
            query,
            &way.pattern,
            &way.corpus_id,
            effective_thresholds(way, session_id),
            &embed_matches,
            near_miss_margin,
        ) {
            PromptMatch::Fired { channel, .. } => matched.push((way.id.clone(), channel)),
            PromptMatch::NearMiss(nm) => {
                log_near_miss(way, &nm, "task", task_scope, &project_dir, session_id, query);
            }
            PromptMatch::NoMatch => {}
        }
    }

    // Write stash file if any ways matched
    if !matched.is_empty() {
        let stash_dir = format!(
            "{}/{session_id}/subagent-stash",
            session::sessions_root()
        );
        std::fs::create_dir_all(&stash_dir)?;

        let ways: Vec<&str> = matched.iter().map(|(id, _)| id.as_str()).collect();
        let channels: Vec<&str> = matched.iter().map(|(_, ch)| ch.as_str()).collect();

        let stash = serde_json::json!({
            "ways": ways,
            "channels": channels,
            "is_teammate": is_teammate,
            "team_name": team.unwrap_or(""),
        });

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stash_file = format!("{stash_dir}/{timestamp}.json");
        std::fs::write(&stash_file, stash.to_string())?;
    }

    Ok(())
}

// ── Command scan ────────────────────────────────────────────────

pub fn command(
    cmd: &str,
    description: Option<&str>,
    session_id: &str,
    project: Option<&str>,
) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(default_project);

    session::bump_epoch(session_id);
    let scope = session::detect_scope(session_id);
    let candidates = collect_candidates(&project_dir);

    let mut context = String::new();

    // Way matching: commands regex + pattern regex
    for way in &candidates {
        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, &project_dir) {
            continue;
        }

        // Commands regex first, then the description pattern — capture the span
        // of whichever matched (ADR-153 §3).
        let matched_span = way
            .commands
            .as_deref()
            .and_then(|p| regex_span(p, cmd))
            .or_else(|| match (description, way.pattern.as_deref()) {
                (Some(desc), Some(pat)) => regex_span(pat, &desc.to_lowercase()),
                _ => None,
            });

        if let Some(span) = matched_span {
            let out = capture_show_way(&way.id, session_id, "bash", None, Some(span.as_str()));
            if !out.is_empty() {
                context.push_str(&out);
            }
        }
    }

    // Check matching: commands regex + semantic scoring.
    // ADR-130: cap embed input. Heredoc bodies (gh pr create --body
    // "$(cat <<EOF…)"), curl -d JSON payloads, and similar argument-
    // body bash commands can run kilobytes long. The regex matcher
    // above already saw the full cmd; only the embed query is reduced.
    let checks = collect_checks(&project_dir);
    let query_for_checks = format!(
        "{} {}",
        cmd,
        description.unwrap_or("")
    );
    let reduced_for_checks = reduce::reduce_for_embed(&query_for_checks, BUDGET_COMMAND);
    let embed_check_matches = batch_embed_score(&reduced_for_checks);

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
            match_score = check_semantic_score(check, session_id, &embed_check_matches);
        }

        if match_score > 0.0 {
            let out = capture_show_check(&check.id, session_id, "bash", match_score);
            if !out.is_empty() {
                context.push_str(&out);
            }
        }
    }

    // Output JSON for PreToolUse
    if !context.is_empty() {
        println!(
            "{}",
            serde_json::json!({
                "decision": "approve",
                "additionalContext": context
            })
        );
    }

    Ok(())
}

// ── File scan ───────────────────────────────────────────────────

pub fn file(filepath: &str, session_id: &str, project: Option<&str>) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(default_project);

    session::bump_epoch(session_id);
    let scope = session::detect_scope(session_id);
    let candidates = collect_candidates(&project_dir);

    let mut context = String::new();

    for way in &candidates {
        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, &project_dir) {
            continue;
        }

        if let Some(ref files_pattern) = way.files {
            if let Some(span) = regex_span(files_pattern, filepath) {
                let out = capture_show_way(&way.id, session_id, "file", None, Some(span.as_str()));
                if !out.is_empty() {
                    context.push_str(&out);
                }
            }
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
            let out = capture_show_check(&check.id, session_id, "file", match_score);
            if !out.is_empty() {
                context.push_str(&out);
            }
        }
    }

    if !context.is_empty() {
        println!(
            "{}",
            serde_json::json!({
                "decision": "approve",
                "additionalContext": context
            })
        );
    }

    Ok(())
}

// ── Matching ────────────────────────────────────────────────────

/// Outcome of matching a prompt against one way.
enum PromptMatch {
    /// The way fired. `channel` is the trigger channel; `score` is the
    /// embedding score that cleared threshold (`None` for deterministic keyword
    /// fires) — logged onto `way_fired` for embed_threshold tuning (ADR-134 D).
    /// `matched_span` is the regex match text for the keyword channel (ADR-153 §3);
    /// `None` for semantic — one embedding per way, so there is no term to recover.
    Fired { channel: String, score: Option<f64>, matched_span: Option<String> },
    /// The way did NOT fire, but at least one model scored within
    /// `near_miss_margin` below its effective threshold (ADR-134). Carries the
    /// already-computed scores for telemetry — no new embedding is done.
    NearMiss(NearMiss),
    /// No match, and not close enough to record.
    NoMatch,
}

/// A below-threshold embedding result close enough to log (ADR-134 Decision 1).
struct NearMiss {
    score_en: Option<f64>,
    score_multi: Option<f64>,
    thr_en: f64,
    thr_multi: f64,
    /// Smallest `threshold - score` among the models within margin — how close
    /// the way came to firing on its best path.
    margin: f64,
}

fn match_prompt(
    query: &str,
    pattern: &Option<String>,
    corpus_id: &str,
    thresholds: EffectiveThresholds,
    scores: &EmbedScores,
    near_miss_margin: f64,
) -> PromptMatch {
    // Channel 1: Regex pattern — deterministic, scoreless. A pattern miss is
    // never a near-miss (there is no margin to be near).
    if let Some(ref pat) = pattern {
        if let Some(span) = regex_span(pat, query) {
            return PromptMatch::Fired {
                channel: "keyword".to_string(),
                score: None,
                matched_span: Some(span),
            };
        }
    }

    // Channel 2: Embedding. Each model path stands on its own threshold;
    // scores don't cross-compare (apples and oranges). Either path firing
    // is sufficient, but the thresholds are calibrated independently so
    // each model's noise band sits below its gate:
    //   - EN model (0.40): sharp on English, noise below 0.35
    //   - multi model (0.55): cross-lingual but coarser, noise at 0.30-0.50
    let score_en = scores.best_en(corpus_id);
    let score_multi = scores.best_multi(corpus_id);

    if score_en.is_some_and(|s| s >= thresholds.en) {
        return PromptMatch::Fired {
            channel: "semantic:embedding:en".to_string(),
            score: score_en,
            matched_span: None,
        };
    }
    if score_multi.is_some_and(|s| s >= thresholds.multi) {
        return PromptMatch::Fired {
            channel: "semantic:embedding:multi".to_string(),
            score: score_multi,
            matched_span: None,
        };
    }

    // No fire. Record a near-miss when a model landed in the band just below
    // its threshold: `thr - margin <= score < thr`. The reported margin is the
    // smallest shortfall across qualifying models. Measured against the SAME
    // effective thresholds the fire path uses, so parent-boost is honored.
    let shortfall = |score: Option<f64>, thr: f64| -> Option<f64> {
        score.and_then(|s| {
            let gap = thr - s;
            (gap > 0.0 && gap <= near_miss_margin).then_some(gap)
        })
    };
    let margin = [
        shortfall(score_en, thresholds.en),
        shortfall(score_multi, thresholds.multi),
    ]
    .into_iter()
    .flatten()
    .fold(None, |acc: Option<f64>, g| Some(acc.map_or(g, |a| a.min(g))));

    match margin {
        Some(margin) => PromptMatch::NearMiss(NearMiss {
            score_en,
            score_multi,
            thr_en: thresholds.en,
            thr_multi: thresholds.multi,
            margin,
        }),
        None => PromptMatch::NoMatch,
    }
}

/// Emit a `way_nearmiss` telemetry event (ADR-134 Decision 1): a way that did
/// not fire but scored within the near-miss margin of its threshold. This is
/// persistence of already-computed scores, not new work — the tuning passes
/// (`ways tune --cadence/--precision`) consume the stream. The leading fields
/// (`event`, `way`, `domain`, `trigger`, `scope`, `project`, `session`) follow
/// the `way_fired` convention (scan/state.rs) for reader symmetry; the score
/// fields are near-miss-specific. There is no `team` field — team attribution
/// lives on fires (show/mod.rs), not on the below-threshold telemetry.
fn log_near_miss(
    way: &WayCandidate,
    nm: &NearMiss,
    trigger: &str,
    scope: &str,
    project_dir: &str,
    session_id: &str,
    query: &str,
) {
    let fmt = |v: Option<f64>| v.map(|s| format!("{s:.4}")).unwrap_or_default();
    let domain = way.id.split('/').next().unwrap_or(&way.id);
    // ADR-134 task E: events.jsonl rotation/cap will bound this stream's growth.
    session::log_event(&[
        ("event", "way_nearmiss"),
        ("way", &way.id),
        ("corpus_id", &way.corpus_id),
        ("domain", domain),
        ("score_en", &fmt(nm.score_en)),
        ("score_multi", &fmt(nm.score_multi)),
        ("thr_en", &format!("{:.4}", nm.thr_en)),
        ("thr_multi", &format!("{:.4}", nm.thr_multi)),
        ("margin", &format!("{:.4}", nm.margin)),
        ("trigger", trigger),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id),
        ("query_tokens", &reduce::approx_tokens(query).to_string()),
    ]);
}

/// Per-model thresholds for a way at a given moment in a session.
#[derive(Clone, Copy)]
struct EffectiveThresholds {
    en: f64,
    multi: f64,
}

/// Compute effective thresholds for both models, accounting for parent-boost.
///
/// Parent-boost (ADR-125): if any ancestor has fired in the session, each
/// model's base threshold is multiplied by `parent_threshold_multiplier`
/// (default 0.8), floored at `parent_boost_floor`. The floor prevents
/// cascading boosts from pushing children into the noise band.
///
/// The EN base comes from the way's frontmatter `embed_threshold:` or
/// `default_embed_threshold`. The multi base uses `default_multi_embed_threshold`
/// uniformly — locale aliases don't carry per-way thresholds (ADR-125).
fn effective_thresholds(way: &WayCandidate, session_id: &str) -> EffectiveThresholds {
    let cfg = crate::config::global();
    let en_base = way.embed_threshold.unwrap_or(cfg.default_embed_threshold);
    let multi_base = cfg.default_multi_embed_threshold;

    let ancestor_shown = {
        let mut path = way.id.as_str();
        let mut found = false;
        while let Some(idx) = path.rfind('/') {
            path = &path[..idx];
            if session::way_is_shown(path, session_id) {
                found = true;
                break;
            }
        }
        found
    };

    if ancestor_shown {
        let boost = cfg.parent_threshold_multiplier;
        let floor = cfg.parent_boost_floor;
        EffectiveThresholds {
            en: (en_base * boost).max(floor),
            multi: (multi_base * boost).max(floor),
        }
    } else {
        EffectiveThresholds { en: en_base, multi: multi_base }
    }
}

/// Semantic score for a check, taking the higher of the two model paths
/// that clears its own threshold. The two models are evaluated
/// independently (apples and oranges); if either path's score >= its
/// threshold, the check fires at that score. Returns 0.0 if neither
/// path clears.
fn check_semantic_score(check: &WayCandidate, session_id: &str, scores: &EmbedScores) -> f64 {
    let t = effective_thresholds(check, session_id);
    let en = scores.best_en(&check.corpus_id).filter(|s| *s >= t.en);
    let mu = scores.best_multi(&check.corpus_id).filter(|s| *s >= t.multi);
    match (en, mu) {
        (Some(e), Some(m)) => e.max(m),
        (Some(s), None) | (None, Some(s)) => s,
        (None, None) => 0.0,
    }
}

fn regex_matches(pattern: &str, text: &str) -> bool {
    Regex::new(pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

/// The first regex match in `text`, length-capped for the event log (ADR-153 §3
/// `matched_span`). `None` if the pattern is invalid or doesn't match — mirroring
/// [`regex_matches`]' error tolerance. The cap bounds how much of the matched
/// input (a prompt/command/path fragment) lands in local telemetry; the pattern
/// itself is author-controlled, so a match is normally a bounded keyword.
fn regex_span(pattern: &str, text: &str) -> Option<String> {
    const MAX_SPAN: usize = 120;
    let m = Regex::new(pattern).ok()?.find(text)?;
    let s = m.as_str();
    Some(match s.char_indices().nth(MAX_SPAN) {
        Some((byte, _)) => format!("{}…", &s[..byte]),
        None => s.to_string(),
    })
}

/// Emit accumulated context using the envelope shape required by the
/// invoking hook event. The Claude Code hook contract treats
/// `hookSpecificOutput` as canonical for all events; the simpler top-level
/// `additionalContext` is a legacy tolerance accepted only on
/// `SessionStart` and `PreToolUse` (where it surfaces as a visible
/// attachment). Defaulting to canonical means new event wirings
/// (`Stop`, `PreCompact`, ...) get the right shape automatically rather
/// than silently re-hitting the bug PR #80 fixed.
pub(super) fn emit_hook_context(hook_event: &str, context: &str) {
    let payload = match hook_event {
        "SessionStart" | "PreToolUse" => {
            serde_json::json!({ "additionalContext": context })
        }
        _ => serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": hook_event,
                "additionalContext": context,
            }
        }),
    };
    println!("{payload}");
}

#[cfg(test)]
mod near_miss_tests {
    //! ADR-134 task A: the near-miss decision in `match_prompt`. These cover
    //! the pure score/threshold arithmetic — no embedding subprocess, no I/O.
    use super::*;

    const THR: EffectiveThresholds = EffectiveThresholds { en: 0.40, multi: 0.55 };
    const MARGIN: f64 = 0.05;

    fn scores(en: Option<f64>, multi: Option<f64>) -> EmbedScores {
        EmbedScores {
            en: en.map(|s| vec![("w".to_string(), s)]),
            multi: multi.map(|s| vec![("w".to_string(), s)]),
        }
    }

    fn run(en: Option<f64>, multi: Option<f64>, pattern: Option<&str>) -> PromptMatch {
        match_prompt(
            "query text",
            &pattern.map(|p| p.to_string()),
            "w",
            THR,
            &scores(en, multi),
            MARGIN,
        )
    }

    #[test]
    fn en_clears_fires_en() {
        assert!(matches!(run(Some(0.41), None, None),
            PromptMatch::Fired { channel: c, .. } if c == "semantic:embedding:en"));
    }

    #[test]
    fn semantic_fire_carries_its_score_keyword_does_not() {
        // ADR-134 D: the firing embedding score rides on Fired for telemetry;
        // a deterministic keyword fire carries none.
        match run(Some(0.41), None, None) {
            PromptMatch::Fired { score, .. } => assert_eq!(score, Some(0.41)),
            _ => panic!("expected Fired"),
        }
        // multi fire (EN below) carries the multi score, not EN's.
        match run(Some(0.20), Some(0.56), None) {
            PromptMatch::Fired { channel, score, matched_span } => {
                assert_eq!(channel, "semantic:embedding:multi");
                assert_eq!(score, Some(0.56));
                assert_eq!(matched_span, None, "semantic carries no matched term");
            }
            _ => panic!("expected multi Fired"),
        }
        match run(Some(0.41), None, Some("query")) {
            PromptMatch::Fired { channel, score, matched_span } => {
                assert_eq!(channel, "keyword");
                assert_eq!(score, None);
                assert_eq!(matched_span.as_deref(), Some("query"), "keyword records the regex match");
            }
            _ => panic!("expected keyword Fired"),
        }
    }

    #[test]
    fn multi_clears_when_en_below_fires_multi() {
        assert!(matches!(run(Some(0.20), Some(0.56), None),
            PromptMatch::Fired { channel: c, .. } if c == "semantic:embedding:multi"));
    }

    #[test]
    fn within_margin_is_near_miss_with_shortfall() {
        match run(Some(0.37), None, None) {
            PromptMatch::NearMiss(nm) => {
                assert!((nm.margin - 0.03).abs() < 1e-9, "margin = thr - score");
                assert_eq!(nm.score_en, Some(0.37));
                assert_eq!(nm.score_multi, None);
            }
            other => panic!("expected NearMiss, got {:?}", discriminant(&other)),
        }
    }

    #[test]
    fn smallest_shortfall_wins_across_models() {
        // en short by 0.03, multi short by 0.02 -> reported margin is 0.02.
        match run(Some(0.37), Some(0.53), None) {
            PromptMatch::NearMiss(nm) => assert!((nm.margin - 0.02).abs() < 1e-9),
            other => panic!("expected NearMiss, got {:?}", discriminant(&other)),
        }
    }

    #[test]
    fn beyond_margin_is_no_match() {
        assert!(matches!(run(Some(0.30), None, None), PromptMatch::NoMatch));
    }

    #[test]
    fn pattern_match_preempts_near_miss() {
        // Scores would be a near-miss, but a keyword hit is a deterministic fire.
        assert!(matches!(run(Some(0.37), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn absent_scores_are_no_match() {
        assert!(matches!(run(None, None, None), PromptMatch::NoMatch));
    }

    #[test]
    fn score_exactly_at_threshold_fires_not_near_miss() {
        // The boundary where the `>=` fire check and the `gap > 0.0` near-miss
        // guard must agree: a score equal to the threshold fires, it is never
        // a (zero-shortfall) near-miss.
        assert!(matches!(run(Some(0.40), None, None),
            PromptMatch::Fired { channel: c, .. } if c == "semantic:embedding:en"));
    }

    fn discriminant(m: &PromptMatch) -> &'static str {
        match m {
            PromptMatch::Fired { .. } => "Fired",
            PromptMatch::NearMiss(_) => "NearMiss",
            PromptMatch::NoMatch => "NoMatch",
        }
    }
}
