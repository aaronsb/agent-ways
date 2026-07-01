//! `ways settings` — the settings.json fragment store (ADR-147).
//!
//! The authoring layer for Claude Code's `settings.json`: a tree of `NN-*.md`
//! fragments, each carrying a YAML `settings:` block (a settings.json fragment
//! spelled in YAML) plus a rationale body, that lint → compile → project into a
//! baked `settings.json`. This slice ships the fragment **loader**, the curated
//! **schema** table, and the three-check **linter** (`ways settings lint`);
//! compile/project land in follow-up slices.
//!
//! Sibling boundary: [`crate::cmd::settings_merge`] is the *reconciler* side —
//! the three-way merge that projects a compiled settings.json into `~/.claude`.
//! This module is the *authoring* side that produces it. Distinct, too, from
//! `crate::config`, the ways runtime config behind `ways config`.

pub mod fragment;
pub mod lint;
pub mod schema;
// Consumed by the scaffold (`ways settings new`) and lint slices; a few
// accessors land ahead of their first caller.
#[allow(dead_code)]
pub mod schema_doc;
pub mod source;

/// `ways settings schema` — report the vendored schema and its (configurable)
/// refresh source. With `source_only`, print just the resolved URL, so the
/// refresh script can consume it.
pub fn schema_command(source_only: bool) {
    let (url, origin) = source::resolve();
    if source_only {
        println!("{url}");
        return;
    }
    let schema = schema_doc::bundled();
    println!("Claude Code settings schema (vendored)");
    println!("  keys:     {}", schema.len());
    println!("  source:   {url}");
    println!("  resolved: {}", origin.as_str());
    println!(
        "  refresh:  WAYS_SETTINGS_SCHEMA_URL=<url> or set `settings_schema_url` \
         in config, then run refresh-settings-schema.sh and rebuild"
    );
    println!(
        "  note:     community SchemaStore, not an official Anthropic artifact; \
         may lag the latest CLI"
    );
}
