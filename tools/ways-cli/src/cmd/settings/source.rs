//! Where the settings schema comes from — a small configuration surface.
//!
//! The schema itself is *vendored* (see [`super::schema_doc`], which bundles it
//! via `include_str!`), so normal operation needs no network. But *refreshing*
//! that vendored copy pulls from a source URL that is **configurable, not
//! hardcoded** — an org can point at an internal mirror or a version-pinned URL,
//! or swap in an official Anthropic schema if one is ever published.
//!
//! Resolution precedence (first wins):
//!   1. `$WAYS_SETTINGS_SCHEMA_URL` — one-shot env override
//!   2. `settings_schema_url` in the ways config (`config.yaml`) — persistent
//!   3. [`DEFAULT_SCHEMA_URL`] — the community SchemaStore schema

/// The default source: the community-maintained SchemaStore Claude Code settings
/// schema (the one editors use for `settings.json` autocomplete). Not an
/// official Anthropic artifact — see anthropics/claude-code#11795.
pub const DEFAULT_SCHEMA_URL: &str = "https://json.schemastore.org/claude-code-settings.json";

/// How the active source URL was chosen — reported for transparency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Env,
    Config,
    Default,
}

impl Origin {
    pub fn as_str(&self) -> &'static str {
        match self {
            Origin::Env => "env (WAYS_SETTINGS_SCHEMA_URL)",
            Origin::Config => "config (settings_schema_url)",
            Origin::Default => "default (SchemaStore)",
        }
    }
}

/// Resolve the schema source URL and where it came from.
pub fn resolve() -> (String, Origin) {
    if let Ok(u) = std::env::var("WAYS_SETTINGS_SCHEMA_URL") {
        if !u.trim().is_empty() {
            return (u.trim().to_string(), Origin::Env);
        }
    }
    if let Some(u) = crate::config::global().settings_schema_url.as_deref() {
        if !u.trim().is_empty() {
            return (u.trim().to_string(), Origin::Config);
        }
    }
    (DEFAULT_SCHEMA_URL.to_string(), Origin::Default)
}
