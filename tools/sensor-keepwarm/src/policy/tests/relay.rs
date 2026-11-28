// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Relay mode over fixed ledgers and a fixed clock: the fill path, the
//! marker, the compaction and the resume, the bounds and the stops. Each
//! test steps the judgment as the mod's checks would, and reports what the
//! mod would report between them.
//!
//! References:
//! - ADR-417: relay mode follows a wrap with a compaction and a resume.

use super::*;

pub(super) const WINDOW: u64 = 1_000_000;
const HANDOFF: &str = "/work/agent-ways/.claude/handoff.wrap.md";

pub(super) fn relay() -> Config {
    let mut c = cfg();
    c.relay = true;
    c
}

pub(super) fn filled(mut c: Config, tokens: u64) -> Config {
    c.fill = Some(Fill { tokens, window: WINDOW, native_at: None });
    c
}

/// The site with the handoff the wrap `id` wrote.
fn site_with(id: Option<&str>) -> Site {
    Site {
        handoff: Some(Handoff {
            path: PathBuf::from(HANDOFF),
            wrap_id: id.map(str::to_string),
            instructions: format!("Summarize this session as the handoff below; keep its TASKS and DO NEXT whole.\n\n2026-10-12 e6bbfbdd wrap={}", id.unwrap_or("none")),
            text: format!("2026-10-12 e6bbfbdd wrap={}\nContinue the demo.\n\nDO NEXT — start #1.", id.unwrap_or("none")),
        }),
        ..site()
    }
}

impl Session {
    pub(super) fn check_at(&mut self, cfg: &Config, gate: Gate, site: &Site, now: u64) -> Decision {
        let r = self.reading();
        let ledger = self.state.ledger(now);
        let text = serde_json::to_string(&ledger).unwrap();
        self.state = State::resume(Some(serde_json::from_str::<Ledger>(&text).unwrap()));
        step(&mut self.state, &r, None, cfg, gate, site, now).1
    }

    /// What `attend keepwarm wrapped transcript --marker <line>` records.
    fn answer(&mut self, marker: Option<&str>, now: u64) {
        wrapped(&mut self.state.policy, WrapOutcome::Transcript, "the reply is in the transcript".into(), None, marker.map(str::to_string), now);
    }
}

fn marker(stop: Option<&str>, goal: &str, id: &str) -> String {
    let head = match stop {
        Some(r) => format!("stop {r}"),
        None => "continue".into(),
    };
    format!("relay: {head} tasks=2 goal={goal} wrap={id} head=e6bbfbdd")
}

/// A fill wrap sent at the hard line and the id its text issued.
fn fill_wrap(s: &mut Session, now: u64) -> (Decision, String) {
    let d = s.check_at(&filled(relay(), 860_000), Gate::Off, &site(), now);
    assert_eq!(d.action, Action::Wrap, "{d:?}");
    let id = d.wrap_id.clone().expect("a relay wrap carries its id");
    (d, id)
}

#[test]
fn the_marker_grammar_is_whole_and_strict() {
    let m = Marker::parse("relay: continue tasks=3 goal=none wrap=20261126T2140Z-3f9a1c2e head=e6bbfbdd").unwrap();
    assert_eq!((m.stop, m.tasks, m.goal_active, m.wrap.as_str(), m.head.as_str()), (None, 3, false, "20261126T2140Z-3f9a1c2e", "e6bbfbdd"));
    let m = Marker::parse("relay: stop decision tasks=0 goal=active wrap=abcdefgh head=0123456789abcdef0123456789abcdef01234567").unwrap();
    assert_eq!((m.stop, m.goal_active), (Some("decision"), true));
    for bad in [
        "relay: continue tasks=3 goal=none wrap=20261126T2140Z-3f9a1c2e head=e6bbfbdd ",
        "relay:  continue tasks=3 goal=none wrap=abcdefgh head=e6bbfbdd",
        "relay: continue tasks=3 goal=none wrap=abcdefg head=e6bbfbdd",
        "relay: continue tasks=3 goal=none wrap=abc_defgh head=e6bbfbdd",
        "relay: continue tasks=3 goal=none wrap=abcdefgh head=E6BBFBDD",
        "relay: continue tasks=3 goal=none wrap=abcdefgh head=e6bbfb",
        "relay: continue tasks=x goal=none wrap=abcdefgh head=e6bbfbdd",
        "relay: continue tasks=3 goal=met wrap=abcdefgh head=e6bbfbdd",
        "relay: stop later tasks=3 goal=none wrap=abcdefgh head=e6bbfbdd",
        "relay: stop tasks=3 goal=none wrap=abcdefgh head=e6bbfbdd",
        "relay: continue goal=none tasks=3 wrap=abcdefgh head=e6bbfbdd",
        "Relay: continue tasks=3 goal=none wrap=abcdefgh head=e6bbfbdd",
    ] {
        assert_eq!(Marker::parse(bad), None, "{bad}");
    }
}

