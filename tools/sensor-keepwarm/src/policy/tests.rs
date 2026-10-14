//! The keepalive policy over fixed transcript readings and a fixed clock
//! (ADR-414 item 7): each test steps the judgment as the mod's checks
//! would, adding the request each submitted turn makes.

use super::*;
use crate::{Ledger, UsageEntry, TTL_5M_SECS};
use jiff::tz::TimeZone;

/// 2026-10-12T00:00:00Z, a Monday.
const MONDAY: u64 = 1_791_763_200;
const MIN: u64 = 60;

fn entry(at: u64, read: u64, write: u64) -> UsageEntry {
    UsageEntry { at_epoch: at, model: "claude-opus-5".into(), input: 20, cache_read: read, cache_creation: write, output: 5, tier: "1h".into() }
}

/// The session's transcript: one request per turn, each reading the whole
/// prefix warm.
struct Session {
    tail: Vec<UsageEntry>,
    state: State,
}

impl Session {
    fn new(at: u64, ctx: u64) -> Self {
        let mut s = Session { tail: vec![entry(at, ctx, 500)], state: State::default() };
        s.state.step(&s.reading(), None, at);
        s
    }

    fn reading(&self) -> Reading {
        let tokens_used = self.tail.last().map(|e| e.context()).unwrap_or(0);
        Reading { tokens_used, model: "claude-opus-5".into(), usage_tail: self.tail.clone() }
    }

    /// A main-loop request at `at`, warm.
    fn turn(&mut self, at: u64) {
        let ctx = self.tail.last().map(|e| e.cache_read).unwrap_or(200_000);
        self.tail.push(entry(at, ctx, 300));
    }

    fn check(&mut self, cfg: &Config, gate: Gate, now: u64) -> Decision {
        self.check_armed(cfg, gate, None, now)
    }

    fn check_armed(&mut self, cfg: &Config, gate: Gate, arm: Option<&ArmFile>, now: u64) -> Decision {
        let r = self.reading();
        // The state crosses the ledger between checks, as `check_once` does.
        let ledger = self.state.ledger(now);
        let text = serde_json::to_string(&ledger).unwrap();
        self.state = State::resume(Some(serde_json::from_str::<Ledger>(&text).unwrap()));
        step(&mut self.state, &r, arm, cfg, gate, &site(), now).1
    }
}

fn site() -> Site {
    Site {
        session_id: "sid-1".into(),
        project: "/work/agent-ways".into(),
        name: "Chaucer-alpha".into(),
        operator: "aaron".into(),
        state_root: PathBuf::from("/state/agent-ways"),
        home: PathBuf::from("/home/aaron"),
    }
}

fn utc(mut cfg: Config) -> Config {
    cfg.windows = cfg.windows.with_zone(TimeZone::UTC);
    cfg
}

fn cfg() -> Config {
    utc(Config::default())
}

fn windows(entries: &[&str]) -> Config {
    utc(Config { windows: Windows::parse(&entries.iter().map(|s| s.to_string()).collect::<Vec<_>>(), ""), ..Config::default() })
}

/// Runs a stretch left alone from a turn at `start`: each check at the
/// `next_at` the last one gave, each submitted turn's request landing at
/// once, as the ADR's arithmetic counts it.
/// Returns each action with its time, minutes after `start`.
fn left_alone(s: &mut Session, cfg: &Config, start: u64) -> Vec<(Action, u64)> {
    let mut seen = Vec::new();
    let mut now = start + 60;
    for _ in 0..40 {
        let d = s.check(cfg, Gate::Off, now);
        match d.action {
            Action::None => {}
            a => {
                seen.push((a, (now - start) / MIN));
                s.turn(now);
            }
        }
        match d.next_at {
            Some(t) if t > now => now = t,
            Some(_) => now += 60,
            None => break,
        }
    }
    seen
}

