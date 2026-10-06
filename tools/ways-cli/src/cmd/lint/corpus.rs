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
//! The checks run against the ways root that contains the lint target, so a
//! subtree or a single file keeps the identities (`softwaredev/code/quality`)
//! it has in the whole root. Only files under the target are reported. A way is
//! a non-check `.md` file that opens with a frontmatter fence
//! ([`crate::scanner::scan_declared_ways`]); See Also entries resolve with
//! [`crate::scanner::resolve_ref`], the resolver `ways author graph` uses.
//!
//! A project or user way may point at a core way, so See Also entries resolve
//! against every root a session reads. A core way resolves against core alone,
//! so a developer's own ways cannot hide a broken core reference.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::scanner::{resolve_ref, scan_declared_ways, WayFile};

/// A corpus finding: the file it belongs to (relative to the root) and the text.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Finding {
    pub rel: String,
    pub message: String,
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

/// See Also entries naming no way. `lookup` lists the other roots a name may
/// resolve in; `root`'s own ways always count.
fn broken_see_also(own: &[WayFile], root: &Path, scope: &Path, lookup: &[PathBuf]) -> Vec<Finding> {
    let mut all: Vec<WayFile> = Vec::new();
    for other in lookup.iter().filter(|r| canonical(r) != *root) {
        all.extend(scan_declared_ways(other));
    }
    all.extend(own.iter().cloned());

    let mut out = Vec::new();
    for way in own.iter().filter(|w| w.path.starts_with(scope)) {
        let Ok(content) = std::fs::read_to_string(&way.path) else { continue };
        for r in crate::frontmatter::extract_way_refs(&content) {
            if resolve_ref(&all, &r.name, &r.domain).is_none() {
                out.push(Finding {
                    rel: rel(root, &way.path),
                    message: format!("See Also target `{}({})` names no way", r.name, r.domain),
                });
            }
        }
    }
    out
}