#[test]
fn a_resumed_turn_s_stop_line_names_its_reason() {
    assert_eq!(stop_line("relay: stop decision which database to use"), Some("decision"));
    assert_eq!(stop_line("relay: stop doubt: the tests contradict the plan"), Some("doubt"));
    assert_eq!(stop_line("relay: stop maybe later"), None);
    assert_eq!(stop_line("relay: stop empty tasks=0 goal=none wrap=abcdefgh head=e6bbfbdd"), None, "a whole marker is the wrap's");
    assert_eq!(stop_line("we could relay: stop decision"), None);
}

#[test]
fn the_relay_settings_are_read_and_held_to_their_ranges() {
    let c = Config::from_settings(Some(
        r#"{"keys":{"mods.attend.continuity.relay":{"value":"on"},"mods.attend.continuity.relay_at_pct":{"value":60},"mods.attend.continuity.max_relays":{"value":2}}}"#,
    ));
    assert_eq!((c.relay, c.relay_at_pct, c.max_relays), (true, 60, 2));
    let c = Config::from_settings(Some(
        r#"{"keys":{"mods.attend.continuity.relay":{"value":"yes"},"mods.attend.continuity.relay_at_pct":{"value":99},"mods.attend.continuity.max_relays":{"value":51}}}"#,
    ));
    assert_eq!((c.relay, c.relay_at_pct, c.max_relays), (false, DEFAULT_RELAY_AT_PCT, DEFAULT_MAX_RELAYS));
}

#[test]
fn a_fill_past_the_hard_line_sends_one_relay_wrap_naming_its_id() {
    let mut s = Session::new(MONDAY, 700_000);
    let below = s.check_at(&filled(relay(), 740_000), Gate::Off, &site(), MONDAY + MIN);
    assert_eq!(below.action, Action::None);
    assert_eq!((below.relay_at_tokens, below.hard_at_tokens), (Some(750_000), Some(850_000)));
    let (d, id) = fill_wrap(&mut s, MONDAY + 2 * MIN);
    assert_eq!(id, "20261012T000200Z-sid1");
    let text = d.text.unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("wrap: context at 86% (860k of 1M), past the hard line of 85%."));
    assert!(text.contains(&format!("This wrap is in relay mode with wrap id {id}: write wrap={id} on the handoff's first line and end your reply with the relay marker.")), "{text}");
    assert!(text.contains("Run the wrap skill with the argument stay"), "{text}");
    assert_eq!(d.wrap.as_ref().unwrap().after, "relay");
    assert_eq!((s.state.policy.trigger.as_deref(), s.state.policy.last_wrap_id.as_deref()), (Some("fill"), Some(id.as_str())));
    // One fill wrap per generation.
    s.turn(MONDAY + 3 * MIN);
    let again = s.check_at(&filled(relay(), 900_000), Gate::Off, &site(), MONDAY + 4 * MIN);
    assert_eq!((again.action, again.state), (Action::None, "wrapped"));
}

#[test]
fn relay_off_has_no_fill_path() {
    let mut s = Session::new(MONDAY, 700_000);
    let d = s.check_at(&filled(cfg(), 900_000), Gate::Off, &site(), MONDAY + MIN);
    assert_eq!(d.action, Action::None);
    assert_eq!((d.relay_at_tokens, d.hard_at_tokens), (None, None));
}

