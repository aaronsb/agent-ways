//! Per-language override validation for `*.locales.jsonl` files.
//!
//! Each line is one JSON object with keys from the `locale_stub:`
//! section of `frontmatter-schema.yaml`. Unknown keys are flagged as
//! UNKNOWN; with `--fix`, the offending key is removed from the line.
//! `x-*` prefixed keys are skipped. Language stubs that the linter
//! can't parse as JSON are reported as ERROR.

use anyhow::Result;
use std::path::Path;

use super::schema::{is_reserved_field, Schema};

pub(super) fn lint_locale_stubs(
    dir: &Path,
    ways_dir: &Path,
    schema: &Schema,
    errors: &mut u32,
    warnings: &mut u32,
    fixes: &mut u32,
    fix: bool,
) -> Result<usize> {
    let mut count = 0usize;
    for path in crate::scanner::files(dir) {
        if !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".locales.jsonl")) {
            continue;
        }
        count += 1;
        lint_one_stub_file(&path, ways_dir, schema, errors, warnings, fixes, fix)?;
    }
    Ok(count)
}

fn lint_one_stub_file(
    path: &Path,
    ways_dir: &Path,
    schema: &Schema,
    errors: &mut u32,
    warnings: &mut u32,
    fixes: &mut u32,
    fix: bool,
) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let relpath = path.strip_prefix(ways_dir).unwrap_or(path);
    let rel = relpath.display();

    let mut out_lines: Vec<String> = Vec::with_capacity(content.lines().count());
    let mut modified = false;

    for (lineno, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            out_lines.push(raw_line.to_string());
            continue;
        }

        let mut val: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "  ERROR: {rel} (line {}) — invalid JSON in locale stub",
                    lineno + 1
                );
                *errors += 1;
                out_lines.push(raw_line.to_string());
                continue;
            }
        };

        let Some(obj) = val.as_object_mut() else {
            eprintln!(
                "  ERROR: {rel} (line {}) — locale stub is not a JSON object",
                lineno + 1
            );
            *errors += 1;
            out_lines.push(raw_line.to_string());
            continue;
        };

        let unknown_keys: Vec<String> = obj
            .keys()
            .filter(|k| !schema.locale_stub_fields.contains(k.as_str()) && !is_reserved_field(k))
            .cloned()
            .collect();

        if unknown_keys.is_empty() {
            out_lines.push(raw_line.to_string());
            continue;
        }

        if fix {
            // shift_remove preserves the original key order. Plain
            // .remove() is swap_remove-equivalent even with
            // preserve_order, which would churn git diffs by moving
            // the tail key into the removed slot.
            for key in &unknown_keys {
                obj.shift_remove(key);
            }
            let fixed = serde_json::to_string(&val)
                .unwrap_or_else(|_| raw_line.to_string());
            out_lines.push(fixed);
            modified = true;
            *fixes += unknown_keys.len() as u32;
            for key in &unknown_keys {
                eprintln!(
                    "  FIXED: {rel} (line {}) — removed foreign key '{key}' from locale stub",
                    lineno + 1
                );
            }
        } else {
            for key in &unknown_keys {
                eprintln!(
                    "  UNKNOWN: {rel} (line {}) — unknown key '{key}' in locale stub",
                    lineno + 1
                );
                *warnings += 1;
            }
            out_lines.push(raw_line.to_string());
        }
    }

    if modified {
        let mut written = out_lines.join("\n");
        if content.ends_with('\n') {
            written.push('\n');
        }
        std::fs::write(path, written)?;
    }
    Ok(())
}
