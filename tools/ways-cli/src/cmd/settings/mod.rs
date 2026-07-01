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