#[test]
fn the_keepalive_is_due_the_margin_before_the_hour_and_not_before() {
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&cfg(), Gate::Off, MONDAY + 49 * MIN);
    assert_eq!(d.action, Action::None);
    assert_eq!(d.next_at, Some(MONDAY + 50 * MIN));
    assert!(d.policy);
    let d = s.check(&cfg(), Gate::Off, MONDAY + 50 * MIN);
    assert_eq!(d.action, Action::Keepalive);
    assert_eq!(d.count, 1);
    assert_eq!(
        d.text.as_deref(),
        Some("keepwarm: 50m idle, the prompt cache lapses in 10m on 201k tokens. Reply with one word and no tools.")
    );
}

#[test]
fn the_margin_moves_the_due_point() {
    let mut c = cfg();
    c.margin_secs = 5 * MIN;
    let mut s = Session::new(MONDAY, 200_000);
    assert_eq!(s.check(&c, Gate::Off, MONDAY).next_at, Some(MONDAY + 55 * MIN));
    assert_eq!(s.check(&c, Gate::Off, MONDAY + 54 * MIN).action, Action::None);
    assert_eq!(s.check(&c, Gate::Off, MONDAY + 55 * MIN).action, Action::Keepalive);
}

#[test]
fn left_alone_eight_keepalives_then_one_wrap_at_seven_and_a_half_hours_then_nothing() {
    let mut s = Session::new(MONDAY, 200_000);
    let seen = left_alone(&mut s, &cfg(), MONDAY);
    let keepalives: Vec<u64> = seen.iter().filter(|(a, _)| *a == Action::Keepalive).map(|(_, m)| *m).collect();
    assert_eq!(keepalives.len(), 8, "{seen:?}");
    // Each lands 50 minutes after the last turn.
    assert_eq!(keepalives[0], 50);
    assert_eq!(keepalives[7], 400);
    let wraps: Vec<u64> = seen.iter().filter(|(a, _)| *a == Action::Wrap).map(|(_, m)| *m).collect();
    assert_eq!(wraps, vec![450], "{seen:?}");
    assert_eq!(seen.last().unwrap().0, Action::Wrap, "nothing after the wrap");
    // An hour on, the policy still sends nothing.
    let d = s.check(&cfg(), Gate::Off, MONDAY + 520 * MIN);
    assert_eq!((d.action, d.state), (Action::None, "wrapped"));
}

#[test]
fn the_estimate_names_the_wrap_time_from_the_first_due_point() {
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&cfg(), Gate::Off, MONDAY + MIN);
    assert_eq!(d.wraps_at, Some(MONDAY + 450 * MIN));
    assert_eq!(d.wraps_at_hm.as_deref(), Some("07:30"));
}

#[test]
fn the_wrap_turn_names_its_reason_and_asks_for_stay() {
    let mut c = cfg();
    c.max = 1;
    let mut s = Session::new(MONDAY, 200_000);
    let mut p = Policy::default();
    reset(&mut p, MONDAY - 10 * MIN);
    s.state.policy = p;
    assert_eq!(s.check(&c, Gate::Off, MONDAY + 50 * MIN).action, Action::Keepalive);
    s.turn(MONDAY + 50 * MIN + 20);
    let d = s.check(&c, Gate::Off, MONDAY + 100 * MIN + 20);
    assert_eq!(d.action, Action::Wrap);
    let text = d.text.unwrap();
    let (first, body) = text.split_once('\n').unwrap();
    assert_eq!(first, "wrap: 1h50m since the operator's last prompt and 1 keepalives, the most this session allows.");
    assert_eq!(
        body,
        "The operator is away. Run the wrap skill with the argument stay: square the task list and write the continuation prompt. Ask no questions, land nothing and start no new work. Nothing keeps the prompt cache warm after this turn."
    );
    assert_eq!(d.wrap.as_ref().unwrap().dest, "transcript");
}

