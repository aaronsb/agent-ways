use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Every regular file under `root`, following symlinks: a projected or linked
/// way tree is walked through its links. Entries that cannot be read are
/// skipped. Walk order, not sorted.
pub fn files(root: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
}

/// Which `.md` files a way walk yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MdKind {
    /// `{way}.md`: every `.md` file but the checks.
    Ways,
    /// `{way}.check.md`.
    Checks,
    /// Both.
    All,
}

/// True for a check file (`{way}.check.md`), by name.
pub fn is_check(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(".check."))
}

/// The `.md` files of `kind` under `root`: the one way-file walk every reader
/// of a way tree goes through ([`files`], filtered by extension and kind).
pub fn md_files(root: &Path, kind: MdKind) -> impl Iterator<Item = PathBuf> {
    files(root).filter(move |p| {
        p.extension().and_then(|e| e.to_str()) == Some("md")
            && match kind {
                MdKind::Ways => !is_check(p),
                MdKind::Checks => is_check(p),
                MdKind::All => true,
            }
    })
}

/// A discovered way file with its derived identity.
#[derive(Debug, Clone)]
pub struct WayFile {
    /// Absolute path to the way file
    pub path: PathBuf,
    /// Way ID derived from directory structure (e.g., "code/quality")
    pub id: String,
    /// Top-level domain (e.g., "softwaredev")
    pub domain: String,
}

/// Scan a directory for way files (identified by YAML frontmatter with `description:` field).
pub fn scan_ways(root: &Path) -> Result<Vec<WayFile>> {
    Ok(scan_with(root, has_way_frontmatter))
}

/// Like [`scan_ways`], but a way is any non-check `.md` file that opens with a
/// frontmatter fence. Ways that fire on a path, a command or a trigger carry no
/// `description:`; they are ways all the same, and a See Also entry can name one.
/// The same identity rule as [`scan_ways`] applies.
pub fn scan_declared_ways(root: &Path) -> Vec<WayFile> {
    scan_with(root, |p| {
        std::fs::read_to_string(p).is_ok_and(|c| crate::frontmatter::opens_with_fence(&c))
    })
}

fn scan_with(root: &Path, is_way: impl Fn(&Path) -> bool) -> Vec<WayFile> {
    let mut ways: Vec<WayFile> = md_files(root, MdKind::Ways)
        .filter(|p| is_way(p))
        .filter_map(|p| way_from_path(&p, root))
        .collect();
    ways.sort_by(|a, b| a.id.cmp(&b.id));
    ways
}

/// The way a See Also entry `name(domain)` names, among `ways`.
///
/// `domain/name` is tried first (`code/quality(softwaredev)` is way
/// `code/quality` in domain `softwaredev`). Entries that spell the domain out
/// resolve too: `data/migrations(data)` is `migrations` in `data`, and
/// `documentation(documentation)` is the domain-root way. A name that lands in a
/// different domain than the one in parentheses does not resolve.
pub fn resolve_ref<'a>(ways: &'a [WayFile], name: &str, domain: &str) -> Option<&'a WayFile> {
    let find = |id: &str| ways.iter().find(|w| w.domain == domain && w.id == id);
    find(name).or_else(|| {
        name.strip_prefix(domain)
            .and_then(|rest| rest.strip_prefix('/'))
            .and_then(find)
    })
}

/// Check if a file has YAML frontmatter containing a `description:` field.
fn has_way_frontmatter(path: &Path) -> bool {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    crate::frontmatter::split(&content)
        .is_some_and(|(fm, _)| fm.lines().any(|l| l.starts_with("description:")))
}

