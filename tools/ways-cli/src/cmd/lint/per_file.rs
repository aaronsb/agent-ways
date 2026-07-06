//! Per-file validation rules: runs every lint rule against a single
//! way or check frontmatter file and accumulates error/warning/fix
//! counts. The orchestration lives in `scanning.rs`; this module owns
//! the rule set.

use anyhow::Result;
use std::path::Path;

use super::helpers::{
    count_multiline_yaml, extract_field_name, extract_frontmatter_raw, extract_indented_field_name,
    extract_when_block, fix_multiline_yaml, get_field_value, has_field, remove_top_level_field,
    remove_when_subfield,
};
use super::requires::{
    extract_requires_list, format_requires_yaml, insert_requires_field, is_valid_permission,
    scan_macro_requires,
};
use super::schema::{is_reserved_field, Schema};

use crate::util::home_dir;

/// Whether a way has at least one live firing path — the predicate behind the
/// functional-activeness lint. A way fires via a frontmatter channel
/// (`fires_on_something`: semantic, pattern, commands, files, trigger) or via an
/// ADR-135 `postcheck.sh` sidecar in its directory (reactive PostToolUse
/// dispatch, keyed off the file's existence rather than frontmatter). `way_dir`
/// is the directory holding the way's `.md`; `None` skips the sidecar check.
pub(super) fn has_firing_path(fm_str: &str, way_dir: Option<&Path>) -> bool {
    crate::frontmatter::fires_on_something(fm_str)
        || way_dir
            .map(|d| d.join("postcheck.sh").is_file())
            .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lint_file(
    path: &Path,
    is_check: bool,
    ways_dir: &Path,
    schema: &Schema,
    errors: &mut u32,
    warnings: &mut u32,
    fixes: &mut u32,
    fix: bool,
) -> Result<()> {
    let mut content = std::fs::read_to_string(path)?;
    let mut modified = false;
    let relpath = path.strip_prefix(ways_dir).unwrap_or(path);
    let rel = relpath.display();

    let fm_str = match extract_frontmatter_raw(&content) {
        Some(s) => s,
        None => {
            eprintln!("  ERROR: {rel} — no YAML frontmatter");
            *errors += 1;
            return Ok(());
        }
    };

    let valid_fields = if is_check {
        &schema.check_fields
    } else {
        &schema.way_fields
    };

    // Multi-line YAML values (> or |) — trigger pipeline can't parse them
    if fix {
        if let Some(fixed) = fix_multiline_yaml(&content) {
            let count = count_multiline_yaml(&fm_str);
            if count > 0 {
                content = fixed;
                modified = true;
                *fixes += count as u32;
                eprintln!(
                    "  FIXED: {rel} — collapsed {count} multi-line YAML value(s) to single line"
                );
            }
        }
    } else {
        for line in fm_str.lines() {
            if let Some(field) = line.strip_suffix(": >").or_else(|| line.strip_suffix(": |")) {
                let field = field.trim();
                eprintln!("  ERROR: {rel} — '{field}' uses multi-line YAML (> or |) which the trigger pipeline cannot parse. Use a single line.");
                *errors += 1;
            }
        }
    }

    // Re-extract frontmatter after the multi-line fix so the rest of the
    // rules see the collapsed form.
    let fm_str = extract_frontmatter_raw(&content).unwrap_or_default();

    // Strict YAML parse gate (ways only). The matching pipeline (corpus, scan)
    // loads frontmatter via serde_yaml, so a value that isn't valid YAML drops the
    // way from matching with no other symptom — and the field-by-field rules below
    // never notice. Run the same parse the matcher uses so lint is a real gate.
    // Checks aren't loaded this way, so they're exempt.
    if !is_check {
        if let Err(e) = crate::frontmatter::parse_str(&fm_str) {
            let root = e.root_cause();
            eprintln!("  ERROR: {rel} — frontmatter is not valid YAML, so the matcher would silently drop this way: {root}. Common cause: an unquoted value containing a colon-space (\": \") — wrap the value in double quotes.");
            *errors += 1;
        }
    }

    // Unknown top-level fields. x-* prefixed fields are intentionally
    // skipped. With --fix, remove the field (and any indented block value
    // that follows) and emit FIXED; otherwise emit UNKNOWN as a warning.
    //
    // We collect unknown-field names first so we don't mutate `content`
    // while iterating over a borrow of `fm_str`.
    let unknown_fields: Vec<String> = fm_str
        .lines()
        .filter_map(extract_field_name)
        .filter(|f| !valid_fields.contains(*f) && !is_reserved_field(f))
        .map(|f| f.to_string())
        .collect();

    for field in &unknown_fields {
        if fix {
            if let Some(new_content) = remove_top_level_field(&content, field) {
                content = new_content;
                modified = true;
                *fixes += 1;
                eprintln!("  FIXED: {rel} — removed foreign field '{field}'");
            } else {
                eprintln!("  UNKNOWN: {rel} — unknown field '{field}' (fix could not locate it)");
                *warnings += 1;
            }
        } else {
            eprintln!("  UNKNOWN: {rel} — unknown field '{field}'");
            *warnings += 1;
        }
    }

    // Re-extract frontmatter if we just removed fields, so downstream
    // rules operate on the cleaned form.
    let fm_str = extract_frontmatter_raw(&content).unwrap_or_default();

    // Attend signal handlers are matched by signal name, not semantic matching
    let is_attend = fm_str.lines().any(|l| l.trim() == "type: attend");

    // Description/vocabulary conditional pairing
    let has_desc = has_field(&fm_str, "description");
    let has_vocab = has_field(&fm_str, "vocabulary");
    if has_desc && !has_vocab && !is_attend {
        eprintln!("  WARNING: {rel} — description without vocabulary (semantic matching incomplete)");
        *warnings += 1;
    }
    if has_vocab && !has_desc {
        eprintln!("  WARNING: {rel} — vocabulary without description (semantic matching incomplete)");
        *warnings += 1;
    }

    // Attend-specific validation: signals field required
    if is_attend {
        let has_signals = fm_str.lines().any(|l| l.trim().starts_with("signals:"));
        if !has_signals {
            eprintln!("  ERROR: {rel} — trigger.type: attend requires signals field");
            *errors += 1;
        } else {
            eprintln!("  INFO: {rel} — attend signal handler (matched by signal name, not semantic)");
        }
    }

    // Functional activeness (ground truth: scan/candidates.rs + check-post.sh).
    // A way must have at least one live firing path, or it can never fire and is
    // dead weight in the corpus (the localize case that motivated this rule).
    // `fires_on_something` covers the frontmatter channels — semantic, pattern,
    // commands, files, and `trigger:` (which is present for both the flat state
    // trigger and the nested attend block). The one channel it can't see is the
    // ADR-135 `postcheck.sh` sidecar: reactive PostToolUse dispatch keyed off the
    // file's existence, not frontmatter, so check for it explicitly.
    //
    // Deliberately NOT checked: "fires but injects nothing" (empty body). It
    // isn't mechanically decidable — a macro.sh or postcheck.sh emits its
    // disclosure conditionally at runtime, so an empty .md body is legitimate.
    // Top-level files (core.md) are base guidance injected unconditionally via
    // `macro: prepend`, not matchable ways — the scanner skips them (their
    // way-id is empty). Mirror that exclusion so the rule targets real ways only.
    let is_top_level = relpath.parent().map(|p| p.as_os_str().is_empty()).unwrap_or(true);
    if !is_check && !is_top_level && !has_firing_path(&fm_str, path.parent()) {
        eprintln!(
            "  WARNING: {rel} — no firing path: none of description+vocabulary, \
             pattern, commands, files, trigger, or a postcheck.sh sidecar. \
             This way can never fire (dead in the corpus)."
        );
        *warnings += 1;
    }

    // ADR-126: fire-bearing ways should carry a `refire:` field. Fire
    // eligibility (which frontmatter fields count as firing channels) is
    // declared in `frontmatter::FIRE_BEARING_FIELDS` so adding a new channel
    // flows through automatically. Check files and attend signal handlers
    // are exempt — checks ride on their parent way's firing, and attend
    // handlers are triggered by signal name rather than the refire engine.
    let has_refire = has_field(&fm_str, "refire");
    let is_fire_bearing =
        !is_check && !is_attend && crate::frontmatter::fires_on_something(&fm_str);
    if is_fire_bearing && !has_refire {
        eprintln!(
            "  WARNING: {rel} — no `refire:` field (ADR-126). \
             Fire-bearing way will never re-disclose after first fire. \
             Add `refire: <fraction|preset>` (e.g., `refire: 0.15` or `refire: normal`)."
        );
        *warnings += 1;
    }

    // The legacy `curve:` field was retired (ADR-159); a stray `curve:` block
    // now falls through to the generic UNKNOWN-field warning, which is the
    // correct treatment for a field the schema no longer recognizes.

    // ADR-126: fail-closed preset/numeric validation. Lint is the primary
    // gate for typo detection; corpus-generation echoes this check. Fire
    // time has a soft fallback (see RefireSpec::fraction) so a bypassed
    // lint doesn't crash a live session, but that path logs to stderr.
    if has_refire {
        if let Some(raw) = get_field_value(&fm_str, "refire") {
            let spec = if let Ok(n) = raw.trim().parse::<f64>() {
                crate::frontmatter::RefireSpec::Numeric(n)
            } else {
                crate::frontmatter::RefireSpec::Preset(raw.trim().to_string())
            };
            let presets = &crate::config::global().refire_presets;
            if let Err(msg) = spec.validate(presets) {
                eprintln!("  ERROR: {rel} — {msg}");
                *errors += 1;
            }
        }
    }

    // Threshold is numeric
    if let Some(val) = get_field_value(&fm_str, "threshold") {
        if val.parse::<f64>().is_err() {
            eprintln!("  ERROR: {rel} — threshold '{val}' is not numeric");
            *errors += 1;
        }
    }

    // Pattern hygiene (ADR-155 §5): the keyword lane should carry exact,
    // anchored, term-of-art triggers; suggestive common words belong in
    // vocabulary:. Advisory WARNINGs, not errors.
    if let Some(val) = get_field_value(&fm_str, "pattern") {
        let keep = get_field_value(&fm_str, "pattern_keep")
            .map(|k| super::pattern::parse_keep(&k))
            .unwrap_or_default();
        super::pattern::check_pattern(&rel, &val, &keep, warnings);
    }

    // Positive-prose hygiene (ADR-160 §6): the embedded alias (description +
    // vocabulary) states only what the way is for, in positive terms — a
    // contrast naming a sibling item pulls the embedding toward it. Each field
    // is checked independently. Advisory WARNINGs, not errors.
    for field in ["description", "vocabulary"] {
        if let Some(val) = get_field_value(&fm_str, field) {
            super::positive_prose::check_positive_prose(&rel, field, &val, warnings);
        }
    }

    // Scope enum
    if let Some(val) = get_field_value(&fm_str, "scope") {
        for part in val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if !schema.valid_scopes.iter().any(|v| v == part) {
                eprintln!(
                    "  ERROR: {rel} — invalid scope '{part}' (valid: {})",
                    schema.valid_scopes.join(", ")
                );
                *errors += 1;
            }
        }
    }

    // Macro enum
    if let Some(val) = get_field_value(&fm_str, "macro") {
        if !schema.valid_macros.iter().any(|v| v == &val) {
            eprintln!(
                "  ERROR: {rel} — invalid macro '{val}' (valid: {})",
                schema.valid_macros.join(", ")
            );
            *errors += 1;
        }
    }

    // requires: field validation (ADR-116)
    let has_macro = has_field(&fm_str, "macro");
    let has_requires = has_field(&fm_str, "requires");
    if has_macro && !has_requires {
        if fix {
            let macro_path = path.parent().map(|p| p.join("macro.sh"));
            if let Some(ref mp) = macro_path {
                if mp.is_file() {
                    let reqs = scan_macro_requires(mp);
                    if !reqs.is_empty() {
                        let requires_yaml = format_requires_yaml(&reqs);
                        content = insert_requires_field(&content, &requires_yaml);
                        modified = true;
                        *fixes += 1;
                        eprintln!("  FIXED: {rel} — added requires: [{}]", reqs.join(", "));
                    }
                }
            }
        } else {
            eprintln!("  WARNING: {rel} — macro: without requires: (run --fix to auto-populate)");
            *warnings += 1;
        }
    }
    if has_requires {
        if let Some(reqs) = extract_requires_list(&fm_str) {
            for req in &reqs {
                if !is_valid_permission(req) {
                    eprintln!("  ERROR: {rel} — invalid requires value '{req}' (expected: Tool, Tool(scope), or *)");
                    *errors += 1;
                }
            }
        }
    }

    // Trigger enum
    if let Some(val) = get_field_value(&fm_str, "trigger") {
        if !schema.valid_triggers.iter().any(|v| v == &val) {
            eprintln!(
                "  ERROR: {rel} — invalid trigger '{val}' (valid: {})",
                schema.valid_triggers.join(", ")
            );
            *errors += 1;
        }
    }

    // when: subfields — validate against schema, flag or remove unknowns
    let when_block = extract_when_block(&fm_str);
    let unknown_when: Vec<String> = when_block
        .lines()
        .filter_map(extract_indented_field_name)
        .filter(|f| !schema.when_subfields.contains(*f) && !is_reserved_field(f))
        .map(|f| f.to_string())
        .collect();
    for field in &unknown_when {
        if fix {
            if let Some(new_content) = remove_when_subfield(&content, field) {
                content = new_content;
                modified = true;
                *fixes += 1;
                eprintln!("  FIXED: {rel} — removed foreign when: sub-field '{field}'");
            } else {
                eprintln!(
                    "  UNKNOWN: {rel} — unknown when: sub-field '{field}' (fix could not locate it)"
                );
                *warnings += 1;
            }
        } else {
            eprintln!("  UNKNOWN: {rel} — unknown when: sub-field '{field}'");
            *warnings += 1;
        }
    }

    // when.project path existence
    let fm_str = extract_frontmatter_raw(&content).unwrap_or_default();
    let when_block = extract_when_block(&fm_str);
    for line in when_block.lines() {
        if let Some(val) = line
            .strip_prefix("  project:")
            .or_else(|| line.strip_prefix("project:"))
        {
            let path_str = val.trim();
            let expanded = path_str.replace("~", &home_dir().display().to_string());
            if !Path::new(&expanded).is_dir() {
                eprintln!("  WARNING: {rel} — when.project path '{path_str}' does not exist");
                *warnings += 1;
            }
        }
    }

    // check file: verify anchor and check sections
    if is_check {
        let has_anchor = content.contains("\n## anchor") || content.starts_with("## anchor");
        let has_check = content.contains("\n## check") || content.starts_with("## check");

        if fix && (!has_anchor || !has_check) {
            let mut appended = String::new();
            if !has_anchor {
                appended.push_str("\n## anchor\n\n(TODO: add anchor context)\n");
                *fixes += 1;
                eprintln!("  FIXED: {rel} — added stub '## anchor' section");
            }
            if !has_check {
                appended.push_str("\n## check\n\n(TODO: add check items)\n");
                *fixes += 1;
                eprintln!("  FIXED: {rel} — added stub '## check' section");
            }
            content.push_str(&appended);
            modified = true;
        } else {
            if !has_anchor {
                eprintln!("  ERROR: {rel} — check file missing '## anchor' section");
                *errors += 1;
            }
            if !has_check {
                eprintln!("  ERROR: {rel} — check file missing '## check' section");
                *errors += 1;
            }
        }
    }

    // Write back if modified
    if modified {
        std::fs::write(path, &content)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::has_firing_path;
    use std::path::Path;

    #[test]
    fn firing_path_via_frontmatter_channels() {
        // Semantic needs both fields; each keyword/bash/file/state channel fires alone.
        assert!(has_firing_path("description: x\nvocabulary: y\n", None));
        assert!(has_firing_path("pattern: '^foo'\n", None));
        assert!(has_firing_path("commands: 'git .*'\n", None));
        assert!(has_firing_path("files: '\\.rs$'\n", None));
        assert!(has_firing_path("trigger: session-start\n", None));
        // The nested attend block still presents a top-level `trigger:` line.
        assert!(has_firing_path("trigger:\n  type: attend\n  signals:\n    - x\n", None));
    }

    #[test]
    fn no_firing_path_on_modifier_only_frontmatter() {
        // scope/refire are modifiers, not channels — a way with only these is dead
        // unless a postcheck.sh sidecar rescues it (next test).
        assert!(!has_firing_path("scope: agent\nrefire: 0.2\n", None));
        assert!(!has_firing_path("description: x\n", None)); // half a semantic pair
    }

    #[test]
    fn postcheck_sidecar_is_a_firing_path() {
        let dir = std::env::temp_dir().join(format!("ways-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Modifier-only frontmatter is dead...
        assert!(!has_firing_path("scope: agent\nrefire: 0.2\n", Some(&dir)));
        // ...until a postcheck.sh sits alongside it (ADR-135, the overbuild case).
        std::fs::write(dir.join("postcheck.sh"), "#!/usr/bin/env bash\nexit 0\n").unwrap();
        assert!(has_firing_path("scope: agent\nrefire: 0.2\n", Some(&dir)));
        // A different sidecar name does not count.
        let dir2 = dir.join("other");
        std::fs::create_dir_all(&dir2).unwrap();
        std::fs::write(dir2.join("macro.sh"), "#!/usr/bin/env bash\n").unwrap();
        assert!(!has_firing_path("scope: agent\n", Some(&dir2)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_way_dir_skips_the_sidecar_check() {
        // `None` (no directory) must not panic and must not conjure a firing path.
        assert!(!has_firing_path("scope: agent\n", Some(Path::new("/nonexistent/xyz"))));
        assert!(!has_firing_path("scope: agent\n", None));
    }
}
