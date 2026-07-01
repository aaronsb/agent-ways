//! `ways config` — the config-fragment store (ADR-147).
//!
//! The authoring layer for Claude Code's `settings.json`: a tree of `NN-*.md`
//! fragments, each carrying a YAML `settings:` block (a settings.json fragment
//! spelled in YAML) plus a rationale body, that lint → compile → project into a
//! baked `settings.json`. This slice ships the fragment **loader**; the schema
//! table, the three lint checks, and compile/project land in follow-up slices.
//!
//! Note the namespace: this is `crate::cmd::config`, deliberately distinct from
//! `crate::config` (the ways *runtime* config). The user-facing command is
//! `ways config`.

// Wired into the CLI in the `ways config lint` slice (see ADR-147 pipeline).
// Until then the loader is exercised only by its unit tests, so silence the
// binary-crate dead-code lint rather than leaving a warning on `main`.
#[allow(dead_code)]
pub mod fragment;
#[allow(dead_code)]
pub mod schema;