/// Derive WayFile identity from filesystem path relative to the ways root.
fn way_from_path(path: &Path, root: &Path) -> Option<WayFile> {
    let parent = path.parent()?;
    let rel = parent.strip_prefix(root).ok()?;
    let components: Vec<&str> = rel.components()
        .map(|c| c.as_os_str().to_str().unwrap_or(""))
        .collect();

    if components.is_empty() {
        return None;
    }

    let domain = components[0].to_string();
    let id = if components.len() > 1 {
        components[1..].join("/")
    } else {
        domain.clone()
    };

    Some(WayFile {
        path: path.to_path_buf(),
        id,
        domain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique scratch dir per test, mirroring the crate's temp-dir idiom
    /// (no external tempdir dependency).
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("ways-scanner-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_way(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
    }

    const WAY: &str = "---\ndescription: a way\nvocabulary: a b\n---\nguidance\n";

    #[test]
    fn discovers_way_and_derives_identity() {
        let root = scratch("identity");
        write_way(&root, "softwaredev/code/quality/quality.md", WAY);

        let ways = scan_ways(&root).unwrap();
        assert_eq!(ways.len(), 1);
        assert_eq!(ways[0].domain, "softwaredev");
        assert_eq!(ways[0].id, "code/quality");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn top_level_way_id_falls_back_to_domain() {
        let root = scratch("toplevel");
        write_way(&root, "meta/meta.md", WAY);

        let ways = scan_ways(&root).unwrap();
        assert_eq!(ways.len(), 1);
        assert_eq!(ways[0].domain, "meta");
        assert_eq!(ways[0].id, "meta");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_files_without_description_frontmatter() {
        let root = scratch("nodesc");
        // Frontmatter present but no `description:` → not a way.
        write_way(&root, "d/w/w.md", "---\nvocabulary: a b\n---\nbody\n");
        // No frontmatter at all → not a way.
        write_way(&root, "d/x/x.md", "# just prose\n");

        assert!(scan_ways(&root).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An unclosed block is not frontmatter: the old scanner accepted a
    /// `description:` line anywhere after the opening fence.
    #[test]
    fn unclosed_frontmatter_is_not_a_way() {
        let root = scratch("unclosed");
        write_way(&root, "d/w/w.md", "---\ndescription: a way\nguidance\n");
        assert!(scan_ways(&root).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn md_files_sorts_ways_from_checks() {
        let root = scratch("mdkinds");
        write_way(&root, "d/w/w.md", WAY);
        write_way(&root, "d/w/w.check.md", WAY);
        write_way(&root, "d/w/notes.txt", WAY);
        let names = |kind| {
            let mut v: Vec<String> = md_files(&root, kind)
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        };
        assert_eq!(names(MdKind::Ways), vec!["w.md"]);
        assert_eq!(names(MdKind::Checks), vec!["w.check.md"]);
        assert_eq!(names(MdKind::All), vec!["w.check.md", "w.md"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The walk follows a linked way directory. The attend-signal lookup in
    /// `ways show` walked without following links and missed such a way.
    #[cfg(unix)]
    #[test]
    fn md_files_follows_a_linked_way_dir() {
        let root = scratch("linked");
        let elsewhere = scratch("linked-target");
        write_way(&elsewhere, "w/w.md", WAY);
        std::fs::create_dir_all(root.join("d")).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("w"), root.join("d/w")).unwrap();
        let found: Vec<PathBuf> = md_files(&root, MdKind::Ways).collect();
        assert_eq!(found, vec![root.join("d/w/w.md")]);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    #[test]
    fn skips_check_and_non_md_files() {
        let root = scratch("checkfiles");
        write_way(&root, "d/w/w.md", WAY);
        write_way(&root, "d/w/w.check.md", WAY); // re-fire check, not a way
        write_way(&root, "d/w/notes.txt", WAY); // non-markdown

        let ways = scan_ways(&root).unwrap();
        assert_eq!(ways.len(), 1);
        assert!(ways[0].path.ends_with("w.md"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn results_are_sorted_by_id() {
        let root = scratch("sorted");
        write_way(&root, "z/alpha/alpha.md", WAY);
        write_way(&root, "a/zulu/zulu.md", WAY);
        write_way(&root, "m/mike/mike.md", WAY);

        let ids: Vec<String> = scan_ways(&root).unwrap().into_iter().map(|w| w.id).collect();
        assert_eq!(ids, vec!["alpha", "mike", "zulu"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn declared_ways_include_those_without_a_description() {
        let root = scratch("declared");
        write_way(&root, "d/with/with.md", WAY);
        write_way(&root, "d/files-only/files-only.md", "---\nfiles: x\n---\nbody\n");
        write_way(&root, "d/plain/plain.md", "# no frontmatter\n");

        assert_eq!(scan_ways(&root).unwrap().len(), 1);
        let ids: Vec<String> = scan_declared_ways(&root).into_iter().map(|w| w.id).collect();
        assert_eq!(ids, vec!["files-only", "with"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_ref_handles_every_spelling_and_refuses_a_wrong_domain() {
        let root = scratch("resolve");
        write_way(&root, "softwaredev/code/quality/quality.md", WAY);
        write_way(&root, "meta/trust/trust.md", WAY);
        write_way(&root, "data/migrations/migrations.md", WAY);
        write_way(&root, "documentation/documentation.md", WAY);
        write_way(&root, "research/research.md", WAY);
        let ways = scan_ways(&root).unwrap();

        let id = |n: &str, d: &str| resolve_ref(&ways, n, d).map(|w| w.id.as_str());
        assert_eq!(id("code/quality", "softwaredev"), Some("code/quality"));
        assert_eq!(id("trust", "meta"), Some("trust"));
        assert_eq!(id("data/migrations", "data"), Some("migrations"));
        assert_eq!(id("documentation", "documentation"), Some("documentation"));
        assert_eq!(id("research", "research"), Some("research"));
        assert_eq!(id("research", "softwaredev"), None);
        assert_eq!(id("code/missing", "softwaredev"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_or_missing_root_yields_no_ways() {
        let root = scratch("empty");
        assert!(scan_ways(&root).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
        // Nonexistent root is not an error — WalkDir yields nothing.
        assert!(scan_ways(&root).unwrap().is_empty());
    }
}
