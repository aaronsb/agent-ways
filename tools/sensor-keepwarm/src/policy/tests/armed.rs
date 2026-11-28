// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The armed exit over fixed ledgers and a fixed clock, at Claude Opus 5.5
//! list prices: the judge's `stop` arms it, and each due point pings or
//! finishes by the ski-rental bound. Each test steps the judgment as the
//! mod's checks would, the ledger crossing serde between them.
//!
//! References:
//! - ADR-418 items 5, 6, 9 and 13: the bound, how long to stay warm, the
//!   states, and the handoff as the exit flag.

use super::*;
use crate::policy::armed::{self as exit, bound, judged, Judgement, Pending, Sufficiency, Verdict};

const OPUS: &str = "claude-opus-5-5";
const WRAP: &str = "20261127T0900Z-3f9a1c2e";
const HANDOFF: &str = "/work/agent-ways/.claude/handoff.wrap.md";

fn handoff(id: &str) -> Handoff {
    Handoff {
        path: PathBuf::from(HANDOFF),
        wrap_id: Some(id.into()),
        instructions: String::new(),
        text: format!("2026-11-27 e6bbfbdd wrap={id}\nContinue the demo.\n\nDO NEXT — start #1."),
    }
}

/// The site with the handoff whose first line carries `id`, or none.
fn carrying(id: Option<&str>) -> Site {
    Site { handoff: id.map(handoff), ..site() }
}

fn judgement(verdict: Verdict, h: Sufficiency) -> Judgement {
    Judgement { wrap_id: WRAP.into(), verdict, handoff: h, own: Verdict::Stop, reason: Some("the release landed".into()) }
}

/// An Opus 5.5 session at `ctx` tokens whose boundary the judge called.
fn judged_session(ctx: u64, verdict: Verdict, h: Sufficiency) -> Session {
    let mut s = Session::on(OPUS, MONDAY, ctx);
    judged(&mut s.state.policy, judgement(verdict, h), Some(&handoff(WRAP)), MONDAY).unwrap();
    s
}

fn awaiting(n: u32) -> Config {
    let mut c = cfg();
    c.pending = Pending { awaiting: n, ..Pending::default() };
    c
}

/// Runs the stretch left alone from `start`: each check at the `next_at`
/// the last one gave, each ping's turn landing at once. Each action with
/// its time in minutes after `start`.
fn ride(s: &mut Session, cfg: &Config, site: &Site, start: u64) -> Vec<(Action, u64)> {
    let mut seen = Vec::new();
    let mut now = start + MIN;
    for _ in 0..200 {
        let d = s.check_at(cfg, Gate::Off, site, now);
        if d.action != Action::None {
            seen.push((d.action, (now - start) / MIN));
        }
        if d.action == Action::Keepalive {
            s.turn(now);
        }
        match d.next_at {
            Some(t) if t > now => now = t,
            Some(_) => now += MIN,
            None => break,
        }
    }
    seen
}

fn count(seen: &[(Action, u64)], a: Action) -> usize {
    seen.iter().filter(|(x, _)| *x == a).count()
}

/// The card's own test: Opus 5.5 at 350k tokens, a sufficient handoff and
/// nothing pending. The first due point finishes the session and sends no
/// ping.
#[test]
fn a_sufficient_handoff_with_nothing_pending_finishes_at_the_first_due_point() {
    let mut s = judged_session(350_000, Verdict::Stop, Sufficiency::Sufficient);
    let site = carrying(Some(WRAP));
    let d = s.check_at(&cfg(), Gate::Off, &site, MONDAY + MIN);
    assert_eq!((d.action, d.state, d.next_at), (Action::None, "armed", Some(MONDAY + 50 * MIN)), "{d:?}");
    let d = s.check_at(&cfg(), Gate::Off, &site, MONDAY + 50 * MIN);
    assert_eq!((d.action, d.state), (Action::Finish, "finished"), "{d:?}");
    assert_eq!(d.text, None, "a finish sends no turn");
    assert_eq!(s.state.pinged_at, None, "no ping went out");
    assert!(d.reason.contains("nothing is pending and the handoff is sufficient"), "{}", d.reason);
    let x = d.exit.unwrap();
    assert_eq!((x.state, x.pings), ("finished", 0));
    // An hour on, the session stays finished and nothing is due.
    let d = s.check_at(&cfg(), Gate::Off, &site, MONDAY + 110 * MIN);
    assert_eq!((d.action, d.state, d.next_at), (Action::None, "finished", None));
}

/// The bound at Opus 5.5 list prices: a ping at 350k is $0.07, a fresh
/// start is 171k × $8/M = $1.368, so 19 pings; a cold reload of the whole
/// context buys 40 pings at any fill.
#[test]
fn the_bound_is_forty_times_the_purchase_over_the_context() {
    let price = crate::price_of(OPUS).unwrap();
    let b = bound(Sufficiency::Sufficient, 350_000, price);
    assert!((b.rent - 0.07).abs() < 1e-12 && (b.purchase - 1.368).abs() < 1e-12, "{b:?}");
    assert_eq!(b.worth, 19);
    assert_eq!(bound(Sufficiency::Sufficient, 200_000, price).worth, 34);
    assert_eq!(bound(Sufficiency::Sufficient, 700_000, price).worth, 9);
    for ctx in [60_000, 200_000, 350_000, 350_320, 700_000, 999_999] {
        assert_eq!(bound(Sufficiency::HoldsDetail, ctx, price).worth, 40, "{ctx}");
    }
}

