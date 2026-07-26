//! The single source of truth for a model's context window (ADR-166).
//!
//! The window is the denominator of nearly everything downstream: the
//! `ways context` gauge divides by it, ADR-126 scales way refire half-lives
//! against it, and `sensor-peers` reports peer pressure as a fraction of it. It
//! used to be answered by three drifting substring heuristics that disagreed with
//! each other; it is now answered here, once.
//!
//! Two facts shape the design:
//!
//! * **Detection cannot see the harness's window setting.** Claude Code writes the
//!   bare model id to the transcript (`claude-opus-4-8`), never a window marker
//!   like `claude-opus-4-8[1m]`. All we can observe is *which model*, not *which
//!   window it was given*. That is why `CLAUDE_CONTEXT_WINDOW` outranks the table
//!   unconditionally rather than merely backstopping it.
//! * **A wrong window used to be indistinguishable from a right one.** So the
//!   resolver reports *how* it arrived at the number ([`WindowSource`]), and a
//!   default is surfaced as a default rather than passed off as a detection.

/// Window assumed for a model we do not recognize.
///
/// Conservative on purpose. Over-reporting the window would suppress way
/// disclosure and under-report usage — the gauge reads comfortable while the
/// session is in fact near its limit. Under-reporting errs the safe way, and
/// [`WindowSource::Default`] makes it visible rather than silent.
pub const DEFAULT_WINDOW: u64 = 200_000;

/// Operator override. Honored ahead of all detection.
pub const ENV_OVERRIDE: &str = "CLAUDE_CONTEXT_WINDOW";

/// Placeholders Claude Code writes into `message.model` that are not models.
///
/// `<synthetic>` marks an interrupt or API-error turn; `-` is `sensor-peers`'
/// no-model placeholder. Both mean *no model spoke*, and both must be treated as
/// an absence rather than an unrecognized id — a transcript whose newest
/// assistant turn is an interrupt still has a real model further back, and
/// resolving it to the default would hand that session a 5x-wrong window.
const SENTINELS: &[&str] = &["<synthetic>", "-", "unknown", ""];

/// True when `model` is a placeholder rather than a real model id.
pub fn is_sentinel(model: &str) -> bool {
    SENTINELS.contains(&model.trim())
}

/// Known model context windows, keyed by model id (ADR-166).
///
/// Matched on the *model-id component* of the string, delimited by a non-alphanumeric
/// boundary — never by loose substring. This accepts the qualified forms other
/// harnesses emit around the same id:
///
/// | Form | Example |
/// |---|---|
/// | bare | `claude-opus-4-8` |
/// | dated | `claude-haiku-4-5-20251001` |
/// | Bedrock/Vertex-qualified | `us.anthropic.claude-opus-4-8-v1:0`, `claude-opus-4-8@20260115` |
/// | window-marked | `claude-opus-4-8[1m]` |
///
/// while still rejecting the near-misses that loose substring matching swallowed:
/// `claude-sonnet-55` is not `claude-sonnet-5`, and a bare `opus-4` no longer
/// matches every Opus 4.x regardless of its actual window.
///
/// **This table is a maintenance obligation.** It must be updated when a model
/// ships or a window changes. An unlisted model resolves to [`DEFAULT_WINDOW`]
/// and reports [`WindowSource::Default`], so the omission is diagnosable — but it
/// is still wrong until corrected.
const MODEL_WINDOWS: &[(&str, u64)] = &[
    // 1M-context models.
    ("claude-fable-5", 1_000_000),
    ("claude-mythos-5", 1_000_000),
    ("claude-mythos-preview", 1_000_000),
    ("claude-opus-5", 1_000_000),
    ("claude-opus-4-8", 1_000_000),
    ("claude-opus-4-7", 1_000_000),
    ("claude-opus-4-6", 1_000_000),
    ("claude-sonnet-5", 1_000_000),
    ("claude-sonnet-4-6", 1_000_000),
    // 200K-context models.
    ("claude-haiku-4-5", 200_000),
];

/// Bare family aliases, as written by subagent model config (`"model": "opus"`).
///
/// Matched on **exact equality only** — never as a prefix. An alias is a whole
/// model reference, not a family stem: letting `sonnet` prefix-match would make
/// `claude-sonnet-4-5` resolve to the current Sonnet's window and report it as a
/// confident detection, which is the over-broad matching this module exists to
/// end. Each alias resolves to the current model of that family, which is what
/// Claude Code itself does with them.
const MODEL_ALIASES: &[(&str, u64)] = &[
    ("fable", 1_000_000),
    ("opus", 1_000_000),
    ("sonnet", 1_000_000),
    ("haiku", 200_000),
];

