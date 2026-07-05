//! `ways lint` — schema-driven validation of way frontmatter,
//! check files, locale stubs, and provenance sidecars.
//!
//! This module is split along natural seams:
//!
//! - [`schema`] loads `frontmatter-schema.yaml` into a [`schema::Schema`]
//!   that every validator reads. Also owns [`schema::is_reserved_field`],
//!   the `x-*` escape hatch.
//! - [`helpers`] is the frontmatter string-level primitives — extract the
//!   raw YAML, pull field names, remove lines, collapse multi-line values.
//! - [`per_file`] runs every lint rule against one way/check file and
//!   accumulates counters. Owns the `--fix` removal logic for foreign
//!   top-level and `when:` sub-fields.
//! - [`scanning`] walks the project's ways directory and hands files to
//!   `per_file`.
//! - [`pattern`] runs the ADR-155 §5 pattern-hygiene rules over the
//!   `pattern:` regex (common words, short unanchored tokens, unbounded `.*`).
//! - [`positive_prose`] runs the ADR-160 §6 rule over the embedded alias
//!   (`description` + `vocabulary`): a contrast construction naming a sibling
//!   corpus item pulls the embedding toward the negated topic.
//! - [`locale_stubs`] validates `*.locales.jsonl` per-language overrides
//!   against the `locale_stub:` schema block.
//! - [`requires`] is the ADR-116 scan-macro-to-permissions machinery.
//! - [`provenance`] validates ADR-110 provenance sidecars.
//!
//! Public surface is `run()` — everything else is `pub(super)` within
//! the module.

mod helpers;
mod locale_stubs;
mod pattern;
mod per_file;
mod positive_prose;
mod provenance;
mod requires;
mod scanning;
mod schema;

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::util::{detect_project_dir, home_dir};

pub fn run(path: Option<String>, schema: bool, check: bool, fix: bool, global: bool) -> Result<()> {
    let home_ways_dir = home_dir().join(".claude/hooks/ways");

    // Determine the corpus to lint:
    // 1. Explicit path arg wins
    // 2. CLAUDE_PROJECT_DIR .claude/ways/ if it exists (unless --global)
    // 3. Global ways dir (the ~/.claude projection)
    let (scan_dir, is_targeted, project_label) = if let Some(ref p) = path {
        (PathBuf::from(p), true, None)
    } else if !global {
        let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
            .ok()
            .or_else(detect_project_dir);
        match project_dir {
            Some(pd) if PathBuf::from(&pd).join(".claude/ways").is_dir() => {
                let pw = PathBuf::from(&pd).join(".claude/ways");
                (pw, true, Some(pd))
            }
            _ => (home_ways_dir.clone(), false, None),
        }
    } else {
        (home_ways_dir.clone(), false, None)
    };

    // Schema resolution — `ways lint` stays self-sufficient, with no coupling to
    // make/install. Prefer the schema that ships alongside the corpus being
    // linted, so `ways lint <repo>/hooks/ways` validates against that repo's own
    // schema with no ~/.claude projection required (make absent ≠ lint broken);
    // fall back to the installed global schema for the projected / --global case.
    let local_schema = scan_dir.join("frontmatter-schema.yaml");
    let schema_path = if local_schema.is_file() {
        local_schema
    } else {
        home_ways_dir.join("frontmatter-schema.yaml")
    };

    if schema {
        let content = std::fs::read_to_string(&schema_path)
            .with_context(|| format!("reading {}", schema_path.display()))?;
        print!("{content}");
        return Ok(());
    }

    let schema_data = schema::load(&schema_path)?;

    eprintln!();
    eprintln!("Way Frontmatter Lint");
    eprintln!("Schema: {}", schema_path.display());
    if let Some(pd) = &project_label {
        eprintln!("Project: {pd}");
    }
    eprintln!();

    let mut errors = 0u32;
    let mut warnings = 0u32;
    let mut fixes = 0u32;

    // Canonical ways root for relpath display and the sub-linters: the corpus
    // actually being scanned (which exists in CI with no projection), not a
    // hard-coded home path that may be absent.
    let ways_dir = scan_dir.clone();

    let file_count = scanning::scan_and_lint(
        &scan_dir,
        &ways_dir,
        &schema_data,
        &mut errors,
        &mut warnings,
        &mut fixes,
        fix,
    )?;

    let stub_count = locale_stubs::lint_locale_stubs(
        &scan_dir,
        &ways_dir,
        &schema_data,
        &mut errors,
        &mut warnings,
        &mut fixes,
        fix,
    )?;

    // Provenance sidecar validation
    provenance::lint_provenance_sidecars(&scan_dir, &ways_dir, &mut errors)?;

    let label = if is_targeted { "Target" } else { "Global" };
    eprintln!(
        "{label}: scanned {file_count} way files, {stub_count} locale stub files"
    );
    eprintln!();
    if fixes > 0 {
        eprintln!("Fixed: {fixes} issue(s)");
    }
    eprintln!("Summary: {errors} errors, {warnings} warnings");

    if check && errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}
