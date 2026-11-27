// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! How full a session's context window is: the one measurement.
//!
//! `ways context`, the `context-threshold` trigger, attend's context sensor and
//! the peers sensor all read a transcript's fill here. The token count is the
//! newest turn's API usage ([`claude_sessions::usage::last_context_tokens`]),
//! and the window comes from [`context_window`]. A second copy of this
//! arithmetic is how attend's sensor came to report 69% of a 1M window that
//! `ways context` read as 17% (#1361).
//!
//! References:
//! - ADR-166: the one resolver for a model's context window.

use crate::context_window::{self, ContextWindow};
use std::path::Path;

/// Whose session the transcript belongs to. It decides whether
/// `CLAUDE_CONTEXT_WINDOW` applies: the override states the window of the
/// process that set it, so it applies to that process's own session only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// The caller's own session: the override applies ([`context_window::resolve`]).
    Own,
    /// A peer's or a recorded session: the model decides
    /// ([`context_window::resolve_for_foreign_session`]).
    Foreign,
}

/// Where the token count came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The newest assistant turn's API usage.
    Api,
    /// No turn reports usage yet; estimated from transcript bytes since the
    /// last summary.
    Bytes,
}

impl Method {
    /// Stable wire name, as `ways context --json` prints it.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Api => "api",
            Method::Bytes => "bytes",
        }
    }
}

/// A transcript's context fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFill {
    pub tokens_used: u64,
    pub window: ContextWindow,
    /// The newest real model; `None` before the first assistant turn.
    pub model: Option<String>,
    pub method: Method,
}

impl ContextFill {
    /// Whole percent of the window used, rounded down. This is the figure
    /// `ways context` prints, so every reader shows the same number.
    pub fn pct_used(&self) -> u64 {
        (self.tokens_used * 100).checked_div(self.window.tokens).unwrap_or(0)
    }

    /// Percent of the window used, unrounded, for rates and comparisons.
    pub fn percent(&self) -> f64 {
        match self.window.tokens {
            0 => 0.0,
            w => self.tokens_used as f64 * 100.0 / w as f64,
        }
    }

    pub fn tokens_remaining(&self) -> u64 {
        self.window.tokens.saturating_sub(self.tokens_used)
    }
}

/// Measure the fill a transcript's text records.
pub fn measure(content: &str, owner: Owner) -> ContextFill {
    let model = claude_sessions::usage::last_model(content);
    let window = match owner {
        Owner::Own => context_window::resolve(model.as_deref()),
        Owner::Foreign => context_window::resolve_for_foreign_session(model.as_deref()),
    };
    let (tokens_used, method) = match claude_sessions::usage::last_context_tokens(content) {
        Some(tokens) => (tokens, Method::Api),
        None => (estimate_from_bytes(content), Method::Bytes),
    };
    ContextFill { tokens_used, window, model, method }
}

/// [`measure`] of the transcript file at `path`.
pub fn measure_transcript(path: &Path, owner: Owner) -> std::io::Result<ContextFill> {
    Ok(measure(&std::fs::read_to_string(path)?, owner))
}

/// The fill of session `session_id`'s own transcript under `claude`, looked
/// for first in the directory of the project at `project`. `None` when the
/// session has no transcript or it cannot be read.
pub fn measure_session(
    claude: &claude_sessions::ClaudeDir,
    project: Option<&str>,
    session_id: &str,
) -> Option<ContextFill> {
    let transcript = claude.find_transcript(project, session_id)?;
    measure_transcript(&transcript, Owner::Own).ok()
}

/// Tokens estimated from the transcript bytes after the last summary, at a
/// conservative 6.3 transcript JSON bytes per token.
fn estimate_from_bytes(content: &str) -> u64 {
    let mut last_summary_end: u64 = 0;
    let mut pos: u64 = 0;
    for line in content.lines() {
        if line.contains("\"type\":\"summary\"") {
            last_summary_end = pos + line.len() as u64 + 1;
        }
        pos += line.len() as u64 + 1;
    }
    let active_bytes = (content.len() as u64).saturating_sub(last_summary_end);
    active_bytes * 10 / 63
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_window::WindowSource;

    fn turn(model: &str, cache_read: u64) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"model":"{model}","usage":{{"input_tokens":0,"cache_read_input_tokens":{cache_read},"cache_creation_input_tokens":0}}}}}}"#
        )
    }

    /// #1361: 173,884 tokens on claude-opus-5-5 is 17% of its 1M window.
    #[test]
    fn opus_5_5_at_174k_is_17_percent() {
        let fill = measure(&turn("claude-opus-5-5", 173_884), Owner::Foreign);
        assert_eq!(fill.window.tokens, 1_000_000);
        assert_eq!(fill.window.source, WindowSource::ModelTable);
        assert_eq!(fill.method, Method::Api);
        assert_eq!(fill.pct_used(), 17);
        assert_eq!(fill.tokens_remaining(), 826_116);
    }

    #[test]
    fn no_usage_estimates_from_bytes() {
        let content = r#"{"type":"user","message":{"content":"hello"}}"#;
        let fill = measure(content, Owner::Foreign);
        assert_eq!(fill.method, Method::Bytes);
        assert_eq!(fill.model, None);
        assert_eq!(fill.window.source, WindowSource::Default);
    }

    #[test]
    fn a_synthetic_last_turn_keeps_the_real_model() {
        let content = format!("{}\n{}\n", turn("claude-opus-5-5", 50_000), turn("<synthetic>", 0));
        let fill = measure(&content, Owner::Foreign);
        assert_eq!(fill.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(fill.window.tokens, 1_000_000);
        assert_eq!(fill.tokens_used, 50_000);
    }
}