/// How a window was arrived at. Carried alongside the number so a default is
/// never mistaken for a detection (ADR-166).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSource {
    /// `CLAUDE_CONTEXT_WINDOW` was set.
    EnvOverride,
    /// The model id was found in [`MODEL_WINDOWS`].
    ModelTable,
    /// Nothing matched; [`DEFAULT_WINDOW`] was assumed.
    Default,
}

impl WindowSource {
    /// Stable wire name, as emitted by `ways context --json`.
    pub fn as_str(self) -> &'static str {
        match self {
            WindowSource::EnvOverride => "env_override",
            WindowSource::ModelTable => "model_table",
            WindowSource::Default => "default",
        }
    }
}

/// A resolved context window and the provenance of that answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextWindow {
    pub tokens: u64,
    pub source: WindowSource,
}

/// Look up a model id in the table. `None` when unrecognized or a sentinel.
///
/// Model ids are matched as a boundary-delimited component of the string, so the
/// provider prefixes and version suffixes other harnesses wrap around the same id
/// (`us.anthropic.claude-opus-4-8-v1:0`) still resolve. Aliases are matched only
/// on exact equality. See [`MODEL_WINDOWS`] and [`MODEL_ALIASES`].
pub fn window_for_model(model: &str) -> Option<u64> {
    let model = model.trim();
    if is_sentinel(model) {
        return None;
    }

    if let Some((_, window)) = MODEL_ALIASES.iter().find(|(alias, _)| model == *alias) {
        return Some(*window);
    }

    // Longest id first, so a shorter entry can never shadow a more specific one.
    let mut ids: Vec<&(&str, u64)> = MODEL_WINDOWS.iter().collect();
    ids.sort_by_key(|(id, _)| std::cmp::Reverse(id.len()));
    ids.iter()
        .find(|(id, _)| contains_component(model, id))
        .map(|(_, window)| *window)
}

/// True when `id` appears in `model` bounded by non-alphanumeric characters (or
/// the string ends).
///
/// This is what lets `claude-opus-4-8` be found inside `us.anthropic.claude-opus-4-8-v1:0`
/// (bounded by `.` and `-`) and `claude-opus-4-8[1m]` (bounded by `[`), while
/// still refusing `claude-sonnet-5` inside `claude-sonnet-55` — the trailing `5`
/// is alphanumeric, so it is a different id, not a qualified form of this one.
fn contains_component(model: &str, id: &str) -> bool {
    let alnum = |c: char| c.is_ascii_alphanumeric();
    model.match_indices(id).any(|(start, _)| {
        let before_ok = start == 0 || !model[..start].chars().next_back().is_some_and(alnum);
        let after = &model[start + id.len()..];
        let after_ok = !after.chars().next().is_some_and(alnum);
        before_ok && after_ok
    })
}

/// Resolve a window from the model table alone, with **no** environment override.
///
/// For windows that belong to *another* session — `sensor-peers` reading a peer's
/// transcript. `CLAUDE_CONTEXT_WINDOW` states the window of the process that set
/// it; applying it to a peer would compute that peer's fill against the observer's
/// window, so an operator with the override set would see every peer's percentage
/// rescaled — hiding a peer that is genuinely about to compact.
pub fn resolve_for_foreign_session(model: Option<&str>) -> ContextWindow {
    match model.and_then(window_for_model) {
        Some(tokens) => ContextWindow {
            tokens,
            source: WindowSource::ModelTable,
        },
        None => ContextWindow {
            tokens: DEFAULT_WINDOW,
            source: WindowSource::Default,
        },
    }
}

/// Resolve the context window for a model, in the order fixed by ADR-166:
/// `CLAUDE_CONTEXT_WINDOW` → model table → [`DEFAULT_WINDOW`].
///
/// `model` is `None` when no assistant turn has been written yet — the launch
/// race a monitor hits when it starts before the model has spoken. That resolves
/// to the default like any other unknown, which is why live views must re-resolve
/// on refresh rather than cache the startup answer.
pub fn resolve(model: Option<&str>) -> ContextWindow {
    resolve_with(model, env_override())
}