#[test]
fn a_prompt_resets_the_count_and_a_delivery_does_not() {
    let mut s = Session::new(MONDAY, 200_000);
    assert_eq!(s.check(&cfg(), Gate::Off, MONDAY + 50 * MIN).count, 1);
    s.turn(MONDAY + 50 * MIN + 20);
    // A delivery: a main-loop request; the clock moves, the count stays.
    s.turn(MONDAY + 70 * MIN);
    let d = s.check(&cfg(), Gate::Off, MONDAY + 71 * MIN);
    assert_eq!((d.count, d.next_at), (1, Some(MONDAY + 120 * MIN)));
    // The operator's prompt.
    reset(&mut s.state.policy, MONDAY + 80 * MIN);
    s.turn(MONDAY + 81 * MIN);
    let d = s.check(&cfg(), Gate::Off, MONDAY + 82 * MIN);
    assert_eq!(d.count, 0);
    assert_eq!(s.state.policy.operator_at, Some(MONDAY + 80 * MIN));
}

#[test]
fn a_prompt_clears_a_wrap_and_a_stop() {
    let mut c = cfg();
    c.max = 0;
    let mut s = Session::new(MONDAY, 200_000);
    assert_eq!(s.check(&c, Gate::Off, MONDAY + 50 * MIN).action, Action::Wrap);
    assert_eq!(s.check(&c, Gate::Off, MONDAY + 51 * MIN).state, "wrapped");
    reset(&mut s.state.policy, MONDAY + 52 * MIN);
    s.turn(MONDAY + 53 * MIN);
    let d = s.check(&c, Gate::Off, MONDAY + 54 * MIN);
    assert_eq!((d.state, d.next_at), ("running", Some(MONDAY + 103 * MIN)));
    stop_by_operator(&mut s.state.policy);
    assert_eq!(s.check(&c, Gate::Off, MONDAY + 104 * MIN).state, "stopped");
    reset(&mut s.state.policy, MONDAY + 105 * MIN);
    assert!(s.state.policy.stopped.is_none());
}

#[test]
fn outside_the_windows_the_first_due_point_wraps_with_no_keepalive() {
    // Idle at 18:45 on a Monday, keepalive hours Mon-Fri 08:00-19:00: the
    // due point at 19:35 is outside, so the wrap goes there.
    let c = windows(&["Mon-Fri 08:00-19:00"]);
    let start = MONDAY + (18 * 60 + 45) * MIN;
    let mut s = Session::new(start, 200_000);
    let seen = left_alone(&mut s, &c, start);
    assert_eq!(seen, vec![(Action::Wrap, 50)]);
    let mut s = Session::new(start, 200_000);
    let d = s.check(&c, Gate::Off, start + 50 * MIN);
    let first = d.text.unwrap().lines().next().unwrap().to_string();
    assert!(first.ends_with("and the keepalive hours (Mon-Fri 08:00-19:00) have ended."), "{first}");
}

#[test]
fn inside_the_windows_keepalives_run_until_they_end() {
    let c = windows(&["Mon-Fri 08:00-19:00"]);
    let start = MONDAY + 16 * 60 * MIN;
    let mut s = Session::new(start, 200_000);
    let seen = left_alone(&mut s, &c, start);
    // 16:50, 17:40, 18:30 inside; 19:20 is out.
    assert_eq!(seen, vec![(Action::Keepalive, 50), (Action::Keepalive, 100), (Action::Keepalive, 150), (Action::Wrap, 200)]);
}

#[test]
fn an_empty_list_wraps_at_the_first_due_point() {
    let c = windows(&[]);
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&c, Gate::Off, MONDAY + 50 * MIN);
    assert_eq!(d.action, Action::Wrap);
    assert!(d.text.unwrap().lines().next().unwrap().ends_with("and no keepalive hours are set."));
}

#[test]
fn with_autocompact_on_or_unknown_the_policy_does_not_run() {
    for gate in [Gate::On, Gate::Unknown] {
        let mut s = Session::new(MONDAY, 200_000);
        for m in [50, 55, 120] {
            let d = s.check(&cfg(), gate, MONDAY + m * MIN);
            assert_eq!((d.action, d.policy, d.state), (Action::None, false, "off"), "{gate:?} at {m}");
        }
        assert_eq!(s.state.policy.count, 0);
        assert_eq!(s.state.policy.gate.as_deref(), Some(gate.as_str()));
    }
}

