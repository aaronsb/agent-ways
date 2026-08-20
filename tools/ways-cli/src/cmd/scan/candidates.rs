//! Candidate collection: finding, parsing, and filtering way files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::session;

use super::WayCandidate;

// ── Collection ─────────────────────────────────────────────────

pub(crate) fn collect_candidates(project_dir: &str) -> Vec<WayCandidate> {
    let mut candidates = Vec::new();

    // Full ADR-143 precedence, by bare id: project > user > core. A higher root
    // shadows a same-named way in every lower root, so what *fires* matches what
    // `resolve_way_file` *renders* (no match/render divergence). `seen`
    // accumulates the claimed bare ids down the chain.
    let mut seen: HashSet<String> = HashSet::new();
    let roots = WayRoots::resolve(project_dir);
    // Canonical paths already collected, across every root (see `WayRoots`).
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();

    // Project-local first. The corpus namespaces project ways as
    // `{encode_project_key(project_dir)}/{id}`; we compute the identical prefix
    // here so the embedding lookup (best_en/best_multi) finds them (Bug B fix).
    if roots.project.is_dir() {
        let prefix = project_corpus_prefix(project_dir);
        collect_from_dir(&roots.project, &prefix, &mut candidates, &HashSet::new(), &mut seen, &mut seen_paths, &roots.foreign_to(&roots.project));
    }

    // User ways ($XDG_CONFIG/agent-ways/ways) — bare ids; drop any project shadows.
    if roots.user.is_dir() {
        let claimed = seen.clone();
        collect_from_dir(&roots.user, "", &mut candidates, &claimed, &mut seen, &mut seen_paths, &roots.foreign_to(&roots.user));
    }

    // Core ways — bare ids; drop any project- or user-claimed id.
    let claimed = seen.clone();
    collect_from_dir(&roots.core, "", &mut candidates, &claimed, &mut seen, &mut seen_paths, &roots.foreign_to(&roots.core));

    candidates
}

/// The three way roots and their canonical forms.
///
/// The walk follows symlinks. A link inside one root that points at another
/// root — or back at its own root — would otherwise yield the same `.md` a
/// second time under a different id (`ways/softwaredev/...` beside
/// `softwaredev/...`), and both copies would fire, render, and stamp their
/// own markers. Two rules close that: a file that resolves inside a *different*
/// root is skipped here (that root collects it under its proper id), and a file
/// that resolves inside *this* root takes its id from the canonical path, so the
/// second sighting collapses onto the first by path. A link that resolves
/// outside every root (a dotfiles checkout, say) keeps its walked id as before.
struct WayRoots {
    project: PathBuf,
    user: PathBuf,
    core: PathBuf,
}

impl WayRoots {
    fn resolve(project_dir: &str) -> Self {
        Self {
            project: PathBuf::from(project_dir).join(".claude/ways"),
            user: crate::paths::user_ways_root(),
            core: super::scoring::home_dir().join(".claude/hooks/ways"),
        }
    }

    /// Canonical paths of every root other than `own` (existing ones only).
    fn foreign_to(&self, own: &Path) -> Vec<PathBuf> {
        [&self.project, &self.user, &self.core]
            .into_iter()
            .filter(|r| *r != own && r.is_dir())
            .map(|r| canonical(r))
            .collect()
    }
}

pub(crate) fn collect_checks(project_dir: &str) -> Vec<WayCandidate> {
    let mut candidates = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let roots = WayRoots::resolve(project_dir);
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();

    if roots.project.is_dir() {
        let prefix = project_corpus_prefix(project_dir);
        collect_checks_from_dir(&roots.project, &prefix, &mut candidates, &HashSet::new(), &mut seen, &mut seen_paths, &roots.foreign_to(&roots.project));
    }

    if roots.user.is_dir() {
        let claimed = seen.clone();
        collect_checks_from_dir(&roots.user, "", &mut candidates, &claimed, &mut seen, &mut seen_paths, &roots.foreign_to(&roots.user));
    }

    let claimed = seen.clone();
    collect_checks_from_dir(&roots.core, "", &mut candidates, &claimed, &mut seen, &mut seen_paths, &roots.foreign_to(&roots.core));

    candidates
}

