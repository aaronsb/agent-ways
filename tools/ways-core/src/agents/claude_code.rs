//! Claude Code agent configuration reader.
//!
//! Reads from:
//!   - ~/.claude/settings.json (user settings)
//!   - .claude/settings.json (project settings)
//!   - ~/.claude.json (global app config)

use super::AgentConfig;
use crate::util::home_dir;

pub struct ClaudeCode;

impl AgentConfig for ClaudeCode {
    fn language(&self) -> Option<String> {
        // Project settings take precedence over user settings
        if let Some(lang) = read_project_settings_language() {
            return Some(lang);
        }
        if let Some(lang) = read_user_settings_language() {
            return Some(lang);
        }
        None
    }
}

/// Read language from ~/.claude/settings.json
fn read_user_settings_language() -> Option<String> {
    let path = home_dir().join(".claude/settings.json");
    read_language_from_json(&path)
}

/// Read language from $CLAUDE_PROJECT_DIR/.claude/settings.json
fn read_project_settings_language() -> Option<String> {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR").ok()?;
    let path = std::path::PathBuf::from(project_dir).join(".claude/settings.json");
    read_language_from_json(&path)
}

fn read_language_from_json(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed
        .get("language")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}
