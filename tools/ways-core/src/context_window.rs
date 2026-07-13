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

/// Known model context windows, keyed by model id (ADR-166).
///
/// Matched exactly, or against a dated variant (`claude-haiku-4-5-20251001`) —
/// never by loose substring. Substring matching is precisely what failed before:
/// `sonnet` swallowed `sonnet-5`, and a bare `opus-4` swallowed every Opus 4.x
/// regardless of its actual window.
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
    ("claude-opus-4-8", 1_000_000),
    ("claude-opus-4-7", 1_000_000),
    ("claude-opus-4-6", 1_000_000),
    ("claude-sonnet-5", 1_000_000),
    ("claude-sonnet-4-6", 1_000_000),
    // 200K-context models.
    ("claude-haiku-4-5", 200_000),
    // Bare aliases, as written by subagent model config. Each resolves to the
    // current model of that family, which is what Claude Code itself does with
    // them.
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

/// Look up a model id in the table. `None` when unrecognized.
///
/// Matches the id exactly, or as a dated variant of a table entry — so
/// `claude-haiku-4-5-20251001` resolves via `claude-haiku-4-5`.
pub fn window_for_model(model: &str) -> Option<u64> {
    let model = model.trim();
    MODEL_WINDOWS
        .iter()
        .find(|(id, _)| {
            model == *id
                || model
                    .strip_prefix(*id)
                    .is_some_and(|rest| rest.starts_with('-'))
        })
        .map(|(_, window)| *window)
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

    #[test]
    fn source_wire_names_are_stable() {
        assert_eq!(WindowSource::EnvOverride.as_str(), "env_override");
        assert_eq!(WindowSource::ModelTable.as_str(), "model_table");
        assert_eq!(WindowSource::Default.as_str(), "default");
    }
}
