// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The drain and the hard line over fixed ledgers and a fixed clock: a fill
//! at `relay_at_pct` starts the drain once per generation, and a fill at
//! `hard_at_pct` wraps mid-work. Each test steps the judgment as the mod's
//! checks would.
//!
//! References:
//! - ADR-418 item 2: the drain point, the hard line and their settings.
//! - ADR-418 item 14: the fill trigger becomes the drain and the hard line.

use super::relay::{filled, relay, WINDOW};
use super::*;

fn native(mut c: Config, tokens: u64, at: u64) -> Config {
    c.fill = Some(Fill { tokens, window: WINDOW, native_at: Some(at) });
    c
}

#[test]
fn a_fill_at_the_drain_point_drains_once_and_the_hard_line_wraps() {
    let mut s = Session::new(MONDAY, 700_000);
    let d = s.check_at(&filled(relay(), 760_000), Gate::Off, &site(), MONDAY + MIN);
    assert_eq!(d.action, Action::Drain, "{d:?}");
    assert!(d.draining);
    assert_eq!((d.drain_at_tokens, d.hard_at_tokens), (Some(750_000), Some(850_000)));
    assert_eq!(s.state.policy.wrapped_at, None, "a drain sends no wrap");
    let text = d.text.unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("drain: context at 76% (760k of 1M), past the drain point of 75%."));
    assert!(text.contains("start nothing new, finish the work in flight and collect your agents' reports"), "{text}");
    assert!(
        text.contains("Run the wrap skill with the argument boundary at the next boundary, where a piece of work lands; the wrap ends with the attend_boundary tool, which refuses continue while the session drains."),
        "{text}"
    );
    assert!(text.contains("The mod wraps mid-work at the hard line of 85%."), "{text}");
    // Once per generation: once the mod reports the turn went in, a later
    // fill under the hard line asks nothing.
    drained(&mut s.state.policy, MONDAY + MIN);
    s.turn(MONDAY + 2 * MIN);
    let again = s.check_at(&filled(relay(), 800_000), Gate::Off, &site(), MONDAY + 3 * MIN);
    assert_eq!((again.action, again.draining), (Action::None, true));
    // The hard line wraps mid-work, and the reason names it.
    s.turn(MONDAY + 4 * MIN);
    let w = s.check_at(&filled(relay(), 860_000), Gate::Off, &site(), MONDAY + 5 * MIN);
    assert_eq!(w.action, Action::Wrap, "{w:?}");
    assert_eq!(w.text.unwrap().lines().next(), Some("wrap: context at 86% (860k of 1M), past the hard line of 85%."));
}

#[test]
fn a_drain_turn_that_did_not_go_in_is_offered_again() {
    let mut s = Session::new(MONDAY, 700_000);
    let d = s.check_at(&filled(relay(), 760_000), Gate::Off, &site(), MONDAY + MIN);
    assert_eq!((d.action, d.draining), (Action::Drain, true));
    assert_eq!(s.state.policy.drained_at, None, "the offer records nothing");
    // A prompt overtook the turn, so the mod reported nothing.
    reset(&mut s.state.policy, MONDAY + 2 * MIN);
    s.turn(MONDAY + 3 * MIN);
    let again = s.check_at(&filled(relay(), 770_000), Gate::Off, &site(), MONDAY + 4 * MIN);
    assert_eq!(again.action, Action::Drain, "{again:?}");
    drained(&mut s.state.policy, MONDAY + 4 * MIN);
    let after = s.check_at(&filled(relay(), 780_000), Gate::Off, &site(), MONDAY + 5 * MIN);
    assert_eq!((after.action, after.draining), (Action::None, true));
}

#[test]
fn a_fill_past_the_hard_line_wraps_with_no_drain_first() {
    let mut s = Session::new(MONDAY, 700_000);
    let d = s.check_at(&filled(relay(), 860_000), Gate::Off, &site(), MONDAY + MIN);
    assert_eq!(d.action, Action::Wrap);
    assert_eq!(s.state.policy.trigger.as_deref(), Some("fill"));
}