#[test]
fn a_continue_marker_with_a_fresh_handoff_compacts_and_resumes() {
    let mut s = Session::new(MONDAY, 700_000);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.turn(MONDAY + 2 * MIN);
    let waiting = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 2 * MIN);
    assert_eq!((waiting.action, waiting.state), (Action::None, "wrapped"), "no answer yet");
    s.answer(Some(&marker(None, "none", &id)), MONDAY + 3 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    assert_eq!((d.action, d.state), (Action::Compact, "compacted"), "{d:?}");
    assert!(d.text.as_deref().unwrap().starts_with("Summarize this session as the handoff below"));
    assert_eq!(
        d.resume.as_deref(),
        Some("relay: the conversation was compacted after a wrap; this is relay 1 of 5 since the operator's last prompt. Read /work/agent-ways/.claude/handoff.wrap.md. The task list survived the compaction; work from it and the handoff's TASKS, then start its DO NEXT. The operator is away: ask no questions. If a step needs a decision only the operator can make, or your doubt about the direction is high, do not start it; end your reply with one line, relay: stop decision or relay: stop doubt, followed by the reason, and the mod sends the handoff to the operator over attend.")
    );
    assert!(!d.send_handoff);
    // The mod compacts, then submits the resume and reports it.
    compacted(&mut s.state.policy, None, MONDAY + 4 * MIN);
    resumed(&mut s.state.policy, MONDAY + 4 * MIN);
    let r = s.check_at(&relay(), Gate::Off, &site(), MONDAY + 4 * MIN);
    assert_eq!((r.state, r.relay_count, r.max_relays, r.count), ("resumed", 1, 5, 0));
    // The resume's turn runs; the next generation can wrap again.
    s.turn(MONDAY + 5 * MIN);
    let r = s.check_at(&filled(relay(), 100_000), Gate::Off, &site(), MONDAY + 6 * MIN);
    assert_eq!((r.state, r.relay_count), ("running", 1));
    let (_, id2) = fill_wrap(&mut s, MONDAY + 30 * MIN);
    assert_ne!(id, id2);
}

#[test]
fn a_stale_handoff_stops_the_relay_without_a_compaction_and_hands_the_reply_over_once() {
    let mut s = Session::new(MONDAY, 700_000);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(None, "none", &id)), MONDAY + 3 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some("20261012T000000Z-older000")), MONDAY + 3 * MIN);
    assert_eq!((d.action, d.state, d.relay_stop.as_deref()), (Action::None, "stopped", Some("stale handoff")));
    assert!(d.send_handoff);
    let w = d.wrap.unwrap();
    assert!(w.handoff.ends_with(&format!("The relay stopped: the handoff's first line does not carry wrap={id}.")), "{}", w.handoff);
    let again = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 4 * MIN);
    assert_eq!((again.action, again.state, again.send_handoff), (Action::None, "stopped", false));
}

#[test]
fn a_marker_naming_another_wrap_is_stale() {
    let mut s = Session::new(MONDAY, 700_000);
    let _ = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(None, "none", "20261012T000000Z-older000")), MONDAY + 3 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some("20261012T000000Z-older000")), MONDAY + 3 * MIN);
    assert_eq!(d.relay_stop.as_deref(), Some("stale handoff"));
    assert_eq!(d.action, Action::None);
}

