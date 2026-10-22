//! The tool lane (ADR-704): ways keyed on the tool's name.

use anyhow::Result;

use crate::cmd::show::ContextBudget;
use crate::session;

use super::candidates::{check_when, collect_candidates};
use super::order::{order_hits, Hit};
use super::scoring::capture_show_way;
use super::{emit_hook_context, regex_span, WayCandidate};

// ── Tool lane (ADR-704) ─────────────────────────────────────────

/// Whether a way's `tool_event:` lists this hook event. The names are the
/// literal hook event names, compared exactly; a missing `tool_event:` is
/// `[PreToolUse]`. A misspelt name admits nothing, and lint names it.
fn event_admits(tool_event: Option<&str>, hook_event: &str) -> bool {
    crate::frontmatter::tool_events(tool_event).iter().any(|e| e == hook_event)
}

/// The tool-lane hits among `candidates` for `tool` at `hook_event`: ways
/// with a `tool_name:`, then scope, `when:`, the event, and the regex.
fn tool_hits(
    candidates: &[WayCandidate],
    tool: &str,
    hook_event: &str,
    scope: &str,
    project_dir: &str,
) -> Vec<Hit<String>> {
    let mut hits: Vec<Hit<String>> = Vec::new();
    for way in candidates {
        let Some(ref name_pattern) = way.tool_name else { continue };
        if !session::scope_matches(&way.scope, scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, project_dir) {
            continue;
        }
        if !event_admits(way.tool_event.as_deref(), hook_event) {
            continue;
        }
        if let Some(span) = regex_span(name_pattern, tool) {
            hits.push(Hit::explicit(&way.id, name_pattern, &span, span.clone()));
        }
    }
    order_hits(&mut hits);
    hits
}

/// Match a tool call against `tool_name:` ways and emit their bodies.
///
/// Wired from PreToolUse and PostToolUse on the tools the settings template
/// names (check-tool.sh). The envelope carries the real event name, so a
/// PostToolUse fire lands after the tool returned. Which events a way fires
/// on is its `tool_event:` list; the default leaves out a failed call
/// (`PostToolUseFailure`), whose stamp would hold the way back from the retry
/// that succeeds (ADR-188 item 2).
pub fn tool(
    tool: &str,
    hook_event: &str,
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
    for hit in &tool_hits(&candidates, tool, hook_event, &scope, &project_dir) {
        let out = capture_show_way(&hit.id, session_id, "tool", None, Some(hit.payload.as_str()), None, Some(&mut budget)).body;
        if !out.is_empty() {
            context.push_str(&out);
        }
    }

    if !context.is_empty() {
        emit_hook_context(hook_event, context.trim_end());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::scan::eligibility_tests::way;

    #[test]
    fn the_tool_lane_matches_the_name_and_the_event() {
        let mut pre = way("pre", "agent", None);
        pre.tool_name = Some("^exitplanmode$".into());
        let mut post = way("post", "agent", None);
        post.tool_name = Some("ExitPlanMode|Workflow".into());
        post.tool_event = Some("[PostToolUse]".into());
        let mut both = way("both", "agent", None);
        both.tool_name = Some("Workflow".into());
        both.tool_event = Some("[PreToolUse, PostToolUse]".into());
        let mut odd = way("odd", "agent", None);
        odd.tool_name = Some("Workflow".into());
        odd.tool_event = Some("[posttooluse]".into());
        let mut failed = way("failed", "agent", None);
        failed.tool_name = Some("Workflow".into());
        failed.tool_event = Some("PostToolUseFailure".into());
        let sub = { let mut w = way("sub", "subagent", None); w.tool_name = Some("Workflow".into()); w };
        let cands = [pre, post, both, odd, failed, sub, way("none", "agent", None)];
        let ids = |tool: &str, event: &str| {
            let mut v: Vec<String> = tool_hits(&cands, tool, event, "agent", "/p").into_iter().map(|h| h.id).collect();
            v.sort();
            v
        };
        assert_eq!(ids("ExitPlanMode", "PreToolUse"), ["pre"], "case-insensitive name, PreToolUse by default");
        assert_eq!(ids("ExitPlanMode", "PostToolUse"), ["post"]);
        assert_eq!(ids("Workflow", "PreToolUse"), ["both"]);
        assert_eq!(ids("Workflow", "PostToolUse"), ["both", "post"], "event names are exact");
        assert!(ids("Bash", "PreToolUse").is_empty());
        assert_eq!(ids("Workflow", "PostToolUseFailure"), ["failed"], "a failed call fires only a way that lists it");
    }

    #[test]
    fn the_tool_lane_honours_when() {
        let mut w = way("w", "agent", None);
        w.tool_name = Some("Workflow".into());
        w.when_file_exists = Some("definitely-not-here/*.md".into());
        assert!(tool_hits(&[w], "Workflow", "PreToolUse", "agent", "/p").is_empty());
    }
}
