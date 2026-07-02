//! The agent-ways 1.0 location taxonomy (ADR-142).
//!
//! Every file agent-ways touches is classified by *durability and ownership*,
//! and placed in the XDG location whose contract matches. This module is the
//! single source of truth for those locations; no call site should hand-build a
//! path under `~/.claude` or `$XDG_*` once migration is complete.
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
//! the reconciler and migrator are validated against, never the live install.

use crate::util::{home_dir, normalize_path_sep};
use std::path::{Path, PathBuf};

/// The XDG application-directory name, shared by all four tiers.
const APP: &str = "agent-ways";

/// The pre-1.0 cache directory name, kept as a read-fallback during migration.
const LEGACY_CACHE: &str = "claude-ways";

// ---------------------------------------------------------------------------
// XDG base directories (spec defaults; Windows home handling via `home_dir`).
// `xdg_cache_dir` already lives in `util`; the other three are defined here so
// the taxonomy is self-contained. A later step folds `config`'s private copy
// into this module.
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

/// Derived state — corpus, embeddings, model. Regenerable; safe to delete.
///
/// During the 1.0 transition this is **read-fallback aware**: it prefers
/// `$XDG_CACHE/agent-ways`, but if that doesn't exist yet while the legacy
/// `$XDG_CACHE/claude-ways` does, it returns the legacy dir. So an un-migrated
/// install keeps using its already-downloaded model + corpus (no re-fetch, no
/// split-brain with the tooling that populated it); the migrator renames the
/// dir, after which the preferred path simply wins. A fresh install gets the
/// new name. Reads and writes both route through here, so they never diverge.
pub fn cache_root() -> PathBuf {
    resolve_cache(&xdg_cache_base())
}

/// The canonical cache root (`$XDG_CACHE/agent-ways`), ignoring the legacy
/// fallback. Use this where the target must be the NEW location regardless of
/// what exists yet — specifically the migrator's rename *destination*. Runtime
/// reads use [`cache_root`] (fallback-aware); the migrator must not, or its
/// rename destination collapses onto the legacy source and the rename no-ops.
pub fn cache_root_canonical() -> PathBuf {
    normalize_path_sep(&xdg_cache_base().join(APP))
}