#[test]
fn a_wrap_with_no_marker_stops_without_a_compaction() {
    let mut s = Session::new(MONDAY, 700_000);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(None, MONDAY + 3 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    assert_eq!((d.action, d.relay_stop.as_deref(), d.send_handoff), (Action::None, Some("no marker"), true));
}

#[test]
fn a_stop_marker_compacts_with_the_handoff_and_submits_no_resume() {
    for reason in ["empty", "decision", "doubt"] {
        let mut s = Session::new(MONDAY, 700_000);
        let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
        s.answer(Some(&marker(Some(reason), "none", &id)), MONDAY + 3 * MIN);
        let d = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
        assert_eq!((d.action, d.state, d.relay_stop.as_deref()), (Action::Compact, "stopped", Some(reason)), "{d:?}");
        assert_eq!(d.resume, None);
        assert!(d.send_handoff && d.text.is_some());
    }
}

#[test]
fn the_cap_compacts_the_last_wrap_and_submits_no_resume() {
    let mut s = Session::new(MONDAY, 700_000);
    let mut at = MONDAY + MIN;
    for n in 1..=5 {
        let (_, id) = fill_wrap(&mut s, at);
        s.answer(Some(&marker(None, "none", &id)), at + MIN);
        let d = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), at + MIN);
        assert_eq!(d.resume.as_deref().map(|r| r.contains(&format!("this is relay {n} of 5"))), Some(true), "{d:?}");
        compacted(&mut s.state.policy, None, at + 2 * MIN);
        resumed(&mut s.state.policy, at + 2 * MIN);
        s.turn(at + 3 * MIN);
        at += 10 * MIN;
    }
    let (_, id) = fill_wrap(&mut s, at);
    s.answer(Some(&marker(None, "none", &id)), at + MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), at + MIN);
    assert_eq!((d.action, d.relay_stop.as_deref(), d.resume.as_deref()), (Action::Compact, Some("5 of 5"), None));
    assert!(d.send_handoff);
    // The operator's prompt starts both counts again, and keeps the id.
    reset(&mut s.state.policy, at + 5 * MIN);
    assert_eq!((s.state.policy.relays, s.state.policy.last_wrap_id.as_deref()), (0, Some(id.as_str())));
}

#[test]
fn a_goal_that_ended_stops_the_relay() {
    let mut s = Session::new(MONDAY, 700_000);
    goal(&mut s.state.policy, true);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(None, "none", &id)), MONDAY + 3 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    assert_eq!((d.action, d.relay_stop.as_deref(), d.resume.as_deref()), (Action::Compact, Some("goal ended"), None));
    // A goal still active relays as usual.
    let mut s = Session::new(MONDAY, 700_000);
    goal(&mut s.state.policy, true);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(None, "active", &id)), MONDAY + 3 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    assert!(d.resume.is_some(), "{d:?}");
}

#[test]
fn a_slash_wrap_s_marker_is_a_wrap_of_its_own() {
    let mut s = Session::new(MONDAY, 300_000);
    let id = "20261012T0010Z-sid1abcd";
    s.answer(Some(&marker(None, "none", id)), MONDAY + 10 * MIN);
    assert_eq!(s.state.policy.trigger.as_deref(), Some("wrap"));
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(id)), MONDAY + 10 * MIN);
    assert_eq!(d.action, Action::Compact);
    assert_eq!(s.state.policy.last_wrap_id.as_deref(), Some(id));
    // With relay off by the time it is judged, it is no relay wrap.
    let mut s = Session::new(MONDAY, 300_000);
    s.answer(Some(&marker(None, "none", id)), MONDAY + 10 * MIN);
    let d = s.check_at(&cfg(), Gate::Off, &site_with(Some(id)), MONDAY + 10 * MIN);
    assert_eq!(d.action, Action::None);
    assert!(!s.state.policy.relay);
    // Its id is still the session's last wrap's, for the PreCompact hook.
    assert_eq!(s.state.policy.last_wrap_id.as_deref(), Some(id));
}

/// A handoff-mode wrap records its id too (`attend keepwarm wrap-id
/// --record`), so a relay wrap's id does not outlive a later `/wrap
/// handoff` as the session's last.
///
/// References:
/// - ADR-417 item 3: the PreCompact hook asks attend for the last wrap's id.
#[test]
fn a_handoff_mode_wrap_records_its_id_over_a_relay_wrap_s() {
    let mut s = Session::new(MONDAY, 300_000);
    let a = "20261012T0010Z-sid1abcd";
    s.answer(Some(&marker(None, "none", a)), MONDAY + 10 * MIN);
    s.check_at(&relay(), Gate::Off, &site_with(Some(a)), MONDAY + 10 * MIN);
    compacted(&mut s.state.policy, None, MONDAY + 11 * MIN);
    resumed(&mut s.state.policy, MONDAY + 11 * MIN);
    reset(&mut s.state.policy, MONDAY + 20 * MIN);
    assert_eq!(s.state.policy.last_wrap_id.as_deref(), Some(a));
    let b = "20261012T0030Z-sid1abcd";
    assert!(record_wrap_id(&mut s.state.policy, b));
    assert_eq!(s.state.policy.last_wrap_id.as_deref(), Some(b));
    assert!(!record_wrap_id(&mut s.state.policy, "bad id!"), "only an id of the marker's shape");
    assert_eq!(s.state.policy.last_wrap_id.as_deref(), Some(b));
}