/// Bare ids of all ways under `root` (dirs holding a frontmatter `.md`).
///
/// The user-shadows-core dedup key, shared with the corpus builder so the two
/// enumerations agree. Deliberately by-directory (any user way, semantic or
/// pattern-only, shadows a same-named core way) — this is what makes a
/// non-semantic user override actually suppress the core way for firing.
pub(crate) fn way_ids(root: &Path) -> HashSet<String> {
    let mut ids = HashSet::new();
    if !root.is_dir() {
        return ids;
    }
    for entry in WalkDir::new(root).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.contains(".check.") {
            continue;
        }
        match std::fs::read_to_string(path) {
            Ok(c) if crate::util::has_frontmatter(&c) => {}
            _ => continue,
        }
        let id = way_id_from_path(path, root);
        if !id.is_empty() {
            ids.insert(id);
        }
    }
    ids
}

/// The corpus-id prefix for project-local ways: `{key}/`, where `key` is the
/// project root's namespace key. Mirrors `ways corpus` exactly.
fn project_corpus_prefix(project_dir: &str) -> String {
    format!("{}/", crate::util::encode_project_key(Path::new(project_dir)))
}

fn collect_from_dir(
    dir: &Path,
    corpus_prefix: &str,
    out: &mut Vec<WayCandidate>,
    skip: &HashSet<String>,
    written: &mut HashSet<String>,
    seen_paths: &mut HashSet<PathBuf>,
    foreign_roots: &[PathBuf],
) {
    let root_canon = canonical(dir);
    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.contains(".check.") {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !crate::util::has_frontmatter(&content) {
            continue;
        }

        let id = match way_identity(path, dir, &root_canon, seen_paths, foreign_roots) {
            Some(id) => id,
            None => continue,
        };

        // Dedup-by-name (ADR-143): a higher-precedence root already claimed this
        // id, so the shadowed candidate is dropped before it can fire.
        if skip.contains(&id) {
            continue;
        }

        // Check domain disable (user scope) and per-way disable (project scope, ADR-131)
        let domain = id.split('/').next().unwrap_or(&id);
        if session::domain_disabled(domain) || session::way_disabled(&id) {
            continue;
        }

        if let Some(candidate) = parse_candidate(&id, corpus_prefix, path, &content) {
            written.insert(id.clone());
            out.push(candidate);
        }
    }
}

fn collect_checks_from_dir(
    dir: &Path,
    corpus_prefix: &str,
    out: &mut Vec<WayCandidate>,
    skip: &HashSet<String>,
    written: &mut HashSet<String>,
    seen_paths: &mut HashSet<PathBuf>,
    foreign_roots: &[PathBuf],
) {
    let root_canon = canonical(dir);
    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.contains(".check.md") {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !crate::util::has_frontmatter(&content) {
            continue;
        }

        let id = match way_identity(path, dir, &root_canon, seen_paths, foreign_roots) {
            Some(id) => id,
            None => continue,
        };

        if skip.contains(&id) {
            continue;
        }

        if let Some(candidate) = parse_candidate(&id, corpus_prefix, path, &content) {
            written.insert(id.clone());
            out.push(candidate);
        }
    }
}

/// Resolved identity of a way file for cross-root dedup. Falls back to the
/// walked path when the filesystem can't resolve it (then nothing is deduped,
/// which is the pre-existing behaviour).
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Bare id for a walked way file, or `None` when the file was already collected
/// (by canonical path), resolves inside another root, or has no id. See
/// `WayRoots` for the rules.
fn way_identity(
    path: &Path,
    dir: &Path,
    root_canon: &Path,
    seen_paths: &mut HashSet<PathBuf>,
    foreign_roots: &[PathBuf],
) -> Option<String> {
    let canon = canonical(path);
    if foreign_roots.iter().any(|r| canon.starts_with(r)) {
        return None;
    }
    if !seen_paths.insert(canon.clone()) {
        return None;
    }
    let id = if canon.starts_with(root_canon) {
        way_id_from_path(&canon, root_canon)
    } else {
        way_id_from_path(path, dir)
    };
    (!id.is_empty()).then_some(id)
}

