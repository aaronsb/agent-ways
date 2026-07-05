//! Scan ways and output matched content — replaces hook scan loops.
//!
//! Combines file walking, frontmatter extraction, matching (pattern + semantic),
//! scope/precondition gating, parent-threshold lowering, and show (display).

pub(crate) mod candidates;
mod machine;
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
    pub scope: String,
    pub when_project: Option<String>,
    pub when_file_exists: Option<String>,
    pub trigger: Option<String>,
    pub repeat: bool,
    pub trigger_path: Option<String>,
}

impl WayCandidate {
    /// Whether this way can appear in the embedding corpus. Mirrors the corpus
    /// builder's gate (`cmd::corpus`): an entry needs both a description and a
    /// vocabulary. The keyword gate (ADR-155) uses this to tell "the way was
    /// never embedded" (fail open) apart from "the lane ran and scored this
    /// way below way-embed's 0.0 emission floor" (gate on 0.0).
    fn embeddable(&self) -> bool {
        !self.description.is_empty() && !self.vocabulary.is_empty()
    }
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
) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(default_project);

    // Bump epoch
    session::bump_epoch(session_id);

    let scope = session::detect_scope(session_id);
    let candidates = collect_candidates(&project_dir);
    let near_miss_margin = crate::config::global().near_miss_margin;
    let keyword_floor = crate::config::global().keyword_floor_probability;

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
    let embed_matches = batch_embed_score(&reduced);
    let masked = mask_nonlinguistic(query);

    // ADR-160: the chunked matching machine IS the semantic matcher. It decides
    // the semantic channel over the reduced surface (chunk → softmax-share →
    // body-confirm), computed once here and consulted per way in match_prompt.
    // The single-vector calibrated gate is retained only as the fail-safe: when
    // the machine can't run (surface too sparse to chunk, engine unavailable) it
    // returns None and match_prompt uses the single-vector scores. The keyword
    // gate and near-miss telemetry keep using the single-vector batch scores.
    let machine = machine::run(&reduced, &machine_bodies(&candidates));

    // Prompt-only embed scores, computed lazily for gate re-checks (ADR-155
    // review): the shared embed vector mixes the response context in, which
    // can dilute a way's score below the gate floor even though the USER's
    // text carries the keyword — a terse "ship it" after an off-topic
    // response must not be vetoed by that response. Only turns where a hit
    // actually lands below the floor pay the second embed pass, and only
    // when response context contributed at all.
    let response_contributed = embed_input != query;
    let mut prompt_only_scores: Option<EmbedScores> = None;

    let mut context = String::new();

    for way in &candidates {
        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, &project_dir) {
            continue;
        }

        // pattern_strict means "this exact text, always": it bypasses the
        // mask as well as the gate, so a strict pattern can target URL or
        // code-fence content the mask would otherwise hide (ADR-155 §2).
        let regex_text: &str = if way.pattern_strict { query } else { &masked };
        let thresholds = effective_thresholds(way, session_id);

        // Additive matching: pattern OR semantic
        let mut outcome = match_prompt(
            regex_text,
            &way.pattern,
            way.pattern_strict,
            way.embeddable(),
            &way.corpus_id,
            thresholds,
            &embed_matches,
            near_miss_margin,
            keyword_floor,
            machine.as_ref(),
        );

        // Gate re-check against the prompt alone before accepting the veto.
        if let PromptMatch::KeywordGated(_) = outcome {
            if response_contributed {
                let scores = prompt_only_scores.get_or_insert_with(|| {
                    batch_embed_score(&reduce::reduce_for_embed(query, BUDGET_PROMPT))
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
                    machine.as_ref(),
                );
            }
        }

        match outcome {
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
    let keyword_floor = crate::config::global().keyword_floor_probability;
    // Session scope for telemetry: the task channel is subagent unless a team
    // name marks it as a teammate dispatch.
    let task_scope = if is_teammate { "teammate" } else { "subagent" };

    // ADR-130: agent delegation prompts are the largest input class in
    // practice. Reduce to the model's window before embedding.
    let reduced = reduce::reduce_for_embed(query, BUDGET_TASK);
    let embed_matches = batch_embed_score(&reduced);
    let masked = mask_nonlinguistic(query);
    // ADR-160: the machine is the semantic matcher on the task surface too;
    // single-vector is the fail-safe when it can't chunk (see scan::prompt).
    let machine = machine::run(&reduced, &machine_bodies(&candidates));

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
            machine.as_ref(),
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
                // Pattern compiles case-insensitively (ADR-157), so match the
                // description in its original case for a truer captured span.
                (Some(desc), Some(pat)) => regex_span(pat, desc),
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

/// Evidence for a gated keyword hit (ADR-155/156): what the pattern matched and
/// the calibrated relevance probability each model lane produced, against the
/// keyword floor probability τ_k that vetoed it.
struct KeywordGated {
    matched_span: String,
    prob_en: Option<f64>,
    prob_multi: Option<f64>,
    floor: f64,
}

/// A below-threshold embedding result close enough to log (ADR-134 Decision 1),
/// now in calibrated probability space (ADR-156).
struct NearMiss {
    prob_en: Option<f64>,
    prob_multi: Option<f64>,
    tau_s: f64,
    /// Smallest `τ_s - probability` among the models within margin — how close
    /// the way came to firing on its best path.
    margin: f64,
}

/// Map each embeddable candidate's corpus id to its `.md` path, for the
/// matching machine's body-confirmation stage (ADR-160).
fn machine_bodies(candidates: &[WayCandidate]) -> std::collections::HashMap<String, PathBuf> {
    candidates
        .iter()
        .filter(|c| c.embeddable())
        .map(|c| (c.corpus_id.clone(), c.path.clone()))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn match_prompt(
    query: &str,
    pattern: &Option<String>,
    pattern_strict: bool,
    embeddable: bool,
    corpus_id: &str,
    thresholds: EffectiveThresholds,
    scores: &EmbedScores,
    near_miss_margin: f64,
    keyword_floor: f64,
    machine: Option<&machine::MachineVerdicts>,
) -> PromptMatch {
    // Calibrated relevance probability per lane (ADR-156). `None` means no
    // calibrated signal: the lane didn't run, the way isn't embeddable, or no
    // calibration is loaded. An embeddable way that ran but is missing from the
    // results scores as g(cos 0.0) — a low probability, not absence of signal.
    let tau_s = thresholds.semantic;
    let tau_k = keyword_floor;
    let prob_en = scores.prob_en(corpus_id, embeddable);
    let prob_multi = scores.prob_multi(corpus_id, embeddable);

    // Channel 1: Regex pattern — deterministic, but gated (ADR-155/156): the hit
    // fires only if the way's calibrated probability also clears the keyword
    // floor τ_k on at least one model lane, using probabilities the batch pass
    // already produced — no extra model work. It fails OPEN when there is
    // genuinely no calibrated signal either direction — the engine didn't run,
    // the way isn't embeddable, or no calibration is loaded: the author's
    // explicit trigger stands. `pattern_strict: true` restores an unconditional
    // keyword fire. A pattern miss is never a near-miss.
    let mut gated: Option<KeywordGated> = None;
    if let Some(ref pat) = pattern {
        if let Some(span) = regex_span(pat, query) {
            let no_signal = prob_en.is_none() && prob_multi.is_none();
            let clears =
                prob_en.is_some_and(|p| p >= tau_k) || prob_multi.is_some_and(|p| p >= tau_k);
            if pattern_strict || no_signal || clears {
                return PromptMatch::Fired {
                    channel: "keyword".to_string(),
                    score: None,
                    matched_span: Some(span),
                };
            }
            // Gated — but τ_k and τ_s are independent (ADR-156), so τ_k may sit
            // above τ_s. Hold the veto and let the semantic lane fire first;
            // matching stays additive-OR (a gated keyword never shadows a way
            // that clears the semantic bar).
            gated = Some(KeywordGated {
                matched_span: span,
                prob_en,
                prob_multi,
                floor: tau_k,
            });
        }
    }

    // Channel 2: Embedding. When the ADR-160 machine is engaged it owns the
    // semantic decision for this way (chunk → softmax-share → body-confirm, run
    // once upstream); otherwise the single-vector calibrated gate does —
    // calibration makes probabilities comparable across models, so both lanes
    // share one threshold τ_s and either firing is sufficient.
    if let Some(m) = machine {
        if let Some(share) = m.fired_score(corpus_id) {
            return PromptMatch::Fired {
                channel: "semantic:machine:en".to_string(),
                score: Some(share),
                matched_span: None,
            };
        }
    } else {
        if prob_en.is_some_and(|p| p >= tau_s) {
            return PromptMatch::Fired {
                channel: "semantic:embedding:en".to_string(),
                score: prob_en,
                matched_span: None,
            };
        }
        if prob_multi.is_some_and(|p| p >= tau_s) {
            return PromptMatch::Fired {
                channel: "semantic:embedding:multi".to_string(),
                score: prob_multi,
                matched_span: None,
            };
        }
    }

    // A keyword hit that was gated and whose way did not clear the semantic bar
    // is a vetoed lexical coincidence — report it (preempts near-miss logging).
    if let Some(kg) = gated {
        return PromptMatch::KeywordGated(kg);
    }

    // No fire. Record a near-miss when a lane's probability landed in the band
    // just below τ_s: `τ_s - margin <= p < τ_s`. The reported margin is the
    // smallest shortfall across lanes, against the SAME τ_s the fire path uses.
    let shortfall = |p: Option<f64>| -> Option<f64> {
        p.and_then(|v| {
            let gap = tau_s - v;
            (gap > 0.0 && gap <= near_miss_margin).then_some(gap)
        })
    };
    let margin = [shortfall(prob_en), shortfall(prob_multi)]
        .into_iter()
        .flatten()
        .fold(None, |acc: Option<f64>, g| Some(acc.map_or(g, |a| a.min(g))));

    match margin {
        Some(margin) => PromptMatch::NearMiss(NearMiss {
            prob_en,
            prob_multi,
            tau_s,
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
        ("prob_en", &fmt(nm.prob_en)),
        ("prob_multi", &fmt(nm.prob_multi)),
        ("tau_s", &format!("{:.4}", nm.tau_s)),
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
    // token_position mirrors way_fired (show/mod.rs): the introspection join
    // clusters gated rows into the same turns as fires, so they must carry
    // the same position signal instead of an implicit 0.
    let token_pos = session::get_token_position(session_id);
    session::log_event(&[
        ("event", "way_keyword_gated"),
        ("way", &way.id),
        ("corpus_id", &way.corpus_id),
        ("domain", domain),
        ("matched_span", &kg.matched_span),
        ("prob_en", &fmt(kg.prob_en)),
        ("prob_multi", &fmt(kg.prob_multi)),
        ("floor", &format!("{:.4}", kg.floor)),
        ("trigger", trigger),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id),
        ("token_position", &token_pos.to_string()),
    ]);
}

/// The effective semantic fire probability τ_s for a way at a given moment in a
/// session (ADR-156). Calibration makes probabilities comparable across models,
/// so one threshold serves both lanes; the keyword floor τ_k is global and read
/// separately.
#[derive(Clone, Copy)]
struct EffectiveThresholds {
    semantic: f64,
}

/// Compute the effective semantic fire probability, accounting for parent-boost.
///
/// Parent-boost (ADR-125): if any ancestor has fired in the session, the base
/// τ_s is multiplied by `parent_threshold_multiplier` (default 0.8), floored at
/// `parent_boost_floor`. The floor prevents cascading boosts from pushing
/// children into the noise band. All values are calibrated probabilities.
fn effective_thresholds(way: &WayCandidate, session_id: &str) -> EffectiveThresholds {
    let cfg = crate::config::global();
    let base = cfg.semantic_fire_probability;

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

    let semantic = if ancestor_shown {
        (base * cfg.parent_threshold_multiplier).max(cfg.parent_boost_floor)
    } else {
        base
    };
    EffectiveThresholds { semantic }
}

/// Semantic score for a check, taking the higher of the two model paths
/// that clears its own threshold. The two models are evaluated
/// independently (apples and oranges); if either path's score >= its
/// threshold, the check fires at that score. Returns 0.0 if neither
/// path clears.
fn check_semantic_score(check: &WayCandidate, session_id: &str, scores: &EmbedScores) -> f64 {
    let t = effective_thresholds(check, session_id);
    let en = scores
        .prob_en(&check.corpus_id, check.embeddable())
        .filter(|p| *p >= t.semantic);
    let mu = scores
        .prob_multi(&check.corpus_id, check.embeddable())
        .filter(|p| *p >= t.semantic);
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

/// Compile a trigger pattern case-insensitively (ADR-157). The keyword lane
/// means the *concept*, not a casing — `\bssh\b` must match `SSH`, `\bpr\b`
/// must match `PR`. The flag lives on the compiled pattern rather than on the
/// text so deliberately-uppercase patterns (`SKILL\.md`) still match and the
/// captured span keeps its original case. A pattern may override with an inline
/// `(?-i)` scope if it ever needs case-sensitivity.
fn compile_trigger(pattern: &str) -> Option<Regex> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .ok()
}

fn regex_matches(pattern: &str, text: &str) -> bool {
    compile_trigger(pattern)
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
    let m = compile_trigger(pattern)?.find(text)?;
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

    use ways_core::calibration::{Calibration, ModelCalibration};

    // ADR-156: match_prompt scores calibrated probabilities. The tests feed raw
    // cosines through a fixed test calibration `g(s) = σ(10·s − 2.5)`, chosen so
    // g(0.25)=0.5 exactly (the τ_s boundary) and g(0.0)≈0.076 (a missing row —
    // negative cosine — lands well below the keyword floor).
    const THR: EffectiveThresholds = EffectiveThresholds { semantic: 0.5 };
    const MARGIN: f64 = 0.05;
    const FLOOR: f64 = 0.15; // τ_k, mirrors config default keyword_floor_probability
    const TEST_CAL: ModelCalibration = ModelCalibration { a: 10.0, b: -2.5, auc: 1.0, n: 0 };

    /// Calibrated probability for a cosine under the test calibration.
    fn p(cosine: f64) -> f64 {
        TEST_CAL.probability(cosine)
    }

    fn scores(en: Option<f64>, multi: Option<f64>) -> EmbedScores {
        EmbedScores {
            en: en.map(|s| vec![("w".to_string(), s)]),
            multi: multi.map(|s| vec![("w".to_string(), s)]),
            calibration: Calibration { en: Some(TEST_CAL), multi: Some(TEST_CAL) },
        }
    }

    fn run(en: Option<f64>, multi: Option<f64>, pattern: Option<&str>) -> PromptMatch {
        run_full(scores(en, multi), pattern, false, true)
    }

    fn run_full(
        scores: EmbedScores,
        pattern: Option<&str>,
        strict: bool,
        embeddable: bool,
    ) -> PromptMatch {
        match_prompt(
            "query text",
            &pattern.map(|p| p.to_string()),
            strict,
            embeddable,
            "w",
            THR,
            &scores,
            MARGIN,
            FLOOR,
            None,
        )
    }

    #[test]
    fn en_clears_fires_en() {
        // cos 0.35 → g ≈ 0.73 ≥ τ_s 0.5.
        assert!(matches!(run(Some(0.35), None, None),
            PromptMatch::Fired { channel: c, .. } if c == "semantic:embedding:en"));
    }

    #[test]
    fn semantic_fire_carries_its_probability_keyword_does_not() {
        // ADR-134 D / ADR-156: the firing calibrated probability rides on Fired
        // for telemetry; a deterministic keyword fire carries none.
        match run(Some(0.35), None, None) {
            PromptMatch::Fired { score, .. } => {
                assert!((score.unwrap() - p(0.35)).abs() < 1e-9);
            }
            _ => panic!("expected Fired"),
        }
        // multi fire (EN below τ_s) carries the multi probability, not EN's.
        match run(Some(0.20), Some(0.35), None) {
            PromptMatch::Fired { channel, score, matched_span } => {
                assert_eq!(channel, "semantic:embedding:multi");
                assert!((score.unwrap() - p(0.35)).abs() < 1e-9);
                assert_eq!(matched_span, None, "semantic carries no matched term");
            }
            _ => panic!("expected multi Fired"),
        }
        match run(Some(0.35), None, Some("query")) {
            PromptMatch::Fired { channel, score, matched_span } => {
                assert_eq!(channel, "keyword");
                assert_eq!(score, None);
                assert_eq!(matched_span.as_deref(), Some("query"), "keyword records the regex match");
            }
            _ => panic!("expected keyword Fired"),
        }
    }

    #[test]
    fn trigger_regex_is_case_insensitive() {
        // ADR-157: lowercase patterns match the uppercase acronyms users type,
        // corpus-wide, without per-way (?i). Both helpers share compile_trigger.
        assert!(regex_matches(r"\bssh\b", "connect over SSH"));
        assert!(regex_matches(r"\bpr\b", "open a PR for this"));
        assert_eq!(
            regex_span(r"\berd\b", "draw an ERD diagram").as_deref(),
            Some("ERD"),
            "captured span keeps the input's original case"
        );
        // Deliberately-uppercase pattern still matches its lowercase form.
        assert!(regex_matches(r"SKILL\.md", "editing skill.md"));
        // An inline (?-i) scope can still opt back into case-sensitivity.
        assert!(!regex_matches(r"(?-i)SSH", "lowercase ssh only"));
    }

    #[test]
    fn multi_clears_when_en_below_fires_multi() {
        assert!(matches!(run(Some(0.20), Some(0.35), None),
            PromptMatch::Fired { channel: c, .. } if c == "semantic:embedding:multi"));
    }

    #[test]
    fn within_margin_is_near_miss_with_shortfall() {
        // cos 0.24 → g ≈ 0.475, just below τ_s 0.5 (shortfall ≈ 0.025 < MARGIN).
        match run(Some(0.24), None, None) {
            PromptMatch::NearMiss(nm) => {
                assert!((nm.margin - (0.5 - p(0.24))).abs() < 1e-9, "margin = τ_s - prob");
                assert!((nm.prob_en.unwrap() - p(0.24)).abs() < 1e-9);
                assert_eq!(nm.prob_multi, None);
            }
            other => panic!("expected NearMiss, got {:?}", discriminant(&other)),
        }
    }

    #[test]
    fn smallest_shortfall_wins_across_models() {
        // en short by ~0.025, multi short by ~0.0125 -> reported margin is the multi one.
        match run(Some(0.24), Some(0.245), None) {
            PromptMatch::NearMiss(nm) => {
                assert!((nm.margin - (0.5 - p(0.245))).abs() < 1e-9);
            }
            other => panic!("expected NearMiss, got {:?}", discriminant(&other)),
        }
    }

    #[test]
    fn beyond_margin_is_no_match() {
        // cos 0.20 → g ≈ 0.378, shortfall ≈ 0.12 > MARGIN.
        assert!(matches!(run(Some(0.20), None, None), PromptMatch::NoMatch));
    }

    #[test]
    fn pattern_match_preempts_near_miss() {
        // Would be a near-miss, but a keyword hit (g ≈ 0.475 ≥ τ_k) fires.
        assert!(matches!(run(Some(0.24), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn absent_scores_are_no_match() {
        assert!(matches!(run(None, None, None), PromptMatch::NoMatch));
    }

    #[test]
    fn probability_exactly_at_threshold_fires_not_near_miss() {
        // cos 0.25 → g = 0.5 exactly = τ_s. The `>=` fire check and the
        // `gap > 0.0` near-miss guard must agree: it fires, never a near-miss.
        assert!(matches!(run(Some(0.25), None, None),
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
        // cos 0.05 → g ≈ 0.119 < τ_k 0.15: a lexical coincidence, vetoed.
        match run(Some(0.05), None, Some("query")) {
            PromptMatch::KeywordGated(kg) => {
                assert_eq!(kg.matched_span, "query");
                assert!((kg.prob_en.unwrap() - p(0.05)).abs() < 1e-9);
                assert_eq!(kg.prob_multi, None);
                assert!((kg.floor - FLOOR).abs() < 1e-9);
            }
            other => panic!("expected KeywordGated, got {}", discriminant(&other)),
        }
    }

    #[test]
    fn keyword_above_gate_floor_fires() {
        // cos 0.10 → g ≈ 0.182 ≥ τ_k 0.15.
        assert!(matches!(run(Some(0.10), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn either_lane_clearing_the_floor_passes_the_gate() {
        // EN below τ_k (g ≈ 0.119), multi above it (g ≈ 0.182) — passes.
        assert!(matches!(run(Some(0.05), Some(0.10), Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn pattern_strict_bypasses_the_gate() {
        assert!(matches!(run_full(scores(Some(0.05), None), Some("query"), true, true),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn gate_fails_open_when_no_lane_ran() {
        // Engine unavailable: the explicit trigger stands.
        assert!(matches!(run(None, None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn missing_row_on_a_ran_lane_gates_an_embeddable_way() {
        // way-embed emits only scores >= 0.0: a lane that ran with no row for an
        // embeddable way means NEGATIVE cosine — the strongest "unrelated"
        // signal. It scores as g(0.0) ≈ 0.076, below τ_k, so it gates rather
        // than fails open (gate monotonicity, ADR-155/156).
        let lane_ran_no_row = EmbedScores {
            en: Some(vec![]),
            multi: None,
            calibration: Calibration { en: Some(TEST_CAL), multi: None },
        };
        match run_full(lane_ran_no_row, Some("query"), false, true) {
            PromptMatch::KeywordGated(kg) => {
                assert!((kg.prob_en.unwrap() - p(0.0)).abs() < 1e-9, "missing row → g(0.0)");
                assert_eq!(kg.prob_multi, None, "lane that didn't run stays None");
            }
            other => panic!("expected KeywordGated, got {}", discriminant(&other)),
        }
    }

    #[test]
    fn non_embeddable_way_fails_open_even_when_lanes_ran() {
        // A trigger-only way (no description/vocabulary) can't be in the corpus;
        // a missing row says nothing about it. The explicit trigger stands.
        let lane_ran_no_row = EmbedScores {
            en: Some(vec![]),
            multi: None,
            calibration: Calibration { en: Some(TEST_CAL), multi: None },
        };
        assert!(matches!(
            run_full(lane_ran_no_row, Some("query"), false, false),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"
        ));
    }

    #[test]
    fn no_calibration_fails_open_on_keyword() {
        // A corpus that predates calibration: no calibrated signal, so the
        // keyword fires open (degraded), never the retired raw-cosine path.
        let uncalibrated = EmbedScores {
            en: Some(vec![("w".to_string(), 0.05)]),
            multi: None,
            calibration: Calibration::default(),
        };
        assert!(matches!(run_full(uncalibrated, Some("query"), false, true),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn gated_keyword_does_not_shadow_a_semantic_fire() {
        // cos 0.35 clears τ_s: it also clears τ_k, so a pattern hit fires on the
        // keyword channel (gate passes trivially).
        assert!(matches!(run(Some(0.35), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn high_keyword_floor_does_not_shadow_a_semantic_fire() {
        // τ_k (0.6) > τ_s (0.5): a pattern hit at cos 0.27 (g ≈ 0.55) fails the
        // keyword floor but clears the semantic bar — it must fire semantically,
        // not be vetoed by the gate (ADR-156: τ_k and τ_s are independent).
        let outcome = match_prompt(
            "query text", &Some("query".to_string()), false, true, "w",
            THR, &scores(Some(0.27), None), MARGIN, 0.6, None,
        );
        assert!(matches!(outcome,
            PromptMatch::Fired { channel: ref c, .. } if c == "semantic:embedding:en"),
            "got {:?}", discriminant(&outcome));
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
