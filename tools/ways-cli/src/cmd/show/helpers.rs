//! Content rendering utilities — pure functions for file processing.

use std::path::Path;
use std::process::Command;

/// Extract a YAML frontmatter field value by name.
pub(crate) fn extract_field(content: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    let mut in_fm = false;
    for (i, line) in content.lines().enumerate() {
        if i == 0 && line == "---" {
            in_fm = true;
            continue;
        }
        if in_fm {
            if line == "---" {
                return None;
            }
            if let Some(val) = line.strip_prefix(&prefix) {
                let val = val.trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Return markdown body (everything after YAML frontmatter).
pub(crate) fn body_text(content: &str) -> String {
    let mut fm_count = 0;
    let mut lines = Vec::new();
    for line in content.lines() {
        if line == "---" {
            fm_count += 1;
            continue;
        }
        if fm_count >= 2 {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// Return check file sections (anchor and/or check).
pub(crate) fn check_sections_text(content: &str, include_anchor: bool) -> String {
    let mut fm_count = 0;
    let mut section = String::new();
    let mut lines = Vec::new();

    for line in content.lines() {
        if line == "---" {
            fm_count += 1;
            continue;
        }
        if fm_count < 2 {
            continue;
        }

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
    let trust_file = home_dir().join(".claude/trusted-project-macros");
    if let Ok(content) = std::fs::read_to_string(&trust_file) {
        content.lines().any(|line| line.trim() == project_dir)
    } else {
        false
    }
}

/// Extract attend signal types from frontmatter.
/// Looks for `type: attend` and collects `signals:` list items.
pub(crate) fn extract_attend_signals(content: &str) -> Vec<String> {
    let mut in_fm = false;
    let mut has_attend_type = false;
    let mut in_signals = false;
    let mut signals = Vec::new();

    for (i, line) in content.lines().enumerate() {
        if i == 0 && line == "---" {
            in_fm = true;
            continue;
        }
        if in_fm && line == "---" {
            break;
        }
        if !in_fm {
            continue;
        }

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

pub(crate) use crate::util::home_dir;

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
}
