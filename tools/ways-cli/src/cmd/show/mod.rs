//! Display ways, checks, and core guidance — session-aware, idempotent.
//!
//! Replaces: show-way.sh, show-check.sh, show-core.sh

mod helpers;
mod metrics;

use anyhow::Result;
use serde_json::json;
use std::path::Path;

use crate::{frontmatter, session};
use helpers::{extract_field, extract_attend_signals, home_dir, is_project_trusted, body_text, check_sections_text, run_macro};
use metrics::{compute_tree_metrics, count_siblings, git_version, dirty_status_text, update_status_text};

// ── ways show way ───────────────────────────────────────────────

pub fn way(id: &str, session_id: &str, trigger: &str) -> Result<String> {
    way_scored(id, session_id, trigger, None, None, None)
}

/// Bound a matched surface to a single readable line for the `surface` telemetry
/// field: collapse all whitespace to single spaces and truncate to ~200 chars on
/// a char boundary (appending `…`). The snippet is a human read-side aid — a
/// judgeable record of *what* the semantic channel matched on — so it favours
/// legibility over fidelity.
const SURFACE_SNIPPET_CHARS: usize = 200;
fn surface_snippet(surface: &str) -> String {
    let collapsed = surface.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SURFACE_SNIPPET_CHARS {
        return collapsed;
    }
    let head: String = collapsed.chars().take(SURFACE_SNIPPET_CHARS).collect();
    format!("{head}…")
}

