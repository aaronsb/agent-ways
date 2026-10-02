//! Directory walk that feeds way/check files into the per-file
//! validator. Also the entry point for schema-based locale stub
//! validation once the main pass is done.

use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::per_file::lint_file;
use super::schema::Schema;

pub(super) fn scan_and_lint(
    dir: &Path,
    ways_dir: &Path,
    schema: &Schema,
    errors: &mut u32,
    warnings: &mut u32,
    fixes: &mut u32,
    fix: bool,
) -> Result<usize> {
    let mut files: Vec<(PathBuf, bool)> = Vec::new(); // (path, is_check)

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        // Check if it has frontmatter
        match std::fs::read_to_string(path) {
            Ok(c) if crate::frontmatter::opens_with_fence(&c) => {}
            _ => continue,
        }

        // Excluded paths — backup/sync tool artifacts that pollute the corpus
        if crate::util::is_excluded_path(path, &schema.excluded_path_segments) {
            let relpath = path.strip_prefix(ways_dir).unwrap_or(path);
            eprintln!(
                "  WARNING: {} — excluded path (backup/sync/tool artifact), skipped",
                relpath.display()
            );
            *warnings += 1;
            continue;
        }

        let is_check = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".check."));

        files.push((path.to_path_buf(), is_check));
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));

    for (path, is_check) in &files {
        lint_file(path, *is_check, ways_dir, schema, errors, warnings, fixes, fix)?;
    }

    Ok(files.len())
}
