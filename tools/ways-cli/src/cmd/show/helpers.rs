//! Content rendering utilities — pure functions for file processing.

use std::path::Path;
use std::process::Command;

fn body_lines(content: &str) -> Vec<&str> {
    ways_core::frontmatter::split(content).map_or_else(Vec::new, |(_, body)| body.lines().collect())
}

/// Return check file sections (anchor and/or check).
pub(crate) fn check_sections_text(content: &str, include_anchor: bool) -> String {
    let mut section = String::new();
    let mut lines = Vec::new();

    for line in body_lines(content) {
        if line.starts_with("## anchor") {
            section = "anchor".to_string();
            continue;
        }
        if line.starts_with("## check") {
            section = "check".to_string();
            continue;
        }
        if line.starts_with("## ") {
            section = "other".to_string();
            continue;
        }

        if section == "check" || (section == "anchor" && include_anchor) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Execute a macro shell script and return its stdout.
///
/// `CLAUDE_SESSION_ID` is exported into the child so a macro can find its own
/// session state. Without it a macro has no way to identify the session it is
/// running for — the hook payload never reaches it (no stdin, no args) — and
/// guessing from directory mtimes picks the wrong session whenever more than one
/// is active.
pub(crate) fn run_macro(macro_file: &Path, session_id: &str) -> Option<String> {
    let output = Command::new("bash")
        .arg(macro_file)
        .env("CLAUDE_SESSION_ID", session_id)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    } else {
        None
    }
}

/// Check whether a project directory is in the trusted-project-macros list.
pub(crate) fn is_project_trusted(project_dir: &str) -> bool {
    let trust_file = crate::paths::trusted_project_macros();
    if let Ok(content) = std::fs::read_to_string(&trust_file) {
        content.lines().any(|line| line.trim() == project_dir)
    } else {
        false
    }
}

/// Extract attend signal types from frontmatter.
/// Looks for `type: attend` and collects `signals:` list items.
pub(crate) fn extract_attend_signals(content: &str) -> Vec<String> {
    let Some((fm, _)) = ways_core::frontmatter::split(content) else {
        return Vec::new();
    };
    let mut has_attend_type = false;
    let mut in_signals = false;
    let mut signals = Vec::new();

    for line in fm.lines() {
        let trimmed = line.trim();

        if trimmed == "type: attend" {
            has_attend_type = true;
        }

        if trimmed == "signals:" {
            in_signals = true;
            continue;
        }

        if in_signals {
            if let Some(signal) = trimmed.strip_prefix("- ") {
                signals.push(signal.trim().to_string());
            } else {
                in_signals = false;
            }
        }
    }

    if has_attend_type { signals } else { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_attend_signals_basic() {
        let content = "---\ntrigger:\n  type: attend\n  signals:\n    - context-pressure\n    - reflection-overdue\n---\nBody text.";
        let signals = extract_attend_signals(content);
        assert_eq!(signals, vec!["context-pressure", "reflection-overdue"]);
    }

    #[test]
    fn extract_attend_signals_not_attend() {
        let content = "---\ndescription: normal way\nvocabulary: test\n---\nBody.";
        let signals = extract_attend_signals(content);
        assert!(signals.is_empty());
    }

    #[test]
    fn extract_attend_signals_no_frontmatter() {
        let content = "Just a plain file.";
        let signals = extract_attend_signals(content);
        assert!(signals.is_empty());
    }

    #[test]
    fn extract_attend_signals_type_without_signals() {
        let content = "---\ntrigger:\n  type: attend\n---\nBody.";
        let signals = extract_attend_signals(content);
        assert!(signals.is_empty());
    }

    #[test]
    fn check_sections_keep_horizontal_rules() {
        let content = "---\ndescription: d\n---\n## anchor\nA\n## check\nfirst\n---\nsecond\n";
        assert_eq!(check_sections_text(content, false), "first\n---\nsecond");
        assert_eq!(check_sections_text(content, true), "A\nfirst\n---\nsecond");
    }
}
