//! Finding projects and transcripts under a Claude Code projects directory.
//!
//! Every function here takes the projects directory explicitly, so tests and
//! other config directories (an ADR-184 target) run against their own tree.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::slug::{project_slug, slug_matches, MAX_SLUG_LEN};

/// The directory holding `project`'s sessions under `projects`, if one exists.
///
/// Tries the exact slug first. For a path whose slug Claude Code truncates,
/// it then looks at every directory named by the 200-character prefix and a
/// hash, as Claude Code does, and takes one only when it is verified to
/// belong to `project` ([`dir_belongs_to`]). Two paths that share their
/// first 200 slug characters share that prefix, so an unverified or
/// ambiguous match is `None`, never a guess.
pub fn find_project_dir_in(projects: &Path, project: &str) -> Option<PathBuf> {
    let direct = projects.join(project_slug(project));
    if direct.is_dir() {
        return Some(direct);
    }
    let mut verified = prefix_candidates_in(projects, project)
        .into_iter()
        .filter(|d| dir_belongs_to(d, project));
    let first = verified.next()?;
    verified.next().is_none().then_some(first)
}

/// Every directory under `projects` named by `project`'s 200-character slug
/// prefix and some hash, sorted. Empty when the slug is not truncated.
pub fn prefix_candidates_in(projects: &Path, project: &str) -> Vec<PathBuf> {
    let slug = project_slug(project);
    if slug.len() <= MAX_SLUG_LEN {
        return Vec::new();
    }
    let prefix = &slug[..=MAX_SLUG_LEN];
    let mut out: Vec<PathBuf> = std::fs::read_dir(projects)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(prefix)))
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// True when a project directory records `project` itself: its
/// `sessions-index.json` `originalPath`, or the first `cwd` of one of its
/// transcripts, equals the path.
pub fn dir_belongs_to(dir: &Path, project: &str) -> bool {
    let original = read_json(&dir.join("sessions-index.json"))
        .and_then(|i| i.get("originalPath").and_then(|v| v.as_str()).map(str::to_string));
    original.as_deref() == Some(project)
        || transcripts_in(dir).iter().any(|t| first_cwd(t).as_deref() == Some(project))
}

/// A session's transcript, `<projects>/<project dir>/<session_id>.jsonl`.
///
/// Looks in `project`'s directory first when one is given. On a miss (no
/// project, a subagent's cwd, a project recorded elsewhere) it scans every
/// project directory, since session ids are unique across projects. An empty
/// session id, or one holding a path separator, finds nothing.
pub fn find_transcript_in(projects: &Path, project: Option<&str>, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() || session_id.contains(['/', '\\']) || session_id == ".." {
        return None;
    }
    let file = format!("{session_id}.jsonl");
    if let Some(dir) = project.and_then(|p| find_project_dir_in(projects, p)) {
        let direct = dir.join(&file);
        if direct.is_file() {
            return Some(direct);
        }
    }
    // `flatten`, not `?` per entry: one unreadable directory must not end the
    // scan, or every session behind it would look absent.
    std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|e| e.path().join(&file))
        .find(|p| p.is_file())
}

/// The most recently modified transcript in one project directory. Files
/// whose path contains `.tmp` are skipped.
pub fn newest_transcript(dir: &Path) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if path.to_str().is_some_and(|s| s.contains(".tmp")) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(t, _)| mtime > *t) {
            newest = Some((mtime, path));
        }
    }
    newest.map(|(_, p)| p)
}

