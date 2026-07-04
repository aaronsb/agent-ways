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
    /// Opt-out from the semantic keyword gate (ADR-155). `true` means every
    /// pattern hit fires regardless of embedding score — for patterns that
    /// genuinely mean "this exact token, always" (e.g. slash-command names).
    pub pattern_strict: bool,
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
    // operates on the masked full prompt (ADR-155 §2: URLs and fenced
    // code are not lexical intent) — only the embed signal sees the
    // reduced form, and it sees the unmasked original.
    let reduced = reduce::reduce_for_embed(query, BUDGET_PROMPT);
    let embed_matches = batch_embed_score(&reduced);
    let masked = mask_nonlinguistic(query);
    let gate_fraction = crate::config::global().keyword_gate_fraction;

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
            &masked,
            &way.pattern,
            way.pattern_strict,
            &way.corpus_id,
            effective_thresholds(way, session_id),
            &embed_matches,
            near_miss_margin,
            gate_fraction,
        ) {
            PromptMatch::Fired { channel, score, matched_span } => {
                let out = capture_show_way(&way.id, session_id, &channel, score, matched_span.as_deref());
                if !out.is_empty() {
                    context.push_str(&out);
                    context.push_str("\n\n");
                }
            }
            PromptMatch::KeywordGated(kg) => {
                log_keyword_gated(way, &kg, "prompt", &scope, &project_dir, session_id);
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
    let masked = mask_nonlinguistic(query);
    let gate_fraction = crate::config::global().keyword_gate_fraction;

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
            &masked,
            &way.pattern,
            way.pattern_strict,
            &way.corpus_id,
            effective_thresholds(way, session_id),
            &embed_matches,
            near_miss_margin,
            gate_fraction,
        ) {
            PromptMatch::Fired { channel, .. } => matched.push((way.id.clone(), channel)),
            PromptMatch::KeywordGated(kg) => {
                log_keyword_gated(way, &kg, "task", task_scope, &project_dir, session_id);
            }
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

    // One embed pass for the whole surface (ways and checks share it).
    // ADR-130: cap embed input. Heredoc bodies (gh pr create --body
    // "$(cat <<EOF…)"), curl -d JSON payloads, and similar argument-
    // body bash commands can run kilobytes long. The regex matchers
    // below see the full cmd; only the embed query is reduced.
    let query_for_embed = format!(
        "{} {}",
        cmd,
        description.unwrap_or("")
    );
    let reduced_for_embed = reduce::reduce_for_embed(&query_for_embed, BUDGET_COMMAND);
    let embed_matches = batch_embed_score(&reduced_for_embed);

    // Way matching: commands regex + pattern regex + semantic (ADR-155 §4)
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
        let t = effective_thresholds(way, session_id);
        let score_en = embed_matches.best_en(&way.corpus_id);
        let score_multi = embed_matches.best_multi(&way.corpus_id);
        let fired = if score_en.is_some_and(|s| s >= t.en) {
            Some(("bash:semantic:en", score_en))
        } else if score_multi.is_some_and(|s| s >= t.multi) {
            Some(("bash:semantic:multi", score_multi))
        } else {
            None
        };
        if let Some((channel, score)) = fired {
            let out = capture_show_way(&way.id, session_id, channel, score, None);
            if !out.is_empty() {
                context.push_str(&out);
            }
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
    /// The pattern matched but the embedding score fell below the keyword gate
    /// floor on every available model lane (ADR-155): a lexical coincidence,
    /// vetoed. Carries the already-computed evidence for `way_keyword_gated`
    /// telemetry — the stream that calibrates `keyword_gate_fraction`.
    KeywordGated(KeywordGated),
    /// The way did NOT fire, but at least one model scored within
    /// `near_miss_margin` below its effective threshold (ADR-134). Carries the
    /// already-computed scores for telemetry — no new embedding is done.
    NearMiss(NearMiss),
    /// No match, and not close enough to record.
    NoMatch,
}

/// Evidence for a gated keyword hit (ADR-155): what the pattern matched and
/// how far below the gate floor each model lane scored.
struct KeywordGated {
    matched_span: String,
    score_en: Option<f64>,
    score_multi: Option<f64>,
    floor_en: f64,
    floor_multi: f64,
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
    pattern_strict: bool,
    corpus_id: &str,
    thresholds: EffectiveThresholds,
    scores: &EmbedScores,
    near_miss_margin: f64,
    gate_fraction: f64,
) -> PromptMatch {
    let score_en = scores.best_en(corpus_id);
    let score_multi = scores.best_multi(corpus_id);

    // Channel 1: Regex pattern — deterministic, but gated (ADR-155): the hit
    // fires only if the way's embedding score also clears
    // `gate_fraction × effective_threshold` on at least one model lane. The
    // gate consumes scores the batch pass already computed — no extra model
    // work. It fails OPEN when no lane produced a score (engine unavailable,
    // way absent from the corpus): with no semantic evidence either direction,
    // the author's explicit trigger stands. `pattern_strict: true` and
    // `gate_fraction: 0.0` both restore unconditional keyword fires.
    // A pattern miss is never a near-miss (there is no margin to be near).
    if let Some(ref pat) = pattern {
        if let Some(span) = regex_span(pat, query) {
            let floor_en = thresholds.en * gate_fraction;
            let floor_multi = thresholds.multi * gate_fraction;
            let no_signal = score_en.is_none() && score_multi.is_none();
            let clears = score_en.is_some_and(|s| s >= floor_en)
                || score_multi.is_some_and(|s| s >= floor_multi);
            if pattern_strict || no_signal || clears {
                return PromptMatch::Fired {
                    channel: "keyword".to_string(),
                    score: None,
                    matched_span: Some(span),
                };
            }
            return PromptMatch::KeywordGated(KeywordGated {
                matched_span: span,
                score_en,
                score_multi,
                floor_en,
                floor_multi,
            });
        }
    }

    // Channel 2: Embedding. Each model path stands on its own threshold;
    // scores don't cross-compare (apples and oranges). Either path firing
    // is sufficient, but the thresholds are calibrated independently so
    // each model's noise band sits below its gate:
    //   - EN model (0.40): sharp on English, noise below 0.35
    //   - multi model (0.55): cross-lingual but coarser, noise at 0.30-0.50
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

/// Emit a `way_keyword_gated` telemetry event (ADR-155): a pattern hit vetoed
/// because the way's embedding score sat below the gate floor on every model
/// lane. Same shape discipline as `log_near_miss` — persistence of
/// already-computed evidence, consumed by the tuning passes to calibrate
/// `keyword_gate_fraction` before any tightening. The `matched_span` names the
/// alternation that would have fired, which is exactly the per-alternation
/// precision signal the pattern-hygiene rework (ADR-155 §5) needs.
fn log_keyword_gated(
    way: &WayCandidate,
    kg: &KeywordGated,
    trigger: &str,
    scope: &str,
    project_dir: &str,
    session_id: &str,
) {
    let fmt = |v: Option<f64>| v.map(|s| format!("{s:.4}")).unwrap_or_default();
    let domain = way.id.split('/').next().unwrap_or(&way.id);
    session::log_event(&[
        ("event", "way_keyword_gated"),
        ("way", &way.id),
        ("corpus_id", &way.corpus_id),
        ("domain", domain),
        ("matched_span", &kg.matched_span),
        ("score_en", &fmt(kg.score_en)),
        ("score_multi", &fmt(kg.score_multi)),
        ("floor_en", &format!("{:.4}", kg.floor_en)),
        ("floor_multi", &format!("{:.4}", kg.floor_multi)),
        ("trigger", trigger),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id),
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

/// Mask non-linguistic spans out of the text the keyword channel matches
/// (ADR-155 §2): fenced code blocks first (they often contain URLs), then
/// URLs. A pasted link containing "github" is not GitHub-workflow intent, and
/// pasted code is quoted material, not the user speaking. Each masked span is
/// replaced by a single space so word boundaries around it survive. The embed
/// lane sees the original text — the ADR-130 reducer already weighs pasted
/// content by sentence salience there. An unclosed fence is left as-is: better
/// to over-match than to blind the keyword channel to half the prompt.
fn mask_nonlinguistic(text: &str) -> String {
    let fenced = Regex::new(r"(?s)```.*?```").expect("static regex");
    let url = Regex::new(r"https?://\S+").expect("static regex");
    let no_fences = fenced.replace_all(text, " ");
    url.replace_all(&no_fences, " ").into_owned()
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

    /// Production-default gate fraction (0.5): keyword floors sit at half the
    /// fire thresholds — 0.20 EN / 0.275 multi against [`THR`].
    const GATE: f64 = 0.5;

    fn run(en: Option<f64>, multi: Option<f64>, pattern: Option<&str>) -> PromptMatch {
        run_gated(en, multi, pattern, false, GATE)
    }

    fn run_gated(
        en: Option<f64>,
        multi: Option<f64>,
        pattern: Option<&str>,
        strict: bool,
        gate_fraction: f64,
    ) -> PromptMatch {
        match_prompt(
            "query text",
            &pattern.map(|p| p.to_string()),
            strict,
            "w",
            THR,
            &scores(en, multi),
            MARGIN,
            gate_fraction,
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
            PromptMatch::KeywordGated(_) => "KeywordGated",
            PromptMatch::NearMiss(_) => "NearMiss",
            PromptMatch::NoMatch => "NoMatch",
        }
    }

    // ── ADR-155: the semantic gate on keyword fires ──────────────

    #[test]
    fn keyword_below_gate_floor_is_gated_with_evidence() {
        // Floor is 0.20 EN; a hit at 0.10 is a lexical coincidence.
        match run(Some(0.10), None, Some("query")) {
            PromptMatch::KeywordGated(kg) => {
                assert_eq!(kg.matched_span, "query");
                assert_eq!(kg.score_en, Some(0.10));
                assert_eq!(kg.score_multi, None);
                assert!((kg.floor_en - 0.20).abs() < 1e-9);
                assert!((kg.floor_multi - 0.275).abs() < 1e-9);
            }
            other => panic!("expected KeywordGated, got {}", discriminant(&other)),
        }
    }

    #[test]
    fn keyword_at_or_above_gate_floor_fires() {
        // Exactly at the floor fires — same >= convention as the fire threshold.
        assert!(matches!(run(Some(0.20), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn either_lane_clearing_its_floor_passes_the_gate() {
        // EN deep below its floor, multi above its own floor (0.275) — passes.
        assert!(matches!(run(Some(0.05), Some(0.30), Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn pattern_strict_bypasses_the_gate() {
        assert!(matches!(run_gated(Some(0.05), None, Some("query"), true, GATE),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn gate_fails_open_without_any_embed_signal() {
        // Engine unavailable / way absent from corpus: the explicit trigger stands.
        assert!(matches!(run(None, None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn zero_gate_fraction_restores_unconditional_keyword_fires() {
        assert!(matches!(run_gated(Some(0.01), None, Some("query"), false, 0.0),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn gated_keyword_does_not_shadow_a_semantic_fire() {
        // Score clears the full threshold: it also clears the gate floor, so a
        // pattern hit fires on the keyword channel (gate passes trivially).
        assert!(matches!(run(Some(0.41), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    // ── ADR-155 §2: masking the keyword lane ─────────────────────

    #[test]
    fn urls_are_masked_but_prose_survives() {
        let masked = mask_nonlinguistic(
            "inspired by https://github.com/example/flow and btop's graphs",
        );
        assert!(!masked.contains("github"), "URL text must not feed the regex lane");
        assert!(masked.contains("btop's graphs"), "prose survives masking");
        // Word boundaries around the masked span survive (replaced by a space,
        // so the neighbors never fuse into one token).
        assert!(!masked.contains("byand"), "masked span must not fuse neighbors: {masked:?}");
    }

    #[test]
    fn fenced_code_is_masked_including_urls_inside() {
        let masked = mask_nonlinguistic(
            "please review\n```\ngit remember = https://github.com/x\n```\nthe diff",
        );
        assert!(!masked.contains("remember"));
        assert!(!masked.contains("github"));
        assert!(masked.contains("please review"));
        assert!(masked.contains("the diff"));
    }

    #[test]
    fn unclosed_fence_is_left_intact() {
        let text = "start ```unclosed block with words";
        assert_eq!(mask_nonlinguistic(text), text);
    }
}