/// Pure fallback resolution, factored out so it's testable without mutating the
/// process-global `$XDG_CACHE_HOME`.
fn resolve_cache(base: &Path) -> PathBuf {
    let preferred = base.join(APP);
    if preferred.exists() {
        return normalize_path_sep(&preferred);
    }
    let legacy = base.join(LEGACY_CACHE);
    if legacy.exists() {
        return normalize_path_sep(&legacy);
    }
    normalize_path_sep(&preferred)
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

/// The lint frontmatter schema shipped with the app.
pub fn frontmatter_schema() -> PathBuf {
    core_ways_root().join("frontmatter-schema.yaml")
}

const SETTINGS_SCHEMA_FILE: &str = "claude-code-settings.schema.json";

/// The durable user copy of the settings schema — `$XDG_CONFIG/agent-ways/…` —
/// which is the `ways settings schema --refresh` write target and outranks the
/// shipped copy. Durable: survives `make update`.
pub fn settings_schema_user_copy() -> PathBuf {
    normalize_path_sep(&config_root().join(SETTINGS_SCHEMA_FILE))
}

/// The Claude Code settings schema, read at runtime by `ways settings` (ADR-147).
///
/// Externalized rather than compiled into the binary: Claude Code's settings
/// surface changes often, so this must update without a rebuild, and the
/// every-turn binary shouldn't carry data only `ways settings` reads. Resolution:
///   1. `$WAYS_SETTINGS_SCHEMA_FILE` — explicit override, returned unconditionally
///      (so an error can name it, and tests can force absence)
///   2. `$XDG_CONFIG/agent-ways/claude-code-settings.schema.json` — durable user
///      copy (the `--refresh` target), if it exists
///   3. `$XDG_DATA/agent-ways/share/claude-code-settings.schema.json` — the
///      shipped copy (refreshed by `make update`); returned as the fallback even
///      if absent, so the caller can name it
pub fn settings_schema_file() -> PathBuf {
    if let Ok(p) = std::env::var("WAYS_SETTINGS_SCHEMA_FILE") {
        let p = p.trim();
        if !p.is_empty() {
            return normalize_path_sep(Path::new(p));
        }
    }
    let user = settings_schema_user_copy();
    if user.exists() {
        return user;
    }
    normalize_path_sep(&data_root().join("share").join(SETTINGS_SCHEMA_FILE))
}

// --- user ($XDG_CONFIG) ---

/// The operator's own ways root: `$XDG_CONFIG/agent-ways/ways` (the new "user"
/// tier of the three-root runtime, ADR-143).
pub fn user_ways_root() -> PathBuf {
    config_root().join("ways")
}

/// User config file: `$XDG_CONFIG/agent-ways/config.yaml` (migrated from the
/// legacy `$XDG_CONFIG/ways/config.yaml` and `~/.claude/ways.json`).
pub fn user_config() -> PathBuf {
    config_root().join("config.yaml")
}

// --- state ($XDG_STATE) ---

/// Telemetry/event log: `$XDG_STATE/agent-ways/events.jsonl` (our telemetry).
///
/// Fallback-aware during the transition, mirroring [`cache_root`]: prefers the
/// `$XDG_STATE` location, but if that doesn't exist yet while the legacy
/// `~/.claude/stats/events.jsonl` does, returns the legacy file so an
/// un-migrated install keeps appending to its existing history. The migrator
/// lifts the file to `$XDG_STATE`, after which the preferred path wins. Reads
/// and the writer both route through here, so they never diverge.
pub fn events_log() -> PathBuf {
    resolve_events(&state_root(), &projection_root())
}

/// Pure fallback resolution for the event log, factored out for testing without
/// touching `$XDG_STATE`/`$HOME`.
fn resolve_events(state: &Path, projection: &Path) -> PathBuf {
    let preferred = state.join("events.jsonl");
    if preferred.exists() {
        return preferred;
    }
    let legacy = projection.join("stats").join("events.jsonl");
    if legacy.exists() {
        return legacy;
    }
    preferred
}

// --- cache ($XDG_CACHE) ---

/// The embedding-engine working dir (model, corpus, manifest):
/// `$XDG_CACHE/agent-ways/user` (was `claude-ways/user`).
pub fn corpus_dir() -> PathBuf {
    cache_root().join("user")
}

// --- projection (~/.claude, Claude-Code-owned — stays) ---

/// Claude Code's settings file. agent-ways *reads* it and surgically merges the
/// hooks block + ways permissions; it does not own it. The one shared-write seam.
pub fn settings_json() -> PathBuf {
    projection_root().join("settings.json")
}

/// Claude Code's per-project transcript root (`~/.claude/projects/<slug>`).
/// Owned by Claude Code; agent-ways is a read-only consumer. **Does not move.**
pub fn transcripts_root() -> PathBuf {
    projection_root().join("projects")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Structural assertions only — no `$XDG_*` mutation, which would race across
    // Rust's parallel test threads (env is process-global). Suffix checks prove
    // the wiring without touching shared state.

    #[test]
    fn roots_carry_the_app_name() {
        // data/config/state are unconditional. cache is fallback-aware (it may
        // resolve to the legacy dir on an un-migrated machine), so it is NOT
        // asserted here — see cache_root_prefers_new_falls_back_to_legacy.
        assert!(data_root().ends_with("agent-ways"));
        assert!(config_root().ends_with("agent-ways"));
        assert!(state_root().ends_with("agent-ways"));
    }

    #[test]
    fn cache_root_prefers_new_falls_back_to_legacy() {
        // Deterministic: drive the pure resolver with a controlled base dir
        // instead of mutating $XDG_CACHE_HOME (process-global, races tests).
        let base = std::env::temp_dir().join(format!("ways-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        // Neither exists → preferred (agent-ways).
        assert!(resolve_cache(&base).ends_with("agent-ways"));
        // Only legacy exists → legacy (so an un-migrated install keeps its cache).
        std::fs::create_dir_all(base.join(LEGACY_CACHE)).unwrap();
        assert!(resolve_cache(&base).ends_with(LEGACY_CACHE));
        // New exists → new wins even with legacy present (post-migration).
        std::fs::create_dir_all(base.join(APP)).unwrap();
        assert!(resolve_cache(&base).ends_with(APP));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn events_log_prefers_state_falls_back_to_legacy_stats() {
        let base = std::env::temp_dir().join(format!("ways-events-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let state = base.join("state");
        let projection = base.join(".claude");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&projection).unwrap();

        // Neither exists → preferred (state).
        assert!(resolve_events(&state, &projection).starts_with(&state));
        // Only legacy ~/.claude/stats/events.jsonl exists → legacy (continuity).
        std::fs::create_dir_all(projection.join("stats")).unwrap();
        std::fs::write(projection.join("stats/events.jsonl"), "{}\n").unwrap();
        assert!(resolve_events(&state, &projection).starts_with(&projection));
        // State events present → state wins (post-migration).
        std::fs::write(state.join("events.jsonl"), "{}\n").unwrap();
        assert!(resolve_events(&state, &projection).starts_with(&state));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cache_root_canonical_ignores_legacy_fallback() {
        // Always the new name regardless of what exists — the migrator's rename
        // destination must never collapse onto the legacy source (cache_root's
        // fallback would, which is correct for reads but wrong as a rename target).
        assert!(cache_root_canonical().ends_with("agent-ways"));
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
        // corpus_dir's parent is the fallback-aware cache root (agent-ways or the
        // legacy claude-ways), so only assert it's one of those — not a fixed name.
        let cache_name = corpus_dir().parent().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        assert!(cache_name == APP || cache_name == LEGACY_CACHE, "unexpected cache dir: {cache_name}");
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