#[test]
fn with_autocompact_on_an_armed_window_still_wakes_at_fifty_minutes() {
    let mut s = Session::new(MONDAY, 200_000);
    let arm = ArmFile { deadline: MONDAY + 2 * 3600, armed_at: MONDAY, window_secs: 7200, auto: false };
    let d = s.check_armed(&cfg(), Gate::On, Some(&arm), MONDAY + 49 * MIN);
    assert_eq!(d.action, Action::None);
    let d = s.check_armed(&cfg(), Gate::On, Some(&arm), MONDAY + 50 * MIN);
    assert_eq!((d.action, d.policy), (Action::Keepalive, false));
    assert!(d.text.unwrap().starts_with("keepwarm: 50m idle"));
}

#[test]
fn keepalive_disabled_or_keepwarm_off_turns_the_policy_off() {
    let mut c = cfg();
    c.enabled = false;
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&c, Gate::Off, MONDAY + 50 * MIN);
    assert_eq!((d.action, d.policy), (Action::None, false));
    assert_eq!(d.reason, "attend_mod.keepalive.enabled is false");
    let mut c = cfg();
    c.keepwarm_on = false;
    let arm = ArmFile { deadline: MONDAY + 2 * 3600, armed_at: MONDAY, window_secs: 7200, auto: false };
    let d = s.check_armed(&c, Gate::Off, Some(&arm), MONDAY + 51 * MIN);
    assert_eq!(d.action, Action::None, "keepwarm off wakes nothing, armed or not");
}

#[test]
fn a_lapsed_cache_at_the_due_point_sends_nothing_and_stops() {
    let mut s = Session::new(MONDAY, 200_000);
    s.check(&cfg(), Gate::Off, MONDAY + MIN);
    // The host slept through the due point.
    let d = s.check(&cfg(), Gate::Off, MONDAY + 59 * MIN);
    assert_eq!((d.action, d.state), (Action::None, "stopped"));
    assert!(d.reason.contains("the cache had lapsed"), "{}", d.reason);
    // A request that is not the operator's restarts the clock, not the policy.
    s.turn(MONDAY + 120 * MIN);
    assert_eq!(s.check(&cfg(), Gate::Off, MONDAY + 170 * MIN).action, Action::None);
}

#[test]
fn the_five_minute_tier_and_a_small_context_get_nothing() {
    let mut s = Session::new(MONDAY, 30_000);
    let d = s.check(&cfg(), Gate::Off, MONDAY + 50 * MIN);
    assert_eq!((d.action, d.next_at), (Action::None, None));
    assert!(d.reason.contains("under 50k"), "{}", d.reason);

    let mut s = Session::new(MONDAY, 200_000);
    s.tail.last_mut().unwrap().tier = "5m".into();
    for at in [MONDAY + TTL_5M_SECS - 60, MONDAY + 50 * MIN] {
        let d = s.check(&cfg(), Gate::Off, at);
        assert_eq!(d.action, Action::None);
        assert!(d.reason.contains("five-minute"), "{}", d.reason);
    }
}

#[test]
fn wrap_off_stops_at_the_bound() {
    let mut c = cfg();
    c.max = 1;
    c.wrap = WrapDest::Off;
    let mut s = Session::new(MONDAY, 200_000);
    assert_eq!(s.check(&c, Gate::Off, MONDAY + MIN).wraps_at, None);
    assert_eq!(s.check(&c, Gate::Off, MONDAY + 50 * MIN).action, Action::Keepalive);
    s.turn(MONDAY + 50 * MIN + 20);
    let d = s.check(&c, Gate::Off, MONDAY + 100 * MIN + 20);
    assert_eq!((d.action, d.state), (Action::None, "stopped"));
    assert!(d.reason.contains("wrap is off"));
}

#[test]
fn a_keepalive_with_no_turn_stops_the_policy() {
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&cfg(), Gate::Off, MONDAY + 50 * MIN);
    assert_eq!(d.action, Action::Keepalive);
    assert_eq!(d.next_at, Some(MONDAY + 60 * MIN + 1));
    assert_eq!(s.check(&cfg(), Gate::Off, MONDAY + 55 * MIN).reason, "waiting for the keepalive's turn");
    let d = s.check(&cfg(), Gate::Off, MONDAY + 60 * MIN + 1);
    assert_eq!(d.state, "stopped");
    assert!(d.reason.contains("no turn within ten minutes"), "{}", d.reason);
}

