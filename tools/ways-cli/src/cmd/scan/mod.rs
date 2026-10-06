//! Scan ways and output matched content — replaces hook scan loops.
//!
//! Combines file walking, frontmatter extraction, matching (pattern + semantic),
//! scope/precondition gating, parent-threshold lowering, and show (display).

pub(crate) mod candidates;
mod candidate_log;
mod gate;
mod late_interaction;
mod lookbehind;
mod order;
mod reduce;
pub(crate) mod scoring;
mod state;
pub(crate) use scoring::{batch_embed_score, batch_embed_score_with, sibling_corpus};

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
use std::collections::HashSet;
use std::path::PathBuf;

use crate::session;

use candidates::{check_when, collect_candidates, collect_checks};
use scoring::{capture_show_check, capture_show_way, EmbedScores};
use crate::cmd::show::ContextBudget;
use order::{order_hits, Hit};

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
/// The project switch (ADR-184 item 6): `enabled: false` in the project's
/// `ways.yaml` makes every scan lane inject nothing there. The user config can
/// carry the same key to switch ways off everywhere.
pub fn enabled_for(project: Option<&str>) -> bool {
    let dir = match project {
        Some(p) => p.to_string(),
        None => crate::util::project_dir(),
    };
    crate::config::Config::load(&dir).enabled
}

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
/// header attend puts on a turn-boundary drain (ADR-172).
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

    if bump_epoch {
        session::bump_epoch(session_id);
    }

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

    // ADR-701 §2: log the top candidates with share and margin, from the rows
    // the scan already holds. Enabled ways only: `candidates` is already
    // filtered by the domain and per-way toggles.
    {
        let enabled: std::collections::HashMap<&str, &str> = candidates
            .iter()
            .filter(|c| c.embeddable() && eligible(c, Lane::Prompt { scope: &scope }, &project_dir))
            .map(|c| (c.corpus_id.as_str(), c.id.as_str()))
            .collect();
        candidate_log::log_scan_candidates(
            &embed_matches,
            &enabled,
            &[("surface", "prompt"), ("scope", &scope), ("project", &project_dir), ("session", session_id), ("hook_event", hook_event)],
        );
    }

    // ADR-160: the chunked late-interaction matcher IS the semantic matcher. It decides
    // the semantic channel over the reduced surface (chunk → softmax-share →
    // body-confirm), computed once here and consulted per way in match_prompt.
    // The single-vector calibrated gate is retained only as the fail-safe: when
    // the matcher can't run (surface too sparse to chunk, engine unavailable) it
    // returns None and match_prompt uses the single-vector scores. The keyword
    // gate and near-miss telemetry keep using the single-vector batch scores.
    let verdicts = late_interaction::run(&reduced, &body_map(candidates.iter().filter(|w| eligible(w, Lane::Prompt { scope: &scope }, &project_dir))));

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
    let mut budget = ContextBudget::hook();
    // Fired ways are collected, then admitted in a fixed order (scan/order.rs).
    // Payload: (channel, matched span, fired only on this scan's parent boost).
    let mut hits: Vec<Hit<(String, Option<String>, bool)>> = Vec::new();
    // Ids fired so far in this scan. Candidates arrive in tree order, so a
    // parent is decided before its children and can boost them (see
    // `effective_thresholds_in_scan`).
    let mut fired_ids: HashSet<String> = HashSet::new();

    for way in &candidates {
        if !eligible(way, Lane::Prompt { scope: &scope }, &project_dir) {
            continue;
        }

        // pattern_strict means "this exact text, always": it bypasses the
        // mask as well as the gate, so a strict pattern can target URL or
        // code-fence content the mask would otherwise hide (ADR-155 §2).
        let regex_text: &str = if way.pattern_strict { query } else { &masked };
        let (thresholds, scan_boost) = effective_thresholds_in_scan(way, session_id, &fired_ids);

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
            verdicts.as_ref(),
        );

        // Gate re-check against the prompt alone before accepting the veto.
        let mut used_prompt_only = false;
        if let PromptMatch::KeywordGated(_) = outcome {
            if response_contributed {
                used_prompt_only = true;
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
                    verdicts.as_ref(),
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
                        _ => &embed_matches,
                    },
                    near_miss_margin,
                    keyword_floor,
                    verdicts.as_ref(),
                ),
                PromptMatch::Fired { .. }
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
            }
            PromptMatch::NearMiss(nm) => {
                log_near_miss(way, &nm, "prompt", &scope, &project_dir, session_id, query);
            }
            PromptMatch::NoMatch => {}
        }
    }

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
    let mut blocked = gate::apply(&pending, query, response_context, &gate_log);

    let mut shown: HashSet<String> = HashSet::new();
    for hit in &hits {
        let (channel, matched_span, needs_parent) = &hit.payload;
        if blocked.contains(&hit.id) {
            continue;
        }
        if *needs_parent && withheld_for_parent(&hit.id, &shown, &pending, &mut blocked, &gate_log) {
            continue;
        }
        let out = capture_show_way(
            &hit.id,
            session_id,
            channel,
            hit.score,
            matched_span.as_deref(),
            Some(reduced.as_str()),
            Some(&mut budget),
        );
        if !out.is_empty() {
            shown.insert(hit.id.clone());
            context.push_str(&out);
            context.push_str("\n\n");
            budget.charge("\n\n");
        }
    }

    if !context.is_empty() {
        emit_hook_context(hook_event, context.trim_end());
    }

    Ok(())
}

