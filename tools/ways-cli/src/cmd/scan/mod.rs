//! Scan ways and output matched content — replaces hook scan loops.
//!
//! Combines file walking, frontmatter extraction, matching (pattern + semantic),
//! scope/precondition gating, parent-threshold lowering, and show (display).

mod candidates;
mod scoring;
pub(crate) use scoring::batch_embed_score;

use anyhow::Result;
use regex::Regex;
use std::path::PathBuf;

use crate::session;

use candidates::{check_when, collect_candidates, collect_checks};
use scoring::{capture_show_check, capture_show_way, default_project, EmbedScores};

pub(crate) struct WayCandidate {
    pub id: String,
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

    let embed_matches = batch_embed_score(query);

    let mut context = String::new();

    for way in &candidates {
        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, &project_dir) {
            continue;
        }

        // Additive matching: pattern OR semantic
        let channel = match_prompt(
            query,
            &way.pattern,
            &way.id,
            effective_thresholds(way, session_id),
            &embed_matches,
        );

        if let Some(trigger) = channel {
            let out = capture_show_way(&way.id, session_id, &trigger);
            if !out.is_empty() {
                context.push_str(&out);
                context.push_str("\n\n");
            }
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

    let embed_matches = batch_embed_score(query);

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

        let channel = match_prompt(
            query,
            &way.pattern,
            &way.id,
            effective_thresholds(way, session_id),
            &embed_matches,
        );

        if let Some(trigger) = channel {
            matched.push((way.id.clone(), trigger));
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

        let mut matched = false;

        if let Some(ref cmds_pattern) = way.commands {
            if regex_matches(cmds_pattern, cmd) {
                matched = true;
            }
        }

        if !matched {
            if let Some(desc) = description {
                if let Some(ref pat) = way.pattern {
                    if regex_matches(pat, &desc.to_lowercase()) {
                        matched = true;
                    }
                }
            }
        }

        if matched {
            let out = capture_show_way(&way.id, session_id, "bash");
            if !out.is_empty() {
                context.push_str(&out);
            }
        }
    }

    // Check matching: commands regex + semantic scoring
    let checks = collect_checks(&project_dir);
    let query_for_checks = format!(
        "{} {}",
        cmd,
        description.unwrap_or("")
    );

    let embed_check_matches = batch_embed_score(&query_for_checks);

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
            if regex_matches(files_pattern, filepath) {
                let out = capture_show_way(&way.id, session_id, "file");
                if !out.is_empty() {
                    context.push_str(&out);
                }
            }
        }
    }

    let checks = collect_checks(&project_dir);
    let embed_matches = batch_embed_score(filepath);

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