#[test]
fn a_keepalive_that_reads_a_cold_cache_stops_the_policy() {
    let mut s = Session::new(MONDAY, 200_000);
    s.check(&cfg(), Gate::Off, MONDAY + 50 * MIN);
    s.tail.push(entry(MONDAY + 50 * MIN + 20, 0, 200_600));
    let d = s.check(&cfg(), Gate::Off, MONDAY + 51 * MIN);
    assert_eq!(d.state, "stopped");
    assert!(d.reason.contains("the cache was already gone"), "{}", d.reason);
}

#[test]
fn a_warm_keepalive_adds_its_cost() {
    let mut s = Session::new(MONDAY, 200_000);
    s.check(&cfg(), Gate::Off, MONDAY + 50 * MIN);
    s.turn(MONDAY + 50 * MIN + 20);
    s.check(&cfg(), Gate::Off, MONDAY + 51 * MIN);
    assert!(!s.state.policy.pending);
    // 200k read at $0.50 per million, plus the small write, input and output.
    assert!((0.10..0.11).contains(&s.state.policy.spent_usd), "{}", s.state.policy.spent_usd);
}

#[test]
fn an_armed_window_overrides_the_count_and_sends_no_wrap() {
    let mut c = cfg();
    c.max = 0;
    let arm = ArmFile { deadline: MONDAY + 3 * 3600, armed_at: MONDAY, window_secs: 3 * 3600, auto: false };
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check_armed(&c, Gate::Off, Some(&arm), MONDAY + 50 * MIN);
    assert_eq!((d.action, d.policy), (Action::Keepalive, true));
    assert_eq!(s.state.policy.count, 0, "an armed wake is not counted");
    s.turn(MONDAY + 50 * MIN + 20);
    let d = s.check_armed(&c, Gate::Off, Some(&arm), MONDAY + 51 * MIN);
    assert_eq!(d.action, Action::None);
    // The window ends: the next due point is judged by the count.
    s.turn(MONDAY + 3 * 3600 - 10 * MIN);
    let d = s.check_armed(&c, Gate::Off, Some(&arm), MONDAY + 3 * 3600 + 40 * MIN);
    assert_eq!(d.action, Action::Wrap);
}

#[test]
fn a_paid_cold_write_arms_nothing_where_the_policy_runs() {
    let mut s = Session::new(MONDAY, 200_000);
    s.check(&cfg(), Gate::Off, MONDAY + MIN);
    s.tail.push(entry(MONDAY + 2 * 3600, 0, 200_700));
    s.check(&cfg(), Gate::Off, MONDAY + 2 * 3600 + 30);
    assert_eq!(s.state.misses.len(), 1, "the write is scored");
    assert_eq!(s.state.deadline, 0, "and arms nothing");
    // Where the policy does not run, it arms three hours, as ADR-182 has it.
    let mut s = Session::new(MONDAY, 200_000);
    s.check(&cfg(), Gate::On, MONDAY + MIN);
    s.tail.push(entry(MONDAY + 2 * 3600, 0, 200_700));
    s.check(&cfg(), Gate::On, MONDAY + 2 * 3600 + 30);
    assert_eq!(s.state.deadline, MONDAY + 2 * 3600 + crate::AUTO_WARM_SECS);
}

#[test]
fn the_wrap_goes_to_a_file_with_its_path_expanded() {
    let mut c = cfg();
    c.max = 0;
    c.wrap = WrapDest::File;
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&c, Gate::Off, MONDAY + 50 * MIN);
    let r = d.wrap.unwrap();
    assert_eq!(r.file, "/state/agent-ways/wrap/sid-1.md");
    assert!(d.text.unwrap().ends_with(" The mod saves your final reply to /state/agent-ways/wrap/sid-1.md."));
    let w = Windows::default().with_zone(TimeZone::UTC);
    assert_eq!(site().wrap_file("~/notes/{project}-{date}.md", &w, MONDAY), "/home/aaron/notes/agent-ways-2026-10-12.md");
    assert_eq!(site().wrap_file("handoffs/{session}.md", &w, MONDAY), "/work/agent-ways/handoffs/sid-1.md");
}

