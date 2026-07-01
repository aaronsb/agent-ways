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
pub mod scaffold;
pub mod schema;
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
    let file = crate::paths::settings_schema_file();
    println!("Claude Code settings schema");
    match schema_doc::active() {
        Some(s) => println!("  file:     {} ({} keys)", file.display(), s.len()),
        None => println!(
            "  file:     {} (NOT FOUND — run refresh-settings-schema.sh)",
            file.display()
        ),
    }
    println!("  source:   {url}");
    println!("  resolved: {}", origin.as_str());
    println!(
        "  refresh:  refresh-settings-schema.sh writes the file from <source> \
         (no rebuild needed); WAYS_SETTINGS_SCHEMA_URL / config `settings_schema_url` \
         override the source"
    );
    println!(
        "  note:     community SchemaStore, not an official Anthropic artifact; \
         may lag the latest CLI"
    );
}