/// Pure core of [`resolve`]: the resolution order itself, with the environment
/// already read. Split out because env vars are process-global and Rust tests run
/// in parallel — a test that set `CLAUDE_CONTEXT_WINDOW` would race every other
/// test resolving a window.
pub fn resolve_with(model: Option<&str>, override_tokens: Option<u64>) -> ContextWindow {
    if let Some(tokens) = override_tokens.filter(|n| *n > 0) {
        return ContextWindow {
            tokens,
            source: WindowSource::EnvOverride,
        };
    }

    if let Some(tokens) = model.and_then(window_for_model) {
        return ContextWindow {
            tokens,
            source: WindowSource::ModelTable,
        };
    }

    ContextWindow {
        tokens: DEFAULT_WINDOW,
        source: WindowSource::Default,
    }
}

/// Read `CLAUDE_CONTEXT_WINDOW`. A zero or unparseable value is ignored rather
/// than honored — it is a denominator, and a zero window would divide by zero at
/// every consumer.
fn env_override() -> Option<u64> {
    std::env::var(ENV_OVERRIDE)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every model id observed in real transcript history, pinned. These are the
    /// strings Claude Code actually writes — the regression this ADR-166 module
    /// exists to prevent was a model that matched no branch and silently defaulted.
    #[test]
    fn pins_every_model_seen_in_transcripts() {
        assert_eq!(window_for_model("claude-opus-5"), Some(1_000_000));
        assert_eq!(window_for_model("claude-opus-4-8"), Some(1_000_000));
        assert_eq!(window_for_model("claude-fable-5"), Some(1_000_000));
        assert_eq!(window_for_model("claude-sonnet-4-6"), Some(1_000_000));
        assert_eq!(window_for_model("claude-sonnet-5"), Some(1_000_000));
        assert_eq!(
            window_for_model("claude-haiku-4-5-20251001"),
            Some(200_000),
            "dated variants resolve via their family entry"
        );

        // Bare aliases, as written by subagent model config.
        assert_eq!(window_for_model("opus"), Some(1_000_000));
        assert_eq!(window_for_model("fable"), Some(1_000_000));
        assert_eq!(window_for_model("sonnet"), Some(1_000_000));
        assert_eq!(window_for_model("haiku"), Some(200_000));
    }

    /// The bug that motivated ADR-166: `claude-fable-5` is a 1M model that every
    /// prior resolver classified as 200K, because it matched none of their
    /// substring branches.
    #[test]
    fn fable_5_is_a_1m_model() {
        let w = resolve(Some("claude-fable-5"));
        assert_eq!(w.tokens, 1_000_000);
        assert_eq!(w.source, WindowSource::ModelTable);
    }

    /// The failure ADR-166 predicted, arriving on schedule: Opus 5 shipped, the
    /// table still ended at 4.8, and a live 1M session read `pct_used: 106` against
    /// a 200K denominator — firing compaction pressure that was not there.
    #[test]
    fn opus_5_is_a_1m_model() {
        let w = resolve(Some("claude-opus-5"));
        assert_eq!(w.tokens, 1_000_000);
        assert_eq!(w.source, WindowSource::ModelTable);

        // The harness names this model `claude-opus-5[1m]` in the system prompt;
        // component matching resolves that form too.
        assert_eq!(window_for_model("claude-opus-5[1m]"), Some(1_000_000));
    }

    /// `sonnet` used to swallow `sonnet-5` and call it 200K. Substring matching is
    /// gone; the full id is matched.
    #[test]
    fn sonnet_5_is_not_swallowed_by_a_sonnet_substring() {
        assert_eq!(window_for_model("claude-sonnet-5"), Some(1_000_000));
    }

    #[test]
    fn unknown_model_defaults_and_says_so() {
        let w = resolve(Some("claude-nonexistent-9"));
        assert_eq!(w.tokens, DEFAULT_WINDOW);
        assert_eq!(
            w.source,
            WindowSource::Default,
            "a default must be reported as a default, not passed off as a detection"
        );
        assert_eq!(w.source.as_str(), "default");

        // `<synthetic>` turns carry no real model.
        assert_eq!(window_for_model("<synthetic>"), None);
    }

    /// The launch race: a monitor can start before the first assistant turn is
    /// written, so there is no model to read.
    #[test]
    fn absent_model_defaults() {
        let w = resolve(None);
        assert_eq!(w.tokens, DEFAULT_WINDOW);
        assert_eq!(w.source, WindowSource::Default);
    }

    /// A near-miss must not match. `claude-opus-4` is not `claude-opus-4-8`, and
    /// the old resolvers treated it as such.
    #[test]
    fn partial_family_ids_do_not_match() {
        assert_eq!(window_for_model("claude-opus-4"), None);
        assert_eq!(window_for_model("claude-opus"), None);
        // A prefix match must be followed by a `-`, not arbitrary characters.
        assert_eq!(window_for_model("claude-sonnet-55"), None);
    }

    /// The override must outrank a *recognized* model, not merely backstop the
    /// unknown ones. Both prior resolvers read `CLAUDE_CONTEXT_WINDOW` only from
    /// their fallback arm, so it silently did nothing on every model they matched
    /// — contradicting the documented contract.
    #[test]
    fn env_override_outranks_a_known_model() {
        let w = resolve_with(Some("claude-opus-4-8"), Some(200_000));
        assert_eq!(
            w.tokens, 200_000,
            "the operator's window must win over the table's"
        );
        assert_eq!(w.source, WindowSource::EnvOverride);
    }

    #[test]
    fn env_override_applies_to_unknown_models_too() {
        let w = resolve_with(Some("claude-nonexistent-9"), Some(500_000));
        assert_eq!(w.tokens, 500_000);
        assert_eq!(w.source, WindowSource::EnvOverride);
    }

    /// A zero window is a division-by-zero at every consumer. Ignore it rather
    /// than honor it, and fall through to detection.
    #[test]
    fn zero_override_is_ignored_not_honored() {
        let w = resolve_with(Some("claude-fable-5"), Some(0));
        assert_eq!(w.tokens, 1_000_000);
        assert_eq!(w.source, WindowSource::ModelTable);
    }

    /// `<synthetic>` is what Claude Code writes for an interrupt or API-error
    /// turn. It is an absence of a model, not an unknown model — 9 transcripts in
    /// local history end on one. Resolving it to the default would hand a live 1M
    /// session a 200K window.
    #[test]
    fn synthetic_and_placeholder_turns_are_absences_not_models() {
        assert!(is_sentinel("<synthetic>"));
        assert!(is_sentinel("-")); // sensor-peers' no-model placeholder
        assert!(is_sentinel("unknown")); // context.rs' no-assistant-turn sentinel
        assert!(is_sentinel(""));
        assert!(!is_sentinel("claude-opus-4-8"));

        assert_eq!(window_for_model("<synthetic>"), None);
    }

    /// Other harnesses wrap the same model id in provider prefixes and version
    /// suffixes. The old `contains("opus-4")` heuristic resolved these to 1M; an
    /// id anchored at byte 0 would regress them to the default.
    #[test]
    fn qualified_model_ids_still_resolve() {
        // Amazon Bedrock / Vertex.
        assert_eq!(
            window_for_model("us.anthropic.claude-opus-4-8-v1:0"),
            Some(1_000_000)
        );
        assert_eq!(
            window_for_model("claude-opus-4-8@20260115"),
            Some(1_000_000)
        );
        // A window marker, should Claude Code ever write one into the transcript.
        assert_eq!(window_for_model("claude-opus-4-8[1m]"), Some(1_000_000));
    }

    /// An alias is a whole model reference, not a family stem. Prefix-matching it
    /// would resolve `claude-sonnet-4-5` to the current Sonnet's window and report
    /// it as a confident detection.
    #[test]
    fn aliases_match_exactly_and_do_not_prefix_match() {
        assert_eq!(window_for_model("sonnet"), Some(1_000_000));
        assert_eq!(
            window_for_model("claude-sonnet-4-5"),
            None,
            "an unlisted model must default visibly, not borrow an alias's window"
        );
    }

    /// A peer's window is a property of the peer's session, not the observer's.
    /// An operator's `CLAUDE_CONTEXT_WINDOW` must not rescale peers' fill.
    #[test]
    fn foreign_session_resolution_ignores_the_operator_override() {
        let w = resolve_for_foreign_session(Some("claude-haiku-4-5"));
        assert_eq!(w.tokens, 200_000);
        assert_eq!(w.source, WindowSource::ModelTable);

        // Even with an override in play, the peer's own model decides. (The
        // override is not read on this path at all — `resolve_with` is the one
        // that consults it.)
        assert_eq!(
            resolve_for_foreign_session(Some("claude-fable-5")).tokens,
            1_000_000
        );
        assert_eq!(
            resolve_for_foreign_session(None).source,
            WindowSource::Default
        );
    }

    #[test]
    fn source_wire_names_are_stable() {
        assert_eq!(WindowSource::EnvOverride.as_str(), "env_override");
        assert_eq!(WindowSource::ModelTable.as_str(), "model_table");
        assert_eq!(WindowSource::Default.as_str(), "default");
    }
}
