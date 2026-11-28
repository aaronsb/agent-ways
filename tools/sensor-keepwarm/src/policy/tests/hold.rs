// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The operator hold over fixed ledgers and a fixed clock, at Claude Opus
//! 5.5 list prices: a stop that waits on the operator computes when the
//! operator is likely back, and the first tick holds the cache that long or
//! lets it lapse, by the rent-or-buy bound. MONDAY is 00:00 in the zone the
//! tests read the night in.
//!
//! References:
//! - ADR-418 items 5 and 6: the bound, and waiting on the operator as
//!   pending.

use super::armed::{carrying, count, handoff, judgement, ride, OPUS, WRAP};
use super::relay::relay;
use super::*;
use crate::policy::armed::{self as exit, Judgement, Pending, Sufficiency, Verdict};
use crate::policy::hold::{self, expected_return, one_way_decisions, Cause, HoldView};
use crate::windows::{resolve_with, Zone, ZoneSource, ZoneView, DEFAULT_WINDOW};

const HOUR: u64 = 3600;

/// `HH:MM` on the test's Monday, or past 24:00 into Tuesday.
fn at(h: u64, m: u64) -> u64 {
    MONDAY + h * HOUR + m * MIN
}

/// A zone the operator set: UTC, so MONDAY's clock reads as the test's.
fn utc_override() -> Zone {
    Zone { tz: TimeZone::UTC, name: Some("UTC".into()), source: Some(ZoneSource::Override), bad: None }
}

/// `c` with the night read in a known zone.
fn known(c: Config) -> Config {
    Config { windows: c.windows.clone().with_resolved(utc_override()), ..c }
}

/// An Opus 5.5 session at `ctx` tokens whose boundary the judge called
/// `doubt` at `stop`, recorded under `c`.
fn doubted(ctx: u64, stop: u64, c: &Config) -> Session {
    let mut s = Session::on(OPUS, stop, ctx);
    let j = Judgement { moved: Verdict::Doubt, ..judgement(Verdict::Doubt, Sufficiency::Sufficient) };
    exit::record(&mut s.state.policy, j, Some(&handoff(WRAP)), c, false, stop).unwrap();
    s
}

#[test]
fn the_expected_return_follows_the_local_night() {
    let utc = TimeZone::UTC;
    let tz = Some(&utc);
    let ret = |h, m| expected_return(at(h, m), 10 * HOUR, 9 * 60, tz);
    // A stop in the night waits until that morning: about 7.75h.
    assert_eq!(ret(1, 15), at(9, 0));
    // A horizon that ends in the night runs on to the morning: 12h.
    assert_eq!(ret(21, 0), at(33, 0));
    // A day stop whose horizon ends before midnight waits the horizon.
    assert_eq!(ret(13, 0), at(23, 0));
    // With the zone unknown, the plain horizon applies.
    assert_eq!(expected_return(at(1, 15), 10 * HOUR, 9 * 60, None), at(11, 15));
    assert_eq!(expected_return(at(21, 0), 10 * HOUR, 9 * 60, None), at(31, 0));
    // The morning setting moves the night's end.
    assert_eq!(expected_return(at(1, 15), 10 * HOUR, 7 * 60 + 30, tz), at(7, 30));
}

#[test]
fn the_pings_needed_round_up() {
    assert_eq!(hold::needed(10 * HOUR, 50 * MIN), 12);
    assert_eq!(hold::needed(7 * HOUR + 45 * MIN, 50 * MIN), 10);
    assert_eq!(hold::needed(50 * MIN, 50 * MIN), 1);
}

