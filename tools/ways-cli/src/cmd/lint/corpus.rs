//! ADR-701 §3 corpus checks: rules about the way tree as a whole rather than
//! about one file.
//!
//! - See Also targets resolve to a way.
//! - No symlink sits inside a ways root. The scanner follows links, so a linked
//!   way would be read as a second way.
//! - A way's file name is unique within the root (ADR-110 §7).
//!
//! The three are errors for the core corpus and warnings for user and project
//! roots, so an upgrade alone turns no one's lint red.
//!
//! A way is a non-check `.md` file that opens with a frontmatter fence, the same
//! test the per-file pass uses. Its location is its directory relative to the
//! root (`softwaredev/code/quality`); a See Also entry `name(domain)` resolves
//! when `domain/name` is such a location, or `name` alone is one that starts
//! with the same domain. The second form is how entries that spell out the
//! domain (`data/migrations(data)`) and entries naming a domain-root way
//! (`documentation(documentation)`) resolve. A name that lands in a different
//! domain than the one in parentheses does not resolve.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// A corpus finding: the path it belongs to (relative to the root) and the text.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Finding {
    pub rel: String,
    pub message: String,
}

/// Every way file in `root`: `(path, directory relative to root, file stem)`.
fn way_files(root: &Path) -> Vec<(PathBuf, String, String)> {
    let mut out = Vec::new();
    for path in crate::scanner::md_files(root, crate::scanner::MdKind::Ways) {
        match std::fs::read_to_string(&path) {
            Ok(c) if crate::frontmatter::opens_with_fence(&c) => {}
            _ => continue,
        }
        let Some(dir) = path.parent().and_then(|d| d.strip_prefix(root).ok()) else { continue };
        let dir = dir
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        out.push((path, dir, stem));
    }
    out.sort();
    out
}

fn rel_file(dir: &str, path: &Path) -> String {
    let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    if dir.is_empty() { file } else { format!("{dir}/{file}") }
}

/// See Also entries naming no way. See [`crate::frontmatter::extract_way_refs`]
/// for which entries are checkable; the rest are not looked at.
pub(super) fn broken_see_also(root: &Path) -> Vec<Finding> {
    let ways = way_files(root);
    let dirs: BTreeSet<&str> = ways.iter().map(|(_, d, _)| d.as_str()).collect();
    let mut out = Vec::new();
    for (path, dir, _) in &ways {
        let Ok(content) = std::fs::read_to_string(path) else { continue };
        for (name, domain) in crate::frontmatter::extract_way_refs(&content) {
            let spelled = format!("{domain}/{name}");
            let domain_led = name == domain || name.starts_with(&format!("{domain}/"));
            if !dirs.contains(spelled.as_str()) && !(domain_led && dirs.contains(name.as_str())) {
                out.push(Finding {
                    rel: rel_file(dir, path),
                    message: format!("See Also target `{name}({domain})` names no way"),
                });
            }
        }
    }
    out
}