/// The skill records its id in either mode. In relay mode the record comes
/// before the marker, which is still this wrap's own, not stale; a marker
/// that repeats the id of the wrap judged before it still is.
#[test]
fn a_recorded_id_does_not_make_its_own_relay_marker_stale() {
    let mut s = Session::new(MONDAY, 300_000);
    let a = "20261012T0010Z-sid1abcd";
    assert!(record_wrap_id(&mut s.state.policy, a));
    s.answer(Some(&marker(None, "none", a)), MONDAY + 10 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(a)), MONDAY + 10 * MIN);
    assert_eq!(d.action, Action::Compact, "{d:?}");
    compacted(&mut s.state.policy, None, MONDAY + 11 * MIN);
    resumed(&mut s.state.policy, MONDAY + 11 * MIN);
    s.turn(MONDAY + 12 * MIN);
    s.answer(Some(&marker(None, "none", a)), MONDAY + 13 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(a)), MONDAY + 13 * MIN);
    assert_eq!((d.action, d.relay_stop.as_deref()), (Action::None, Some("stale handoff")), "{d:?}");
}

/// The compaction is recorded when the mod reports it ran, not when it is
/// asked for: a check that asked for one and heard no report asks again,
/// as when a turn started before the mod could compact.
///
/// References:
/// - ADR-417 item 4: a turn between the wrap's answer and the compaction
///   cancels it, and the next turn's end judges again.
#[test]
fn a_compaction_the_mod_did_not_run_is_asked_for_again() {
    let mut s = Session::new(MONDAY, 700_000);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(None, "none", &id)), MONDAY + 3 * MIN);
    let first = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    assert_eq!(first.action, Action::Compact);
    assert_eq!(s.state.policy.compacted_at, None, "asked for, not run");
    // A turn ran instead; its end judges again.
    s.turn(MONDAY + 4 * MIN);
    let again = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 5 * MIN);
    assert_eq!((again.action, again.state), (Action::Compact, "compacted"), "{again:?}");
    assert_eq!((again.text.as_deref(), again.resume.as_deref()), (first.text.as_deref(), first.resume.as_deref()));
    compacted(&mut s.state.policy, None, MONDAY + 6 * MIN);
    assert_eq!(s.state.policy.compacted_at, Some(MONDAY + 6 * MIN));
    resumed(&mut s.state.policy, MONDAY + 6 * MIN);
    let r = s.check_at(&relay(), Gate::Off, &site(), MONDAY + 6 * MIN);
    assert_eq!((r.state, r.relay_count), ("resumed", 1));

    // A stop's compaction is asked for again too, with the reply handed
    // over once.
    let mut s = Session::new(MONDAY, 700_000);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(Some("decision"), "none", &id)), MONDAY + 3 * MIN);
    let first = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    assert_eq!((first.action, first.send_handoff), (Action::Compact, true));
    s.turn(MONDAY + 4 * MIN);
    let again = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 5 * MIN);
    assert_eq!((again.action, again.state, again.send_handoff, again.resume.as_deref()), (Action::Compact, "stopped", false, None), "{again:?}");
    compacted(&mut s.state.policy, None, MONDAY + 6 * MIN);
    let after = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 7 * MIN);
    assert_eq!((after.action, after.state), (Action::None, "stopped"));
}