/// 490k at 01:15: 10 pings to 09:00 fit within the bound of 13, so the
/// first tick holds, and the tick after 09:00 lets the cache lapse.
#[test]
fn a_doubt_at_0115_on_490k_holds_to_0900_then_lapses() {
    let c = known(cfg());
    let stop = at(1, 15);
    let mut s = doubted(490_000, stop, &c);
    let w = s.state.policy.wait.clone().unwrap();
    assert_eq!((w.cause, w.until, w.until_hm.as_str(), w.zone.as_deref()), (Cause::Doubt, at(9, 0), "09:00", Some("UTC")));
    assert_eq!(hold::view(&s.state.policy).decision, None, "the first tick decides");
    let seen = ride(&mut s, &c, &carrying(Some(WRAP)), stop);
    // Pings 50 to 450 minutes after the stop; the tick at 500, 09:35, is
    // past 09:00, and the cache is warm until 09:45.
    assert_eq!(count(&seen, Action::Keepalive), 9, "{seen:?}");
    assert_eq!(seen.last(), Some(&(Action::Finish, 500)), "{seen:?}");
    let v = hold::view(&s.state.policy);
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"decision":"hold","until":1791795600,"pings":10,"reason":"the judge's doubt went to the operator: holding to 09:00, 10 pings within the bound of 13"}"#
    );
    assert_eq!(v.until, Some(at(9, 0)));
    assert_eq!(
        hold::card_line(&s.state.policy).as_deref(),
        Some("hold        holding to 09:00 (10 pings) · the judge's doubt went to the operator")
    );
}

/// 600k at 13:00: 12 pings to 23:00 against a bound of 11. The first tick
/// lets the cache lapse and sends no ping.
#[test]
fn a_doubt_at_1300_on_600k_lapses_at_the_first_tick() {
    let c = known(cfg());
    let stop = at(13, 0);
    let mut s = doubted(600_000, stop, &c);
    let seen = ride(&mut s, &c, &carrying(Some(WRAP)), stop);
    assert_eq!(seen, vec![(Action::Finish, 50)], "no partial pinging");
    let v = hold::view(&s.state.policy);
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"decision":"lapse","until":null,"pings":12,"reason":"the judge's doubt went to the operator: lapsing, 12 pings to 23:00 pass the bound of 11"}"#
    );
    assert_eq!(hold::card_line(&s.state.policy).as_deref(), Some("hold        lapsing: the bound ends before 23:00 · the judge's doubt went to the operator"));
}

/// 900k: the bound is 7, under the 10 pings to 09:00 and the 12 of a day
/// stop.
#[test]
fn a_doubt_on_900k_lapses() {
    let c = known(cfg());
    for stop in [at(1, 15), at(13, 0)] {
        let mut s = doubted(900_000, stop, &c);
        let seen = ride(&mut s, &c, &carrying(Some(WRAP)), stop);
        assert_eq!(seen, vec![(Action::Finish, 50)], "{seen:?}");
        let d = s.state.policy.wait.as_ref().and_then(|w| w.decided).unwrap();
        assert_eq!((d.hold, d.worth), (false, 7));
    }
}

/// 300k at 13:00: 12 pings to 23:00 within the bound of 22. It holds the
/// ten hours and then lapses.
#[test]
fn a_doubt_at_1300_on_300k_holds_ten_hours_then_lapses() {
    let c = known(cfg());
    let stop = at(13, 0);
    let mut s = doubted(300_000, stop, &c);
    let seen = ride(&mut s, &c, &carrying(Some(WRAP)), stop);
    assert_eq!(count(&seen, Action::Keepalive), 11, "{seen:?}");
    assert_eq!(seen.last(), Some(&(Action::Finish, 600)), "{seen:?}");
    let v = hold::view(&s.state.policy);
    assert_eq!((v.decision, v.until, v.pings), (Some("hold"), Some(at(23, 0)), Some(12)));
}

/// A stop at 21:00 waits to 09:00, twelve hours.
#[test]
fn a_stop_at_2100_waits_to_0900() {
    let s = doubted(300_000, at(21, 0), &known(cfg()));
    let w = s.state.policy.wait.unwrap();
    assert_eq!((w.until, w.until.saturating_sub(w.at)), (at(33, 0), 12 * HOUR));
}

