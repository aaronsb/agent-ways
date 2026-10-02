//! Permission audit — diff requires: fields against settings.json grants (ADR-116).

use agent_fmt::permissions;
use anyhow::Result;
use std::path::{Path, PathBuf};


/// Run `ways author permissions`.
pub fn audit(global: bool) -> Result<()> {
    let ways_dir = crate::paths::projected_ways_root();
    let settings_path = crate::paths::settings_json();

    // Determine scan dirs (same logic as lint)
    let mut scan_dirs = vec![ways_dir.clone()];
    if !global {
        let project_dir = crate::util::project_root();
        if let Some(ref pd) = project_dir {
            let project_ways = PathBuf::from(pd).join(".claude/ways");
            if project_ways.is_dir() {
                scan_dirs.push(project_ways);
            }
        }
    }

    // Collect (way_id, requires) pairs
    let mut requirements: Vec<(String, Vec<String>)> = Vec::new();
    for scan_dir in &scan_dirs {
        collect_way_requirements(scan_dir, &ways_dir, &mut requirements)?;
    }

    // Load settings.json grants
    let grants = permissions::load_settings_permissions(&settings_path);

    if grants.is_empty() {
        eprintln!("Warning: no permissions found in {}", settings_path.display());
    }

    // Run audit
    let results = permissions::audit(&requirements, &grants);

    // Check for trusted-project-macros deprecation
    let tpm_path = crate::paths::trusted_project_macros();
    let has_tpm = tpm_path.is_file();

    // Display results
    permissions::display_audit("Permissions Audit", "Way", &results, has_tpm);

    Ok(())
}

/// Scan way files and collect (way_id, requires_list) pairs.
fn collect_way_requirements(
    dir: &Path,
    ways_dir: &Path,
    out: &mut Vec<(String, Vec<String>)>,
) -> Result<()> {
    for path in crate::scanner::md_files(dir, crate::scanner::MdKind::Ways) {
        let path = path.as_path();

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let Some((fm, _)) = crate::frontmatter::split(&content) else {
            continue;
        };

        if let Some(reqs) = extract_requires(&fm) {
            if !reqs.is_empty() {
                let way_id = path
                    .strip_prefix(ways_dir)
                    .unwrap_or(path)
                    .with_extension("")
                    .display()
                    .to_string();
                out.push((way_id, reqs));
            }
        }
    }
    Ok(())
}

/// Parse requires: field from frontmatter.
/// Also used by lint.rs for validation and --fix.
pub fn extract_requires(fm: &str) -> Option<Vec<String>> {
    let prefix = "requires:";
    for (i, line) in fm.lines().enumerate() {
        if !line.starts_with(prefix) {
            continue;
        }
        let rest = line[prefix.len()..].trim();

        // Inline array: requires: ["Bash(gh:*)", "Read"]
        if rest.starts_with('[') && rest.ends_with(']') {
            let inner = &rest[1..rest.len() - 1];
            let items: Vec<String> = inner
                .split(',')
                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
                .collect();
            return Some(items);
        }

        // YAML list
        if rest.is_empty() {
            let mut items = Vec::new();
            for subsequent in fm.lines().skip(i + 1) {
                let trimmed = subsequent.trim();
                if let Some(val) = trimmed.strip_prefix("- ") {
                    items.push(val.trim_matches('"').trim_matches('\'').to_string());
                } else {
                    break;
                }
            }
            return Some(items);
        }

        return None;
    }
    None
}