/// Like [`way`], but records the embedding score that caused a semantic fire
/// onto the `way_fired` event (ADR-134 task D telemetry). `fire_score` is
/// `Some` only for embedding-channel fires from the in-process scan path;
/// keyword/state/CLI fires pass `None` and log no score (they have none). The
/// firing model is already implicit in `trigger` (`semantic:embedding:en|multi`).
///
/// `surface` is the noise-stripped surface the matcher scored (the `reduce_for_embed`
/// output common to both the late-interaction and single-vector paths). It is logged
/// — bounded by [`surface_snippet`] — only alongside a `fire_score`, i.e. on semantic
/// fires, giving the read-side precision instrument a judgeable record of what fired
/// each way without re-embedding history. Keyword fires already carry `matched_span`.
pub fn way_scored(
    id: &str,
    session_id: &str,
    trigger: &str,
    fire_score: Option<f64>,
    matched_span: Option<&str>,
    surface: Option<&str>,
) -> Result<String> {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));

    // Disable checks: domain (user scope) and per-way (project scope, ADR-131)
    let domain = id.split('/').next().unwrap_or(id);
    if session::domain_disabled(domain) || session::way_disabled(id) {
        return Ok(String::new());
    }

    // Scope check
    let scope = session::detect_scope(session_id);
    let (way_file, is_project_local) = match session::resolve_way_file(id, &project_dir) {
        Some(r) => r,
        None => return Ok(String::new()),
    };

    // Read frontmatter for scope field
    let content = std::fs::read_to_string(&way_file)?;
    let scope_field = extract_field(&content, "scope").unwrap_or_default();
    if !session::scope_matches(&scope_field, &scope) {
        return Ok(String::new());
    }

    // Session firing gate: consult the engine with this way's resolved curve.
    // First-fire always allowed; re-fire when the outward gate's salience has
    // decayed below REFIRE_FLOOR; otherwise suppress.
    //
    // ADR-126: the cadence comes from `refire:` (fraction of window), resolved
    // against the session's current window — fetched from the active
    // transcript; 200k is the conservative fallback when detection fails.
    let fm = frontmatter::parse(&way_file)?;
    let window = crate::cmd::context::get_context(Some(&project_dir))
        .map(|c| c.tokens_total)
        .unwrap_or(200_000);
    let curve = fm.resolved_curve(window).ok_or_else(|| {
        anyhow::anyhow!(
            "way {} is missing a `refire:` field in its frontmatter (ADR-126)",
            id
        )
    })?;
    let outcome = session::way_fire_outcome(id, session_id, &curve);
    if !outcome.is_allowed() {
        return Ok(String::new());
    }
    let is_redisclosure = outcome.is_redisclosure();
    session::record_way_fire(id, session_id, &curve);

    // Macro handling
    let macro_pos = extract_field(&content, "macro");
    let way_dir = way_file.parent().unwrap_or(Path::new("."));
    let macro_file = way_dir.join("macro.sh");
    let macro_out = if macro_pos.is_some() && macro_file.is_file() {
        if is_project_local && !is_project_trusted(&project_dir) {
            Some(format!(
                "**Note**: Project-local macro skipped (add {} to ~/.claude/trusted-project-macros to enable)",
                project_dir
            ))
        } else {
            run_macro(&macro_file)
        }
    } else {
        None
    };

    // Build output
    let mut output = String::new();

    if macro_pos.as_deref() == Some("prepend") {
        if let Some(ref out) = macro_out {
            output.push_str(out);
            output.push_str("\n\n");
        }
    }

    output.push_str(&body_text(&content));

    if macro_pos.as_deref() == Some("append") {
        if let Some(ref out) = macro_out {
            output.push('\n');
            output.push_str(out);
        }
    }

    // Stamp markers
    let token_pos = session::get_token_position(session_id);
    session::stamp_way_marker(id, session_id, token_pos);
    session::stamp_way_tokens(id, session_id, token_pos);

    let epoch = session::get_epoch(session_id);
    session::stamp_way_epoch(id, session_id, epoch);

    // Tree disclosure tracking
    let (tree_depth, parent_id, parent_epoch, epoch_from_parent) =
        compute_tree_metrics(id, session_id);

    let (sibling_total, sibling_fired) = count_siblings(id, &project_dir, session_id);

    // Metrics JSONL
    let agent_id = std::env::var("CLAUDE_AGENT_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "main".to_string());

    session::append_metric(
        session_id,
        &json!({
            "way": id,
            "parent": parent_id.as_deref().unwrap_or("none"),
            "depth": tree_depth,
            "epoch": epoch,
            "parent_epoch": parent_epoch,
            "epoch_distance": epoch_from_parent,
            "sibling_total": sibling_total,
            "sibling_fired": sibling_fired,
            "trigger": trigger,
            "agent_id": agent_id,
        }),
    );

    // Event logging
    let mut log_fields: Vec<(&str, String)> = vec![
        ("event", if is_redisclosure { "way_redisclosed" } else { "way_fired" }.to_string()),
        ("way", id.to_string()),
        ("domain", domain.to_string()),
        ("trigger", trigger.to_string()),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id.to_string()),
        ("token_position", token_pos.to_string()),
    ];
    // ADR-134 task D: the calibrated probability that fired this way, feeding the
    // fire-score telemetry that calibration (ADR-156) is fit from. Logged on every
    // semantic fire — first-fire *and* redisclosure — so precision is measurable
    // across the whole fire population, not just the first-fire slice. The score is
    // worth writing at fire time because it is not cheaply recoverable later (that
    // would mean re-embedding the historical surface against the corpus).
    // Calibration still isolates the *placement* population cleanly by filtering on
    // the event kind (`way_fired`, per tune_precision.rs), so widening the score to
    // redisclosures does not bias the derivation — it just makes the redisclosure
    // score available as a cadence signal. Keyword/state/CLI fires pass None and log
    // no score (they have none).
    if let Some(score) = fire_score {
        log_fields.push(("fire_score", format!("{score:.4}")));
        // The surface rides with the score (semantic fires only): together they make
        // a fire judgeable on the read side — "this score, on this text" — without
        // re-embedding the historical surface. A redisclosure carries its own surface,
        // so this is not gated on first-fire (unlike `matched_span`).
        if let Some(s) = surface.filter(|s| !s.is_empty()) {
            log_fields.push(("surface", surface_snippet(s)));
        }
    }
    // ADR-153 §3: the deterministic-channel match text, so introspection can show
    // *what* fired the way without transcript replay. First-fire only (a
    // redisclosure's re-trigger is a different match); semantic never carries one.
    if let (false, Some(span)) = (is_redisclosure, matched_span) {
        // A zero-width author pattern (e.g. `^`) fires but matches an empty string;
        // recording an empty span is pure telemetry noise, so skip it (the fire
        // decision was already made upstream — this only gates what's logged).
        if !span.is_empty() {
            log_fields.push(("matched_span", span.to_string()));
        }
    }
    if let Some(ref p) = parent_id {
        log_fields.push(("parent", p.clone()));
        log_fields.push(("tree_depth", tree_depth.to_string()));
        if let Some(dist) = epoch_from_parent {
            log_fields.push(("epoch_distance", dist.to_string()));
        }
    }
    let team = session::detect_team(session_id);
    if let Some(t) = team {
        log_fields.push(("team", t));
    }
    let refs: Vec<(&str, &str)> = log_fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
    session::log_event(&refs);

    Ok(output)
}