/// The transcripts (`*.jsonl`) directly inside one project directory, sorted
/// by name.
pub fn transcripts_in(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl") && p.is_file())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The project directories under `projects`, sorted by name. Dot-named
/// entries are skipped.
pub fn project_dirs_in(projects: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(projects)
        .map(|rd| {
            rd.flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The project path a directory under `projects` was named from.
///
/// A directory name cannot be decoded on its own: `/a/b_c` and `/a/b-c` both
/// become `-a-b-c`. Each source below is accepted only when the path it
/// gives encodes back to `name`:
///
/// 1. `sessions-index.json`: its `originalPath`, then each entry's
///    `projectPath`.
/// 2. The `cwd` of the first record that has one, in each transcript.
/// 3. A walk of the filesystem that tries `/` and `-` at each dash and keeps
///    the split that names existing directories.
pub fn resolve_project_path(projects: &Path, name: &str) -> Option<String> {
    let dir = projects.join(name);

    if let Some(index) = read_json(&dir.join("sessions-index.json")) {
        let original = index.get("originalPath").and_then(|v| v.as_str());
        let entries = index.get("entries").and_then(|v| v.as_array());
        let from_entries = entries
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("projectPath").and_then(|v| v.as_str()));
        for candidate in original.into_iter().chain(from_entries) {
            if !candidate.is_empty() && slug_matches(name, candidate) {
                return Some(candidate.to_string());
            }
        }
    }

    for transcript in transcripts_in(&dir) {
        if let Some(cwd) = first_cwd(&transcript) {
            if slug_matches(name, &cwd) {
                return Some(cwd);
            }
        }
    }

    resolve_on_disk(name)
}

/// The `cwd` of the first record that carries one, reading at most 64 lines.
fn first_cwd(transcript: &Path) -> Option<String> {
    let file = std::fs::File::open(transcript).ok()?;
    for line in BufReader::new(file).lines().take(64) {
        let Ok(line) = line else { break };
        if !line.contains("\"cwd\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
            return Some(cwd.to_string());
        }
    }
    None
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Walk the filesystem from `/`, descending into each directory whose
/// encoded name continues `name`, until the whole name is consumed. Reads only
/// the directories along matching prefixes. A truncated name (over
/// [`MAX_SLUG_LEN`]) does not resolve this way.
fn resolve_on_disk(name: &str) -> Option<String> {
    fn walk(dir: &Path, rest: &str, depth: usize) -> Option<PathBuf> {
        if rest.is_empty() {
            return Some(dir.to_path_buf());
        }
        if depth > 64 {
            return None;
        }
        let mut entries: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
            .collect();
        entries.sort();
        for (entry, path) in entries {
            let piece = format!("-{}", project_slug(&entry));
            let Some(after) = rest.strip_prefix(&piece) else { continue };
            if (after.is_empty() || after.starts_with('-')) && path.is_dir() {
                if let Some(found) = walk(&path, after, depth + 1) {
                    return Some(found);
                }
            }
        }
        None
    }
    if name.len() > MAX_SLUG_LEN || !name.starts_with('-') {
        return None;
    }
    walk(Path::new("/"), name, 0).map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempTree;

    #[test]
    fn finds_transcript_under_the_exact_slug() {
        let t = TempTree::new("exact");
        t.file("projects/-srv-my-proj/s1.jsonl", "{}\n");
        let projects = t.path("projects");
        assert_eq!(
            find_transcript_in(&projects, Some("/srv/my proj"), "s1"),
            Some(projects.join("-srv-my-proj/s1.jsonl"))
        );
    }

    #[test]
    fn falls_back_to_scanning_every_project() {
        let t = TempTree::new("scan");
        t.file("projects/-elsewhere/s2.jsonl", "{}\n");
        let projects = t.path("projects");
        assert_eq!(
            find_transcript_in(&projects, Some("/srv/p"), "s2"),
            Some(projects.join("-elsewhere/s2.jsonl"))
        );
        assert_eq!(find_transcript_in(&projects, None, "s2"), Some(projects.join("-elsewhere/s2.jsonl")));
        assert_eq!(find_transcript_in(&projects, None, "absent"), None);
    }

    #[test]
    fn rejects_empty_and_path_like_session_ids() {
        let t = TempTree::new("ids");
        t.file("projects/-p/.jsonl", "{}\n");
        t.file("projects/x.jsonl", "{}\n");
        let projects = t.path("projects");
        assert_eq!(find_transcript_in(&projects, None, ""), None);
        assert_eq!(find_transcript_in(&projects, None, "../x"), None);
    }

    #[test]
    fn finds_a_long_project_by_prefix() {
        let t = TempTree::new("long");
        let project = format!("/{}", "a".repeat(240));
        let slug = project_slug(&project);
        // Named with a different hash, as another runtime may have written it.
        let other = format!("{}-otherhash", &slug[..MAX_SLUG_LEN]);
        t.file(&format!("projects/{other}/s3.jsonl"), &format!("{{\"cwd\":\"{project}\"}}\n"));
        let projects = t.path("projects");
        assert_eq!(find_project_dir_in(&projects, &project), Some(projects.join(&other)));
        assert_eq!(
            find_transcript_in(&projects, Some(&project), "s3"),
            Some(projects.join(&other).join("s3.jsonl"))
        );
    }

    #[test]
    fn prefix_lookup_refuses_an_unverified_or_ambiguous_match() {
        // `<base>/projA` and `<base>/projB` share their first 200 slug
        // characters; neither may be returned for the other.
        let t = TempTree::new("sibling");
        let base = format!("/{}", "a".repeat(220));
        let (a, b) = (format!("{base}/projA"), format!("{base}/projB"));
        let name_b = project_slug(&b);
        t.file(&format!("projects/{name_b}/s.jsonl"), &format!("{{\"cwd\":\"{b}\"}}\n"));
        let projects = t.path("projects");
        assert_eq!(find_project_dir_in(&projects, &a), None);
        assert_eq!(prefix_candidates_in(&projects, &a), vec![projects.join(&name_b)]);
        assert_eq!(find_transcript_in(&projects, Some(&a), "missing"), None);

        // Two directories that both claim `a` (another runtime's hash): ambiguous.
        let slug_a = project_slug(&a);
        for hash in ["h1", "h2"] {
            t.file(
                &format!("projects/{}-{hash}/s.jsonl", &slug_a[..MAX_SLUG_LEN]),
                &format!("{{\"cwd\":\"{a}\"}}\n"),
            );
        }
        assert_eq!(find_project_dir_in(&projects, &a), None);
    }

    #[test]
    fn finds_transcripts_of_non_ascii_paths() {
        let t = TempTree::new("nonascii");
        let projects = t.path("projects");
        for (project, sid) in [("/srv/项目 x", "cjk"), ("/srv/🦀/crab", "crab")] {
            let name = project_slug(project);
            t.file(&format!("projects/{name}/{sid}.jsonl"), "{}\n");
            assert_eq!(
                find_transcript_in(&projects, Some(project), sid),
                Some(projects.join(&name).join(format!("{sid}.jsonl")))
            );
        }
        assert_eq!(project_slug("/srv/项目 x"), "-srv----x");
        assert_eq!(project_slug("/srv/🦀/crab"), "-srv----crab");
    }

    #[test]
    fn newest_transcript_skips_tmp_files() {
        let t = TempTree::new("newest");
        t.file("p/a.jsonl", "{}\n");
        std::thread::sleep(std::time::Duration::from_millis(20));
        t.file("p/b.jsonl", "{}\n");
        std::thread::sleep(std::time::Duration::from_millis(20));
        t.file("p/c.tmp.jsonl", "{}\n");
        assert_eq!(newest_transcript(&t.path("p")), Some(t.path("p/b.jsonl")));
        assert_eq!(newest_transcript(&t.path("missing")), None);
    }

    #[test]
    fn resolves_a_path_from_a_transcript_cwd() {
        let t = TempTree::new("resolve");
        t.file(
            "projects/-srv-a-b-c/s.jsonl",
            "{\"type\":\"mode\"}\n{\"type\":\"user\",\"cwd\":\"/srv/a_b.c\"}\n",
        );
        let projects = t.path("projects");
        assert_eq!(resolve_project_path(&projects, "-srv-a-b-c"), Some("/srv/a_b.c".to_string()));
    }

    #[test]
    fn resolve_ignores_an_index_path_of_another_directory() {
        // A subdirectory's projectPath encodes to a different name and is not
        // taken; the originalPath that matches is.
        let t = TempTree::new("index");
        t.file(
            "projects/-srv-kg/sessions-index.json",
            r#"{"entries":[{"projectPath":"/srv/kg/fuse"}],"originalPath":"/srv/kg"}"#,
        );
        let projects = t.path("projects");
        assert_eq!(resolve_project_path(&projects, "-srv-kg"), Some("/srv/kg".to_string()));
    }

    #[test]
    fn resolves_on_disk_when_nothing_names_the_path() {
        // `_former` and `.cfg` encode with a leading dash, which a split on
        // dashes cannot tell from a path separator.
        let t = TempTree::new("disk");
        t.dir("tree/_former/my app/.cfg");
        let real = t.path("tree/_former/my app/.cfg");
        let real = real.to_str().unwrap();
        let name = project_slug(real);
        let projects = t.path("projects");
        std::fs::create_dir_all(projects.join(&name)).unwrap();
        assert_eq!(resolve_project_path(&projects, &name).as_deref(), Some(real));
        assert_eq!(resolve_project_path(&projects, "-no-such-place-anywhere"), None);
    }
}