#[test]
fn the_hard_line_sits_under_autocompact_s_threshold_and_says_so() {
    let c = native(relay(), 0, 880_000);
    assert_eq!(c.hard_at_tokens(Gate::On), Some(830_000), "five points of the window under the native threshold");
    assert_eq!(c.drain_at_tokens(Gate::On), Some(750_000), "the drain point stays");
    assert_eq!(c.hard_at_tokens(Gate::Off), Some(850_000), "with autocompact off its threshold does not apply");
    let c = native(relay(), 0, 950_000);
    assert_eq!(c.hard_at_tokens(Gate::On), Some(850_000), "hard_at_pct when it is lower");
    // A threshold under the drain point leaves no drain band.
    let c = native(relay(), 0, 770_000);
    assert_eq!((c.drain_at_tokens(Gate::On), c.hard_at_tokens(Gate::On)), (Some(720_000), Some(720_000)));
    // The capped line is named in the drain's text and in the wrap's.
    let mut s = Session::new(MONDAY, 700_000);
    let d = s.check_at(&native(relay(), 760_000, 880_000), Gate::On, &site(), MONDAY + MIN);
    assert_eq!(d.action, Action::Drain);
    let text = d.text.unwrap();
    assert!(text.contains("The mod wraps mid-work at the hard line of 83%, five points under Claude Code's own compaction at 88%."), "{text}");
    s.turn(MONDAY + 2 * MIN);
    let w = s.check_at(&native(relay(), 840_000, 880_000), Gate::On, &site(), MONDAY + 3 * MIN);
    assert_eq!(w.action, Action::Wrap);
    assert_eq!(
        w.text.unwrap().lines().next(),
        Some("wrap: context at 84% (840k of 1M), past the hard line of 83%, five points under Claude Code's own compaction at 88%.")
    );
}

#[test]
fn a_hard_line_not_above_the_drain_point_leaves_no_drain_band() {
    let mut c = relay();
    c.hard_at_pct = 70;
    let c = filled(c, 0);
    assert_eq!((c.drain_at_tokens(Gate::Off), c.hard_at_tokens(Gate::Off)), (Some(700_000), Some(700_000)));
    let mut s = Session::new(MONDAY, 600_000);
    let mut at = c.clone();
    at.fill = Some(Fill { tokens: 710_000, window: WINDOW, native_at: None });
    let d = s.check_at(&at, Gate::Off, &site(), MONDAY + MIN);
    assert_eq!(d.action, Action::Wrap, "the hard line is the drain point: {d:?}");
}

#[test]
fn a_prompt_keeps_the_drain_and_a_compaction_or_a_lower_fill_clears_it() {
    let mut s = Session::new(MONDAY, 700_000);
    assert_eq!(s.check_at(&filled(relay(), 760_000), Gate::Off, &site(), MONDAY + MIN).action, Action::Drain);
    drained(&mut s.state.policy, MONDAY + MIN);
    reset(&mut s.state.policy, MONDAY + 2 * MIN);
    let d = s.check_at(&filled(relay(), 770_000), Gate::Off, &site(), MONDAY + 3 * MIN);
    assert_eq!((d.action, d.draining), (Action::None, true), "an operator prompt does not end the drain");
    // A fill under the drain point, as after a compaction Claude Code ran.
    let d = s.check_at(&filled(relay(), 120_000), Gate::Off, &site(), MONDAY + 4 * MIN);
    assert!(!d.draining, "{d:?}");
    assert_eq!(s.check_at(&filled(relay(), 760_000), Gate::Off, &site(), MONDAY + 5 * MIN).action, Action::Drain);
    drained(&mut s.state.policy, MONDAY + 5 * MIN);
    // The mod's report of a compaction ends it too.
    compacted(&mut s.state.policy, None, MONDAY + 6 * MIN);
    assert_eq!(s.state.policy.drained_at, None);
    // And a resume starts a new generation.
    s.state.policy.drained_at = Some(MONDAY);
    resumed(&mut s.state.policy, MONDAY + 7 * MIN);
    assert_eq!(s.state.policy.drained_at, None);
}

#[test]
fn with_relay_off_nothing_drains_and_no_line_is_given() {
    let mut s = Session::new(MONDAY, 700_000);
    let d = s.check_at(&filled(cfg(), 900_000), Gate::Off, &site(), MONDAY + MIN);
    assert_eq!((d.action, d.draining, d.drain_at_tokens, d.hard_at_tokens), (Action::None, false, None, None));
}

#[test]
fn hard_at_pct_is_read_and_held_to_its_range() {
    let c = Config::from_settings(Some(r#"{"keys":{"mods.attend.continuity.hard_at_pct":{"value":90}}}"#));
    assert_eq!(c.hard_at_pct, 90);
    let c = Config::from_settings(Some(r#"{"keys":{"mods.attend.continuity.hard_at_pct":{"value":99}}}"#));
    assert_eq!(c.hard_at_pct, DEFAULT_HARD_AT_PCT);
    assert_eq!(Config::default().hard_at_pct, 85);
}

#[test]
fn the_card_names_the_drain_point_the_hard_line_and_the_drain() {
    let mut s = Session::new(MONDAY, 700_000);
    reset(&mut s.state.policy, MONDAY);
    s.check_at(&filled(relay(), 760_000), Gate::Off, &site(), MONDAY + MIN);
    drained(&mut s.state.policy, MONDAY + MIN);
    let last = card(&s.state, &s.reading(), &relay(), &site(), MONDAY + 5 * MIN).last().cloned().unwrap();
    assert_eq!(last, "relay       on · 0 of 5 since your prompt at 00:00 · drains at 75% and wraps mid-work at 85% of the window · draining");
}