// ── Parsing ────────────────────────────────────────────────────

fn parse_candidate(id: &str, corpus_prefix: &str, path: &Path, content: &str) -> Option<WayCandidate> {
    let fm = extract_frontmatter(content)?;

    Some(WayCandidate {
        id: id.to_string(),
        // Bare id keeps driving session markers, show, and parent-boost; the
        // corpus_id (prefixed for project ways) is used only for embedding lookup.
        corpus_id: format!("{corpus_prefix}{id}"),
        path: path.to_path_buf(),
        pattern: get_fm_field(&fm, "pattern"),
        // Lenient boolean: authors write `true`/`True`, and a quoted "true"
        // also survives get_fm_field's trim. Anything else is false.
        pattern_strict: get_fm_field(&fm, "pattern_strict")
            .is_some_and(|v| v.trim_matches('"').eq_ignore_ascii_case("true")),
        commands: get_fm_field(&fm, "commands"),
        files: get_fm_field(&fm, "files"),
        description: get_fm_field(&fm, "description").unwrap_or_default(),
        vocabulary: get_fm_field(&fm, "vocabulary").unwrap_or_default(),
        // threshold: only read for ways with trigger: context-threshold (percentage).
        // Post-ADR-125, no semantic/BM25 meaning; default 0.0 is never compared for other triggers.
        threshold: get_fm_field(&fm, "threshold")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        // config::global() — future migration: ctx.config.default_scope
        scope: get_fm_field(&fm, "scope")
            .unwrap_or_else(|| crate::config::global().default_scope.clone()),
        when_project: get_when_field(&fm, "project"),
        when_file_exists: get_when_field(&fm, "file_exists"),
        trigger: get_fm_field(&fm, "trigger"),
        trigger_path: get_fm_field(&fm, "path"),
    })
}

pub(crate) fn way_id_from_path(path: &Path, base: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    crate::util::path_to_id(parent.strip_prefix(base).unwrap_or(parent))
}

pub(crate) fn extract_frontmatter(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    let mut fm_lines = Vec::new();
    for line in lines {
        if line == "---" {
            return Some(fm_lines.join("\n"));
        }
        fm_lines.push(line);
    }
    None
}