/// One thread awaiting a reply at 350k: 19 pings, then `finish` at the next
/// due point.
#[test]
fn one_thread_awaiting_pings_nineteen_times_then_finishes() {
    let mut s = judged_session(350_000, Verdict::Stop, Sufficiency::Sufficient);
    let seen = ride(&mut s, &awaiting(1), &carrying(Some(WRAP)), MONDAY);
    assert_eq!(count(&seen, Action::Keepalive), 19, "{seen:?}");
    assert_eq!(seen.last(), Some(&(Action::Finish, 20 * 50)), "the twentieth due point finishes: {seen:?}");
    assert_eq!(count(&seen, Action::Finish), 1);
    assert_eq!(s.state.policy.count, 0, "the armed exit's pings are not the keepalive count");
    let x = exit::view(&s.state).unwrap();
    assert_eq!((x.state, x.pings, x.pings_left), ("finished", 19, Some(0)));
    assert!((x.rent_usd.unwrap() - 19.0 * 0.070064).abs() < 1e-9, "{x:?}");
}

/// A handoff that holds detail stays warm for 40 pings whatever is
/// pending, then finishes.
#[test]
fn a_handoff_that_holds_detail_pings_forty_times_then_finishes() {
    for ctx in [350_000, 700_000] {
        let mut s = judged_session(ctx, Verdict::Stop, Sufficiency::HoldsDetail);
        let seen = ride(&mut s, &cfg(), &carrying(Some(WRAP)), MONDAY);
        assert_eq!(count(&seen, Action::Keepalive), 40, "{ctx}: {seen:?}");
        assert_eq!(seen.last().map(|(a, _)| *a), Some(Action::Finish), "{ctx}: {seen:?}");
    }
}

/// A handoff whose first line carries another id, or no handoff at all,
/// means the tick pings as today: the keepalive count's first wake.
#[test]
fn a_stale_or_missing_handoff_pings_as_today() {
    for site in [carrying(Some("20261127T1000Z-0badc0de")), carrying(None)] {
        let mut s = judged_session(350_000, Verdict::Stop, Sufficiency::Sufficient);
        let d = s.check_at(&cfg(), Gate::Off, &site, MONDAY + 50 * MIN);
        assert_eq!((d.action, d.state, d.count), (Action::Keepalive, "running", 1), "{d:?}");
        assert_eq!(d.exit.map(|x| x.state), Some("armed"), "the exit stays armed for a fresh handoff");
    }
}

/// A verdict other than `stop` is recorded and arms nothing.
#[test]
fn a_verdict_other_than_stop_is_recorded_and_arms_nothing() {
    for v in [Verdict::Continue, Verdict::Compact, Verdict::Doubt] {
        let mut s = judged_session(350_000, v, Sufficiency::Sufficient);
        let d = s.check_at(&cfg(), Gate::Off, &carrying(Some(WRAP)), MONDAY + 50 * MIN);
        assert_eq!((d.action, d.state, d.count), (Action::Keepalive, "running", 1), "{v:?}: {d:?}");
        let x = d.exit.unwrap();
        assert_eq!((x.state, x.verdict, x.handoff, x.own), (v.as_str(), v.as_str(), "sufficient", "stop"));
    }
}

/// The judgment is refused, and changes nothing, when the handoff is
/// missing, carries another id, or the id is no wrap id.
#[test]
fn a_judgment_without_the_wrap_s_handoff_is_refused() {
    let mut p = Policy::default();
    let j = || judgement(Verdict::Stop, Sufficiency::Sufficient);
    assert!(judged(&mut p, j(), None, MONDAY).unwrap_err().contains("missing"));
    let other = handoff("20261127T1000Z-0badc0de");
    assert!(judged(&mut p, j(), Some(&other), MONDAY).unwrap_err().contains(&format!("does not carry wrap={WRAP}")));
    let bad = Judgement { wrap_id: "short".into(), ..j() };
    assert!(judged(&mut p, bad, Some(&handoff("short")), MONDAY).is_err());
    assert_eq!(p, Policy::default(), "a refusal changes nothing");
    judged(&mut p, j(), Some(&handoff(WRAP)), MONDAY).unwrap();
    let x = p.exit.as_ref().unwrap();
    assert!(x.armed());
    assert_eq!((x.reason.as_deref(), p.last_wrap_id.as_deref()), (Some("the release landed"), Some(WRAP)));
}

