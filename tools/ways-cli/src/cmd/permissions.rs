//! Permission audit — diff requires: fields against settings.json grants (ADR-116).

use agent_fmt::permissions;
use anyhow::Result;
use std::path::Path;


/// Run `ways author permissions`.
pub fn audit(global: bool) -> Result<()> {
    let ways_dir = crate::paths::shipped_ways_root();
    let settings_path = crate::paths::settings_json();

    // Every root the engine reads; --global leaves the project's out.
    let project_dir = if global { None } else { crate::util::project_root() };
    let project_ways = project_dir.as_deref().map(|p| Path::new(p).join(".claude/ways"));
    let user_ways = crate::paths::user_ways_root();
    let roots: Vec<(&str, std::path::PathBuf)> = crate::paths::ways_roots(project_dir.as_deref().map(Path::new))
        .into_iter()
        .map(|r| {
            let label = if Some(&r) == project_ways.as_ref() {
                "project:"
            } else if r == user_ways && r != ways_dir {
                "user:"
            } else {
                ""
            };
            (label, r)
        })
        .collect();

    // Collect (way_id, requires) pairs
    let mut requirements: Vec<(String, Vec<String>)> = Vec::new();
    collect_requirements(&roots, &mut requirements);

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

/// Collect (way_id, requires_list) pairs from `roots`, in precedence order.
/// A way is audited once, by the copy that resolves: a way in an earlier root
/// shadows the same directory in a later one. Ids are relative to their own
/// root, with the root's label in front (empty for the shipped root).
fn collect_requirements(roots: &[(&str, std::path::PathBuf)], out: &mut Vec<(String, Vec<String>)>) {
    let mut claimed: std::collections::HashSet<String> = Default::default();
    for (label, root) in roots {
        let mut here: Vec<String> = Vec::new();
        for path in crate::scanner::md_files(root, crate::scanner::MdKind::Ways) {
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let Some((fm, _)) = crate::frontmatter::split(&content) else { continue };
            let Ok(rel) = path.strip_prefix(root) else { continue };
            let dir_id = rel.parent().map(|p| p.display().to_string()).unwrap_or_default();
            if claimed.contains(&dir_id) {
                continue;
            }
            here.push(dir_id);
            if let Some(reqs) = extract_requires(&fm).filter(|r| !r.is_empty()) {
                out.push((format!("{label}{}", rel.with_extension("").display()), reqs));
            }
        }
        claimed.extend(here);
    }
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


#[cfg(test)]
mod tests {
    use super::collect_requirements;

    fn way(root: &std::path::Path, id: &str, requires: &str) {
        let leaf = id.rsplit('/').next().unwrap();
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{leaf}.md")), format!("---\ndescription: x\n{requires}---\n# x\n")).unwrap();
    }

    #[test]
    fn ids_are_relative_to_their_root_and_shadowed_copies_are_skipped() {
        let base = std::env::temp_dir().join(format!("ways-perm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (proj, user, ship) = (base.join("proj"), base.join("user"), base.join("ship"));
        way(&proj, "fx/shared", "requires: [\"Bash(project:*)\"]\n");
        way(&user, "fx/shared", "requires: [\"Bash(user:*)\"]\n");
        way(&user, "fx/uonly", "requires: [\"Read\"]\n");
        way(&ship, "fx/shared", "requires: [\"Bash(ship:*)\"]\n");
        way(&ship, "fx/sonly", "requires: [\"Edit\"]\n");
        let roots = vec![("project:", proj), ("user:", user), ("", ship)];
        let mut out = Vec::new();
        collect_requirements(&roots, &mut out);
        let mut ids: Vec<&str> = out.iter().map(|(i, _)| i.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["fx/sonly/sonly", "project:fx/shared/shared", "user:fx/uonly/uonly"]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
