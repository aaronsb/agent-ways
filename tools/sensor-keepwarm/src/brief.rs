// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The keepwarm part of the continuance brief's conditions: the context
//! fill and what one ping costs at it, from the transcript reading, and the
//! minutes since the operator's last prompt and the relay count, from the
//! ledger, where the mod gives no prompt time of its own. `attend keepwarm
//! brief` adds the rest. A measure that cannot be
//! had is `None`, which the brief prints as `"unknown"`.
//!
//! References:
//! - ADR-418 §10: the conditions row of the brief table.

use crate::{context_json, price_of, Ledger, Reading};
use serde::Deserialize;
use std::time::Duration;

/// `ways context --json` as the brief reads it: the keepwarm reading, and
/// the window `ways` resolved for the model, with how it resolved it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    pub reading: Reading,
    /// The model's whole window in tokens, `0` where `ways` gave none.
    pub tokens_total: u64,
    /// How `ways` had the window: `env_override`, `model_table`, or
    /// `default` when it assumed one.
    ///
    /// References:
    /// - ADR-166: a defaulted window is never taken for a detected one.
    pub window_source: String,
}

#[derive(Deserialize)]
struct Window {
    #[serde(default)]
    tokens_total: u64,
    #[serde(default)]
    window_source: String,
}

impl Context {
    /// `ways context --json` output, or `None` when it is not that.
    pub fn parse(bytes: &[u8]) -> Option<Context> {
        let reading: Reading = serde_json::from_slice(bytes).ok()?;
        let w: Window = serde_json::from_slice(bytes).ok()?;
        Some(Context { reading, tokens_total: w.tokens_total, window_source: w.window_source })
    }

    /// The window to take a percent of: a resolved one, never a default
    /// `ways` assumed for a model it does not know.
    fn window(&self) -> Option<u64> {
        (self.tokens_total > 0 && self.window_source != "default").then_some(self.tokens_total)
    }
}

/// Session `session_id`'s reading, run from `working_dir` within `timeout`.
pub fn read(session_id: &str, working_dir: &str, timeout: Duration) -> Option<Context> {
    Context::parse(&context_json(session_id, working_dir, timeout)?)
}

/// What keepwarm measures for the brief.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measures {
    /// The context fill in percent of the window, to one decimal.
    pub fill_pct: Option<f64>,
    pub fill_tokens: Option<u64>,
    /// One cache read of the whole context at the model's list price, in
    /// USD; `None` for a model the price table does not hold.
    pub price_per_ping_usd: Option<f64>,
    pub minutes_since_operator_prompt: Option<u64>,
    /// Relay `n` of at most `max`: the resumes since the operator's last
    /// prompt, and `mods.attend.continuity.max_relays`.
    pub relays: Option<(u32, u32)>,
}

/// The measures from `context`, the session's reading; `window`, the
/// window the host reported, which wins over the one `ways` resolved;
/// `ledger`; `operator_at`, the operator's last prompt as the mod saw it,
/// in epoch seconds, which wins over the ledger's (the ledger hears of a
/// prompt only while the autocompact gate reads off); and `max_relays`,
/// `None` where the settings could not be read.
pub fn measure(context: Option<&Context>, window: Option<u64>, ledger: Option<&Ledger>, operator_at: Option<u64>, max_relays: Option<u32>, now: u64) -> Measures {
    let tokens = context.map(|c| c.reading.tokens_used);
    let window = window.filter(|w| *w > 0).or_else(|| context.and_then(Context::window));
    let fill_pct = tokens.zip(window).map(|(t, w)| (t as f64 * 1000.0 / w as f64).round() / 10.0);
    let price_per_ping_usd = context
        .and_then(|c| price_of(&c.reading.model).map(|p| c.reading.tokens_used as f64 * p.read / 1e6))
        .map(|usd| (usd * 1e6).round() / 1e6);
    let policy = ledger.map(|l| &l.policy);
    Measures {
        fill_pct,
        fill_tokens: tokens,
        price_per_ping_usd,
        minutes_since_operator_prompt: operator_at.or_else(|| policy.and_then(|p| p.operator_at)).map(|at| now.saturating_sub(at) / 60),
        relays: policy.zip(max_relays).map(|(p, max)| (p.relays, max)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(tokens: u64, model: &str, total: u64, source: &str) -> Context {
        let doc = format!(r#"{{"tokens_used":{tokens},"model":"{model}","tokens_total":{total},"window_source":"{source}","usage_tail":[]}}"#);
        Context::parse(doc.as_bytes()).unwrap()
    }

    #[test]
    fn the_fill_and_the_ping_come_from_the_reading() {
        let c = context(250_000, "claude-opus-5-5", 1_000_000, "model_table");
        let m = measure(Some(&c), None, None, None, None, 0);
        assert_eq!(m.fill_tokens, Some(250_000));
        assert_eq!(m.fill_pct, Some(25.0));
        // One read of 250k tokens at $0.20 per million.
        assert_eq!(m.price_per_ping_usd, Some(0.05));
        // The host's window wins over the one ways resolved.
        assert_eq!(measure(Some(&c), Some(400_000), None, None, None, 0).fill_pct, Some(62.5));
    }

    #[test]
    fn a_default_window_or_an_unpriced_model_is_unknown() {
        let c = context(150_000, "some-other-model", 200_000, "default");
        let m = measure(Some(&c), None, None, None, None, 0);
        assert_eq!(m.fill_tokens, Some(150_000));
        assert_eq!(m.fill_pct, None);
        assert_eq!(m.price_per_ping_usd, None);
        assert_eq!(measure(None, Some(200_000), None, None, Some(5), 0), Measures::default());
    }

    #[test]
    fn the_ledger_gives_the_minutes_and_the_relays() {
        let mut l = Ledger::default();
        l.policy.operator_at = Some(1_000);
        l.policy.relays = 2;
        let m = measure(None, None, Some(&l), None, Some(5), 1_000 + 42 * 60 + 59);
        assert_eq!(m.minutes_since_operator_prompt, Some(42));
        assert_eq!(m.relays, Some((2, 5)));
        // No settings, no max; no prompt reported, no minutes.
        l.policy.operator_at = None;
        let m = measure(None, None, Some(&l), None, None, 5_000);
        assert_eq!((m.minutes_since_operator_prompt, m.relays), (None, None));
    }

    /// The mod sees every prompt, where the ledger hears of one only while
    /// the gate reads off: the time the mod gives wins over the ledger's,
    /// and stands without a ledger.
    #[test]
    fn the_mods_prompt_time_wins_over_the_ledgers() {
        let mut l = Ledger::default();
        l.policy.operator_at = Some(1_000);
        let now = 10_000;
        assert_eq!(measure(None, None, Some(&l), None, None, now).minutes_since_operator_prompt, Some(150));
        assert_eq!(measure(None, None, Some(&l), Some(now - 7 * 60), None, now).minutes_since_operator_prompt, Some(7));
        assert_eq!(measure(None, None, None, Some(now - 7 * 60), None, now).minutes_since_operator_prompt, Some(7));
    }
}
