//! `attend config lint` — schema-driven validation of attend config
//! files (user scope + project scope).
//!
//! Pattern matches ways lint deliberately: the same UNKNOWN/DEPRECATED
//! warning categories, the same `x-*` escape hatch, the same `--fix`
//! semantics that surgically remove offending lines without perturbing
//! YAML formatting.
//!
//! The runtime loader in `config.rs` is intentionally forgiving —
//! unknown keys are silently ignored so a typo never crashes attend at
//! startup. This module is the opposite: strict validation that
//! surfaces those typos and offers to remove them.

use std::path::{Path, PathBuf};

/// Known top-level sections of attend config.
const SECTIONS: &[&str] = &["governor", "engagement", "cleanup", "sensors"];

/// Valid keys under `governor:`.
const GOVERNOR_KEYS: &[&str] = &["base_cooldown", "max_per_window", "rate_window"];

/// Valid keys under `engagement:`. See ADR-123 + docs/attend-and-monitor/
/// configuration.md for the yaml-to-runtime mapping.
const ENGAGEMENT_KEYS: &[&str] = &[
    "burst_threshold",
    "step_multiplier",
    "absolute_refractory",
    "decay_per_minute",
    "peer_activity_window",
];

/// Engagement keys that are still parsed for back-compat but have no
/// runtime effect under ADR-123. Currently empty — `burst_window` was
/// the only entry, and ADR-123 phase 2 (issue #50) removed it from the
/// parser entirely, so `attend run` now hard-rejects the legacy key at
/// load time via `config::detect_legacy_burst_window`. The linter still
/// flags a leftover `burst_window:` line as UNKNOWN (same as any foreign
/// key) and `--fix` removes it surgically, which is the supported
/// migration path. This array stays declared so the classification
/// pipeline keeps the DEPRECATED category wired up for the next time a
/// soft deprecation is needed.
const ENGAGEMENT_DEPRECATED: &[(&str, &str)] = &[];

/// Valid keys under `cleanup:`. `retention` was removed with the switch to
/// project-liveness reaping (issue #141) — a leftover `retention:` is now
/// flagged UNKNOWN and `--fix` removes it, same as any foreign key. The
/// retired `signals:` section is likewise off the schema (see SECTIONS).
const CLEANUP_KEYS: &[&str] = &["enabled", "interval"];

/// Valid per-sensor properties (indent 4 under any sensor block).
const SENSOR_KEYS: &[&str] = &[
    "interval",
    "min_interval",
    "threshold",
    "decay_threshold",
    "enabled",
    "script",
    "requires",
    "watch",
];

#[derive(Default)]
struct LintContext {
    errors: u32,
    warnings: u32,
    fixes: u32,
}

/// Top-level entry point for `attend config lint`.
pub fn run(fix: bool, check: bool) -> i32 {
    let user_path = user_config_path();
    let project_path = project_config_path();

    eprintln!();
    eprintln!("Attend Config Lint");
    eprintln!();

    let mut ctx = LintContext::default();

    if user_path.exists() {
        eprintln!("User:    {}", user_path.display());
        lint_one_file(&user_path, fix, &mut ctx);
    } else {
        eprintln!("User:    {} (not present — using defaults)", user_path.display());
    }

    if project_path.exists() {
        eprintln!("Project: {}", project_path.display());
        lint_one_file(&project_path, fix, &mut ctx);
    } else {
        eprintln!("Project: {} (not present)", project_path.display());
    }

    eprintln!();
    if ctx.fixes > 0 {
        eprintln!("Fixed: {} issue(s)", ctx.fixes);
    }
    eprintln!(
        "Summary: {} errors, {} warnings",
        ctx.errors, ctx.warnings
    );

    if check && ctx.errors > 0 {
        return 1;
    }
    0
}

fn user_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", home));
    PathBuf::from(config_dir).join("attend").join("config.yaml")
}

fn project_config_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join(".claude").join("attend.yaml")
}