// ── Authoring diagnostic (task #5) ─────────────────────────────

pub(crate) use late_interaction::{DiagRow, DIAG_CONFIRM_GATE, DIAG_PEAK_GATE, DIAG_SHARE_GATE};

/// Run the late-interaction matcher over `query` for way authoring — the modern
/// equivalent of the single-vector `ways author match`. Reduces the query exactly as the
/// prompt scan does (so the diagnostic sees the surface production sees), then
/// returns the top candidates' evidence (peak / share / body-confirm / fired) with
/// the reduced surface for context. `None` means late-interaction could not run
/// (engine unavailable, or the surface is too sparse to chunk) — the caller then
/// falls back to the single-vector view, mirroring production's fail-safe.
///
/// The competing set is the one a prompt scan in `agent` scope uses (toggles,
/// scope and `when:` against `project`), so the shares match the live fire path.
/// `unfiltered` competes every candidate instead, for seeing how a way would rank
/// among all of them.
pub fn diagnose(query: &str, project: Option<&str>, top_n: usize, unfiltered: bool) -> Option<(String, Vec<DiagRow>)> {
    let project_dir = project.map(|s| s.to_string()).unwrap_or_else(crate::util::project_dir);
    let candidates = collect_candidates(&project_dir);
    let reduced = reduce::reduce_for_embed(query, BUDGET_PROMPT);
    let bodies = body_map(diag_candidates(&candidates, &project_dir, unfiltered).into_iter());
    let rows = late_interaction::run_diagnostic(&reduced, &bodies, top_n)?;
    Some((reduced, rows))
}