pub(crate) fn get_fm_field(fm: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    for line in fm.lines() {
        if let Some(val) = line.strip_prefix(&prefix) {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

pub(crate) fn get_when_field(fm: &str, name: &str) -> Option<String> {
    let mut in_when = false;
    let prefix = format!("  {name}:");
    for line in fm.lines() {
        if line == "when:" {
            in_when = true;
            continue;
        }
        if in_when {
            if let Some(val) = line.strip_prefix(&prefix) {
                return Some(val.trim().to_string());
            }
            if !line.starts_with("  ") && !line.is_empty() {
                break;
            }
        }
    }
    None
}

pub(crate) fn check_when(
    when_project: &Option<String>,
    when_file_exists: &Option<String>,
    project_dir: &str,
) -> bool {
    if when_project.is_none() && when_file_exists.is_none() {
        return true;
    }

    if let Some(ref wp) = when_project {
        let expanded = wp.replace("~", &super::scoring::home_dir().display().to_string());
        let resolved = std::fs::canonicalize(&expanded)
            .unwrap_or_else(|_| PathBuf::from(&expanded));
        let current = std::fs::canonicalize(project_dir)
            .unwrap_or_else(|_| PathBuf::from(project_dir));
        if resolved != current {
            return false;
        }
    }

    if let Some(ref wfe) = when_file_exists {
        let resolved_dir = std::fs::canonicalize(project_dir)
            .unwrap_or_else(|_| PathBuf::from(project_dir));
        if !resolved_dir.join(wfe).exists() {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const LF: &str = "---\ndescription: hello\npattern: foo\n---\n# Body\n";
    const CRLF: &str = "---\r\ndescription: hello\r\npattern: foo\r\n---\r\n# Body\r\n";

    /// A symlink inside a root that points back at the root (the shape the
    /// pre-1.0 migrator's lift-user phase left on one install) must not yield
    /// the same way file twice under two ids.
    #[test]
    fn self_referential_symlink_yields_each_way_once() {
        let root = std::env::temp_dir().join(format!("ways-cand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("dom/alpha")).unwrap();
        std::fs::write(root.join("dom/alpha/alpha.md"), LF).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, root.join("ways")).unwrap();

        let mut out = Vec::new();
        let mut written = HashSet::new();
        let mut seen_paths = HashSet::new();
        collect_from_dir(&root, "", &mut out, &HashSet::new(), &mut written, &mut seen_paths, &[]);

        let ids: Vec<&str> = out.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["dom/alpha"], "got {ids:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A link from the user root into the core root: the user root yields
    /// nothing for it, the core root yields it once under its proper id.
    #[cfg(unix)]
    #[test]
    fn link_into_another_root_is_collected_by_that_root_only() {
        let base = std::env::temp_dir().join(format!("ways-xroot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let user = base.join("user");
        let core = base.join("core");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(core.join("dom/alpha")).unwrap();
        std::fs::write(core.join("dom/alpha/alpha.md"), LF).unwrap();
        std::os::unix::fs::symlink(&core, user.join("ways")).unwrap();

        let mut out = Vec::new();
        let mut written = HashSet::new();
        let mut seen_paths = HashSet::new();
        collect_from_dir(&user, "", &mut out, &HashSet::new(), &mut written, &mut seen_paths, &[canonical(&core)]);
        assert!(out.is_empty(), "user root must not collect core's files: {:?}", out.iter().map(|c| &c.id).collect::<Vec<_>>());
        let claimed = written.clone();
        collect_from_dir(&core, "", &mut out, &claimed, &mut written, &mut seen_paths, &[canonical(&user)]);

        let ids: Vec<&str> = out.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["dom/alpha"], "got {ids:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn fields_parse_identically_across_line_endings() {
        let lf = extract_frontmatter(LF).expect("LF frontmatter");
        let crlf = extract_frontmatter(CRLF).expect("CRLF frontmatter");
        assert_eq!(get_fm_field(&lf, "pattern").as_deref(), Some("foo"));
        assert_eq!(get_fm_field(&crlf, "pattern").as_deref(), Some("foo"));
        assert_eq!(get_fm_field(&crlf, "description").as_deref(), Some("hello"));
    }

    fn write_way(root: &Path, id: &str) {
        let leaf = id.rsplit('/').next().unwrap_or(id);
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{leaf}.md")), "---\npattern: x\n---\n# w\n").unwrap();
    }

    #[test]
    fn user_candidate_fires_and_shadows_core() {
        // collect_from_dir takes the dir explicitly (no env), so the dedup chain
        // is testable directly. Models the user→core leg of collect_candidates.
        let base =
            std::env::temp_dir().join(format!("ways-cand-{}-{}", std::process::id(), "shadow"));
        let _ = std::fs::remove_dir_all(&base);
        let user = base.join("user");
        let core = base.join("core");
        write_way(&user, "meta/foo"); // shadows core foo
        write_way(&user, "meta/baz"); // unique user way — must still become a candidate
        write_way(&core, "meta/foo");
        write_way(&core, "meta/bar");

        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        let mut seen_paths = HashSet::new();
        collect_from_dir(&user, "", &mut candidates, &HashSet::new(), &mut seen, &mut seen_paths, &[]);
        let claimed = seen.clone();
        collect_from_dir(&core, "", &mut candidates, &claimed, &mut seen, &mut seen_paths, &[]);

        let ids: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
        // The blocker: a unique user way must be a candidate (i.e. can fire).
        assert!(ids.contains(&"meta/baz"), "unique user way must become a candidate: {ids:?}");
        assert!(ids.contains(&"meta/bar"), "core-only way present: {ids:?}");
        // Shadow: foo appears once, and it's the USER file (its gating fields win).
        assert_eq!(ids.iter().filter(|i| **i == "meta/foo").count(), 1, "foo deduped: {ids:?}");
        let foo = candidates.iter().find(|c| c.id == "meta/foo").unwrap();
        assert!(foo.path.starts_with(&user), "the surviving foo candidate is the user's");

        let _ = std::fs::remove_dir_all(&base);
    }
}
