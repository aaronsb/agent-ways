//! ADR-131: project-scope per-way toggles.
//!
//! `ways disable <name>` and `ways enable <name>` edit
//! `{project}/.claude/ways.yaml`, round-tripping comments and unrelated
//! keys by rewriting only the lines inside the `ways:` block.
//!
//! Project scope only — there is no `--global` flag. Default state is
//! enabled (absence of an entry).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const HEADER: &str = "# Project-scope ways overlay — see ADR-115, ADR-131\n";

// ── Public entry points ────────────────────────────────────────

pub fn disable(name: &str) -> Result<()> {
    validate_way_name(name)?;
    warn_if_unknown(name);

    let path = project_overlay_path()?;
    let content = read_or_empty(&path)?;
    let updated = rewrite_block(&content, name, true);
    write_overlay(&path, &updated)?;
    println!("disabled {name} (project: {})", path.display());
    Ok(())
}

pub fn enable(name: &str) -> Result<()> {
    validate_way_name(name)?;

    let path = project_overlay_path()?;
    if !path.exists() {
        println!("{name} is already enabled (no project overlay at {})", path.display());
        return Ok(());
    }
    let content = read_or_empty(&path)?;
    if !is_disabled(&content, name) {
        println!("{name} is already enabled");
        return Ok(());
    }
    let updated = rewrite_block(&content, name, false);
    write_overlay(&path, &updated)?;
    println!("enabled {name} (project: {})", path.display());
    Ok(())
}

pub fn list() -> Result<()> {
    let cfg = crate::config::Config::load(&project_dir());
    if cfg.disabled_ways.is_empty() {
        println!("no ways are disabled for this project");
        return Ok(());
    }
    for w in &cfg.disabled_ways {
        let marker = if way_exists(w) { " " } else { "?" };
        println!("{marker} {w}");
    }
    if cfg.disabled_ways.iter().any(|w| !way_exists(w)) {
        eprintln!("\nentries marked `?` do not match any way currently on disk \
                   (may have been renamed or removed upstream)");
    }
    Ok(())
}

// ── Path resolution ─────────────────────────────────────────────

fn project_dir() -> String {
    std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()))
}

fn project_overlay_path() -> Result<PathBuf> {
    let dir = PathBuf::from(project_dir());
    Ok(dir.join(".claude").join("ways.yaml"))
}

// ── Validation ──────────────────────────────────────────────────

fn validate_way_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("way name cannot be empty");
    }
    if name.starts_with('/') || name.ends_with('/') {
        anyhow::bail!("way name must not start or end with '/'");
    }
    if name.contains("..") || name.contains(':') || name.contains('\n') {
        anyhow::bail!("invalid characters in way name");
    }
    Ok(())
}

fn way_exists(name: &str) -> bool {
    let project = PathBuf::from(project_dir()).join(".claude/ways").join(name);
    if project.is_dir() {
        return true;
    }
    let global = crate::util::home_dir().join(".claude/hooks/ways").join(name);
    global.is_dir()
}

fn warn_if_unknown(name: &str) {
    if !way_exists(name) {
        eprintln!(
            "[ways] warning: '{name}' does not match any way on disk. \
             Writing entry anyway (use `ways disable --list` to audit)."
        );
    }
}

// ── YAML edit ───────────────────────────────────────────────────

fn read_or_empty(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_overlay(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing {}", path.display()))
}

/// Returns true if `name` currently parses to disabled in the given content.
/// Uses the same parser the runtime config uses, so writer/reader agree.
fn is_disabled(content: &str, name: &str) -> bool {
    let mut cfg = crate::config::Config::default();
    cfg.apply_project_ways_overlay_public(content);
    cfg.disabled_ways.iter().any(|w| w == name)
}

/// Rewrite `content` so that `name` is either disabled (`disable=true`) or
/// removed from the `ways:` block (`disable=false`). Preserves comments and
/// every other key by editing only the lines that belong to the way's entry.
pub(crate) fn rewrite_block(content: &str, name: &str, disable: bool) -> String {
    let lines: Vec<&str> = content.lines().collect();

    // Find the `ways:` block (column-0 `ways:` key).
    let ways_start = lines.iter().position(|l| l.trim_end_matches(' ') == "ways:" || matches_ways_key(l));
    let (block_start, block_end, base_indent) = match ways_start {
        Some(s) => {
            let (end, indent) = find_block_end(&lines, s);
            (s, end, indent)
        }
        None => {
            // No `ways:` block. If we're disabling, append one; if enabling, no-op.
            if !disable {
                return content.to_string();
            }
            let mut out = content.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if out.is_empty() {
                out.push_str(HEADER);
            }
            out.push_str("ways:\n");
            out.push_str(&format!("  {name}: false\n"));
            return out;
        }
    };

    // Find existing entry for this way inside the block.
    let entry_range = find_entry(&lines, block_start + 1, block_end, name, base_indent);

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 2);
    out.extend(lines[..=block_start].iter().map(|s| s.to_string()));

    for (i, line) in lines[block_start + 1..block_end].iter().enumerate() {
        let abs = block_start + 1 + i;
        match entry_range {
            Some((s, e)) if abs >= s && abs < e => {
                // Skip — this is the existing entry's lines (replaced below or removed).
                continue;
            }
            _ => out.push((*line).to_string()),
        }
    }

    if disable {
        // Insert or re-insert the entry at the end of the block.
        let indent = format!("{:width$}", "", width = base_indent + 2);
        out.push(format!("{indent}{name}: false"));
    }

    // Tail (anything after the `ways:` block).
    for line in &lines[block_end..] {
        out.push((*line).to_string());
    }

    // If we just emptied the block, drop the `ways:` header too.
    if !disable && block_is_empty(&out, block_start) {
        out.remove(block_start);
    }

    let mut s = out.join("\n");
    if content.ends_with('\n') || !s.is_empty() {
        s.push('\n');
    }
    s
}