/// Schema-escape hatch. Fields starting with `x-` are intentionally
/// foreign and left alone — same convention as the ways linter.
fn is_reserved_field(name: &str) -> bool {
    name.starts_with("x-")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyClass {
    Known,
    Deprecated(&'static str),
    Unknown,
}

fn classify_section_key(section: &str, key: &str) -> KeyClass {
    if is_reserved_field(key) {
        return KeyClass::Known;
    }
    match section {
        "governor" => {
            if GOVERNOR_KEYS.contains(&key) {
                KeyClass::Known
            } else {
                KeyClass::Unknown
            }
        }
        "engagement" => {
            if let Some((_, reason)) =
                ENGAGEMENT_DEPRECATED.iter().find(|(k, _)| *k == key)
            {
                KeyClass::Deprecated(reason)
            } else if ENGAGEMENT_KEYS.contains(&key) {
                KeyClass::Known
            } else {
                KeyClass::Unknown
            }
        }
        "cleanup" => {
            if CLEANUP_KEYS.contains(&key) {
                KeyClass::Known
            } else {
                KeyClass::Unknown
            }
        }
        _ => KeyClass::Known, // sensors handled separately
    }
}

fn classify_sensor_property(key: &str) -> KeyClass {
    if is_reserved_field(key) {
        return KeyClass::Known;
    }
    if SENSOR_KEYS.contains(&key) {
        KeyClass::Known
    } else {
        KeyClass::Unknown
    }
}

fn lint_one_file(path: &Path, fix: bool, ctx: &mut LintContext) {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  ERROR: could not read {}: {}", path.display(), e);
            ctx.errors += 1;
            return;
        }
    };

    let rel = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<config>");

    // Track section and sensor context as we walk the file. We never
    // modify the content in-place during the scan — we accumulate the
    // indices of removable lines and rewrite once at the end so fix
    // passes are line-exact and preserve all other formatting.
    let mut current_section = String::new();
    let mut current_sensor = String::new();
    let mut indent_of_current_sensor: usize = 0;
    let mut removable_lines: Vec<usize> = Vec::new();

    for (lineno, line) in content.lines().enumerate() {
        let trimmed_start = line.trim_start();
        if trimmed_start.is_empty() || trimmed_start.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed_start.len();
        let trimmed = trimmed_start.trim();

        // Block-form list items under a multi-line key. Not a key line.
        if trimmed.starts_with("- ") {
            continue;
        }

        // Top-level section header (indent 0, ends with colon).
        if indent == 0 && trimmed.ends_with(':') {
            let name = trimmed.trim_end_matches(':');
            if SECTIONS.contains(&name) || is_reserved_field(name) {
                current_section = name.to_string();
                current_sensor.clear();
            } else {
                eprintln!(
                    "  UNKNOWN: {rel} (line {}) — unknown top-level section '{name}'",
                    lineno + 1
                );
                ctx.warnings += 1;
                // Can't safely remove an entire section via line-surgery
                // without also dropping its nested contents. Leave it;
                // the user decides.
                current_section = String::new();
                current_sensor.clear();
            }
            continue;
        }

        // Under `sensors:` we have a second level of nesting — sensor
        // name lines (indent 2, ending with `:`) and property lines
        // (indent 4+). Other sections only have key-value at indent 2.
        if current_section == "sensors" {
            if indent == 2 && trimmed.ends_with(':') {
                let raw = trimmed.trim_end_matches(':').trim();
                // `+name:` and `-name:` are valid sensor-level directives.
                // The name itself isn't schema-checked here — runtime
                // resolution handles missing sensors.
                let name = raw
                    .trim_start_matches('+')
                    .trim_start_matches('-')
                    .trim();
                current_sensor = name.to_string();
                indent_of_current_sensor = indent;
                continue;
            }
            if indent >= 4 && !current_sensor.is_empty() {
                if let Some((key, _)) = trimmed.split_once(':') {
                    let key = key.trim();
                    match classify_sensor_property(key) {
                        KeyClass::Known => {}
                        KeyClass::Deprecated(reason) => {
                            report_deprecated(rel, lineno, key, reason, fix, ctx);
                            if fix {
                                removable_lines.push(lineno);
                            }
                        }
                        KeyClass::Unknown => {
                            report_unknown(rel, lineno, &format!("sensors.{current_sensor}.{key}"), fix, ctx);
                            if fix {
                                removable_lines.push(lineno);
                            }
                        }
                    }
                }
                continue;
            }
            // Falling out of sensor scope — reset.
            if indent <= indent_of_current_sensor && !current_sensor.is_empty() {
                current_sensor.clear();
            }
            continue;
        }

        // Other sections: any indent-2 `key: value` line.
        if indent == 2 && !current_section.is_empty() {
            if let Some((key, _)) = trimmed.split_once(':') {
                let key = key.trim();
                match classify_section_key(&current_section, key) {
                    KeyClass::Known => {}
                    KeyClass::Deprecated(reason) => {
                        report_deprecated(
                            rel,
                            lineno,
                            &format!("{current_section}.{key}"),
                            reason,
                            fix,
                            ctx,
                        );
                        if fix {
                            removable_lines.push(lineno);
                        }
                    }
                    KeyClass::Unknown => {
                        report_unknown(
                            rel,
                            lineno,
                            &format!("{current_section}.{key}"),
                            fix,
                            ctx,
                        );
                        if fix {
                            removable_lines.push(lineno);
                        }
                    }
                }
            }
        }
    }

    if fix && !removable_lines.is_empty() {
        let lines: Vec<&str> = content.lines().collect();
        let mut drop_set: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        for &anchor in &removable_lines {
            let (start, end) = expand_block_at(&lines, anchor);
            for i in start..end {
                drop_set.insert(i);
            }
        }
        let mut drops: Vec<usize> = drop_set.into_iter().collect();
        drops.sort_unstable();
        let new_content = rewrite_without_lines(&content, &drops);
        if let Err(e) = std::fs::write(path, &new_content) {
            eprintln!("  ERROR: could not write fix to {}: {}", path.display(), e);
            ctx.errors += 1;
        }
    }
}

