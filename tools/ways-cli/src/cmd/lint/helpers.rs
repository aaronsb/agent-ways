//! Low-level frontmatter string operations used by the linter.
//!
//! These are the string-level primitives — extract the raw YAML block
//! between `---` markers, pull a field name out of a line, ask whether
//! a key exists. No schema knowledge, no per-file validation logic,
//! just text handling. Isolated here so the per-file validator
//! (`per_file.rs`) can stay focused on the lint rules rather than the
//! shape of the frontmatter format.

pub(super) fn extract_field_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    // Only top-level fields (no leading whitespace for frontmatter fields)
    if trimmed.len() != line.len() && !line.starts_with("  ") {
        return None;
    }
    // Top-level: starts with a-z
    if line.starts_with(|c: char| c.is_ascii_lowercase()) {
        let colon_pos = line.find(':')?;
        let field = &line[..colon_pos];
        if field.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '-') {
            return Some(field);
        }
    }
    None
}

pub(super) fn extract_indented_field_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let colon_pos = trimmed.find(':')?;
    let field = &trimmed[..colon_pos];
    if field.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '-') {
        Some(field)
    } else {
        None
    }
}

pub(super) fn has_field(fm: &str, name: &str) -> bool {
    let prefix = format!("{name}:");
    fm.lines().any(|l| l.starts_with(&prefix))
}

pub(super) fn get_field_value(fm: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    for line in fm.lines() {
        if let Some(rest) = line.strip_prefix(&prefix) {
            let val = rest.trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

pub(super) fn extract_when_block(fm: &str) -> String {
    let mut lines = Vec::new();
    let mut in_when = false;
    for line in fm.lines() {
        if line == "when:" {
            in_when = true;
            continue;
        }
        if in_when {
            if line.starts_with("  ") {
                lines.push(line);
            } else if !line.is_empty() {
                break;
            }
        }
    }
    lines.join("\n")
}

pub(super) fn count_multiline_yaml(fm: &str) -> usize {
    fm.lines()
        .filter(|l| l.ends_with(": >") || l.ends_with(": |"))
        .count()
}

/// Collapse multi-line YAML values (> or |) in frontmatter to single lines.
pub(super) fn fix_multiline_yaml(content: &str) -> Option<String> {
    let close_idx = ways_core::frontmatter::closing_fence_line(content)?;
    let mut lines = content.lines().enumerate().skip(1).peekable();
    let mut result = vec!["---".to_string()];
    let mut in_frontmatter = true;

    while let Some((i, line)) = lines.next() {
        if i == close_idx {
            in_frontmatter = false;
            result.push("---".to_string());
            continue;
        }

        if in_frontmatter {
            if line.ends_with(": >") || line.ends_with(": |") {
                let field_prefix = &line[..line.len() - 3]; // strip ": >" or ": |", keep "field"
                let mut parts = Vec::new();
                while let Some(&(_, next)) = lines.peek() {
                    if next.starts_with("  ") || next.is_empty() {
                        let trimmed = next.trim();
                        if !trimmed.is_empty() {
                            parts.push(trimmed.to_string());
                        }
                        lines.next();
                    } else {
                        break;
                    }
                }
                let collapsed = parts.join(" ");
                result.push(format!("{field_prefix}: {collapsed}"));
            } else {
                result.push(line.to_string());
            }
        } else {
            result.push(line.to_string());
        }
    }

    let mut out = result.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

/// Remove a top-level frontmatter field (and any indented continuation
/// lines) from a way file's raw content. Returns `None` if the field
/// isn't present at top level. Only touches the frontmatter between
/// the first and second `---` markers.
pub(super) fn remove_top_level_field(content: &str, field_name: &str) -> Option<String> {
    let field_prefix = format!("{field_name}:");
    let lines: Vec<&str> = content.lines().collect();

    // Find frontmatter bounds.
    let close_idx = ways_core::frontmatter::closing_fence_line(content)?;

    // Find the target field line.
    let mut target_idx = None;
    for (i, line) in lines.iter().enumerate().take(close_idx).skip(1) {
        if line.starts_with(&field_prefix) || *line == field_name {
            target_idx = Some(i);
            break;
        }
    }
    let start = target_idx?;

    // Consume indented continuation lines (block value or nested mapping).
    let mut end = start + 1;
    while end < close_idx {
        let line = lines[end];
        if line.starts_with(' ') || line.starts_with('\t') || line.is_empty() {
            end += 1;
        } else {
            break;
        }
    }
    // If the trailing block ends with a blank line right before another
    // top-level key, keep that blank line alone — it wasn't ours. Rewind
    // so we don't eat it.
    while end > start + 1 && lines[end - 1].is_empty() {
        end -= 1;
    }

    let mut out: Vec<&str> = Vec::with_capacity(lines.len() - (end - start));
    out.extend_from_slice(&lines[..start]);
    out.extend_from_slice(&lines[end..]);

    let mut s = out.join("\n");
    if content.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

/// Remove a `when:`-scoped sub-field line from frontmatter. The field
/// must appear as a single indented line (no nested mappings under a
/// when-sub-field in the current schema). Returns `None` if not found.
pub(super) fn remove_when_subfield(content: &str, field_name: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let close_idx = ways_core::frontmatter::closing_fence_line(content)?;

    let mut in_when = false;
    let mut target_idx = None;
    for (i, line) in lines.iter().enumerate().take(close_idx).skip(1) {
        if *line == "when:" {
            in_when = true;
            continue;
        }
        if in_when {
            // `when:` block ends at the first non-indented non-empty line.
            if !line.starts_with("  ") && !line.is_empty() {
                break;
            }
            let trimmed = line.trim_start();
            if let Some(colon) = trimmed.find(':') {
                let name = &trimmed[..colon];
                if name == field_name {
                    target_idx = Some(i);
                    break;
                }
            }
        }
    }
    let idx = target_idx?;
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() - 1);
    out.extend_from_slice(&lines[..idx]);
    out.extend_from_slice(&lines[idx + 1..]);

    let mut s = out.join("\n");
    if content.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_top_level_scalar_field() {
        let src = "---\ndescription: x\nbogus: should go\nthreshold: 2.0\n---\nbody\n";
        let out = remove_top_level_field(src, "bogus").expect("found");
        assert_eq!(out, "---\ndescription: x\nthreshold: 2.0\n---\nbody\n");
    }

    #[test]
    fn remove_top_level_block_field_with_continuation() {
        let src = "---\ndescription: x\ncurve:\n  type: Exponential\n  half_life: 100\nthreshold: 2.0\n---\nbody\n";
        let out = remove_top_level_field(src, "curve").expect("found");
        assert_eq!(out, "---\ndescription: x\nthreshold: 2.0\n---\nbody\n");
    }

    #[test]
    fn remove_top_level_missing_returns_none() {
        let src = "---\ndescription: x\n---\nbody\n";
        assert!(remove_top_level_field(src, "nope").is_none());
    }

    #[test]
    fn remove_when_subfield_basic() {
        let src = "---\ndescription: x\nwhen:\n  project: /foo\n  bogus: nope\n---\nbody\n";
        let out = remove_when_subfield(src, "bogus").expect("found");
        assert_eq!(
            out,
            "---\ndescription: x\nwhen:\n  project: /foo\n---\nbody\n"
        );
    }
}
