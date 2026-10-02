//! Shared utility functions used across multiple modules.

use std::path::{Path, PathBuf};

/// Normalize path separators to the OS-native separator.
///
/// On Windows, PathBuf::join stores forward slashes verbatim when the join
/// argument contains them (e.g. join("foo/bar") stores "foo/bar" not "foo\bar").
/// Subprocesses receiving mixed-separator paths can fail (e.g. on atomic rename).
/// Rebuilding via components() normalizes to the OS separator on all platforms.
pub fn normalize_path_sep(path: &Path) -> PathBuf {
    path.components().collect()
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

/// Home directory, separator-normalised: [`claude_sessions::home_dir`]
/// (USERPROFILE first on Windows, else $HOME, else /tmp; an empty value is
/// unset), with its components rebuilt by [`normalize_path_sep`].
///
/// On Windows, $HOME is often set by Git Bash to a Unix-style path like /c/Users/name,
/// which Rust's PathBuf treats as root-relative (\c\Users\name) rather than C:\Users\name.
/// USERPROFILE is always the correct Windows absolute path, so it comes first there.
pub fn home_dir() -> PathBuf {
    normalize_path_sep(&claude_sessions::home_dir())
}

/// `CLAUDE_PROJECT_DIR` when it is set and not empty. The one read of that
/// variable: an empty value (a hook that exported an empty cwd) counts as
/// unset everywhere.
pub fn env_project_dir() -> Option<String> {
    std::env::var("CLAUDE_PROJECT_DIR").ok().filter(|s| !s.is_empty())
}

/// The directory a command acts on: `CLAUDE_PROJECT_DIR` when set, else
/// `$PWD` when it names the current directory (bash's rule, which keeps the
/// logical path through a symlink), else the current directory, else `.`.
pub fn project_dir() -> String {
    project_dir_from(
        env_project_dir(),
        std::env::var("PWD").ok(),
        std::env::current_dir().ok(),
    )
}

fn project_dir_from(env: Option<String>, pwd: Option<String>, cwd: Option<PathBuf>) -> String {
    if let Some(env) = env {
        return env;
    }
    let pwd = pwd.filter(|s| !s.is_empty());
    match (pwd, cwd) {
        (Some(pwd), Some(cwd)) if same_dir(Path::new(&pwd), &cwd) => pwd,
        (_, Some(cwd)) => cwd.to_string_lossy().into_owned(),
        // No current directory to check against: $PWD is all there is.
        (Some(pwd), None) => pwd,
        (None, None) => ".".to_string(),
    }
}

/// Whether two paths name the same directory: device and inode on Unix,
/// canonical paths elsewhere.
fn same_dir(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
    }
}

/// Whether a recorded project path is in the project `scope`: the same path,
/// or a path under it, trailing separators aside. A worktree under a project,
/// such as an agent's in `.claude/worktrees/`, is part of it; a sibling whose
/// name only begins the same (`/a/foo-bar` for `/a/foo`) is not. `/` and `\`
/// both separate, since Windows records backslash paths. An empty scope is no
/// project and holds nothing.
pub fn in_project(project: &str, scope: &str) -> bool {
    const SEP: [char; 2] = ['/', '\\'];
    if scope.is_empty() {
        return false;
    }
    let (p, s) = (project.trim_end_matches(SEP), scope.trim_end_matches(SEP));
    p == s || p.strip_prefix(s).is_some_and(|rest| rest.starts_with(SEP))
}

/// A `--project` path as events record it: a relative one, such as `.`,
/// resolved against the working directory.
pub fn project_arg(path: &str) -> String {
    let p = Path::new(path);
    // A rooted path stands as given: `/x` has no drive on Windows, yet names
    // no working directory either.
    if p.is_absolute() || p.has_root() {
        return path.to_string();
    }
    // On Windows `absolute` normalizes `..` and gives the `C:\...` form events
    // record, where `canonicalize` would give a `\\?\` verbatim path; on Unix
    // `absolute` keeps `..`, so the real path is taken.
    #[cfg(windows)]
    let resolved = std::path::absolute(p);
    #[cfg(not(windows))]
    let resolved = std::fs::canonicalize(p).or_else(|_| std::path::absolute(p));
    resolved.map(|a| a.to_string_lossy().into_owned()).unwrap_or_else(|_| path.to_string())
}

/// The project a command scopes to: `CLAUDE_PROJECT_DIR` when set, else the
/// project enclosing the current directory ([`detect_project_dir`]). `None`
/// outside any project.
pub fn project_root() -> Option<String> {
    env_project_dir().or_else(detect_project_dir)
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
    let schema_path = crate::paths::projected_ways_root().join("frontmatter-schema.yaml");
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

    /// `$PWD` stands only when it names the current directory (bash's rule);
    /// a stale one inherited from a non-shell parent gives way to the real cwd.
    #[test]
    fn project_dir_ignores_a_stale_pwd() {
        let cwd = std::env::temp_dir().join(format!("ways-pwd-{}", std::process::id()));
        std::fs::create_dir_all(&cwd).unwrap();
        let stale = Some("/nonexistent/stale/pwd".to_string());
        assert_eq!(project_dir_from(None, stale, Some(cwd.clone())), cwd.to_string_lossy());
        #[cfg(unix)]
        {
            let link = std::env::temp_dir().join(format!("ways-pwd-link-{}", std::process::id()));
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&cwd, &link).unwrap();
            let logical = link.to_string_lossy().into_owned();
            assert_eq!(project_dir_from(None, Some(logical.clone()), Some(cwd.clone())), logical);
            let _ = std::fs::remove_file(&link);
        }
        let _ = std::fs::remove_dir_all(&cwd);
    }

    /// An empty `CLAUDE_PROJECT_DIR` is unset. Most readers used to take it as
    /// the project, so a hook that exported an empty cwd scoped to "".
    #[test]
    fn project_dir_treats_an_empty_env_as_unset() {
        let pwd = Some("/work/p".to_string());
        assert_eq!(project_dir_from(None, pwd.clone(), None), "/work/p");
        assert_eq!(project_dir_from(Some("/env/p".into()), pwd.clone(), None), "/env/p");
        assert_eq!(project_dir_from(None, Some(String::new()), Some(PathBuf::from("/cwd"))), "/cwd");
        assert_eq!(project_dir_from(None, None, None), ".");
    }

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

    #[test]
    fn in_project_takes_the_project_and_what_is_under_it_on_either_separator() {
        assert!(in_project("/a/proj", "/a/proj/"));
        assert!(in_project("/a/proj/.claude/worktrees/x", "/a/proj"));
        assert!(!in_project("/a/proj-2", "/a/proj"));
        assert!(in_project(r"C:\a\proj\.claude\worktrees\x", r"C:\a\proj"));
        assert!(in_project(r"C:\a\proj", r"C:\a\proj\"));
        assert!(!in_project(r"C:\a\proj-2", r"C:\a\proj"));
        assert!(in_project("/a/proj", "/"), "the root holds every path");
        assert!(!in_project("/a/proj", ""), "an empty scope holds nothing");
    }

    #[test]
    fn a_relative_project_resolves_to_the_working_directory() {
        let cwd = std::env::current_dir().unwrap();
        let resolved = super::project_arg(".");
        assert!(!resolved.starts_with(r"\\?\"), "no verbatim prefix: {resolved}");
        assert!(in_project(&cwd.to_string_lossy(), &resolved) || in_project(&std::fs::canonicalize(&cwd).unwrap().to_string_lossy(), &resolved), "{resolved}");
        assert_eq!(super::project_arg("/abs/p"), "/abs/p");
    }
}