fn matches_ways_key(line: &str) -> bool {
    // Column-0 `ways:` with optional trailing comment / whitespace.
    let trimmed = line.trim_end();
    if let Some(rest) = trimmed.strip_prefix("ways:") {
        return rest.is_empty() || rest.starts_with(' ') || rest.starts_with('#');
    }
    false
}

/// Returns (end_line_exclusive, base_indent_of_ways_key).
fn find_block_end(lines: &[&str], start: usize) -> (usize, usize) {
    // `ways:` is at column 0, so base_indent = 0. Block ends at next column-0
    // non-blank, non-comment line (i.e., next sibling key).
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.is_empty() || line.trim().starts_with('#') {
            continue;
        }
        let first = line.chars().next().unwrap_or(' ');
        if first != ' ' && first != '\t' {
            end = i;
            break;
        }
    }
    (end, 0)
}

/// Find the line range [start, end) covering `name`'s entry inside the block.
/// Handles both shorthand (`name: false`) and long-form (`name:\n  enabled: false`).
fn find_entry(
    lines: &[&str],
    block_start: usize,
    block_end: usize,
    name: &str,
    base_indent: usize,
) -> Option<(usize, usize)> {
    let entry_indent = base_indent + 2;
    let prefix = format!("{:width$}", "", width = entry_indent);
    let needle = format!("{prefix}{name}:");

    for (i, line) in lines[block_start..block_end].iter().enumerate() {
        let abs = block_start + i;
        if line.trim_start().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        // Match if the line starts with `<prefix><name>:` AND nothing precedes
        // the entry key (no extra indent — would mean it's a sub-key).
        if line.starts_with(&needle) {
            // Determine entry end: walk forward over deeper-indented lines.
            let mut end = abs + 1;
            while end < block_end {
                let l = lines[end];
                if l.trim_start().is_empty() {
                    end += 1;
                    continue;
                }
                let this_indent = l.len() - l.trim_start().len();
                if this_indent > entry_indent {
                    end += 1;
                } else {
                    break;
                }
            }
            return Some((abs, end));
        }
    }
    None
}

fn block_is_empty(lines: &[String], block_start: usize) -> bool {
    for line in lines.iter().skip(block_start + 1) {
        if line.is_empty() || line.trim().starts_with('#') {
            continue;
        }
        let first = line.chars().next().unwrap_or(' ');
        if first == ' ' || first == '\t' {
            return false; // still has children
        }
        return true; // hit next sibling key
    }
    true
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disable_creates_block_in_empty_file() {
        let out = rewrite_block("", "itops/incident", true);
        assert!(out.contains("ways:"));
        assert!(out.contains("itops/incident: false"));
    }

    #[test]
    fn disable_appends_to_existing_block() {
        let input = "ways:\n  meta/introspection: false\n";
        let out = rewrite_block(input, "itops/incident", true);
        assert!(out.contains("meta/introspection: false"));
        assert!(out.contains("itops/incident: false"));
    }

    #[test]
    fn enable_removes_entry_keeps_others() {
        let input = "ways:\n  meta/introspection: false\n  itops/incident: false\n";
        let out = rewrite_block(input, "itops/incident", false);
        assert!(out.contains("meta/introspection: false"));
        assert!(!out.contains("itops/incident"));
    }

    #[test]
    fn enable_removes_ways_block_when_last_entry() {
        let input = "ways:\n  itops/incident: false\n";
        let out = rewrite_block(input, "itops/incident", false);
        assert!(!out.contains("ways:"));
        assert!(!out.contains("itops/incident"));
    }

    #[test]
    fn preserves_unrelated_keys_and_comments() {
        let input = "\
# top-level comment
language: en

ways:
  # comment inside ways
  meta/introspection: false

parent_boost_floor: 0.40
";
        let out = rewrite_block(input, "itops/incident", true);
        assert!(out.contains("# top-level comment"));
        assert!(out.contains("# comment inside ways"));
        assert!(out.contains("language: en"));
        assert!(out.contains("parent_boost_floor: 0.40"));
        assert!(out.contains("meta/introspection: false"));
        assert!(out.contains("itops/incident: false"));
    }

    #[test]
    fn handles_longform_entry_replacement() {
        // Long-form entry should be replaced by shorthand when re-disabled
        // (no harm; the schema accepts either).
        let input = "\
ways:
  itops/incident:
    enabled: false
";
        let out = rewrite_block(input, "itops/incident", true);
        // Should still contain one (and only one) disable for itops/incident.
        let count = out.matches("itops/incident").count();
        assert_eq!(count, 1);
        assert!(out.contains("itops/incident: false"));
    }

    #[test]
    fn enable_when_block_missing_is_noop() {
        let input = "language: en\n";
        let out = rewrite_block(input, "itops/incident", false);
        assert_eq!(out, input);
    }

    #[test]
    fn round_trip_through_real_config_load() {
        // Writer's output must parse back to the same disabled set.
        let out = rewrite_block("", "itops/incident", true);
        let mut cfg = crate::config::Config::default();
        cfg.apply_project_ways_overlay_public(&out);
        assert_eq!(cfg.disabled_ways, vec!["itops/incident".to_string()]);

        let out2 = rewrite_block(&out, "meta/introspection", true);
        let mut cfg2 = crate::config::Config::default();
        cfg2.apply_project_ways_overlay_public(&out2);
        assert_eq!(cfg2.disabled_ways.len(), 2);
    }
}