// ── ways show check ─────────────────────────────────────────────

pub fn check(id: &str, session_id: &str, trigger: &str, match_score: f64) -> Result<String> {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));

    // Disable checks: domain (user scope) and per-way (project scope, ADR-131)
    let domain = id.split('/').next().unwrap_or(id);
    if session::domain_disabled(domain) || session::way_disabled(id) {
        return Ok(String::new());
    }

    // Scope check
    let scope = session::detect_scope(session_id);

    let (check_file, _is_project_local) = match session::resolve_check_file(id, &project_dir) {
        Some(r) => r,
        None => return Ok(String::new()),
    };

    let check_content = std::fs::read_to_string(&check_file)?;
    let scope_field = extract_field(&check_content, "scope").unwrap_or_default();
    if !scope_field.is_empty() && !session::scope_matches(&scope_field, &scope) {
        return Ok(String::new());
    }

    // Epoch distance
    let epoch = session::get_epoch(session_id);
    let way_has_fired = session::way_is_shown(id, session_id);
    let epoch_distance = if way_has_fired {
        session::epoch_distance(id, session_id).min(30)
    } else {
        30
    };

    // Fire count
    let fire_count = session::get_check_fires(id, session_id);

    // Scoring curve
    let distance_factor = ((epoch_distance as f64) + 1.0).ln() + 1.0;
    let decay_factor = 1.0 / (fire_count as f64 + 1.0);
    let effective_score = match_score * distance_factor * decay_factor;

    // Threshold
    let threshold: f64 = extract_field(&check_content, "threshold")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);

    if effective_score < threshold {
        return Ok(String::new());
    }

    let mut output = String::new();

    // If parent way hasn't fired, pull it in alongside the check
    if !way_has_fired {
        let parent_out = way(id, session_id, "check-pull")?;
        if !parent_out.is_empty() {
            output.push_str(&parent_out);
            output.push('\n');
        }
    }

    // Include anchor section when epoch distance >= 5
    let include_anchor = epoch_distance >= 5;
    output.push_str(&check_sections_text(&check_content, include_anchor));

    // Bump fire count
    session::bump_check_fires(id, session_id);

    // Log
    let anchored = if include_anchor { "true" } else { "false" };
    let way_epoch = session::get_way_epoch(id, session_id);
    session::log_event(&[
        ("event", "check_fired"),
        ("check", id),
        ("domain", domain),
        ("trigger", trigger),
        ("epoch", &epoch.to_string()),
        ("way_epoch", &way_epoch.to_string()),
        ("distance", &epoch_distance.to_string()),
        ("fire_count", &(fire_count + 1).to_string()),
        ("match_score", &format!("{match_score:.2}")),
        ("effective_score", &format!("{effective_score:.2}")),
        ("anchored", anchored),
        ("scope", &scope),
        ("project", &project_dir),
        ("session", session_id),
    ]);

    Ok(output)
}

// ── ways show core ──────────────────────────────────────────────

pub fn core(session_id: &str) -> Result<String> {
    let ways_dir = home_dir().join(".claude/hooks/ways");
    let mut output = String::new();

    // Run the macro for the dynamic ways table
    let macro_file = ways_dir.join("macro.sh");
    if macro_file.is_file() {
        if let Some(out) = run_macro(&macro_file) {
            output.push_str(&out);
            output.push('\n');
        }
    }

    // Output core.md body with language substitution
    let core_file = ways_dir.join("core.md");
    if core_file.is_file() {
        let content = std::fs::read_to_string(&core_file)?;
        let mut body = body_text(&content);

        // Replace hardcoded English directive with configured language
        let lang = crate::agents::resolve_language();
        if lang != "en" {
            body = body.replace(
                "All file output (commit messages, comments, documentation, PR descriptions) must be in English regardless of interface language setting.",
                &format!("All file output (commit messages, comments, documentation, PR descriptions) must be in {lang}. Code identifiers (variable names, function names) should remain in English."),
            );
        }

        output.push_str(&body);
    }

    // Version info
    let claude_dir = home_dir().join(".claude");
    let version = git_version(&claude_dir);
    output.push_str(&format!("\n---\n_Ways version: {version}_"));

    // Update status from cache
    output.push_str(&update_status_text());

    // Dirty file enumeration
    output.push_str(&dirty_status_text(&claude_dir));

    // Stamp core marker
    session::stamp_core(session_id);

    Ok(output)
}

