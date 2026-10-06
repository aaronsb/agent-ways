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

/// An `$XDG_*` directory variable, treating an empty or relative value as
/// unset. The one guard every `$XDG_*` read goes through.
///
/// The spec says relative `$XDG_*` values must be ignored. This matters here
/// because these roots now drive a destructive relocate — a stray empty env var
/// must never resolve a root to the current directory.
pub fn xdg_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// Resolve an `$XDG_*` base through [`xdg_dir`], else `fallback`.
fn xdg_base(var: &str, fallback: impl Fn() -> PathBuf) -> PathBuf {
    xdg_dir(var).unwrap_or_else(fallback)
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

/// A log stream's live file: `$XDG_STATE/agent-ways/<stem>.jsonl`.
pub fn stream_log(stream: crate::event_archive::Stream) -> PathBuf {
    state_root().join(stream.live_name())
}

/// Telemetry/event log: `$XDG_STATE/agent-ways/events.jsonl` (our telemetry).
/// Readers route through here. The writer joins [`state_root`] with the
/// stream's own live name, which [`stream_log`] also uses, so both name the
/// same file.
pub fn events_log() -> PathBuf {
    stream_log(crate::event_archive::EVENTS)
}

/// Per-turn decision records (ADR-701 §2):
/// `$XDG_STATE/agent-ways/decisions.jsonl`, archived like the event log.
pub fn decisions_log() -> PathBuf {
    stream_log(crate::event_archive::DECISIONS)
}

/// Append-only ledger of assembled compliance findings (ADR-201):
/// `$XDG_STATE/agent-ways/findings.jsonl`. agent-ways owns this — findings are
/// session-derived records, not Claude-Code state — so it sits beside the event
/// log in `$XDG_STATE`, never in the `~/.claude` projection.
pub fn findings_ledger() -> PathBuf {
    state_root().join("findings.jsonl")
}

/// The files of one log stream a reader should read, in time order: the dated
/// gzip archives oldest first (ADR-701 §2), then [`stream_log`] when it exists.
/// Read each with [`crate::event_archive::read_source`], which decompresses.
pub fn stream_log_sources(stream: crate::event_archive::Stream) -> Vec<PathBuf> {
    let log = stream_log(stream);
    let mut sources = log.parent().map(|dir| crate::event_archive::archives(dir, stream)).unwrap_or_default();
    if log.exists() {
        sources.push(log);
    }
    sources
}

/// [`stream_log_sources`] for the event log.
pub fn events_log_sources() -> Vec<PathBuf> {
    stream_log_sources(crate::event_archive::EVENTS)
}

/// [`stream_log_sources`] for the decision log.
pub fn decisions_log_sources() -> Vec<PathBuf> {
    stream_log_sources(crate::event_archive::DECISIONS)
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

/// The core ways as a session reads them: `~/.claude/hooks/ways`, the
/// projection of [`core_ways_root`] (a link into the app, or into a dev
/// checkout after `ways reconcile`). Every runtime reader of the core ways
/// resolves them here; [`core_ways_root`] is the app copy behind it.
pub fn projected_ways_root() -> PathBuf {
    projection_root().join("hooks").join("ways")
}

/// The shipped ways as the engine reads them: the projection, or the app
/// copy itself before the projection exists (a fresh install builds the
/// corpus first).
pub fn shipped_ways_root() -> PathBuf {
    let projected = projected_ways_root();
    if projected.is_dir() { projected } else { core_ways_root() }
}

/// The ways roots a session reads, in the engine's order: the project's
/// `.claude/ways/` when `project` is given and has one, the user's own, then
/// the shipped ways. Only roots that exist are listed.
pub fn ways_roots(project: Option<&Path>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = project.map(|p| p.join(".claude/ways")).into_iter().collect();
    roots.push(user_ways_root());
    roots.push(shipped_ways_root());
    roots.retain(|r| r.is_dir());
    roots
}

/// The schema file only the shipped corpus carries, at the top of its ways root.
pub const SCHEMA_FILE: &str = "frontmatter-schema.yaml";

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The ways root that holds `path` (a way, or a directory inside a root): the
/// nearest ancestor that is one of [`ways_roots`], carries [`SCHEMA_FILE`], or
/// is a `.claude/ways` directory. `None` when `path` sits in no ways root.
pub fn containing_ways_root(path: &Path, project: Option<&Path>) -> Option<PathBuf> {
    let path = canonical(path);
    let known: Vec<PathBuf> = ways_roots(project).iter().map(|r| canonical(r)).collect();
    let start = if path.is_dir() { Some(path.as_path()) } else { path.parent() };
    start?.ancestors().find_map(|dir| {
        let dotclaude_ways = dir.file_name().is_some_and(|n| n == "ways")
            && dir.parent().and_then(|p| p.file_name()).is_some_and(|n| n == ".claude");
        (known.iter().any(|k| k == dir) || dir.join(SCHEMA_FILE).is_file() || dotclaude_ways)
            .then(|| dir.to_path_buf())
    })
}

/// Whether `root` is the core corpus: it carries [`SCHEMA_FILE`], or it is the
/// shipped, app or projected ways root. User and project roots are not core.
pub fn is_core_root(root: &Path) -> bool {
    let root = canonical(root);
    root.join(SCHEMA_FILE).is_file()
        || [shipped_ways_root(), core_ways_root(), projected_ways_root()]
            .iter()
            .any(|c| canonical(c) == root)
}

/// The trusted-project-macros list: `~/.claude/trusted-project-macros`. The
/// projects whose own macros may run; `ways show` reads it and `ways
/// author permissions` reports it.
pub fn trusted_project_macros() -> PathBuf {
    projection_root().join("trusted-project-macros")
}

/// The projected binaries: `~/.claude/bin`.
pub fn projected_bin_root() -> PathBuf {
    projection_root().join("bin")
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

// --- the embedding engine ---

/// The English embedding model's file name in the engine dir ([`corpus_dir`]).
pub const EN_MODEL: &str = "minilm-l6-v2.gguf";

/// The multilingual embedding model's file name in the engine dir.
pub const MULTI_MODEL: &str = "multilingual-minilm-l12-v2-q8.gguf";

/// The `way-embed` binary for the engine dir `engine_dir`: its own copy, else
/// the projected `~/.claude/bin` one. On a projection install the binary lives
/// only in `~/.claude/bin`, so a lookup that checks the engine dir alone
/// silently disables semantic matching.
pub fn way_embed_in(engine_dir: &Path) -> Option<PathBuf> {
    [engine_dir.join("way-embed"), projected_bin_root().join("way-embed")]
        .into_iter()
        .find(|p| p.is_file())
}

/// The `way-embed` binary for the canonical engine dir ([`corpus_dir`]).
pub fn way_embed() -> Option<PathBuf> {
    way_embed_in(&corpus_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_project_macros_lives_in_the_projection() {
        let p = trusted_project_macros();
        assert!(p.ends_with(".claude/trusted-project-macros"), "{}", p.display());
    }

    #[test]
    fn projected_ways_root_is_the_claude_hooks_ways_projection() {
        let p = projected_ways_root();
        assert!(p.ends_with(".claude/hooks/ways"), "{}", p.display());
        assert!(p.starts_with(projection_root()));
    }

    #[test]
    fn way_embed_prefers_the_engine_dir_copy() {
        let dir = std::env::temp_dir().join(format!("ways-embed-loc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("way-embed"), "bin").unwrap();
        assert_eq!(way_embed_in(&dir), Some(dir.join("way-embed")));
        let _ = std::fs::remove_dir_all(&dir);
    }

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

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ways-paths-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        canonical(&d)
    }

    #[test]
    fn a_subtree_and_a_file_find_their_core_root() {
        let d = scratch("core");
        let root = d.join("hooks/ways");
        std::fs::create_dir_all(root.join("softwaredev/code")).unwrap();
        std::fs::write(root.join(SCHEMA_FILE), "x: 1\n").unwrap();
        std::fs::write(root.join("softwaredev/code/code.md"), "---\ndescription: d\n---\n").unwrap();

        assert_eq!(containing_ways_root(&root, None), Some(root.clone()));
        assert_eq!(containing_ways_root(&root.join("softwaredev"), None), Some(root.clone()));
        assert_eq!(containing_ways_root(&root.join("softwaredev/code/code.md"), None), Some(root.clone()));
        assert!(is_core_root(&root));
    }

    #[test]
    fn an_unrelated_hooks_ways_dir_is_neither_a_root_nor_core() {
        let d = scratch("unrelated");
        let dir = d.join("hooks/ways");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        assert_eq!(containing_ways_root(&dir.join("a"), None), None);
        assert!(!is_core_root(&dir));
    }

    #[test]
    fn a_dotclaude_ways_dir_is_a_non_core_root() {
        let d = scratch("project");
        let root = d.join("proj/.claude/ways");
        std::fs::create_dir_all(root.join("x")).unwrap();
        assert_eq!(containing_ways_root(&root.join("x"), None), Some(root.clone()));
        assert!(!is_core_root(&root));
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
        assert_eq!(decisions_log().parent(), events_log().parent(), "the decision log sits beside the event log");
        assert!(decisions_log().ends_with("decisions.jsonl"));
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