fn match_prompt(
    query: &str,
    pattern: &Option<String>,
    way_id: &str,
    thresholds: EffectiveThresholds,
    scores: &EmbedScores,
) -> Option<String> {
    // Channel 1: Regex pattern — always checked first, always fires on match.
    if let Some(ref pat) = pattern {
        if regex_matches(pat, query) {
            return Some("keyword".to_string());
        }
    }

    // Channel 2: Embedding. Each model path stands on its own threshold;
    // scores don't cross-compare (apples and oranges). Either path firing
    // is sufficient, but the thresholds are calibrated independently so
    // each model's noise band sits below its gate:
    //   - EN model (0.40): sharp on English, noise below 0.35
    //   - multi model (0.55): cross-lingual but coarser, noise at 0.30-0.50
    let en_fires = scores.best_en(way_id).is_some_and(|s| s >= thresholds.en);
    let multi_fires = scores.best_multi(way_id).is_some_and(|s| s >= thresholds.multi);

    if en_fires {
        Some("semantic:embedding:en".to_string())
    } else if multi_fires {
        Some("semantic:embedding:multi".to_string())
    } else {
        None
    }
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
    let en = scores.best_en(&check.id).filter(|s| *s >= t.en);
    let mu = scores.best_multi(&check.id).filter(|s| *s >= t.multi);
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

/// Emit accumulated context using the envelope shape required by the
/// invoking hook event. The Claude Code hook contract treats
/// `hookSpecificOutput` as canonical for all events; the simpler top-level
/// `additionalContext` is a legacy tolerance accepted only on
/// `SessionStart` and `PreToolUse` (where it surfaces as a visible
/// attachment). Defaulting to canonical means new event wirings
/// (`Stop`, `PreCompact`, ...) get the right shape automatically rather
/// than silently re-hitting the bug PR #80 fixed.
fn emit_hook_context(hook_event: &str, context: &str) {
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

// ── State scan ──────────────────────────────────────────────────

pub fn state(
    session_id: &str,
    project: Option<&str>,
    transcript: Option<&str>,
    hook_event: &str,
) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(default_project);

    let scope = session::detect_scope(session_id);
    let candidates = collect_candidates(&project_dir);

    let mut context = String::new();

    // Core re-injection safety net
    if !session::core_is_shown(session_id) {
        let out = capture_show_core(session_id);
        if !out.is_empty() {
            context.push_str(&out);
            context.push_str("\n\n");
        }
    } else if let Some(tp) = transcript {
        // Check for stale core (context cleared under us)
        let ctx_size = transcript_size_since_summary(tp);
        if let Some(marker_ts) = session::core_marker_ts(session_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let age = now.saturating_sub(marker_ts);
            if ctx_size < 5000 && age > 30 {
                session::clear_core(session_id);
                let out = capture_show_core(session_id);
                if !out.is_empty() {
                    context.push_str(&out);
                    context.push_str("\n\n");
                }
            }
        }
    }

    // State trigger evaluation
    for way in &candidates {
        let trigger_type = match &way.trigger {
            Some(t) => t.as_str(),
            None => continue,
        };

        if !session::scope_matches(&way.scope, &scope) {
            continue;
        }

        let triggered = match trigger_type {
            "context-threshold" => {
                evaluate_context_threshold(way.threshold as u64, transcript)
            }
            "file-exists" => {
                if let Some(ref pattern) = way.trigger_path {
                    evaluate_file_exists(pattern, &project_dir)
                } else {
                    false
                }
            }
            "session-start" => true,
            _ => false,
        };

        if !triggered {
            continue;
        }

        // Handle repeating context-threshold ways
        if trigger_type == "context-threshold" && way.repeat {
            let tasks_marker = format!("{}/{session_id}/tasks-active", crate::session::sessions_root());
            if std::path::Path::new(&tasks_marker).exists() {
                continue; // tasks active, suppress repeat
            }
            // Repeating: output body directly (no marker gating)
            let content = std::fs::read_to_string(&way.path).unwrap_or_default();
            let body = strip_frontmatter(&content);
            if !body.is_empty() {
                context.push_str(&body);
                context.push_str("\n\n");
                session::log_event(&[
                    ("event", "way_fired"),
                    ("way", &way.id),
                    ("domain", way.id.split('/').next().unwrap_or(&way.id)),
                    ("trigger", "state"),
                    ("scope", &scope),
                    ("project", &project_dir),
                    ("session", session_id),
                ]);
            }
        } else {
            // Standard one-shot: marker-gated via show
            let out = capture_show_way(&way.id, session_id, "state");
            if !out.is_empty() {
                context.push_str(&out);
                context.push_str("\n\n");
            }
        }
    }

    if !context.is_empty() {
        emit_hook_context(hook_event, context.trim_end());
    }

    Ok(())
}

fn evaluate_context_threshold(threshold_pct: u64, transcript: Option<&str>) -> bool {
    // Guard: a missing or 0 threshold on a context-threshold trigger is a bug
    // (would fire on every non-empty transcript). Caller should have set a
    // percentage in frontmatter. Refuse to fire rather than spam.
    if threshold_pct == 0 {
        return false;
    }

    let transcript = match transcript {
        Some(t) if std::path::Path::new(t).is_file() => t,
        _ => return false,
    };

    // Detect model for context window size
    let window_chars: u64 = detect_window_chars(transcript);
    let limit = window_chars * threshold_pct / 100;
    let size = transcript_size_since_summary(transcript);

    size > limit
}

fn detect_window_chars(transcript: &str) -> u64 {
    let content = match std::fs::read_to_string(transcript) {
        Ok(c) => c,
        Err(_) => return 620_000,
    };
    for line in content.lines().rev() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(model) = val.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
                    if model.contains("opus-4") {
                        return 3_800_000;
                    }
                }
                break;
            }
        }
    }
    620_000 // default: ~155K tokens × 4 chars/token
}

fn transcript_size_since_summary(transcript: &str) -> u64 {
    let file_size = match std::fs::metadata(transcript) {
        Ok(m) => m.len(),
        Err(_) => return 0,
    };

    // Check last 100KB for summary markers
    let content = match std::fs::read_to_string(transcript) {
        Ok(c) => c,
        Err(_) => return file_size,
    };

    // Find last summary line position
    let mut last_summary_pos = 0u64;
    let mut pos = 0u64;
    for line in content.lines() {
        if line.contains("\"type\":\"summary\"") {
            last_summary_pos = pos + line.len() as u64 + 1;
        }
        pos += line.len() as u64 + 1;
    }

    file_size.saturating_sub(last_summary_pos)
}

fn evaluate_file_exists(pattern: &str, project_dir: &str) -> bool {
    // Use glob matching for patterns like "*.md" or ".claude/todo-*.md"
    let full_pattern = format!("{project_dir}/{pattern}");
    glob::glob(&full_pattern)
        .map(|paths| paths.filter_map(|p| p.ok()).next().is_some())
        .unwrap_or(false)
}

fn strip_frontmatter(content: &str) -> String {
    let mut fm_count = 0;
    let mut body = Vec::new();
    for line in content.lines() {
        if line == "---" {
            fm_count += 1;
            continue;
        }
        if fm_count >= 2 {
            body.push(line);
        }
    }
    body.join("\n")
}

fn capture_show_core(session_id: &str) -> String {
    crate::cmd::show::core(session_id).unwrap_or_default()
}