/// A host on UTC with no override has no zone keepwarm can trust: the
/// plain horizon applies, 01:15 to 11:15, and status names no zone.
#[test]
fn with_the_zone_unknown_the_plain_horizon_applies() {
    let c = cfg();
    assert_eq!(ZoneView::from(c.windows.zone()), ZoneView { zone: None, source: None });
    let s = doubted(490_000, at(1, 15), &c);
    let w = s.state.policy.wait.clone().unwrap();
    assert_eq!((w.until, w.zone), (at(11, 15), None));
    assert!(hold::view(&s.state.policy).reason.unwrap().contains("the zone unknown, so the plain horizon"));
}

/// The host's zone, and an override, resolved through the one function the
/// keepalive windows use: a host on UTC or with no zone is unknown, a
/// named host zone is known, and an override wins over either.
#[test]
fn the_zone_comes_from_the_override_else_a_named_host_zone() {
    let berlin = TimeZone::get("Europe/Berlin").expect("the system zone database knows Europe/Berlin");
    let view = |z: &Zone| serde_json::to_string(&ZoneView::from(z)).unwrap();
    assert_eq!(view(&resolve_with("", Some(TimeZone::UTC))), r#"{"zone":null,"source":null}"#);
    assert_eq!(view(&resolve_with("", None)), r#"{"zone":null,"source":null}"#);
    assert_eq!(view(&resolve_with("", Some(TimeZone::fixed(jiff::tz::offset(2))))), r#"{"zone":null,"source":null}"#);
    assert_eq!(view(&resolve_with("", Some(berlin.clone()))), r#"{"zone":"Europe/Berlin","source":"host"}"#);
    assert_eq!(view(&resolve_with("America/Denver", Some(berlin.clone()))), r#"{"zone":"America/Denver","source":"override"}"#);
    assert_eq!(view(&resolve_with("America/Denver", None)), r#"{"zone":"America/Denver","source":"override"}"#);
    let bad = resolve_with("Not/AZone", Some(berlin));
    assert_eq!((view(&bad), bad.bad.as_deref()), (r#"{"zone":"Europe/Berlin","source":"host"}"#.to_string(), Some("Not/AZone")));
    // The windows read the same zone.
    let w = Windows::parse(&[DEFAULT_WINDOW.to_string()], "America/Denver");
    assert_eq!(ZoneView::from(w.zone()).zone.as_deref(), Some("America/Denver"));
}

/// With an override, the morning is computed in that zone: 01:15 in
/// Denver (07:15 UTC, daylight time) waits until 09:00 in Denver, 15:00
/// UTC.
#[test]
fn with_an_override_the_morning_is_computed_in_that_zone() {
    let c = Config { windows: Windows::parse(&[DEFAULT_WINDOW.to_string()], "America/Denver"), ..cfg() };
    let stop = at(7, 15);
    let w = hold::begin(Cause::Doubt, &c, stop);
    assert_eq!((w.until, w.until_hm.as_str(), w.zone.as_deref()), (at(15, 0), "09:00", Some("America/Denver")));
}

/// The operator's prompt ends a hold: the wait and the exit clear, and the
/// next due point pings as the keepalive count's.
#[test]
fn a_hold_ends_on_the_operators_prompt() {
    let c = known(cfg());
    let stop = at(1, 15);
    let mut s = doubted(490_000, stop, &c);
    let site = carrying(Some(WRAP));
    let d = s.check_at(&c, Gate::Off, &site, stop + 50 * MIN);
    assert_eq!((d.action, d.state), (Action::Keepalive, "armed"), "{d:?}");
    assert!(d.reason.contains("holding to 09:00"), "{}", d.reason);
    s.turn(stop + 50 * MIN);
    reset(&mut s.state.policy, stop + 60 * MIN);
    assert_eq!((s.state.policy.wait.as_ref(), s.state.policy.exit.as_ref()), (None, None));
    assert_eq!(hold::view(&s.state.policy), HoldView { decision: None, until: None, pings: None, reason: None });
    s.turn(stop + 61 * MIN);
    let d = s.check_at(&c, Gate::Off, &site, stop + 111 * MIN);
    assert_eq!((d.action, d.state, d.count), (Action::Keepalive, "running", 1), "{d:?}");
}

/// A relay that stopped on a decision waits on the operator: the wait is
/// recorded with the stop, and once the stop's compaction ran, the hold
/// pings by itself until 09:00, at the compacted context's price. Without
/// the hold a relay stop runs no keepalive.
#[test]
fn a_relay_decision_stop_holds_until_the_operator_is_likely_back() {
    let c = known(relay());
    let stop = at(21, 0);
    let mut s = Session::on(OPUS, stop - 2 * MIN, 700_000);
    let id = "20261012T2100Z-3f9a1c2e";
    let marker = format!("relay: stop decision tasks=2 goal=none wrap={id} head=e6bbfbdd");
    wrapped(&mut s.state.policy, WrapOutcome::Transcript, "the reply is in the transcript".into(), None, Some(marker), stop);
    let site = Site { handoff: Some(Handoff { wrap_id: Some(id.into()), ..handoff(id) }), ..site() };
    let d = s.check_at(&c, Gate::Off, &site, stop);
    assert_eq!((d.action, d.relay_stop.as_deref()), (Action::Compact, Some("decision")), "{d:?}");
    let w = s.state.policy.wait.clone().expect("the decision stop waits on the operator");
    assert_eq!((w.cause, w.until), (Cause::Decision, at(33, 0)));
    // The compaction ran, and its request read a 60k context.
    compacted(&mut s.state.policy, None, stop + 5 * MIN);
    s.tail.push(UsageEntry { model: OPUS.into(), ..entry(stop + 5 * MIN, 60_000, 300) });
    let r = s.reading();
    assert_eq!(next_tick_at(&s.state, &r, None, &c, &site, stop + 6 * MIN), Some(stop + 55 * MIN), "a supervisor sees the next tick");
    let seen = ride(&mut s, &c, &site, stop + 5 * MIN);
    // Ticks 50 minutes apart from 21:05: fourteen fall before 09:00.
    assert_eq!(count(&seen, Action::Keepalive), 14, "{seen:?}");
    assert_eq!(count(&seen, Action::Finish), 0, "a relay stop's hold lapses with no finish mark: {seen:?}");
    let v = hold::view(&s.state.policy);
    assert_eq!((v.decision, v.until, v.pings), (Some("hold"), Some(at(33, 0)), Some(15)));
    let w = s.state.policy.wait.as_ref().unwrap();
    assert!(w.ended_at.is_some_and(|t| t >= at(33, 0)), "{w:?}");
    let d = s.check_at(&c, Gate::Off, &site, at(35, 0));
    assert_eq!((d.action, d.state, d.next_at), (Action::None, "stopped", None), "{d:?}");
}

/// A resumed turn's `relay: stop decision` line waits on the operator the
/// same way.
#[test]
fn a_stop_line_on_a_decision_waits_on_the_operator() {
    let c = known(relay());
    let mut p = Policy { relays: 1, ..Policy::default() };
    assert!(stopped_by_agent(&mut p, "decision", "which db", &c, at(21, 0)));
    let w = p.wait.unwrap();
    assert_eq!((w.cause, w.until), (Cause::Decision, at(33, 0)));
    let mut p = Policy { relays: 1, ..Policy::default() };
    assert!(stopped_by_agent(&mut p, "empty", "", &c, at(21, 0)));
    assert_eq!(p.wait, None, "an empty stop waits on no one");
}

/// A boundary `stop` whose handoff lists a one-way decision waits on the
/// operator; a reversible one does not.
#[test]
fn a_stop_with_a_one_way_decision_waits_on_the_operator() {
    let text = |reversible: &str| {
        format!(
            "2026-11-27 e6bbfbdd wrap={WRAP}\nContinue the demo.\n\nDECISIONS — choices left to the operator\n1. Cut 1.58.0 now? — blocks: #3 — lean: yes — reversible: {reversible}\n2. Rename the flag? — blocks: none — lean: no — reversible: yes\n\nCONVENTIONS — branch, PR, merge\n\nDO NEXT — start #1."
        )
    };
    assert_eq!(one_way_decisions(&text("no")), vec!["Cut 1.58.0 now?".to_string()]);
    assert!(one_way_decisions(&text("yes")).is_empty());
    assert!(one_way_decisions("no such section\n1. Cut? — blocks: #1 — lean: yes — reversible: no").is_empty());
    let c = known(cfg());
    for (reversible, waits) in [("no", true), ("yes", false)] {
        let mut s = Session::on(OPUS, at(13, 0), 300_000);
        let h = Handoff { text: text(reversible), ..handoff(WRAP) };
        exit::record(&mut s.state.policy, judgement(Verdict::Stop, Sufficiency::Sufficient), Some(&h), &c, false, at(13, 0)).unwrap();
        assert_eq!(s.state.policy.wait.as_ref().map(|w| w.cause), waits.then_some(Cause::Decisions), "{reversible}");
    }
    // A later boundary that arms nothing clears the wait.
    let mut s = doubted(300_000, at(13, 0), &c);
    let j = Judgement { wrap_id: WRAP.into(), ..judgement(Verdict::Continue, Sufficiency::Sufficient) };
    exit::record(&mut s.state.policy, j, Some(&handoff(WRAP)), &c, false, at(13, 5)).unwrap();
    assert_eq!(s.state.policy.wait, None);
}

/// An awaited reply keeps its old rule: a doubt whose hold lapses still
/// pings by the bound while a thread awaits a reply, then finishes.
#[test]
fn an_awaited_reply_behaves_as_before_when_the_hold_lapses() {
    let c = Config { pending: Pending { awaiting: 1, ..Pending::default() }, ..known(cfg()) };
    let stop = at(13, 0);
    let mut s = doubted(600_000, stop, &c);
    let seen = ride(&mut s, &c, &carrying(Some(WRAP)), stop);
    assert_eq!(count(&seen, Action::Keepalive), 11, "the bound at 600k: {seen:?}");
    assert_eq!(seen.last().map(|x| x.0), Some(Action::Finish), "{seen:?}");
    assert_eq!(hold::view(&s.state.policy).decision, Some("lapse"));
}

/// `operator_horizon`, `morning` and `timezone` from the settings: a
/// value out of range or of the wrong form keeps its default.
#[test]
fn the_horizon_and_the_morning_come_from_the_settings() {
    let doc = |h: &str, m: &str| {
        format!(
            r#"{{"keys":{{"mods.attend.continuity.operator_horizon":{{"value":"{h}"}},"mods.attend.continuity.morning":{{"value":"{m}"}},"mods.attend.continuity.timezone":{{"value":"America/Denver"}}}}}}"#
        )
    };
    let c = Config::from_settings(Some(&doc("8h30m", "07:45")));
    assert_eq!((c.horizon_secs, c.morning_min), (8 * HOUR + 30 * MIN, 7 * 60 + 45));
    assert_eq!(ZoneView::from(c.windows.zone()), ZoneView { zone: Some("America/Denver".into()), source: Some(ZoneSource::Override) });
    for (h, m) in [("30m", "25:00"), ("49h", "9am"), ("soon", "24:00")] {
        let c = Config::from_settings(Some(&doc(h, m)));
        assert_eq!((c.horizon_secs, c.morning_min), (10 * HOUR, 9 * 60), "{h} {m}");
    }
    let d = Config::default();
    assert_eq!((d.horizon_secs, d.morning_min), (10 * HOUR, 9 * 60));
}