/// An operator prompt clears both `armed` and `finished`: the next due
/// point pings as today.
#[test]
fn an_operator_prompt_clears_armed_and_finished() {
    let mut s = judged_session(350_000, Verdict::Stop, Sufficiency::Sufficient);
    let site = carrying(Some(WRAP));
    assert_eq!(s.check_at(&cfg(), Gate::Off, &site, MONDAY + 50 * MIN).action, Action::Finish);
    reset(&mut s.state.policy, MONDAY + 60 * MIN);
    assert_eq!(s.state.policy.exit, None);
    s.turn(MONDAY + 61 * MIN);
    let d = s.check_at(&cfg(), Gate::Off, &site, MONDAY + 111 * MIN);
    assert_eq!((d.action, d.state, d.count), (Action::Keepalive, "running", 1), "{d:?}");
    // Armed and not yet finished, a prompt disarms as well.
    let mut s = judged_session(350_000, Verdict::Stop, Sufficiency::HoldsDetail);
    reset(&mut s.state.policy, MONDAY + MIN);
    assert_eq!(s.state.policy.exit, None);
}

/// Outside the keepalive hours an armed exit sends no ping, so it finishes
/// at the tick.
#[test]
fn outside_the_keepalive_hours_an_armed_exit_finishes() {
    let mut c = windows(&["daily 00:00-00:30"]);
    c.pending = Pending { agents: 1, ..Pending::default() };
    let mut s = judged_session(350_000, Verdict::Stop, Sufficiency::Sufficient);
    let d = s.check_at(&c, Gate::Off, &carrying(Some(WRAP)), MONDAY + 50 * MIN);
    assert_eq!(d.action, Action::Finish, "{d:?}");
    assert!(d.reason.contains("the keepalive hours have ended"), "{}", d.reason);
}

/// The mod's `--pending agents=N,shells=N,watchers=N`.
#[test]
fn pending_parses_the_mod_s_counts() {
    assert_eq!(Pending::parse("agents=2,shells=0,watchers=1"), Ok(Pending { awaiting: 0, agents: 2, shells: 0, watchers: 1 }));
    assert_eq!(Pending::parse("shells=3"), Ok(Pending { shells: 3, ..Pending::default() }));
    assert_eq!(Pending::parse(""), Ok(Pending::default()));
    assert!(!Pending::default().any());
    for bad in ["agents", "agents=x", "pets=1", "agents=1,agents=2", "agents=-1"] {
        assert!(Pending::parse(bad).is_err(), "{bad}");
    }
}

/// A compaction the boundary tool starts counts one relay once its resume
/// goes in. A failed one, or a resume report with none behind it, counts
/// nothing.
#[test]
fn a_boundary_compaction_counts_toward_max_relays() {
    let mut p = Policy::default();
    drained(&mut p, MONDAY);
    boundary_compacted(&mut p, None, MONDAY + MIN);
    assert_eq!((p.drained_at, p.trigger.as_deref()), (None, Some("boundary")));
    assert!(boundary_resumed(&mut p, MONDAY + 2 * MIN));
    assert_eq!(p.relays, 1);
    assert!(!boundary_resumed(&mut p, MONDAY + 3 * MIN), "one resume per compaction");
    boundary_compacted(&mut p, Some("a turn was running".into()), MONDAY + 4 * MIN);
    assert!(!boundary_resumed(&mut p, MONDAY + 5 * MIN));
    boundary_compacted(&mut p, None, MONDAY + 6 * MIN);
    reset(&mut p, MONDAY + 7 * MIN);
    assert!(!boundary_resumed(&mut p, MONDAY + 8 * MIN), "the operator's prompt came between");
    assert_eq!(p.relays, 0);
}

/// The status card's exit line: armed or finished, the rent paid, the
/// purchase price and the pings left.
#[test]
fn the_card_shows_the_exit_its_rent_purchase_and_pings_left() {
    let mut s = judged_session(350_000, Verdict::Stop, Sufficiency::Sufficient);
    let site = carrying(Some(WRAP));
    let c = awaiting(1);
    for at in [50, 100] {
        assert_eq!(s.check_at(&c, Gate::Off, &site, MONDAY + at * MIN).action, Action::Keepalive);
        s.turn(MONDAY + at * MIN);
    }
    s.check_at(&c, Gate::Off, &site, MONDAY + 102 * MIN);
    assert_eq!(
        exit::card_line(&s.state, MONDAY + 102 * MIN).unwrap(),
        format!("exit        armed · wrap {WRAP}, the judge said stop, the handoff is sufficient, own call stop · 2 pings rent $0.14 paid of $1.37 (a fresh start) · 17 pings left while something is pending, else it finishes at the next tick")
    );
    let lines = crate::card(&s.reading(), None, Some(&s.state.ledger(MONDAY + 102 * MIN)), MONDAY + 102 * MIN);
    assert!(lines.iter().any(|l| l.starts_with("exit        armed · ")), "{lines:?}");
    let d = s.check_at(&cfg(), Gate::Off, &site, MONDAY + 150 * MIN);
    assert_eq!(d.action, Action::Finish);
    assert_eq!(
        exit::card_line(&s.state, MONDAY + 200 * MIN).unwrap(),
        format!("exit        finished 50m ago · wrap {WRAP}, the judge said stop, the handoff is sufficient, own call stop · 2 pings rent $0.14 paid of $1.37 (a fresh start) · your next prompt clears it")
    );
}
