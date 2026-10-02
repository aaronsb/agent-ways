//! The agent-ways 1.0 location taxonomy (ADR-142).
//!
//! Every file agent-ways touches is classified by *durability and ownership*,
//! and placed in the XDG location whose contract matches. This module is the
//! single source of truth for those locations; no call site should hand-build a
//! path under `~/.claude` or `$XDG_*`.
//!
//! | Root | Holds | Durability |
//! |---|---|---|
//! | [`data_root`]   `$XDG_DATA_HOME/agent-ways`   | the application (ways, skills, hooks, bin, docs) | replaced wholesale on update |
//! | [`config_root`] `$XDG_CONFIG_HOME/agent-ways` | the operator's own ways/macros + config           | never touched by update |
//! | [`state_root`]  `$XDG_STATE_HOME/agent-ways`  | session substrate (ledger, events, focus)         | survives a `~/.claude` wipe |
//! | [`cache_root`]  `$XDG_CACHE_HOME/agent-ways`  | derived (corpus, embeddings, model)               | regenerable; safe to delete |
//! | [`projection_root`] `~/.claude`               | the Claude-Code-owned projection floor            | regenerable from the manifest |
//!
//! **Naming.** The XDG *application directory* is `agent-ways` across all four
//! tiers (harmonized in ADR-142). The domain term *ways* is untouched: the
//! `ways` binary, way files, `hooks/ways/`, and the inner `…/agent-ways/ways/`
//! user root all keep that name. "agent-ways" is the app; "ways" is what it's
//! made of.
//!
//! Because every root resolves through `$XDG_*`, pointing those env vars at a
//! tmpdir gives every consumer a sandbox `HOME` — this module is the test seam
//! the reconciler is validated against, never the live install.

use crate::util::{home_dir, normalize_path_sep};
use std::path::{Path, PathBuf};

/// The XDG application-directory name, shared by all four tiers.
const APP: &str = "agent-ways";

// ---------------------------------------------------------------------------
// XDG base directories (spec defaults; Windows home handling via `home_dir`).
// Defined here so the taxonomy is self-contained. A later step folds `config`'s
// private copy into this module.
// ---------------------------------------------------------------------------

/// Resolve an `$XDG_*` base, treating an empty or relative value as unset.
///
/// The spec says relative `$XDG_*` values must be ignored. This matters here
/// because these roots now drive a destructive relocate — a stray empty env var
/// must never resolve a root to the current directory.
fn xdg_base(var: &str, fallback: impl Fn() -> PathBuf) -> PathBuf {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() && Path::new(&v).is_absolute() => PathBuf::from(v),
        _ => fallback(),
    }
}

/// XDG data base ($XDG_DATA_HOME or ~/.local/share).
fn xdg_data_base() -> PathBuf {
    xdg_base("XDG_DATA_HOME", || home_dir().join(".local").join("share"))
}

/// XDG config base ($XDG_CONFIG_HOME or ~/.config).
fn xdg_config_base() -> PathBuf {
    xdg_base("XDG_CONFIG_HOME", || home_dir().join(".config"))
}

/// XDG state base ($XDG_STATE_HOME or ~/.local/state).
fn xdg_state_base() -> PathBuf {
    xdg_base("XDG_STATE_HOME", || home_dir().join(".local").join("state"))
}

/// XDG cache base ($XDG_CACHE_HOME or ~/.cache), with the same empty/relative guard.
fn xdg_cache_base() -> PathBuf {
    xdg_base("XDG_CACHE_HOME", || home_dir().join(".cache"))
}

// ---------------------------------------------------------------------------
// The five taxonomy roots.
// ---------------------------------------------------------------------------

/// The application: exactly what is on GitHub. Read-only to the user; replaced
/// wholesale on update. Losing it is a re-install, not data loss.
pub fn data_root() -> PathBuf {
    normalize_path_sep(&xdg_data_base().join(APP))
}

/// The operator's own ways, macros, and config. Durable; out of every update's
/// blast radius.
pub fn config_root() -> PathBuf {
    normalize_path_sep(&xdg_config_base().join(APP))
}

/// Session substrate — ledger, events, focus — that must survive a `~/.claude`
/// wipe. Durable; survives reinstall/repair.
pub fn state_root() -> PathBuf {
    normalize_path_sep(&xdg_state_base().join(APP))
}

/// Derived state — corpus, embeddings, model. Regenerable; safe to delete:
/// `$XDG_CACHE/agent-ways`.
pub fn cache_root() -> PathBuf {
    normalize_path_sep(&xdg_cache_base().join(APP))
}

/// The Claude-Code-owned projection floor (`~/.claude`). What *stays*:
/// transcripts, auto-memory, and `settings.json` live here and are owned by
/// Claude Code; agent-ways only reads them (and surgically merges settings).
pub fn projection_root() -> PathBuf {
    normalize_path_sep(&home_dir().join(".claude"))
}

// ---------------------------------------------------------------------------
// Convenience accessors — the concrete files/dirs, so no call site rebuilds a
// path. Grouped by which root they derive from.
// ---------------------------------------------------------------------------

// --- app ($XDG_DATA) ---

/// Shipped (core) ways: `$XDG_DATA/agent-ways/hooks/ways`.
pub fn core_ways_root() -> PathBuf {
    data_root().join("hooks").join("ways")
}