// ── ways show attend/<signal> ──────────────────────────────────

pub fn attend(signal: &str, session_id: &str) -> Result<String> {
    let ways_dir = home_dir().join(".claude/hooks/ways");

    // Also check project-local ways
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));
    let project_ways = std::path::PathBuf::from(&project_dir).join(".claude/ways");

    let dirs: Vec<&std::path::Path> = if project_ways.is_dir() {
        vec![ways_dir.as_path(), project_ways.as_path()]
    } else {
        vec![ways_dir.as_path()]
    };

    // Scan for ways with trigger.type: attend and matching signal
    let mut matched_ids: Vec<String> = Vec::new();

    for dir in &dirs {
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() { continue; }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".md") || name.contains(".check.") { continue; }

            if let Ok(content) = std::fs::read_to_string(path) {
                let signals = extract_attend_signals(&content);
                if signals.iter().any(|s| s == signal) {
                    // Derive way ID from path relative to ways dir
                    if let Ok(rel) = path.strip_prefix(dir) {
                        let id = rel.with_extension("")
                            .to_string_lossy()
                            .replace('\\', "/");
                        // Remove trailing /way-name if it matches parent dir name
                        // e.g., "attend/context-pressure/context-pressure" → "attend/context-pressure"
                        let id = normalize_way_id(&id);
                        matched_ids.push(id);
                    }
                }
            }
        }
    }

    if matched_ids.is_empty() {
        return Ok(format!("No way handles attend signal '{signal}'.\n"));
    }

    // Show first matching way through the standard disclosure pipeline
    let mut output = String::new();
    for id in &matched_ids {
        let trigger = format!("attend:{signal}");
        let result = way(id, session_id, &trigger)?;
        if !result.is_empty() {
            output.push_str(&result);
            break; // First eligible way wins
        }
    }

    Ok(output)
}

/// Normalize a way ID: if the last segment matches its parent dir name, collapse.
/// e.g., "attend/context-pressure/context-pressure" → "attend/context-pressure"
fn normalize_way_id(id: &str) -> String {
    let parts: Vec<&str> = id.split('/').collect();
    if parts.len() >= 2 && parts[parts.len() - 1] == parts[parts.len() - 2] {
        parts[..parts.len() - 1].join("/")
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_snippet_collapses_whitespace() {
        assert_eq!(
            surface_snippet("write   an\nADR\tand   open a PR"),
            "write an ADR and open a PR"
        );
    }

    #[test]
    fn surface_snippet_bounds_long_input_on_char_boundary() {
        let long = "é".repeat(500); // multi-byte; truncation must not split a char
        let out = surface_snippet(&long);
        assert_eq!(out.chars().count(), SURFACE_SNIPPET_CHARS + 1, "200 chars + ellipsis");
        assert!(out.ends_with('…'));
    }

    #[test]
    fn surface_snippet_leaves_short_input_untruncated() {
        let s = "short surface";
        assert_eq!(surface_snippet(s), s);
    }

    #[test]
    fn normalize_collapses_duplicate_leaf() {
        assert_eq!(
            normalize_way_id("meta/attend/context-pressure/context-pressure"),
            "meta/attend/context-pressure"
        );
    }

    #[test]
    fn normalize_preserves_distinct_leaf() {
        assert_eq!(
            normalize_way_id("softwaredev/code/testing"),
            "softwaredev/code/testing"
        );
    }

    #[test]
    fn normalize_single_segment() {
        assert_eq!(normalize_way_id("testing"), "testing");
    }
}
