//! Shared utility functions used across multiple modules.

use std::path::{Path, PathBuf};

/// XDG cache directory ($XDG_CACHE_HOME or ~/.cache).
pub fn xdg_cache_dir() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".cache"))
}

/// Home directory from $HOME, falling back to /tmp.
pub fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Detect the project root by walking up from cwd looking for .claude/settings.json or CLAUDE.md.
pub fn detect_project_dir() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        let claude_dir = dir.join(".claude");
        if claude_dir.is_dir()
            && (claude_dir.join("settings.json").exists()
                || dir.join("CLAUDE.md").exists()
                || claude_dir.join("settings.local.json").exists())
        {
            return Some(dir.to_string_lossy().to_string());
        }
        dir = dir.parent()?;
    }
}

/// Load excluded path segments from frontmatter-schema.yaml.
/// Returns empty vec if schema can't be read (non-fatal).
pub fn load_excluded_segments() -> Vec<String> {
    let schema_path = home_dir().join(".claude/hooks/ways/frontmatter-schema.yaml");
    let content = match std::fs::read_to_string(&schema_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let doc: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    doc.get("lint")
        .and_then(|v| v.get("excluded_path_segments"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract a locale code from a filename like "security.ja.md" → Some("ja").
/// Validates against languages.json to avoid false matches (e.g., "foo.setup.md").
pub fn extract_locale_from_filename(filename: &str) -> Option<String> {
    if filename.contains(".check.") {
        return None;
    }
    let stem = filename.strip_suffix(".md")?;
    let parts: Vec<&str> = stem.split('.').collect();
    if parts.len() >= 2 {
        let candidate = parts[parts.len() - 1];
        if candidate.len() >= 2
            && candidate.len() <= 5
            && candidate.chars().all(|c| c.is_ascii_lowercase() || c == '-')
        {
            // Validate against languages.json (active languages only)
            if crate::agents::is_language_active(candidate) {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

/// Check if a path should be excluded based on schema-defined segments.
pub fn is_excluded_path(path: &Path, excluded_segments: &[String]) -> bool {
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return false,
    };
    for segment in excluded_segments {
        if path_str.contains(segment.as_str()) {
            return true;
        }
    }
    // Timestamp filenames from sync tools (e.g., 2026-03-30T13_13_26.616Z.Desktop.md)
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        let stem = stem.strip_suffix(".check").unwrap_or(stem);
        if stem.starts_with("20") && stem.contains('T') && stem.contains('.') {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_locale_codes() {
        assert_eq!(extract_locale_from_filename("security.ja.md"), Some("ja".to_string()));
        assert_eq!(extract_locale_from_filename("security.de.md"), Some("de".to_string()));
        assert_eq!(extract_locale_from_filename("security.ar.md"), Some("ar".to_string()));
        assert_eq!(extract_locale_from_filename("security.es.md"), Some("es".to_string()));
        assert_eq!(extract_locale_from_filename("security.pt-br.md"), Some("pt-br".to_string()));
    }

    #[test]
    fn rejects_inactive_locale_codes() {
        // zh-tw is in languages.json but inactive
        assert_eq!(extract_locale_from_filename("security.zh-tw.md"), None);
        // bg is in languages.json but inactive
        assert_eq!(extract_locale_from_filename("security.bg.md"), None);
    }

    #[test]
    fn rejects_non_locale_dotted_names() {
        // "setup" is not a language code
        assert_eq!(extract_locale_from_filename("foo.setup.md"), None);
        // "test" is not a language code
        assert_eq!(extract_locale_from_filename("bar.test.md"), None);
        // "main" is not a language code
        assert_eq!(extract_locale_from_filename("way.main.md"), None);
    }

    #[test]
    fn rejects_check_files() {
        assert_eq!(extract_locale_from_filename("security.check.md"), None);
        assert_eq!(extract_locale_from_filename("security.ja.check.md"), None);
    }

    #[test]
    fn rejects_non_md_extensions() {
        assert_eq!(extract_locale_from_filename("security.ja.yaml"), None);
        assert_eq!(extract_locale_from_filename("security.ja.sh"), None);
    }

    #[test]
    fn rejects_plain_way_files() {
        // No dot-separated locale segment
        assert_eq!(extract_locale_from_filename("security.md"), None);
        assert_eq!(extract_locale_from_filename("briefing.md"), None);
    }

    #[test]
    fn rejects_uppercase_and_numbers() {
        assert_eq!(extract_locale_from_filename("way.EN.md"), None);
        assert_eq!(extract_locale_from_filename("way.j2.md"), None);
    }

    #[test]
    fn handles_deeply_dotted_names() {
        // Last segment is the locale candidate
        assert_eq!(extract_locale_from_filename("some.way.name.ja.md"), Some("ja".to_string()));
    }
}