fn report_unknown(rel: &str, lineno: usize, key: &str, fix: bool, ctx: &mut LintContext) {
    if fix {
        eprintln!(
            "  FIXED: {rel} (line {}) — removed foreign key '{key}'",
            lineno + 1
        );
        ctx.fixes += 1;
    } else {
        eprintln!(
            "  UNKNOWN: {rel} (line {}) — unknown key '{key}'",
            lineno + 1
        );
        ctx.warnings += 1;
    }
}

fn report_deprecated(
    rel: &str,
    lineno: usize,
    key: &str,
    reason: &str,
    fix: bool,
    ctx: &mut LintContext,
) {
    if fix {
        eprintln!(
            "  FIXED: {rel} (line {}) — removed deprecated key '{key}' ({reason})",
            lineno + 1
        );
        ctx.fixes += 1;
    } else {
        eprintln!(
            "  DEPRECATED: {rel} (line {}) — '{key}': {reason} (run --fix to remove)",
            lineno + 1
        );
        ctx.warnings += 1;
    }
}

/// Given an anchor line index, return the range `[start, end)` of lines
/// to drop: the anchor plus its indented continuation. Continuation is
/// every following line that is either blank or has strictly deeper
/// indentation than the anchor — so removing a block-valued key pulls
/// its nested mapping or list items out with it. Trailing blank lines
/// inside that range are rewound so a blank separator before the next
/// sibling key stays put instead of being silently coalesced.
///
/// Mirrors the continuation semantic in ways-cli's
/// `cmd::lint::helpers::remove_top_level_field`, generalized to any
/// indent level so attend can apply it to nested section keys and
/// sensor properties.
fn expand_block_at(lines: &[&str], anchor: usize) -> (usize, usize) {
    if anchor >= lines.len() {
        return (anchor, anchor);
    }
    let anchor_line = lines[anchor];
    let anchor_indent = anchor_line.len() - anchor_line.trim_start().len();

    let mut end = anchor + 1;
    while end < lines.len() {
        let line = lines[end];
        if line.is_empty() {
            end += 1;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent > anchor_indent {
            end += 1;
        } else {
            break;
        }
    }
    while end > anchor + 1 && lines[end - 1].is_empty() {
        end -= 1;
    }
    (anchor, end)
}

/// Remove a top-level (indent-0) YAML key plus any indented
/// continuation. Returns `None` if the field isn't present at the top
/// level. Semantic parity with ways-cli's
/// `cmd::lint::helpers::remove_top_level_field` so both tools apply the
/// same block-removal logic on equivalent fixtures. The attend fix
/// pipeline doesn't currently remove top-level keys (top level is
/// sections, which `--fix` never drops), but this primitive exists so
/// future schema changes can reuse it directly.
#[allow(dead_code)]
fn remove_top_level_field(content: &str, field_name: &str) -> Option<String> {
    let field_prefix = format!("{field_name}:");
    let lines: Vec<&str> = content.lines().collect();

    let anchor = lines
        .iter()
        .position(|l| l.starts_with(&field_prefix) || *l == field_name)?;
    // Must actually be top-level (no leading whitespace).
    if lines[anchor]
        .chars()
        .next()
        .map(|c| c.is_whitespace())
        .unwrap_or(false)
    {
        return None;
    }
    let (start, end) = expand_block_at(&lines, anchor);
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len() - (end - start));
    kept.extend_from_slice(&lines[..start]);
    kept.extend_from_slice(&lines[end..]);
    let mut out = kept.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

/// Rewrite `content` with the given line indices (0-based) removed.
/// Preserves line endings and the trailing newline of the original.
fn rewrite_without_lines(content: &str, drop_indices: &[usize]) -> String {
    let drop_set: std::collections::HashSet<usize> = drop_indices.iter().copied().collect();
    let kept: Vec<&str> = content
        .lines()
        .enumerate()
        .filter_map(|(i, l)| if drop_set.contains(&i) { None } else { Some(l) })
        .collect();
    let mut out = kept.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_governor_keys() {
        assert_eq!(
            classify_section_key("governor", "base_cooldown"),
            KeyClass::Known
        );
        assert_eq!(classify_section_key("governor", "bogus"), KeyClass::Unknown);
        assert_eq!(
            classify_section_key("governor", "x-experimental"),
            KeyClass::Known
        );
    }

    #[test]
    fn classify_engagement_keys() {
        assert_eq!(
            classify_section_key("engagement", "burst_threshold"),
            KeyClass::Known
        );
        assert_eq!(
            classify_section_key("engagement", "decay_per_minute"),
            KeyClass::Known
        );
        // burst_window is no longer in the schema at all — linter sees
        // it as a generic unknown key (and `--fix` removes it). The
        // parser hard-rejects it separately before attend run.
        assert_eq!(
            classify_section_key("engagement", "burst_window"),
            KeyClass::Unknown
        );
        assert_eq!(
            classify_section_key("engagement", "bogus"),
            KeyClass::Unknown
        );
    }

    #[test]
    fn classify_cleanup_keys() {
        assert_eq!(classify_section_key("cleanup", "enabled"), KeyClass::Known);
        assert_eq!(classify_section_key("cleanup", "interval"), KeyClass::Known);
        // retention was retired with project-liveness reaping (issue #141)
        // — now a generic unknown key the `--fix` pass removes.
        assert_eq!(
            classify_section_key("cleanup", "retention"),
            KeyClass::Unknown
        );
    }

    #[test]
    fn signals_section_is_retired() {
        // The `signals:` section is off the schema (issue #141). It is no
        // longer a recognized top-level section, so a leftover block draws
        // an UNKNOWN-section warning. (`classify_section_key` itself returns
        // Known via its catch-all arm because section-level recognition is
        // handled in `lint_one_file` against SECTIONS, not here.)
        assert!(!SECTIONS.contains(&"signals"));
    }

    #[test]
    fn rewrite_without_lines_drops_selected_lines() {
        let src = "a\nb\nc\nd\n";
        let out = rewrite_without_lines(src, &[1]); // drop "b"
        assert_eq!(out, "a\nc\nd\n");
    }

    #[test]
    fn rewrite_without_trailing_newline_is_preserved() {
        let src = "a\nb\nc";
        let out = rewrite_without_lines(src, &[1]);
        assert_eq!(out, "a\nc");
    }

    // Block-continuation helpers — mirror of ways-cli::cmd::lint::helpers
    // tests so both tools can be cross-verified on equivalent fixtures.

    #[test]
    fn remove_top_level_scalar_field() {
        let src = "description: x\nbogus: should go\nthreshold: 2.0\n";
        let out = remove_top_level_field(src, "bogus").expect("found");
        assert_eq!(out, "description: x\nthreshold: 2.0\n");
    }

    #[test]
    fn remove_top_level_block_field_with_continuation() {
        let src = "description: x\ncurve:\n  type: Exponential\n  half_life: 100\nthreshold: 2.0\n";
        let out = remove_top_level_field(src, "curve").expect("found");
        assert_eq!(out, "description: x\nthreshold: 2.0\n");
    }

    #[test]
    fn remove_top_level_missing_returns_none() {
        let src = "description: x\n";
        assert!(remove_top_level_field(src, "nope").is_none());
    }

    #[test]
    fn remove_top_level_ignores_indented_match() {
        // A key nested under a section should not match the top-level helper.
        let src = "engagement:\n  bogus: 1\n";
        assert!(remove_top_level_field(src, "bogus").is_none());
    }

    #[test]
    fn expand_block_at_scalar_anchor_is_single_line() {
        let lines = vec!["engagement:", "  bogus: 1", "  keep: 2"];
        assert_eq!(expand_block_at(&lines, 1), (1, 2));
    }

    #[test]
    fn expand_block_at_block_anchor_consumes_nested_mapping() {
        let lines = vec![
            "engagement:",
            "  progressive_staircase:",
            "    - [0, 1.0]",
            "    - [15000, 0.5]",
            "  burst_threshold: 3.0",
        ];
        assert_eq!(expand_block_at(&lines, 1), (1, 4));
    }

    #[test]
    fn expand_block_at_rewinds_trailing_blank_before_sibling() {
        let lines = vec![
            "engagement:",
            "  curve:",
            "    type: Exponential",
            "",
            "  burst_threshold: 3.0",
        ];
        // The blank at index 3 separates curve from burst_threshold — it
        // should stay with burst_threshold, not be eaten by the removal.
        assert_eq!(expand_block_at(&lines, 1), (1, 3));
    }

    #[test]
    fn fix_removes_unknown_block_value_without_orphans() {
        // End-to-end: lint_one_file should drop a block-valued unknown
        // engagement key *and* its nested list items.
        let src = "\
engagement:
  bogus_block:
    - [0, 1.0]
    - [15000, 0.5]
  burst_threshold: 3.0
";
        let lines: Vec<&str> = src.lines().collect();
        // The scanner would flag line index 1 (`  bogus_block:`) as
        // UNKNOWN. Simulate the post-scan expansion path.
        let (start, end) = expand_block_at(&lines, 1);
        assert_eq!((start, end), (1, 4));
        let drops: Vec<usize> = (start..end).collect();
        let out = rewrite_without_lines(src, &drops);
        assert_eq!(out, "engagement:\n  burst_threshold: 3.0\n");
    }
}