/// The candidates `ways author match` competes: the prompt lane in `agent`
/// scope, or all of them when `unfiltered`.
fn diag_candidates<'a>(candidates: &'a [WayCandidate], project_dir: &str, unfiltered: bool) -> Vec<&'a WayCandidate> {
    candidates.iter().filter(|w| unfiltered || eligible(w, Lane::Prompt { scope: "agent" }, project_dir)).collect()
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
    let embed_matches = batch_embed_score(&reduced);
    let masked = mask_nonlinguistic(query);
    // ADR-160: the matcher is the semantic matcher on the task surface too;
    // single-vector is the fail-safe when it can't chunk (see scan::prompt).
    let verdicts = late_interaction::run(&reduced, &body_map(candidates.iter().filter(|w| eligible(w, Lane::Task { teammate: is_teammate }, &project_dir))));
    // ADR-701 §2: the task lane logs its candidates too, over the ways eligible there.
    {
        let lane = Lane::Task { teammate: is_teammate };
        let enabled: std::collections::HashMap<&str, &str> = candidates
            .iter()
            .filter(|c| c.embeddable() && eligible(c, lane, &project_dir))
            .map(|c| (c.corpus_id.as_str(), c.id.as_str()))
            .collect();
        candidate_log::log_scan_candidates(
            &embed_matches,
            &enabled,
            &[("surface", "task"), ("scope", task_scope), ("project", &project_dir), ("session", session_id)],
        );
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
            }
            PromptMatch::NearMiss(nm) => {
                log_near_miss(way, &nm, "task", task_scope, &project_dir, session_id, query);
            }
            PromptMatch::NoMatch => {}
        }
    }

    order_hits(&mut hits);
    let matched: Vec<(String, String)> = hits.into_iter().map(|h| (h.id, h.payload)).collect();

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
        // Published whole: SubagentStart may claim the stash the moment it
        // appears, and the writer's temporary name does not end in `.json`.
        let stash_file = format!("{stash_dir}/{timestamp}.json");
        agent_settings::writer::write_atomic(std::path::Path::new(&stash_file), stash.to_string())?;
    }

    Ok(())
}

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

    // Way matching: commands regex + pattern regex + semantic (ADR-155 §4).
    // Hits are collected, then admitted in a fixed order (scan/order.rs).
    // Payload: (channel, matched span, fired only on this scan's parent boost).
    let mut hits: Vec<Hit<(&'static str, Option<String>, bool)>> = Vec::new();
    let mut fired_ids: HashSet<String> = HashSet::new();
    for way in &candidates {
        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, &project_dir) {
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
        );
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
        let out = capture_show_way(&hit.id, session_id, "file", None, Some(hit.payload.as_str()), None, Some(&mut budget));
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

/// The surface a scan serves, which decides what may fire on it.
#[derive(Clone, Copy)]
enum Lane<'a> {
    /// A user prompt, in the session's scope.
    Prompt { scope: &'a str },
    /// A subagent or teammate dispatch.
    Task { teammate: bool },
}

/// Whether `way` can fire on this lane at all: scope, state trigger and `when:`
/// preconditions. The scan loops apply it to each candidate, and the matchers
/// take their competing set from it, so a way that cannot fire here takes no
/// softmax share and no confirmation slot (ADR-701 §1). Toggles are already
/// applied by `collect_candidates`.
fn eligible(way: &WayCandidate, lane: Lane<'_>, project_dir: &str) -> bool {
    let lane_ok = match lane {
        Lane::Prompt { scope } => session::scope_matches(&way.scope, scope),
        // A task scan needs subagent scope (or teammate for a team) and skips
        // state-triggered ways.
        Lane::Task { teammate } => {
            (way.scope.contains("subagent") || (teammate && way.scope.contains("teammate"))) && way.trigger.is_none()
        }
    };
    lane_ok && check_when(&way.when_project, &way.when_file_exists, project_dir)
}

/// Map each embeddable candidate's corpus id to its `.md` path, for the
/// late-interaction matcher's body-confirmation stage (ADR-160) and as the set
/// of ways allowed to compete.
fn body_map<'a>(candidates: impl Iterator<Item = &'a WayCandidate>) -> std::collections::HashMap<String, PathBuf> {
    candidates.filter(|c| c.embeddable()).map(|c| (c.corpus_id.clone(), c.path.clone())).collect()
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
    verdicts: Option<&late_interaction::Verdicts>,
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

    // Channel 2: Embedding. When the ADR-160 late-interaction matcher is engaged
    // it owns the semantic decision for this way (chunk → softmax-share →
    // body-confirm, run
    // once upstream); otherwise the single-vector calibrated gate does —
    // calibration makes probabilities comparable across models, so both lanes
    // share one threshold τ_s and either firing is sufficient.
    if let Some(m) = verdicts {
        if let Some(share) = m.fired_score(corpus_id) {
            return PromptMatch::Fired {
                channel: "semantic:late-interaction:en".to_string(),
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
/// of ADR-134 consume the stream. The leading fields
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
    // events.jsonl is bounded by age (retention days) and size; see session::log_event.
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
    effective_thresholds_in_scan(way, session_id, &HashSet::new()).0
}

/// Proper ancestors of a way id, nearest first (`a/b/c` → `a/b`, `a`).
fn ancestors(id: &str) -> impl Iterator<Item = &str> {
    let mut path = id;
    std::iter::from_fn(move || {
        let idx = path.rfind('/')?;
        path = &path[..idx];
        Some(path)
    })
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

fn has_shown_ancestor(id: &str, shown: &HashSet<String>) -> bool {
    ancestors(id).any(|a| shown.contains(a))
}

/// [`effective_thresholds`], also boosting a way whose ancestor fired earlier
/// in the same scan (`fired_in_scan`). Lanes show their hits after matching
/// (scan/order.rs), so an ancestor fired in this scan has no session marker yet;
/// the lanes walk candidates in tree order, so the ancestor is decided first.
/// The flag is true when only this scan's ancestor supplied the boost: the
/// caller then shows the way only if that ancestor is shown.
fn effective_thresholds_in_scan(
    way: &WayCandidate,
    session_id: &str,
    fired_in_scan: &HashSet<String>,
) -> (EffectiveThresholds, bool) {
    let cfg = crate::config::global();
    let base = cfg.semantic_fire_probability;

    let by_session = ancestors(&way.id).any(|a| session::way_is_shown(a, session_id));
    let by_scan = !by_session && ancestors(&way.id).any(|a| fired_in_scan.contains(a));

    let semantic = if by_session || by_scan {
        (base * cfg.parent_threshold_multiplier).max(cfg.parent_boost_floor)
    } else {
        base
    };
    (EffectiveThresholds { semantic }, by_scan)
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

/// Emit accumulated context in the canonical `hookSpecificOutput` envelope —
/// the only JSON shape the current hooks reference documents, for any event.
///
/// A bare top-level `additionalContext` was emitted for `SessionStart` here
/// until session transcripts proved the harness never delivered it: the
/// stdout landed in `hook_success` bookkeeping, no context attachment was
/// created, and the payload (the ways catalog and the core posture) reached
/// zero sessions on record. Undocumented JSON stdout is dropped silently, so
/// nothing but the canonical envelope belongs here.
///
/// The PreToolUse lanes ([`command`], [`file`]) made the same mistake with a
/// top-level `{"decision":"approve","additionalContext":…}`: over 30 days,
/// about 5,458 PreToolUse invocations delivered context 0 times, while
/// PostToolUse, already canonical, delivered 143 of 143 (#528). They route
/// through here now. The envelope deliberately carries no
/// `permissionDecision`: per the hooks reference
/// (<https://code.claude.com/docs/en/hooks.md>, "PreToolUse decision
/// control"), omitting it means the hook makes no decision and the tool call
/// continues through the normal permission flow. `"allow"` would bypass the
/// permission prompt, which a guidance hook must never do.
///
/// Callers keep `context` within [`crate::cmd::show::HOOK_CONTEXT_CAP`] via a
/// [`ContextBudget`]; over that cap Claude Code replaces the string with a
/// file path and a 2,000-character preview.
pub(crate) fn emit_hook_context(hook_event: &str, context: &str) {
    let payload = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": hook_event,
            "additionalContext": context,
        }
    });
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

#[cfg(test)]
mod queued_tests {
    //! ADR-161: pure selection of queued mid-turn operator messages from a
    //! transcript — dedup by mark, envelope filtering, burst aggregation. No
    //! I/O, no matcher.

    use super::*;

    #[test]
    fn envelopes_are_not_operator_intent() {
        assert!(is_system_envelope("<task-notification> <task-id>x</task-id> attend: peers"));
        assert!(is_system_envelope("  <System-Reminder>hook output</System-Reminder>"));
        assert!(is_system_envelope("[attend] 2 peer message(s) delivered at the turn boundary"));
        assert!(is_system_envelope("[ATTEND sensor=peers priority=high] ssh started"));
        assert!(is_system_envelope("Base directory for this skill: /home/u/.claude/skills/attend\n\n# Attend"));
    }

    #[test]
    fn operator_prose_is_scanned() {
        assert!(!is_system_envelope("set up ssh to the bastion"));
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

    #[test]
    fn enabled_for_reads_the_project_switch() {
        let dir = std::env::temp_dir().join(format!("ways-enabled-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/ways.yaml"), "enabled: false\n").unwrap();
        assert!(!super::enabled_for(Some(dir.to_str().unwrap())));
        std::fs::write(dir.join(".claude/ways.yaml"), "ways: {}\n").unwrap();
        assert!(super::enabled_for(Some(dir.to_str().unwrap())));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ancestors_walk_up_nearest_first() {
        let got: Vec<&str> = super::ancestors("a/b/c").collect();
        assert_eq!(got, vec!["a/b", "a"]);
        assert_eq!(super::ancestors("top").count(), 0);
    }

    #[test]
    fn shown_ancestor_is_any_level_up() {
        let shown: std::collections::HashSet<String> = ["a".to_string()].into_iter().collect();
        assert!(super::has_shown_ancestor("a/b/c", &shown));
        assert!(!super::has_shown_ancestor("a", &shown), "a way is not its own ancestor");
        assert!(!super::has_shown_ancestor("ab/c", &shown), "a text prefix is not an ancestor");
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
}

#[cfg(test)]
mod eligibility_tests {
    //! ADR-701 §1: the competing set is the ways that can fire on the lane.
    use super::*;

    fn way(id: &str, scope: &str, trigger: Option<&str>) -> WayCandidate {
        WayCandidate {
            id: id.to_string(),
            corpus_id: id.to_string(),
            path: PathBuf::from(format!("/{id}.md")),
            pattern: None,
            pattern_strict: false,
            commands: None,
            files: None,
            description: "d".into(),
            vocabulary: "v".into(),
            threshold: 0.0,
            scope: scope.to_string(),
            when_project: None,
            when_file_exists: None,
            trigger: trigger.map(String::from),
            trigger_path: None,
        }
    }

    #[test]
    fn each_lane_admits_what_its_scan_loop_admits() {
        let agent = way("a", "agent", None);
        let sub = way("s", "subagent", None);
        let team = way("t", "teammate", None);
        let state = way("st", "subagent", Some("context-threshold"));
        let p = Lane::Prompt { scope: "agent" };
        assert!(eligible(&agent, p, "/p") && !eligible(&sub, p, "/p"));
        let task = Lane::Task { teammate: false };
        assert!(eligible(&sub, task, "/p") && !eligible(&agent, task, "/p") && !eligible(&team, task, "/p"));
        assert!(!eligible(&state, task, "/p"), "a state-triggered way skips the task lane");
        assert!(eligible(&team, Lane::Task { teammate: true }, "/p"));
    }

    #[test]
    fn a_when_precondition_that_fails_makes_a_way_ineligible() {
        let mut w = way("w", "agent", None);
        w.when_project = Some("/definitely/not/this/project".into());
        assert!(!eligible(&w, Lane::Prompt { scope: "agent" }, "/p"));
    }

    /// An agent-scope way that beats every chunk must not take share from the
    /// subagent-scope ways that can fire on the task lane.
    #[test]
    fn an_ineligible_by_scope_way_takes_no_share_on_the_task_lane() {
        let cands = [way("agent-only", "agent", None), way("a", "subagent", None), way("b", "subagent", None)];
        let row = |v: &[(&str, f64)]| v.iter().map(|(i, c)| (i.to_string(), *c)).collect::<Vec<_>>();
        let rows = vec![
            row(&[("agent-only", 0.9), ("a", 0.6), ("b", 0.5)]),
            row(&[("agent-only", 0.8), ("b", 0.55), ("a", 0.3)]),
        ];
        let lane = Lane::Task { teammate: false };
        let eligible_map = body_map(cands.iter().filter(|w| eligible(w, lane, "/p")));
        let got = late_interaction::shares_for_test(rows.clone(), &eligible_map);
        let absent: Vec<Vec<(String, f64)>> =
            rows.iter().map(|r| r.iter().filter(|(i, _)| i != "agent-only").cloned().collect()).collect();
        let want = late_interaction::shares_for_test(absent, &eligible_map);
        assert_eq!(got, want);
        assert!(got.iter().all(|(id, _)| id != "agent-only"));
        // With the old toggle-only set it would have competed.
        let all = body_map(cands.iter());
        assert!(late_interaction::shares_for_test(rows, &all).iter().any(|(id, _)| id == "agent-only"));
    }

    #[test]
    fn the_authoring_view_competes_the_prompt_lane_unless_asked_for_all() {
        let cands = [way("agent", "agent", None), way("sub", "subagent", None)];
        let ids = |unfiltered| diag_candidates(&cands, "/p", unfiltered).iter().map(|w| w.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(false), ["agent"], "default: what a prompt scan in agent scope competes");
        assert_eq!(ids(true), ["agent", "sub"]);
    }
}