#[test]
fn the_wrap_sent_over_attend_goes_to_the_operator_by_default_with_a_handoff_line() {
    let mut c = cfg();
    c.max = 0;
    c.wrap = WrapDest::Send;
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&c, Gate::Off, MONDAY + 50 * MIN);
    let r = d.wrap.unwrap();
    assert_eq!(r.to, "aaron");
    assert_eq!(r.handoff, "handoff: continuation prompt from Chaucer-alpha (agent-ways), wrapped at 00:50 while its operator was away.");
    assert!(d.text.unwrap().ends_with(" The mod sends your final reply to your operator, aaron, over attend."));
    c.wrap_to = "#deploy".into();
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&c, Gate::Off, MONDAY + 50 * MIN);
    assert_eq!(d.wrap.unwrap().to, "#deploy");
    assert!(d.text.unwrap().ends_with(" The mod posts your final reply in #deploy over attend, where another session may take up the work."));
    c.wrap_to = "Chaucer-beta".into();
    let mut s = Session::new(MONDAY, 200_000);
    let d = s.check(&c, Gate::Off, MONDAY + 50 * MIN);
    assert!(d.text.unwrap().ends_with(" The mod sends your final reply to Chaucer-beta over attend as a handoff."));
}

#[test]
fn the_settings_are_read_and_a_wrong_type_keeps_its_default() {
    let doc = r##"{"view":"effective","keys":{
        "attend_mod.keepwarm":{"value":"on"},
        "attend_mod.keepalive.enabled":{"value":false},
        "attend_mod.keepalive.max_keepalives":{"value":3},
        "attend_mod.keepalive.margin_minutes":{"value":"ten"},
        "attend_mod.keepalive.windows":{"value":["Mon-Fri 08:00-19:00","bad"]},
        "attend_mod.keepalive.timezone":{"value":"UTC"},
        "attend_mod.keepalive.wrap":{"value":"send"},
        "attend_mod.keepalive.wrap_to":{"value":" #deploy "},
        "attend_mod.keepalive.after_wrap":{"value":"compact"}}}"##;
    let c = Config::from_settings(Some(doc));
    assert!(!c.enabled);
    assert_eq!(c.max, 3);
    assert_eq!(c.margin_secs, 10 * MIN, "a wrong type keeps the default");
    assert_eq!(c.windows.describe(), "Mon-Fri 08:00-19:00");
    assert_eq!(c.windows.dropped.len(), 1);
    assert_eq!((c.wrap, c.wrap_to.as_str(), c.after_wrap), (WrapDest::Send, "#deploy", AfterWrap::Compact));
    assert_eq!(c.wrap_file, DEFAULT_WRAP_FILE);
    let d = Config::from_settings(Some(r#"{"keys":{"attend_mod.keepalive.enabled":{"value":true}}}"#));
    assert!(d.enabled && d.keepwarm_on && d.settings_ok);
    assert_eq!((d.max, d.margin_secs, d.wrap), (8, 600, WrapDest::Transcript));
    assert_eq!(d.windows.describe(), "daily 00:00-24:00");
    let out_of_range = Config::from_settings(Some(r#"{"keys":{"attend_mod.keepalive.max_keepalives":{"value":99}}}"#));
    assert_eq!(out_of_range.max, 8);
    let off = Config::from_settings(Some(r#"{"keys":{"attend_mod.keepwarm":{"value":"off"}}}"#));
    assert!(!off.keepwarm_on);
}

#[test]
fn the_card_shows_the_count_the_next_due_point_the_wrap_and_the_cost() {
    let mut s = Session::new(MONDAY, 490_000);
    reset(&mut s.state.policy, MONDAY - 5 * MIN);
    s.check(&cfg(), Gate::Off, MONDAY + 50 * MIN);
    s.turn(MONDAY + 50 * MIN + 20);
    s.check(&cfg(), Gate::Off, MONDAY + 60 * MIN);
    let lines = card(&s.state, &s.reading(), &cfg(), &site(), MONDAY + 60 * MIN);
    assert_eq!(
        lines[0],
        "keepalive   1 of 8 since your prompt at 23:55 · next in 40m · wraps about 07:30 · windows daily 00:00-24:00"
    );
    assert!(lines[1].starts_with("cost        1 wake $0.25 so far · at most $1.72 more in wakes before the wrap"), "{}", lines[1]);
    assert!(lines[1].ends_with("· a cold re-write now $4.90"), "{}", lines[1]);
}

#[test]
fn the_card_says_why_the_policy_is_off_or_stopped() {
    let mut s = Session::new(MONDAY, 200_000);
    assert!(card(&s.state, &s.reading(), &cfg(), &site(), MONDAY)[0].contains("not run in this session (the attend mod runs it"));
    s.check(&cfg(), Gate::On, MONDAY + MIN);
    assert_eq!(card(&s.state, &s.reading(), &cfg(), &site(), MONDAY)[0], "keepalive   off: autocompact is on (armed windows only)");
    s.check(&cfg(), Gate::Off, MONDAY + 59 * MIN);
    assert!(card(&s.state, &s.reading(), &cfg(), &site(), MONDAY + 59 * MIN)[0].starts_with("keepalive   stopped: the cache had lapsed"));
    let mut c = windows(&["Mon 9am-5pm"]);
    c.windows.bad_zone = Some("Mars/Base".into());
    let mut s = Session::new(MONDAY, 200_000);
    s.check(&c, Gate::Off, MONDAY + MIN);
    let lines = card(&s.state, &s.reading(), &c, &site(), MONDAY + MIN);
    assert!(lines.iter().any(|l| l.starts_with("keepalive   window entry \"Mon 9am-5pm\" dropped")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("timezone \"Mars/Base\" is unknown")), "{lines:#?}");
}

#[test]
fn the_card_names_where_the_wrap_went() {
    let mut c = cfg();
    c.max = 0;
    let mut s = Session::new(MONDAY, 200_000);
    s.check(&c, Gate::Off, MONDAY + 50 * MIN);
    s.state.policy.wrap_outcome = Some(WrapOutcome::Refused.describe("Chaucer-beta: no live session answers to that name"));
    let line = &card(&s.state, &s.reading(), &c, &site(), MONDAY + 60 * MIN)[0];
    assert_eq!(line, "keepalive   wrapped at 00:50 after 0 keepalives since 00:00 · handoff refused: Chaucer-beta: no live session answers to that name");
}

#[test]
fn the_policy_survives_a_ledger_without_it() {
    let old = r#"{"misses":[],"last_ping":null,"stopped":null,"pinged_at":null,"updated_at":5}"#;
    let l: Ledger = serde_json::from_str(old).unwrap();
    assert!(l.policy.is_empty());
    assert!(!serde_json::to_string(&l).unwrap().contains("policy"), "an empty policy is not written");
}

/// ADR-414 item 8 as amended: `enabled` fails closed. Settings that cannot
/// be read, or a document with no `enabled` (a ways that does not know the
/// key), run no policy.
#[test]
fn unreadable_settings_or_no_enabled_key_turn_the_policy_off() {
    for (doc, why) in [
        (None, "the attend_mod settings could not be read"),
        (Some("not json"), "the attend_mod settings could not be read"),
        (Some(r#"{"keys":{}}"#), "attend_mod.keepalive.enabled is false"),
        (Some(r#"{"keys":{"attend_mod.keepalive.enabled":{"value":"yes"}}}"#), "attend_mod.keepalive.enabled is false"),
    ] {
        let c = Config::from_settings(doc);
        assert_eq!(c.why_off(Gate::Off), Some(why), "{doc:?}");
        let mut s = Session::new(MONDAY, 200_000);
        let d = s.check(&c, Gate::Off, MONDAY + 50 * MIN);
        assert_eq!((d.action, d.policy), (Action::None, false), "{doc:?}");
        let lines = card(&s.state, &s.reading(), &c, &site(), MONDAY + 50 * MIN);
        assert_eq!(lines[0], format!("keepalive   off: {why} (armed windows only)"));
    }
}

/// The sensor's own arm after a paid cold write is no operator's word: the
/// policy reads it as no window, so the count still bounds the stretch.
#[test]
fn an_auto_arm_is_ignored_by_the_policy_and_an_operator_arm_is_not() {
    let mut c = cfg();
    c.max = 0;
    for (auto, want) in [(true, Action::Wrap), (false, Action::Keepalive)] {
        let mut s = Session::new(MONDAY, 200_000);
        let arm = ArmFile { deadline: MONDAY + 3 * 3600, armed_at: MONDAY, window_secs: 3 * 3600, auto };
        // The cursor knows the window from the plain check that armed it.
        s.state.deadline = arm.deadline;
        let d = s.check_armed(&c, Gate::Off, Some(&arm), MONDAY + 50 * MIN);
        assert_eq!(d.action, want, "auto {auto}");
    }
}

/// wrap_file is confined to {state}/wrap/, names {session}, and passes
/// through no symlink; anything else falls back to the default, with why.
#[test]
fn wrap_file_is_confined_to_the_wrap_directory() {
    let w = Windows::default().with_zone(TimeZone::UTC);
    let default = "/state/agent-ways/wrap/sid-1.md".to_string();
    assert_eq!(site().confined_wrap_file("{state}/wrap/{date}-{session}.md", &w, MONDAY), ("/state/agent-ways/wrap/2026-10-12-sid-1.md".into(), None));
    assert_eq!(site().confined_wrap_file("{state}/wrap/{project}/{session}.md", &w, MONDAY).1, None);
    for (template, why) in [
        ("~/.bashrc", "names no {session}"),
        ("{state}/wrap/handoff.md", "names no {session}"),
        ("~/.ssh/{session}", "outside {state}/wrap/"),
        ("{state}/wrap/../../../etc/{session}", "outside {state}/wrap/"),
        ("/../../{session}", "climbs above the root"),
        ("{state}/wrap{session}", "outside {state}/wrap/"),
    ] {
        let (file, note) = site().confined_wrap_file(template, &w, MONDAY);
        assert_eq!(file, default, "{template}");
        let note = note.unwrap_or_default();
        assert!(note.contains(why) && note.ends_with("the reply goes to /state/agent-ways/wrap/sid-1.md"), "{template}: {note}");
    }
    // A symlink under the wrap directory is not followed out of it.
    let root = std::env::temp_dir().join(format!("keepwarm-wrap-test-{}", std::process::id()));
    std::fs::create_dir_all(root.join("agent-ways/wrap")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc", root.join("agent-ways/wrap/out")).unwrap();
    let s = Site { state_root: root.join("agent-ways"), ..site() };
    let (file, note) = s.confined_wrap_file("{state}/wrap/out/{session}", &w, MONDAY);
    std::fs::remove_dir_all(&root).ok();
    #[cfg(unix)]
    {
        assert_eq!(file, root.join("agent-ways/wrap/sid-1.md").to_string_lossy());
        assert!(note.as_deref().unwrap_or_default().contains("passes through a symlink"));
    }
    let _ = (file, note);
}

/// wrap_to #open would broadcast to every listening session: refused, and
/// the reply goes to the operator.
#[test]
fn wrap_to_open_is_refused() {
    let mut c = cfg();
    c.max = 0;
    c.wrap = WrapDest::Send;
    for to in ["#open", "#OPEN", "# open "] {
        c.wrap_to = to.into();
        let mut s = Session::new(MONDAY, 200_000);
        let r = s.check(&c, Gate::Off, MONDAY + 50 * MIN).wrap.unwrap();
        assert_eq!(r.to, "aaron", "{to}");
        assert!(r.notes.iter().any(|n| n.contains("#open reaches every listening session")), "{to}: {:?}", r.notes);
    }
    c.wrap_to = "#deploy".into();
    let mut s = Session::new(MONDAY, 200_000);
    assert!(s.check(&c, Gate::Off, MONDAY + 50 * MIN).wrap.unwrap().notes.is_empty());
}
