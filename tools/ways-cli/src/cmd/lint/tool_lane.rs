//! Tool-lane rules (ADR-704): `tool_name:` and `tool_event:`.
//!
//! The lane reaches only what the shipped settings.json wires on
//! check-tool.sh, and a user or project way cannot widen that. A way whose
//! regex names no wired tool, or whose events are all unwired, lints clean on
//! the schema alone and never fires, so these rules name the wired set.

use super::helpers::{get_field_value, has_field};

/// What lint reads from the schema for this lane. A list is empty when the
/// schema predates it (a newer binary over an older projected schema); the
/// rule that reads it is then skipped rather than failing every way.
pub(super) struct ToolLane<'a> {
    /// The `tool_event:` names Claude Code filters by tool name.
    pub(super) valid_events: &'a [String],
    pub(super) wired_events: &'a [String],
    pub(super) wired_names: &'a [String],
}

/// The tool-lane findings for one way's frontmatter, as `(errors, warnings)`
/// message lists.
pub(super) fn check(lane: &ToolLane, fm: &str) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let tool_name = get_field_value(fm, "tool_name");

    if has_field(fm, "tool_event") {
        match get_field_value(fm, "tool_event") {
            // A block list leaves the key's own line empty. The fire path reads
            // one line, so it would fall back to [PreToolUse] without a word.
            None => errors.push(
                "tool_event must be a one-line list, e.g. tool_event: [PostToolUse]".to_string(),
            ),
            Some(raw) => {
                let events = crate::frontmatter::tool_events(Some(&raw));
                if events.is_empty() {
                    errors.push("tool_event lists no event, so the tool lane never fires this way".to_string());
                }
                for e in &events {
                    if !lane.valid_events.is_empty() && !lane.valid_events.iter().any(|v| v == e) {
                        errors.push(format!("invalid tool_event '{e}' (valid: {})", lane.valid_events.join(", ")));
                    } else if !lane.wired_events.is_empty() && !lane.wired_events.iter().any(|v| v == e) {
                        warnings.push(format!(
                            "tool_event '{e}' is not wired: the shipped settings.json runs check-tool.sh on {} only",
                            lane.wired_events.join(", ")
                        ));
                    }
                }
            }
        }
        if tool_name.is_none() {
            warnings.push("tool_event without tool_name: has no effect".to_string());
        }
    }

    if let Some(pattern) = tool_name {
        match crate::cmd::scan::compile_trigger(&pattern) {
            None => errors.push(format!("tool_name '{pattern}' is not a valid regex")),
            Some(re) if !lane.wired_names.is_empty() && !lane.wired_names.iter().any(|t| re.is_match(t)) => {
                warnings.push(format!(
                    "tool_name '{pattern}' matches none of the wired tools ({}): the shipped settings.json \
                     matcher sends no other tool to the lane, so this way never fires there",
                    lane.wired_names.join(", ")
                ))
            }
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

    fn run(fm: &str) -> (Vec<String>, Vec<String>) {
        let valid = strings(&["PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "PermissionDenied"]);
        let wired = strings(&["PreToolUse", "PostToolUse"]);
        let names = strings(&["EnterPlanMode", "ExitPlanMode", "Workflow"]);
        let lane = ToolLane { valid_events: &valid, wired_events: &wired, wired_names: &names };
        check(&lane, fm)
    }

    #[test]
    fn a_wired_name_and_wired_events_lint_clean() {
        assert_eq!(run("tool_name: ^exitplanmode$\ntool_event: [PreToolUse, PostToolUse]\n"), (vec![], vec![]));
        assert_eq!(run("tool_name: Workflow\n"), (vec![], vec![]));
    }

    #[test]
    fn a_name_no_wired_tool_matches_warns_with_the_wired_set() {
        let (errors, warnings) = run("tool_name: ^Read$\n");
        assert!(errors.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("EnterPlanMode, ExitPlanMode, Workflow"), "{warnings:?}");
    }

    #[test]
    fn an_invalid_regex_is_an_error() {
        let (errors, _) = run("tool_name: (Workflow\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
    }

    #[test]
    fn events_outside_the_five_are_errors_and_unwired_ones_warn() {
        let (errors, warnings) = run("tool_name: Workflow\ntool_event: [PostToolUse, posttooluse, PermissionRequest]\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("'posttooluse'"));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("'PermissionRequest'") && warnings[0].contains("PreToolUse, PostToolUse"));
    }

    #[test]
    fn an_empty_list_is_an_error_and_events_alone_warn() {
        assert_eq!(run("tool_name: Workflow\ntool_event: []\n").0.len(), 1);
        let (errors, warnings) = run("tool_event: PostToolUse\n");
        assert!(errors.is_empty());
        assert_eq!(warnings, ["tool_event without tool_name: has no effect"]);
    }

    #[test]
    fn a_block_list_is_an_error() {
        let (errors, warnings) = run("tool_name: ExitPlanMode\ntool_event:\n  - PostToolUse\nrefire: 0.15\n");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("one-line list"), "{errors:?}");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn an_older_schema_without_the_lists_skips_their_rules() {
        let lane = ToolLane { valid_events: &[], wired_events: &[], wired_names: &[] };
        assert_eq!(check(&lane, "tool_name: ^Read$\ntool_event: [PermissionRequest]\n"), (vec![], vec![]));
    }
}
