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
//!   `pattern:` regex (common words, short unanchored tokens, greedy `.*`).
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
mod provenance;
mod requires;
mod scanning;
mod schema;

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::util::{detect_project_dir, home_dir};

pub fn run(path: Option<String>, schema: bool, check: bool, fix: bool, global: bool) -> Result<()> {
    let ways_dir = home_dir().join(".claude/hooks/ways");
    let schema_path = ways_dir.join("frontmatter-schema.yaml");

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
    eprintln!();

    let mut errors = 0u32;
    let mut warnings = 0u32;
    let mut fixes = 0u32;

    // Determine scan directory:
    // 1. Explicit path arg wins
    // 2. CLAUDE_PROJECT_DIR .claude/ways/ if it exists (unless --global)
    // 3. Global ways dir
    let (scan_dir, is_targeted) = if let Some(ref p) = path {
        (PathBuf::from(p), true)
    } else if !global {
        let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
            .ok()
            .or_else(detect_project_dir);
        if let Some(ref pd) = project_dir {
            let project_ways = PathBuf::from(pd).join(".claude/ways");
            if project_ways.is_dir() {
                eprintln!("Project: {pd}");
                eprintln!();
                (project_ways, true)
            } else {
                (ways_dir.clone(), false)
            }
        } else {
            (ways_dir.clone(), false)
        }
    } else {
        (ways_dir.clone(), false)
    };

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
