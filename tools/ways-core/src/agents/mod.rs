//! Agent-specific configuration readers.
//!
//! Each supported agent gets its own module. The trait provides a uniform
//! interface so ways can query agent config without knowing which agent
//! is active.

pub mod claude_code;
pub mod system;

/// Trait for reading agent-specific configuration.
pub trait AgentConfig {
    /// The agent's configured output language (e.g., "japanese", "en", "ja").
    /// Returns None if the agent has no language preference set.
    fn language(&self) -> Option<String>;
}

/// Resolve output language with cascade:
///   1. Config language field (from ways.json / XDG config / project overlay)
///   2. Active agent's language setting
///   3. System locale ($LANG)
///   4. "en" fallback
///
/// config::global() — future migration: ctx.config.language
pub fn resolve_language() -> String {
    // 1. Config (layered: ways.json → XDG → project)
    let cfg_lang = &crate::config::global().language;
    if cfg_lang != "auto" {
        return normalize_language(cfg_lang);
    }

    // 2. Agent config (currently only Claude Code)
    let agent = claude_code::ClaudeCode;
    if let Some(lang) = agent.language() {
        return normalize_language(&lang);
    }

    // 3. System locale
    if let Some(lang) = system::locale_language() {
        return lang;
    }

    // 4. Fallback
    "en".to_string()
}

/// Language config embedded at compile time from languages.json.
pub const LANGUAGES_JSON: &str = include_str!("../../languages.json");

/// Normalize language input to the display name from languages.json.
/// Accepts codes ("ja"), English names ("japanese"), native names ("日本語").
/// Passes through unknown values as-is.
fn normalize_language(input: &str) -> String {
    let lower = input.to_lowercase();

    // Parse the languages config
    let parsed: serde_json::Value = match serde_json::from_str(LANGUAGES_JSON) {
        Ok(v) => v,
        Err(_) => return input.to_string(),
    };
    let languages = match parsed.get("languages").and_then(|v| v.as_object()) {
        Some(m) => m,
        None => return input.to_string(),
    };

    // Direct code match (e.g., "ja" → "Japanese")
    if let Some(entry) = languages.get(&lower) {
        if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
            // Return "en" for English (keeps the short form for default)
            return if name == "English" { "en".to_string() } else { name.to_string() };
        }
    }

    // Search by English name or native name
    for (_code, entry) in languages {
        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let native = entry.get("native").and_then(|v| v.as_str()).unwrap_or("");
        if name.to_lowercase() == lower || native == input {
            return if name == "English" { "en".to_string() } else { name.to_string() };
        }
    }

    // Unknown — pass through as-is
    input.to_string()
}

/// Check whether a language code is marked `active: true` in languages.json.
pub fn is_language_active(lang_code: &str) -> bool {
    let parsed: serde_json::Value = match serde_json::from_str(LANGUAGES_JSON) {
        Ok(v) => v,
        Err(_) => return false,
    };
    parsed
        .get("languages")
        .and_then(|v| v.get(lang_code))
        .and_then(|v| v.get("active"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Return all language codes marked `active: true`, sorted.
pub fn get_active_languages() -> Vec<String> {
    let parsed: serde_json::Value = match serde_json::from_str(LANGUAGES_JSON) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut codes: Vec<String> = parsed
        .get("languages")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter(|(_, v)| v.get("active").and_then(|a| a.as_bool()).unwrap_or(false))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    codes.sort();
    codes
}

/// Best-effort language name → code lookup (e.g., "Japanese" → "ja").
/// Returns the input unchanged if it's already a short code.
pub fn resolve_to_lang_code(lang: &str) -> String {
    let lower = lang.to_lowercase();
    if lower.len() <= 5 && lower.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        return lower;
    }
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(LANGUAGES_JSON) {
        if let Some(languages) = parsed.get("languages").and_then(|v| v.as_object()) {
            for (code, entry) in languages {
                let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if name.to_lowercase() == lower {
                    return code.clone();
                }
            }
        }
    }
    "en".to_string()
}