/// Symlinks anywhere under `root`, files and directories alike. The walk does
/// not follow links, so each is reported once at its own path.
pub(super) fn symlinks(root: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if entry.path_is_symlink() {
            let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
            out.push(Finding {
                rel: rel.display().to_string(),
                message: "symlink inside a ways root; the scanner follows links and would read a linked way as a second way".to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// Way file names used more than once in the root. One finding per extra file,
/// naming the first.
pub(super) fn repeated_basenames(root: &Path) -> Vec<Finding> {
    let mut by_stem: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, dir, stem) in way_files(root) {
        by_stem.entry(stem).or_default().push(rel_file(&dir, &path));
    }
    let mut out = Vec::new();
    for (stem, paths) in by_stem {
        for later in paths.iter().skip(1) {
            out.push(Finding {
                rel: later.clone(),
                message: format!(
                    "way file name `{stem}.md` repeats {} (ADR-110 §7: names are unique within a root)",
                    paths[0]
                ),
            });
        }
    }
    out
}

/// Whether `root` is the core corpus. The shipped root, the app copy and the
/// projection count, as does any `hooks/ways` directory (a checkout linted by
/// path). User and project roots are anything else.
pub(super) fn is_core_root(root: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let r = canon(root);
    if [
        crate::paths::shipped_ways_root(),
        crate::paths::core_ways_root(),
        crate::paths::projected_ways_root(),
    ]
    .iter()
    .any(|c| canon(c) == r)
    {
        return true;
    }
    r.ends_with("hooks/ways")
}

/// Run the three checks over `root`, printing each finding as ERROR when
/// `core` and WARNING otherwise, and adding to the matching counter.
pub(super) fn lint_corpus(root: &Path, core: bool, errors: &mut u32, warnings: &mut u32) {
    let findings = broken_see_also(root)
        .into_iter()
        .chain(symlinks(root))
        .chain(repeated_basenames(root));
    for f in findings {
        if core {
            eprintln!("  ERROR: {} — {}", f.rel, f.message);
            *errors += 1;
        } else {
            eprintln!("  WARNING: {} — {}", f.rel, f.message);
            *warnings += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ways-lint-corpus-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn way(root: &Path, rel: &str, see_also: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = format!("---\ndescription: d\nvocabulary: v\n---\n# W\n\n## See Also\n\n{see_also}\n");
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn see_also_resolves_every_spelling_and_flags_a_miss() {
        let root = scratch("seealso");
        way(
            &root,
            "softwaredev/code/quality/quality.md",
            "- code/testing(softwaredev) — ok\n- trust(meta) — domain way\n- data/migrations(data) — domain spelled out\n- documentation(documentation) — domain root\n- code/missing(softwaredev) — broken\n- research(softwaredev) — wrong domain\n- develop (skill) — not a way\n- `docs/x.md` — not a way",
        );
        way(&root, "softwaredev/code/testing/testing.md", "");
        way(&root, "meta/trust/trust.md", "");
        way(&root, "data/migrations/migrations.md", "");
        way(&root, "documentation/documentation.md", "");
        way(&root, "research/research.md", "");

        let found = broken_see_also(&root);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].message.contains("code/missing(softwaredev)"));
        assert!(found[1].message.contains("research(softwaredev)"));
        assert_eq!(found[0].rel, "softwaredev/code/quality/quality.md");
    }

    #[test]
    fn see_also_target_without_frontmatter_is_not_a_way() {
        let root = scratch("seealso-nofm");
        way(&root, "a/one/one.md", "- two(a) — a README, not a way");
        std::fs::create_dir_all(root.join("a/two")).unwrap();
        std::fs::write(root.join("a/two/two.md"), "# no frontmatter\n").unwrap();
        assert_eq!(broken_see_also(&root).len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_reported_once_each() {
        let root = scratch("symlink");
        way(&root, "a/real/real.md", "");
        std::os::unix::fs::symlink(root.join("a/real"), root.join("a/linked")).unwrap();
        std::os::unix::fs::symlink(root.join("a/real/real.md"), root.join("a/real/alias.md")).unwrap();
        let found = symlinks(&root);
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["a/linked", "a/real/alias.md"]);
    }

    #[test]
    fn no_symlinks_no_findings() {
        let root = scratch("nosymlink");
        way(&root, "a/real/real.md", "");
        assert!(symlinks(&root).is_empty());
    }

    #[test]
    fn repeated_basenames_name_the_first_holder() {
        let root = scratch("basename");
        way(&root, "data/documentation/documentation.md", "");
        way(&root, "documentation/documentation.md", "");
        way(&root, "a/unique/unique.md", "");
        let found = repeated_basenames(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].rel, "documentation/documentation.md");
        assert!(found[0].message.contains("data/documentation/documentation.md"));
    }

    #[test]
    fn check_files_do_not_count_as_repeats() {
        let root = scratch("basename-check");
        way(&root, "a/x/x.md", "");
        std::fs::write(root.join("a/x/x.check.md"), "---\ndescription: d\n---\n").unwrap();
        way(&root, "b/y/y.md", "");
        assert!(repeated_basenames(&root).is_empty());
    }

    #[test]
    fn severity_follows_the_root() {
        let root = scratch("severity");
        way(&root, "a/one/one.md", "- gone(a) — broken");

        let (mut e, mut w) = (0, 0);
        lint_corpus(&root, true, &mut e, &mut w);
        assert_eq!((e, w), (1, 0));

        let (mut e, mut w) = (0, 0);
        lint_corpus(&root, false, &mut e, &mut w);
        assert_eq!((e, w), (0, 1));
    }

    #[test]
    fn a_scratch_root_is_not_core_and_a_hooks_ways_dir_is() {
        let root = scratch("core");
        assert!(!is_core_root(&root));
        let hw = root.join("hooks/ways");
        std::fs::create_dir_all(&hw).unwrap();
        assert!(is_core_root(&hw));
    }
}
