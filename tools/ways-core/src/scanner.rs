use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// A discovered way file with its derived identity.
#[derive(Debug)]
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
    let mut ways = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        // Skip check files
        if path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".check."))
        {
            continue;
        }

        if has_way_frontmatter(path) {
            if let Some(way) = way_from_path(path, root) {
                ways.push(way);
            }
        }
    }

    ways.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ways)
}

/// Check if a file has YAML frontmatter containing a `description:` field.
fn has_way_frontmatter(path: &Path) -> bool {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let mut in_frontmatter = false;
    for (i, line) in content.lines().enumerate() {
        if i == 0 && line == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if line == "---" {
                return false; // closed without description
            }
            if line.starts_with("description:") {
                return true;
            }
        }
    }
    false
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
    fn empty_or_missing_root_yields_no_ways() {
        let root = scratch("empty");
        assert!(scan_ways(&root).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
        // Nonexistent root is not an error — WalkDir yields nothing.
        assert!(scan_ways(&root).unwrap().is_empty());
    }
}