/// A check that finds the conversation compacted after a relay wrap and no
/// resume reported has lost the mod that compacted (a reload, a restart):
/// the relay stops and hands the handoff to the operator, once, and
/// submits no resume it cannot tell went in already.
///
/// References:
/// - ADR-417 item 4: a resume that does not go in stops the relay.
#[test]
fn a_compaction_with_no_resume_after_it_stops_and_hands_the_handoff_over() {
    let mut s = Session::new(MONDAY, 700_000);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(None, "none", &id)), MONDAY + 3 * MIN);
    s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    compacted(&mut s.state.policy, None, MONDAY + 4 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 5 * MIN);
    assert_eq!((d.action, d.state, d.relay_stop.as_deref(), d.send_handoff), (Action::None, "stopped", Some("resume lost"), true), "{d:?}");
    assert_eq!(d.resume, None);
    assert_eq!(d.text.as_deref(), Some(format!("2026-10-12 e6bbfbdd wrap={id}\nContinue the demo.\n\nDO NEXT — start #1.").as_str()), "the handoff, to send");
    assert!(d.reason.contains("the resume never went in"), "{}", d.reason);
    let again = s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 6 * MIN);
    assert_eq!((again.state, again.send_handoff), ("stopped", false));
}

#[test]
fn a_failed_compaction_or_resume_stops_the_relay() {
    let mut s = Session::new(MONDAY, 700_000);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(None, "none", &id)), MONDAY + 3 * MIN);
    s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    compacted(&mut s.state.policy, Some("skipped: a hook vetoed it".into()), MONDAY + 4 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site(), MONDAY + 4 * MIN);
    assert_eq!((d.state, d.relay_stop.as_deref()), ("stopped", Some("compaction failed")));
    assert!(d.reason.contains("skipped: a hook vetoed it"), "{}", d.reason);

    let mut s = Session::new(MONDAY, 700_000);
    let (_, id) = fill_wrap(&mut s, MONDAY + MIN);
    s.answer(Some(&marker(None, "none", &id)), MONDAY + 3 * MIN);
    s.check_at(&relay(), Gate::Off, &site_with(Some(&id)), MONDAY + 3 * MIN);
    compacted(&mut s.state.policy, None, MONDAY + 4 * MIN);
    resume_failed(&mut s.state.policy, "the prompt did not enter");
    let d = s.check_at(&relay(), Gate::Off, &site(), MONDAY + 4 * MIN);
    assert_eq!((d.state, d.relay_stop.as_deref()), ("stopped", Some("resume failed")));
}

#[test]
fn a_resumed_generation_s_stop_line_stops_the_relay() {
    let mut p = Policy::default();
    assert!(!stopped_by_agent(&mut p, "decision", "which db", &relay(), MONDAY), "no relay since the prompt");
    p.relays = 1;
    assert!(!stopped_by_agent(&mut p, "later", "", &relay(), MONDAY));
    assert!(stopped_by_agent(&mut p, "decision", "which  db", &relay(), MONDAY));
    assert_eq!((p.relay_stop.as_deref(), p.stopped.as_deref()), (Some("decision"), Some("the agent stopped the relay: decision, which db")));
    // The next check hands the turn's reply to the mod, once.
    let mut s = Session::new(MONDAY, 300_000);
    s.state.policy = p;
    let d = s.check_at(&relay(), Gate::Off, &site(), MONDAY + MIN);
    assert_eq!((d.state, d.send_handoff, d.relay_stop.as_deref()), ("stopped", true, Some("decision")));
    assert!(d.wrap.unwrap().handoff.ends_with("The relay stopped: the agent stopped the relay: decision, which db."));
    let d = s.check_at(&relay(), Gate::Off, &site(), MONDAY + 2 * MIN);
    assert_eq!((d.state, d.send_handoff), ("stopped", false));
}

#[test]
fn with_relay_on_the_idle_path_runs_whatever_autocompact_says() {
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check_at(&relay(), Gate::On, &site(), MONDAY + 50 * MIN);
    assert_eq!((d.action, d.policy), (Action::Keepalive, true));
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check_at(&cfg(), Gate::On, &site(), MONDAY + 50 * MIN);
    assert_eq!((d.policy, d.reason.as_str()), (false, "autocompact is on"));
}

