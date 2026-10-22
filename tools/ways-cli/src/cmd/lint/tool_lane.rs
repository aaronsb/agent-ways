//! Tool-lane rules (ADR-704): `tool_name:` and `tool_event:`.
//!
//! The lane reaches only what the shipped settings.json wires on
//! check-tool.sh, and a user or project way cannot widen that. A way whose
//! regex names no wired tool, or whose events are all unwired, lints clean on
//! the schema alone and never fires, so these rules name the wired set.

/// What lint reads from the schema for this lane.
pub(super) struct ToolLane<'a> {
    /// The `tool_event:` names Claude Code filters by tool name.
    pub(super) valid_events: &'a [String],
    pub(super) wired_events: &'a [String],
    pub(super) wired_names: &'a [String],
}

/// The tool-lane findings for one way's `tool_name:` and raw `tool_event:`
/// values, as `(errors, warnings)` message lists.
pub(super) fn check(
    lane: &ToolLane,
    tool_name: Option<&str>,
    tool_event: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if let Some(raw) = tool_event {
        let events = crate::frontmatter::tool_events(Some(raw));
        if events.is_empty() {
            errors.push("tool_event lists no event, so the tool lane never fires this way".to_string());
        }
        for e in &events {
            if !lane.valid_events.iter().any(|v| v == e) {
                errors.push(format!("invalid tool_event '{e}' (valid: {})", lane.valid_events.join(", ")));
            } else if !lane.wired_events.iter().any(|v| v == e) {
                warnings.push(format!(
                    "tool_event '{e}' is not wired: the shipped settings.json runs check-tool.sh on {} only",
                    lane.wired_events.join(", ")
                ));
            }
        }
        if tool_name.is_none() {
            warnings.push("tool_event without tool_name: has no effect".to_string());
        }
    }

    if let Some(pattern) = tool_name {
        match crate::cmd::scan::compile_trigger(pattern) {
            None => errors.push(format!("tool_name '{pattern}' is not a valid regex")),
            Some(re) if !lane.wired_names.iter().any(|t| re.is_match(t)) => warnings.push(format!(
                "tool_name '{pattern}' matches none of the wired tools ({}): the shipped settings.json \
                 matcher sends no other tool to the lane, so this way never fires there",
                lane.wired_names.join(", ")
            )),
            Some(_) => {}
        }
    }

    (errors, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn run(tool_name: Option<&str>, tool_event: Option<&str>) -> (Vec<String>, Vec<String>) {
        let valid = strings(&["PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "PermissionDenied"]);
        let wired = strings(&["PreToolUse", "PostToolUse"]);
        let names = strings(&["EnterPlanMode", "ExitPlanMode", "Workflow"]);
        let lane = ToolLane { valid_events: &valid, wired_events: &wired, wired_names: &names };
        check(&lane, tool_name, tool_event)
    }

    #[test]
    fn a_wired_name_and_wired_events_lint_clean() {
        assert_eq!(run(Some("^exitplanmode$"), Some("[PreToolUse, PostToolUse]")), (vec![], vec![]));
        assert_eq!(run(Some("Workflow"), None), (vec![], vec![]));
    }

    #[test]
    fn a_name_no_wired_tool_matches_warns_with_the_wired_set() {
        let (errors, warnings) = run(Some("^Read$"), None);
        assert!(errors.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("EnterPlanMode, ExitPlanMode, Workflow"), "{warnings:?}");
    }

    #[test]
    fn an_invalid_regex_is_an_error() {
        let (errors, _) = run(Some("(Workflow"), None);
        assert_eq!(errors.len(), 1, "{errors:?}");
    }

    #[test]
    fn events_outside_the_five_are_errors_and_unwired_ones_warn() {
        let (errors, warnings) = run(Some("Workflow"), Some("[PostToolUse, posttooluse, PermissionRequest]"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("'posttooluse'"));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("'PermissionRequest'") && warnings[0].contains("PreToolUse, PostToolUse"));
    }

    #[test]
    fn an_empty_list_is_an_error_and_events_alone_warn() {
        assert_eq!(run(Some("Workflow"), Some("[]")).0.len(), 1);
        let (errors, warnings) = run(None, Some("PostToolUse"));
        assert!(errors.is_empty());
        assert_eq!(warnings, ["tool_event without tool_name: has no effect"]);
    }
}
