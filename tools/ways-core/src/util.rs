//! Shared utility functions used across multiple modules.

use std::path::{Path, PathBuf};

/// XDG cache directory ($XDG_CACHE_HOME or ~/.cache).
pub fn xdg_cache_dir() -> PathBuf {
    let p = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".cache"));
    normalize_path_sep(&p)
}

/// Normalize path separators to the OS-native separator.
///
/// On Windows, PathBuf::join stores forward slashes verbatim when the join
/// argument contains them (e.g. join("foo/bar") stores "foo/bar" not "foo\bar").
/// Subprocesses receiving mixed-separator paths can fail (e.g. on atomic rename).
/// Rebuilding via components() normalizes to the OS separator on all platforms.
pub fn normalize_path_sep(path: &Path) -> PathBuf {
    path.components().collect()
}

/// True if `content` opens with a `---` YAML frontmatter delimiter.
///
/// Uses `lines()` (which strips a trailing `\r`) so a way authored on Windows
/// with CRLF endings is recognized. A hard `content.starts_with("---\n")` check
/// fails on `---\r\n` and silently drops the file — on the scan/resolve path
/// that means the way never matches or renders. Every frontmatter gate routes
/// through here so the behavior is uniform across platforms.
pub fn has_frontmatter(content: &str) -> bool {
    content.lines().next() == Some("---")
}

/// Join a path's components with '/' regardless of OS separator.
///
/// Way IDs are a stable, cross-platform namespace: a way at
/// `softwaredev/code/quality.md` has id `softwaredev/code` on every OS. Using
/// `Path::display()` would leak backslashes on Windows, so corpus IDs and scan
/// candidate IDs would silently never match. Both sides must route through here.
pub fn path_to_id(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Encode a real project path into the namespace key that prefixes
/// project-local way IDs in the corpus.
///
/// Computed from the REAL path — not the lossy `~/.claude/projects/` encoded
/// dir name — so the corpus side (CLAUDE_PROJECT_DIR / a resolved project path)
/// and the scan side (`--project`) produce an identical key for the same
/// project. The result is a flat token (every separator and ':' becomes '-'),
/// so the only '/' in the resulting `{key}/{bare_id}` corpus id is the boundary
/// between the namespace key and the bare way id.
///
/// Canonicalize is authoritative (resolves symlinks, case, and trailing
/// components); the lexical fallback keeps the key stable when the path does not
/// exist on disk. Both call sites apply this identical rule.
pub fn encode_project_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let normalized = normalize_path_sep(&resolved);
    let mut s = normalized.to_string_lossy().into_owned();

    // Strip the verbatim prefixes canonicalize() adds on Windows.
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        s = format!(r"\\{rest}");
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }

    #[cfg(windows)]
    {
        s = s.to_lowercase();
    }

    s.chars()
        .map(|c| if c == '\\' || c == '/' || c == ':' { '-' } else { c })
        .collect()
}

/// Home directory from $HOME (or USERPROFILE on Windows), falling back to /tmp.
///
/// On Windows, $HOME is often set by Git Bash to a Unix-style path like /c/Users/name,
/// which Rust's PathBuf treats as root-relative (\c\Users\name) rather than C:\Users\name.
/// USERPROFILE is always the correct Windows absolute path, so we prefer it on Windows.
pub fn home_dir() -> PathBuf {
    let p = {
        #[cfg(windows)]
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return normalize_path_sep(&PathBuf::from(profile));
        }
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
    };
    normalize_path_sep(&p)
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

    #[test]
    fn has_frontmatter_tolerates_crlf() {
        assert!(has_frontmatter("---\ndescription: x\n---\n"));
        assert!(has_frontmatter("---\r\ndescription: x\r\n---\r\n"));
        assert!(!has_frontmatter("no frontmatter\n"));
        assert!(!has_frontmatter(""));
    }

    #[test]
    fn path_to_id_uses_forward_slashes() {
        assert_eq!(path_to_id(Path::new("softwaredev/code")), "softwaredev/code");
        // A relative path built from OS-native parts still joins with '/'.
        let p: PathBuf = ["softwaredev", "code", "quality"].iter().collect();
        assert_eq!(path_to_id(&p), "softwaredev/code/quality");
        assert_eq!(path_to_id(Path::new("")), "");
    }

    #[test]
    fn encode_project_key_is_a_flat_token() {
        // No path separators or ':' survive — exactly one boundary later when
        // joined with a bare id.
        let key = encode_project_key(Path::new("/nonexistent/proj/sub"));
        assert!(!key.contains('/'), "key must be flat: {key}");
        assert!(!key.contains('\\'), "key must be flat: {key}");
        assert!(!key.contains(':'), "key must be flat: {key}");
    }

    #[test]
    fn encode_project_key_ignores_trailing_slash_and_dot() {
        // Lexical fallback (paths don't exist) must normalize trailing slash and
        // '.' so corpus-time and scan-time keys agree.
        let a = encode_project_key(Path::new("/nonexistent/proj"));
        let b = encode_project_key(Path::new("/nonexistent/proj/"));
        let c = encode_project_key(Path::new("/nonexistent/proj/."));
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn encode_project_key_matches_for_existing_dir() {
        // The contract that makes Bug B fix work: corpus-time and scan-time both
        // canonicalize the same real dir to the same key.
        let dir = std::env::temp_dir();
        assert_eq!(encode_project_key(&dir), encode_project_key(&dir));
    }
}