/// Symlinks under `scope`, files and directories alike. The walk does not
/// follow links, so each is reported once at its own path. `scope` itself may
/// be a link: the projection (`~/.claude/hooks/ways`) is one.
fn symlinks(root: &Path, scope: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    for entry in WalkDir::new(scope).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if entry.depth() > 0 && entry.path_is_symlink() {
            out.push(Finding {
                rel: rel(root, entry.path()),
                message: "symlinks are not allowed inside a ways root".to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// Whether any component of `path` below `root` is a symlink.
fn runs_through_symlink(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return false };
    let mut cur = root.to_path_buf();
    rel.components().any(|c| {
        cur.push(c);
        std::fs::symlink_metadata(&cur).is_ok_and(|m| m.file_type().is_symlink())
    })
}

/// Way file names used more than once in the root, reported on each holder
/// under `scope` and naming the others.
fn repeated_basenames(own: &[WayFile], root: &Path, scope: &Path) -> Vec<Finding> {
    let mut by_stem: BTreeMap<String, Vec<&WayFile>> = BTreeMap::new();
    // A way reached through a linked directory is a second view of a way that
    // is already counted; the symlink finding covers it.
    for w in own.iter().filter(|w| !runs_through_symlink(root, &w.path)) {
        let stem = w.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        by_stem.entry(stem).or_default().push(w);
    }
    let mut out = Vec::new();
    for (stem, holders) in by_stem.iter().filter(|(_, h)| h.len() > 1) {
        for w in holders.iter().filter(|w| w.path.starts_with(scope)) {
            let others: Vec<String> = holders
                .iter()
                .filter(|o| o.path != w.path)
                .map(|o| rel(root, &o.path))
                .collect();
            out.push(Finding {
                rel: rel(root, &w.path),
                message: format!(
                    "way file name `{stem}.md` is also used by {} (ADR-110 §7: names are unique within a root)",
                    others.join(", ")
                ),
            });
        }
    }
    out
}

/// All three checks over `root`, limited to files under `scope`.
fn findings(root: &Path, scope: &Path, lookup: &[PathBuf]) -> Vec<Finding> {
    let own = scan_declared_ways(root);
    let mut out = broken_see_also(&own, root, scope, lookup);
    out.extend(symlinks(root, scope));
    out.extend(repeated_basenames(&own, root, scope));
    out
}

/// Run the corpus checks for the lint target `target` (a root, a subtree, or a
/// way file), printing each finding as ERROR when the containing root is core
/// and WARNING otherwise, and adding to the matching counter. A target inside no
/// ways root gets one printed line and no checks.
pub(super) fn lint_corpus(
    target: &Path,
    project: Option<&Path>,
    errors: &mut u32,
    warnings: &mut u32,
) {
    use crate::paths::{containing_ways_root, is_core_root, ways_roots};

    let scope = canonical(target);
    let Some(root) = containing_ways_root(&scope, project) else {
        eprintln!(
            "  corpus checks skipped: {} is not inside a ways root",
            target.display()
        );
        return;
    };
    let core = is_core_root(&root);
    let lookup = if core { vec![root.clone()] } else { ways_roots(project) };

    for f in findings(&root, &scope, &lookup) {
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
    use crate::paths::SCHEMA_FILE;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ways-lint-corpus-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        canonical(&dir)
    }

    fn way(root: &Path, rel: &str, see_also: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = format!("---\ndescription: d\nvocabulary: v\n---\n# W\n\n## See Also\n\n{see_also}\n");
        std::fs::write(path, body).unwrap();
    }

    fn core_root(tag: &str) -> PathBuf {
        let root = scratch(tag).join("hooks/ways");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(SCHEMA_FILE), "x: 1\n").unwrap();
        root
    }

    fn msgs(f: &[Finding]) -> Vec<String> {
        f.iter().map(|f| format!("{}: {}", f.rel, f.message)).collect()
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

        let found = findings(&root, &root, &[]);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].message.contains("code/missing(softwaredev)"));
        assert!(found[1].message.contains("research(softwaredev)"));
        assert_eq!(found[0].rel, "softwaredev/code/quality/quality.md");
    }

    #[test]
    fn a_target_without_frontmatter_is_not_a_way() {
        let root = scratch("seealso-nofm");
        way(&root, "a/one/one.md", "- two(a) — a README, not a way");
        std::fs::create_dir_all(root.join("a/two")).unwrap();
        std::fs::write(root.join("a/two/two.md"), "# no frontmatter\n").unwrap();
        assert_eq!(
            msgs(&findings(&root, &root, &[])),
            vec!["a/one/one.md: See Also target `two(a)` names no way"]
        );
    }

    #[test]
    fn a_way_without_a_description_is_still_a_target() {
        let root = scratch("seealso-files-only");
        way(&root, "a/one/one.md", "- two(a) — fires on files only");
        std::fs::create_dir_all(root.join("a/two")).unwrap();
        std::fs::write(root.join("a/two/two.md"), "---\nfiles: x\n---\nbody\n").unwrap();
        assert!(findings(&root, &root, &[]).is_empty());
    }

    #[test]
    fn a_project_way_may_name_a_way_that_exists_only_in_the_shipped_root() {
        let shipped = scratch("xroot-shipped");
        way(&shipped, "softwaredev/code/quality/quality.md", "");
        let project = scratch("xroot-project").join(".claude/ways");
        way(&project, "mine/thing/thing.md", "- code/quality(softwaredev) — core\n- code/gone(softwaredev) — nowhere");

        let found = findings(&project, &project, &[project.clone(), shipped.clone()]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains("code/gone(softwaredev)"));

        // Without the second root the core reference is a miss too.
        assert_eq!(findings(&project, &project, &[]).len(), 2);
    }

    #[test]
    fn a_subtree_keeps_its_domain_prefix_and_reports_only_its_own_files() {
        let root = core_root("subtree");
        way(&root, "softwaredev/code/quality/quality.md", "- code/testing(softwaredev) — ok\n- trust(meta) — other domain, outside the subtree");
        way(&root, "softwaredev/code/testing/testing.md", "- code/gone(softwaredev) — broken");
        way(&root, "meta/trust/trust.md", "- nowhere(meta) — broken, but outside the target");

        let scope = root.join("softwaredev");
        let found = findings(&root, &scope, std::slice::from_ref(&root));
        assert_eq!(
            msgs(&found),
            vec!["softwaredev/code/testing/testing.md: See Also target `code/gone(softwaredev)` names no way"]
        );
    }

    #[test]
    fn a_single_file_target_is_judged_in_its_root() {
        let root = core_root("file");
        way(&root, "softwaredev/code/quality/quality.md", "- code/testing(softwaredev) — ok");
        way(&root, "softwaredev/code/testing/testing.md", "- code/gone(softwaredev) — broken");
        let file = root.join("softwaredev/code/quality/quality.md");
        assert!(findings(&root, &file, std::slice::from_ref(&root)).is_empty());
        let file = root.join("softwaredev/code/testing/testing.md");
        assert_eq!(findings(&root, &file, std::slice::from_ref(&root)).len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_reported_once_each() {
        let root = scratch("symlink");
        way(&root, "a/real/real.md", "");
        std::os::unix::fs::symlink(root.join("a/real"), root.join("a/linked")).unwrap();
        std::os::unix::fs::symlink(root.join("a/real/real.md"), root.join("a/real/alias.md")).unwrap();
        let found = symlinks(&root, &root);
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["a/linked", "a/real/alias.md"]);
        assert_eq!(found[0].message, "symlinks are not allowed inside a ways root");
    }

    #[cfg(unix)]
    #[test]
    fn one_linked_directory_is_one_finding() {
        let root = scratch("dirlink");
        way(&root, "meta/trust/trust.md", "");
        way(&root, "meta/trust/prose/prose.md", "");
        way(&root, "meta/other/other.md", "");
        std::os::unix::fs::symlink(root.join("meta/trust"), root.join("meta/trust2")).unwrap();

        let found = findings(&root, &root, &[]);
        assert_eq!(msgs(&found), vec!["meta/trust2: symlinks are not allowed inside a ways root"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_root_is_not_a_finding() {
        let real = scratch("linkedroot-real");
        way(&real, "a/real/real.md", "");
        let link = scratch("linkedroot-link").join("ways");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(symlinks(&link, &link).is_empty());
    }

    #[test]
    fn no_symlinks_no_findings() {
        let root = scratch("nosymlink");
        way(&root, "a/real/real.md", "");
        assert!(symlinks(&root, &root).is_empty());
    }

    #[test]
    fn a_repeated_basename_is_reported_on_each_holder_under_the_target() {
        let root = scratch("basename");
        way(&root, "data/documentation/documentation.md", "");
        way(&root, "documentation/documentation.md", "");
        way(&root, "a/unique/unique.md", "");

        let own = scan_declared_ways(&root);
        let found = repeated_basenames(&own, &root, &root);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].message.contains("documentation/documentation.md"));

        let scope = root.join("data");
        let found = repeated_basenames(&own, &root, &scope);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rel, "data/documentation/documentation.md");
        assert!(found[0].message.contains("also used by documentation/documentation.md"));
    }

    #[test]
    fn severity_follows_the_containing_root() {
        let core = core_root("sev-core");
        way(&core, "a/one/one.md", "- gone(a) — broken");
        let (mut e, mut w) = (0, 0);
        lint_corpus(&core, None, &mut e, &mut w);
        assert_eq!((e, w), (1, 0));

        // A core subtree is core.
        let (mut e, mut w) = (0, 0);
        lint_corpus(&core.join("a"), None, &mut e, &mut w);
        assert_eq!((e, w), (1, 0));

        let user = scratch("sev-user").join("proj/.claude/ways");
        way(&user, "a/one/one.md", "- gone(a) — broken");
        let (mut e, mut w) = (0, 0);
        lint_corpus(&user, None, &mut e, &mut w);
        assert_eq!((e, w), (0, 1));
    }

    #[test]
    fn an_unrelated_hooks_ways_dir_is_skipped_not_core() {
        let dir = scratch("unrelated").join("hooks/ways");
        way(&dir, "a/one/one.md", "- gone(a) — broken");
        let (mut e, mut w) = (0, 0);
        lint_corpus(&dir, None, &mut e, &mut w);
        assert_eq!((e, w), (0, 0));
    }
}