/// Shipped binaries: `$XDG_DATA/agent-ways/bin`.
pub fn bin_root() -> PathBuf {
    data_root().join("bin")
}

// --- user ($XDG_CONFIG) ---

/// The operator's own ways root: `$XDG_CONFIG/agent-ways/ways` (the new "user"
/// tier of the three-root runtime, ADR-143).
pub fn user_ways_root() -> PathBuf {
    config_root().join("ways")
}

/// Per-target configuration root (ADR-184): `$XDG_CONFIG/agent-ways/targets/<key>`.
/// A target's own `config.yaml` there is layered over the user config for
/// sessions running under that target's config directory.
pub fn target_config_root(target_dir: &Path) -> PathBuf {
    config_root().join("targets").join(crate::util::encode_project_key(target_dir))
}

/// The Claude Code config directory this process runs under: `CLAUDE_CONFIG_DIR`
/// when set, else the default projection root. Hooks inherit the variable from
/// the session, so this names the active target at runtime.
pub fn current_config_dir() -> PathBuf {
    match std::env::var("CLAUDE_CONFIG_DIR") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => projection_root(),
    }
}

/// User config file: `$XDG_CONFIG/agent-ways/config.yaml`.
pub fn user_config() -> PathBuf {
    config_root().join("config.yaml")
}

// --- state ($XDG_STATE) ---

/// Telemetry/event log: `$XDG_STATE/agent-ways/events.jsonl` (our telemetry).
/// The Rust writer and every reader route through here.
pub fn events_log() -> PathBuf {
    state_root().join("events.jsonl")
}

/// Append-only ledger of assembled compliance findings (ADR-201):
/// `$XDG_STATE/agent-ways/findings.jsonl`. agent-ways owns this — findings are
/// session-derived records, not Claude-Code state — so it sits beside the event
/// log in `$XDG_STATE`, never in the `~/.claude` projection.
pub fn findings_ledger() -> PathBuf {
    state_root().join("findings.jsonl")
}

/// The event-log files a reader should read: [`events_log`] when it exists.
pub fn events_log_sources() -> Vec<PathBuf> {
    let log = events_log();
    if log.exists() { vec![log] } else { Vec::new() }
}

// --- cache ($XDG_CACHE) ---

/// The embedding-engine working dir (model, corpus, manifest):
/// `$XDG_CACHE/agent-ways/user`.
pub fn corpus_dir() -> PathBuf {
    cache_root().join("user")
}

// --- projection (~/.claude, Claude-Code-owned — stays) ---

/// Claude Code's settings file. agent-ways *reads* it and surgically merges the
/// hooks block + ways permissions; it does not own it. The one shared-write seam.
pub fn settings_json() -> PathBuf {
    projection_root().join("settings.json")
}

/// Claude Code's config directory as agent-ways sees it: `~/.claude`, the
/// projection root. Owned by Claude Code; [`claude_sessions`] locates its
/// projects, transcripts and session records.
pub fn claude_dir() -> claude_sessions::ClaudeDir {
    claude_sessions::ClaudeDir::at(projection_root())
}

/// Claude Code's per-project transcript root (`~/.claude/projects/<slug>`).
/// Owned by Claude Code; agent-ways is a read-only consumer. **Does not move.**
/// The directory name a project gets is [`claude_sessions::project_slug`].
pub fn transcripts_root() -> PathBuf {
    claude_dir().projects_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Structural assertions only — no `$XDG_*` mutation, which would race across
    // Rust's parallel test threads (env is process-global). Suffix checks prove
    // the wiring without touching shared state.

    #[test]
    fn roots_carry_the_app_name() {
        assert!(data_root().ends_with("agent-ways"));
        assert!(config_root().ends_with("agent-ways"));
        assert!(state_root().ends_with("agent-ways"));
        assert!(cache_root().ends_with("agent-ways"));
    }

    #[test]
    fn projection_is_dotclaude() {
        assert!(projection_root().ends_with(".claude"));
    }

    #[test]
    fn ways_term_survives_inside_roots() {
        // The app dir renames to agent-ways, but "ways" persists as the domain term.
        assert!(core_ways_root().ends_with("ways"));
        assert!(core_ways_root().parent().unwrap().ends_with("hooks"));
        assert!(user_ways_root().ends_with("ways"));
        // ...and the user ways root sits *inside* the agent-ways app dir.
        assert!(user_ways_root().parent().unwrap().ends_with("agent-ways"));
    }

    #[test]
    fn accessors_land_in_the_right_tier() {
        assert!(corpus_dir().ends_with("user"));
        assert!(corpus_dir().parent().unwrap().ends_with(APP));
        assert!(events_log().ends_with("events.jsonl"));
        assert!(bin_root().ends_with("bin"));
    }

    #[test]
    fn claude_owned_surfaces_stay_under_projection() {
        // settings.json and transcripts must resolve under ~/.claude, not XDG —
        // they are Claude Code's, and the taxonomy must not relocate them.
        assert!(settings_json().ends_with("settings.json"));
        assert!(settings_json().parent().unwrap().ends_with(".claude"));
        assert!(transcripts_root().parent().unwrap().ends_with(".claude"));
    }
}