#[test]
fn with_keepalive_off_relay_runs_only_its_fill_path() {
    let mut c = relay();
    c.enabled = false;
    let mut s = Session::new(MONDAY, 700_000);
    let d = s.check_at(&c, Gate::Off, &site(), MONDAY + 50 * MIN);
    assert_eq!((d.action, d.policy), (Action::None, true));
    assert_eq!(d.reason, "mods.attend.continuity.keepalive is false, so only relay's fill path runs");
    let d = s.check_at(&filled(c, 860_000), Gate::Off, &site(), MONDAY + 51 * MIN);
    assert_eq!(d.action, Action::Wrap);
}

#[test]
fn an_idle_wrap_in_relay_mode_is_a_relay_wrap_with_no_stretch_after_it() {
    let mut c = relay();
    c.max = 0;
    c.after_wrap = AfterWrap::Compact;
    c.after_max = 3;
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check_at(&c, Gate::Off, &site(), MONDAY + 50 * MIN);
    assert_eq!(d.action, Action::Wrap);
    let text = d.text.unwrap();
    assert!(text.starts_with("wrap: 50m since a long idle stretch") || text.starts_with("wrap: "), "{text}");
    assert!(text.contains("This wrap is in relay mode with wrap id"), "{text}");
    assert!(!text.contains("keeps the prompt cache warm"), "no after_wrap sentence: {text}");
    let w = d.wrap.unwrap();
    assert_eq!((w.after, w.after_keepalives, d.next_at), ("relay", 0, None));
    assert_eq!(s.state.policy.trigger.as_deref(), Some("idle"));
}

#[test]
fn an_unfinished_relay_wrap_stops_the_relay() {
    let mut s = Session::new(MONDAY, 700_000);
    let _ = fill_wrap(&mut s, MONDAY + MIN);
    wrapped(&mut s.state.policy, WrapOutcome::Unfinished, "the wrap did not finish (the operator prompted)".into(), None, None, MONDAY + 2 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site(), MONDAY + 2 * MIN);
    assert_eq!((d.state, d.relay_stop.as_deref()), ("stopped", Some("wrap unfinished")));
}

#[test]
fn the_card_names_the_mode_the_count_and_a_stop() {
    let mut s = Session::new(MONDAY, 700_000);
    reset(&mut s.state.policy, MONDAY);
    let last = |s: &Session, c: &Config| card(&s.state, &s.reading(), c, &site(), MONDAY + 5 * MIN).last().cloned().unwrap();
    s.check_at(&cfg(), Gate::Off, &site(), MONDAY + MIN);
    assert_eq!(last(&s, &cfg()), "relay       off: a wrap ends with a /compact line for the operator (handoff mode)");
    s.state.policy.relays = 2;
    assert_eq!(last(&s, &relay()), "relay       on · 2 of 5 since your prompt at 00:00 · drains at 75% and wraps mid-work at 85% of the window");
    stop_relay(&mut s.state.policy, "decision", "the wrap reported stop decision: the next step needs a decision only the operator can make");
    assert_eq!(
        last(&s, &relay()),
        "relay       stopped: the wrap reported stop decision: the next step needs a decision only the operator can make; your next prompt starts it again"
    );
}

/// The end-to-end run's finding: a resumed turn that ends with a whole
/// marker repeating the last wrap's id wrote no new handoff, so it is
/// stale, not a wrap of its own.
#[test]
fn a_marker_that_repeats_the_last_wrap_s_id_is_stale() {
    let mut s = Session::new(MONDAY, 300_000);
    let id = "20261012T0010Z-sid1abcd";
    s.answer(Some(&marker(None, "none", id)), MONDAY + 10 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(id)), MONDAY + 10 * MIN);
    assert_eq!(d.action, Action::Compact);
    compacted(&mut s.state.policy, None, MONDAY + 11 * MIN);
    resumed(&mut s.state.policy, MONDAY + 11 * MIN);
    s.turn(MONDAY + 12 * MIN);
    s.answer(Some(&marker(Some("empty"), "none", id)), MONDAY + 13 * MIN);
    let d = s.check_at(&relay(), Gate::Off, &site_with(Some(id)), MONDAY + 13 * MIN);
    assert_eq!((d.action, d.relay_stop.as_deref(), d.send_handoff), (Action::None, Some("stale handoff"), true), "{d:?}");
}
