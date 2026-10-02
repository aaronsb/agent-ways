//! The introspect screens through agent-tui's test kit (ADR-504 §12): a
//! fixed session drawn headless, keys through the real handler, and golden
//! frames at 80x25 and the reference size, 120x40.
//!
//! Golden frames live in `tests/fixtures/introspect-tui/`. `AGENT_TUI_BLESS=1`
//! records them for review, and that run fails by design; a run without it
//! checks them.

use std::collections::HashMap;
use std::path::Path;

use agent_theme::{ColorDepth, ThemeSet};
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::screen::Screen;
use agent_tui::testkit::render_screen as render;
use agent_tui::testkit::{frame, text, Goldens};
use agent_tui::theme::{Palette, Shape};
use agent_tui::timeline::Playback;
use ways_core::introspection::{CriteriaMap, FiredWay, IntrospectionSummary, JoinConfidence, MatchCriteria, MatchDetail, SessionIntrospection, Turn, WayMeta};

use super::frames::{build_frames, has_verdicts};
use super::model::WayEvent;
use super::report::{Reports, Spend};
use super::picker::Picker;
use super::screen::{reselect_by_anchor, session_spend, Introspect, Replay};
use super::sessions::{gather_sessions, SessionInfo};
use super::why::build_why_index;

const SESSION: &str = "8f3a2c1d-5e6f-4a7b-9c8d-0e1f2a3b4c5d";
const PROJECT: &str = "/home/dev/proj";

/// The sizes every screen is checked at: the smallest a screen must work
/// in, and the reference.
const SIZES: [(u16, u16); 2] = [(80, 25), (120, 40)];

fn ev(ts: &str, event: &str, way: &str, trigger: &str) -> WayEvent {
    WayEvent {
        ts: ts.into(),
        event: event.into(),
        way: if event == "check_fired" { String::new() } else { way.into() },
        trigger: trigger.into(),
        check: if event == "check_fired" { way.into() } else { String::new() },
        p_yes: if event == "way_judged" { "0.050".into() } else { String::new() },
        verdict: if event == "way_judged" { "block".into() } else { String::new() },
        ancestor: String::new(),
    }
}

/// A session of two compaction windows: three ways fire, a check fires, a
/// way re-discloses and the gate keeps one out; after the compaction one
/// way re-discloses.
fn replay(live: bool) -> Replay {
    replay_of(session_events(), live)
}

fn session_events() -> Vec<WayEvent> {
    vec![
        ev("2026-07-03T16:52:00Z", "session_start", "", ""),
        ev("2026-07-03T16:52:01Z", "way_fired", "softwaredev/code/testing", "semantic:embedding:en"),
        ev("2026-07-03T16:52:02Z", "way_fired", "softwaredev/delivery/commits", "keyword"),
        ev("2026-07-03T16:53:00Z", "way_fired", "softwaredev/docs/adr", "file"),
        ev("2026-07-03T16:53:01Z", "check_fired", "softwaredev/delivery/commits", ""),
        ev("2026-07-03T16:55:00Z", "way_redisclosed", "softwaredev/code/testing", "semantic:embedding:en"),
        ev("2026-07-03T16:55:01Z", "way_judged", "itops/incident", ""),
        ev("2026-07-03T17:52:00Z", "session_start", "", ""),
        ev("2026-07-03T17:52:01Z", "way_redisclosed", "softwaredev/delivery/commits", "keyword"),
    ]
}

fn replay_of(events: Vec<WayEvent>, live: bool) -> Replay {
    let tokens: Vec<(String, u64)> = [("2026-07-03T16:52:00Z", 18), ("2026-07-03T16:53:00Z", 64), ("2026-07-03T16:55:00Z", 132), ("2026-07-03T17:52:00Z", 31)]
        .iter()
        .map(|(t, k)| (t.to_string(), *k))
        .collect();
    let refire: HashMap<String, u64> =
        [("softwaredev/code/testing", 40), ("softwaredev/delivery/commits", 30), ("softwaredev/docs/adr", 80)].iter().map(|(w, k)| (w.to_string(), *k)).collect();
    let frames = build_frames(&events, &tokens, &refire, 50);
    let play = if live { Playback::live(frames.len()) } else { Playback::replay(frames.len()) };
    let mut r = Replay::new(SESSION.into(), PROJECT.into(), 200, frames, play);
    r.judged = has_verdicts(&events);
    r.now = agent_fmt::when::parse_utc_iso("2026-07-03T17:52:06Z").unwrap();
    r.why = Some(build_why_index(&model()));
    r.bodies.insert("/ways/softwaredev/code/testing/testing.md".into(), Some(BODY.into()));
    r
}

const BODY: &str = "# Testing\n\nWrite the test **first**, watch it fail, then make it pass. Run `make test` before a commit.\n\n- one behaviour per test\n- name it for what it shows\n\n| Kind | Where |\n|---|---|\n| unit | beside the code |\n| golden | tests/fixtures |\n";

fn fired(way: &str, channel: &str, path: Option<&str>, span: Option<&str>, score: Option<f64>) -> FiredWay {
    FiredWay {
        way_id: way.into(),
        trigger_channel: channel.into(),
        gated: false,
        suppressed: None,
        redisclosed: false,
        fire_score: score,
        way_path: path.map(str::to_string),
        criteria: MatchCriteria { vocabulary: Some("test tdd unit golden fixture assert".into()), ..Default::default() },
        match_detail: span.map(|s| MatchDetail { matched_span: Some(s.into()), confidence: JoinConfidence::Keyed }),
        judge: None,
    }
}

fn model() -> SessionIntrospection {
    let ways = vec![
        fired("softwaredev/code/testing", "semantic:embedding:en", Some("/ways/softwaredev/code/testing/testing.md"), None, Some(0.71)),
        fired("softwaredev/delivery/commits", "keyword", None, Some("commit"), None),
    ];
    SessionIntrospection {
        id: SESSION.into(),
        project: PROJECT.into(),
        window_k: 200,
        summary: IntrospectionSummary::default(),
        turns: vec![Turn { epoch: 1, token_position: 0, ts: "2026-07-03T16:52:01Z".into(), transcript_uuid: None, join_confidence: JoinConfidence::Keyed, fired_ways: ways }],
    }
}

fn sessions() -> Vec<SessionInfo> {
    let content = [
        (SESSION, "2026-07-03T16:52:00Z", "/home/dev/proj", 9),
        ("1b2c3d4e-0000-4000-8000-000000000001", "2026-07-01T09:10:00Z", "/home/dev/proj", 3),
        ("9e8d7c6b-0000-4000-8000-000000000002", "2026-06-28T22:05:00Z", "/home/dev/other_proj", 1),
    ]
    .iter()
    .rev()
    .flat_map(|(id, ts, p, fires)| {
        let mut l = vec![format!("{{\"event\":\"session_start\",\"session\":\"{id}\",\"ts\":\"{ts}\",\"project\":\"{p}\"}}")];
        for i in 0..*fires {
            l.push(format!("{{\"event\":\"way_fired\",\"session\":\"{id}\",\"ts\":\"{}\",\"way\":\"w/{i}\"}}", ts.replace(":00Z", &format!(":{:02}Z", 10 + i))));
        }
        l
    })
    .collect::<Vec<_>>()
    .join("\n");
    let mut s = gather_sessions(&content, None);
    for x in &mut s {
        x.transcript = x.id != "9e8d7c6b-0000-4000-8000-000000000002";
    }
    s
}

fn terminal() -> Palette {
    Palette::terminal(ColorDepth::TrueColor)
}

fn nord() -> Palette {
    Palette::new(ThemeSet::bundled().get("nord"), ColorDepth::TrueColor)
}

fn picker(palette: Palette) -> Introspect {
    let open = Box::new(|id: &str| if id == SESSION { Ok(replay(false)) } else { Err(format!("no events for session {}", &id[..12])) });
    Introspect::picking(Picker::new(sessions(), PROJECT.into()), open, no_spend(), palette, Shape::PLAIN)
}

/// Report tabs with no events.
fn no_spend() -> Reports {
    with_spend(Spend::new(Vec::new(), None, PROJECT.into()))
}

/// Reports with `spend` and empty usage and precision.
fn with_spend(spend: Spend) -> Reports {
    Reports::new("", None, PROJECT.into()).with_spend(spend)
}

/// The screen opened on one session, as `--session` and `live` open it.
fn showing(r: Replay, palette: Palette, shape: Shape) -> Introspect {
    Introspect::showing(r, no_spend(), palette, shape)
}

fn press(s: &mut Introspect, keys: &[KeyCode]) -> bool {
    keys.iter().all(|k| s.key(KeyEvent::new(*k, KeyModifiers::NONE)))
}

fn goldens() -> Goldens {
    Goldens::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/introspect-tui"))
}

/// Check `s` at both sizes under `name`.
fn check(g: &mut Goldens, name: &str, s: &mut Introspect) {
    for (w, h) in SIZES {
        g.check(&format!("{name}-{w}x{h}"), &render(s, w, h));
    }
}

#[test]
fn golden_frames() {
    let mut g = goldens();

    // The picker: newest first, the second session selected, one whose
    // transcript is gone.
    let mut p = picker(terminal());
    press(&mut p, &[KeyCode::Down]);
    check(&mut g, "picker", &mut p);

    // A replay three frames in, the second way selected.
    let mut r = showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut r, &[KeyCode::Right, KeyCode::Right, KeyCode::Down]);
    check(&mut g, "replay", &mut r);

    // The same frame in a theme, playing.
    let mut t = showing(replay(false), nord(), Shape::ROUND);
    press(&mut t, &[KeyCode::Right, KeyCode::Right, KeyCode::Char(' ')]);
    check(&mut g, "replay-nord", &mut t);

    // Why the first way fired: its trigger, criteria and body as markdown.
    let mut w = showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut w, &[KeyCode::Right, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    check(&mut g, "why", &mut w);

    // Live, following the newest frame, which fired five seconds ago.
    let mut l = showing(replay(true), terminal(), Shape::PLAIN);
    check(&mut g, "live", &mut l);
    // Looking back stops the follow.
    press(&mut l, &[KeyCode::Left]);
    check(&mut g, "live-paused", &mut l);

    // No colour: reverse video marks the selection and the lozenges.
    let mut n = picker(Palette::terminal(ColorDepth::NoColor));
    check(&mut g, "picker-nocolor", &mut n);
    g.finish();
}

#[test]
fn enter_opens_a_session_esc_comes_back_and_q_quits() {
    let mut s = picker(terminal());
    assert!(press(&mut s, &[KeyCode::Enter]));
    assert!(text(&render(&mut s, 120, 40)).contains(&format!("Session {SESSION}")), "the replay of the chosen session");
    assert!(press(&mut s, &[KeyCode::Esc]));
    assert!(text(&render(&mut s, 120, 40)).contains("3 sessions in /home/dev/proj"), "back on the picker");
    // A session that cannot open says why and stays on the picker.
    assert!(press(&mut s, &[KeyCode::Down, KeyCode::Enter]));
    assert!(text(&render(&mut s, 120, 40)).contains("no events for session 1b2c3d4e-000"));
    assert!(!press(&mut s, &[KeyCode::Char('q')]), "q ends the session");
    // Ctrl-C ends it from anywhere.
    let mut r = showing(replay(false), terminal(), Shape::PLAIN);
    assert!(!r.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    // A replay opened directly ends on Esc: there is no picker to go back to.
    let mut d = showing(replay(false), terminal(), Shape::PLAIN);
    assert!(!press(&mut d, &[KeyCode::Esc]));
}

/// A scope too long for the pane's title loses its end, not the count.
#[test]
fn a_long_scope_keeps_the_session_count_in_view() {
    let open = Box::new(|_: &str| Err("none".to_string()));
    let scope = format!("/var/folders/{}/proj", "x".repeat(120));
    let mut s = Introspect::picking(Picker::new(sessions(), scope), open, no_spend(), terminal(), Shape::PLAIN);
    assert!(text(&render(&mut s, 80, 10)).contains("3 sessions in /var/folders/"));
}

#[test]
fn esc_leaves_the_why_view_for_the_timeline_before_it_leaves_the_session() {
    let mut s = picker(terminal());
    press(&mut s, &[KeyCode::Enter, KeyCode::Right, KeyCode::Enter]);
    assert!(text(&render(&mut s, 120, 40)).contains("why it fired"));
    press(&mut s, &[KeyCode::Esc]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains(" ways at epoch ") && t.contains(&format!("Session {SESSION}")), "{t}");
}

#[test]
fn playing_moves_a_frame_per_tick_and_stops_at_the_last() {
    let mut s = showing(replay(false), terminal(), Shape::PLAIN);
    assert_eq!(s.tick_every(), None, "paused, nothing to tick");
    press(&mut s, &[KeyCode::Char(' ')]);
    assert_eq!(s.tick_every(), Some(std::time::Duration::from_millis(1000)));
    press(&mut s, &[KeyCode::Char('+')]);
    assert_eq!(s.tick_every(), Some(std::time::Duration::from_millis(500)));
    let n = s.replay.as_ref().unwrap().frames.len();
    for _ in 0..n + 2 {
        s.tick();
    }
    let r = s.replay.as_ref().unwrap();
    assert_eq!(r.play.pos(), n - 1);
    assert!(!r.play.playing());
}

#[test]
fn the_selection_stays_on_its_way_across_frames() {
    let mut s = showing(replay(false), terminal(), Shape::PLAIN);
    // Frame 2 holds testing and commits (epoch 1, in id order) and adr
    // (epoch 2); select adr, then go back a frame, where adr has not fired:
    // the cursor takes commits, the last way that fired before it.
    press(&mut s, &[KeyCode::Right, KeyCode::Down, KeyCode::Down]);
    let sel = |s: &mut Introspect| {
        let t = text(&render(s, 120, 40));
        t.lines().find(|l| l.contains('▌')).unwrap_or("").to_string()
    };
    assert!(sel(&mut s).contains("softwaredev/docs/adr"), "{}", sel(&mut s));
    press(&mut s, &[KeyCode::Left]);
    assert!(sel(&mut s).contains("softwaredev/delivery/commits"), "{}", sel(&mut s));
    press(&mut s, &[KeyCode::Right]);
    assert!(sel(&mut s).contains("softwaredev/delivery/commits"), "the same way, not row 0: {}", sel(&mut s));
}

#[test]
fn anchor_keeps_the_same_way_or_the_nearest_earlier_one() {
    let r = replay(false);
    let f = &r.frames[2].shown(false);
    let ids: Vec<&str> = f.ways.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["softwaredev/delivery/commits", "softwaredev/docs/adr", "softwaredev/code/testing"]);
    assert_eq!(reselect_by_anchor(f, "softwaredev/docs/adr", 2), 1, "still active: the same way");
    assert_eq!(reselect_by_anchor(f, "gone/way", 2), 1, "gone: the last way fired at or before epoch 2");
    assert_eq!(reselect_by_anchor(f, "gone/way", 0), 0, "nothing earlier: the first row");
}

/// A session id that is not ASCII is shortened by characters, not bytes.
#[test]
fn a_non_ascii_session_id_is_cut_by_characters() {
    match Replay::load("", "aéééééééééééé", None, false) {
        Err(e) => assert_eq!(e, "no events for session aééééééééééé"),
        Ok(_) => panic!("no events, no replay"),
    }
}

/// Live and following, the cursor rides the newest way as frames arrive.
/// Moving it up is reviewing: it stays put through a refresh, and End
/// resumes the follow on the newest way.
#[test]
fn a_live_cursor_follows_the_newest_way_until_the_reader_looks_back() {
    let first = |n: usize| replay(true).frames.into_iter().take(n).collect::<Vec<_>>();
    let sel = |s: &mut Introspect| {
        let t = text(&render(s, 120, 40));
        t.lines().find(|l| l.contains('▌')).unwrap_or("").to_string()
    };
    // Frame 2 lists commits, adr, testing: testing is the newest row.
    let mut live = replay(true);
    live.take_frames(first(2));
    let mut s = showing(live, terminal(), Shape::PLAIN);
    s.replay.as_mut().unwrap().take_frames(first(3));
    assert!(sel(&mut s).contains("softwaredev/code/testing"), "follows the newest: {}", sel(&mut s));
    press(&mut s, &[KeyCode::Up]);
    s.replay.as_mut().unwrap().take_frames(first(3));
    assert!(sel(&mut s).contains("softwaredev/docs/adr"), "looking back holds the cursor: {}", sel(&mut s));
    assert!(text(&render(&mut s, 120, 40)).contains("LIVE paused"));
    press(&mut s, &[KeyCode::End]);
    assert!(text(&render(&mut s, 120, 40)).contains("● LIVE"));
    s.replay.as_mut().unwrap().take_frames(first(3));
    assert!(sel(&mut s).contains("softwaredev/code/testing"), "End resumes the follow: {}", sel(&mut s));
}

/// Live, new events on the log read the frames again; the why-fired
/// reader keeps its place and its index rather than going back to the top.
#[test]
fn a_live_refresh_keeps_the_why_reader_where_it_was() {
    let mut s = showing(replay(true), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Left, KeyCode::Down, KeyCode::Down, KeyCode::Tab]);
    press(&mut s, &[KeyCode::Char('j'); 5]);
    let before = frame(&render(&mut s, 80, 25));
    assert!(before.contains("why it fired 6–"), "{before}");
    s.replay.as_mut().unwrap().take_frames(replay(true).frames);
    let after = text(&render(&mut s, 80, 25));
    assert!(after.contains("why it fired 6–"), "the reader went back to the top: {after}");
    assert!(after.contains("• e3 softwaredev/code/t"), "the why index is still there: {after}");
}

#[test]
fn the_why_reader_scrolls_within_its_document() {
    let mut s = showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Right, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    let top = frame(&render(&mut s, 80, 25));
    assert!(top.contains("1–"), "a long document shows its range: {top}");
    press(&mut s, &[KeyCode::Char('G')]);
    let end = text(&render(&mut s, 80, 25));
    assert!(end.contains("golden"), "the end of the body is reached: {end}");
    press(&mut s, &[KeyCode::Char('g')]);
    assert!(text(&render(&mut s, 80, 25)).contains("Trigger"), "and the top again");
}

/// One frame the relevance judge saw: it passed testing, would have kept
/// commits out in shadow mode, and blocked incident; a frame later adr fires.
fn judged_replay() -> Replay {
    let judged = |way: &str, verdict: &str| WayEvent { verdict: verdict.into(), ..ev("2026-07-03T16:52:01Z", "way_judged", way, "") };
    let events = vec![
        ev("2026-07-03T16:52:00Z", "session_start", "", ""),
        judged("softwaredev/code/testing", "pass"),
        judged("softwaredev/delivery/commits", "would_block"),
        judged("itops/incident", "block"),
        ev("2026-07-03T16:52:01Z", "way_fired", "softwaredev/code/testing", "semantic:embedding:en"),
        ev("2026-07-03T16:52:01Z", "way_fired", "softwaredev/delivery/commits", "keyword"),
        ev("2026-07-03T16:53:00Z", "way_fired", "softwaredev/docs/adr", "file"),
    ];
    let tokens: Vec<(String, u64)> = [("2026-07-03T16:52:00Z", 18), ("2026-07-03T16:53:00Z", 64)].iter().map(|(t, k)| (t.to_string(), *k)).collect();
    let refire: HashMap<String, u64> =
        [("softwaredev/code/testing", 40), ("softwaredev/delivery/commits", 30), ("softwaredev/docs/adr", 80)].iter().map(|(w, k)| (w.to_string(), *k)).collect();
    let frames = build_frames(&events, &tokens, &refire, 50);
    let mut r = Replay::new(SESSION.into(), PROJECT.into(), 200, frames, Playback::replay(2));
    r.judged = has_verdicts(&events);
    // The why index from the same events, as the screens read it.
    let log: Vec<serde_json::Value> = events
        .iter()
        .map(|e| serde_json::json!({"session": SESSION, "ts": e.ts, "event": e.event, "way": e.way, "trigger": e.trigger, "p_yes": e.p_yes, "verdict": e.verdict, "threshold": "0.30"}))
        .collect();
    let criteria: CriteriaMap = [(
        "softwaredev/code/testing".to_string(),
        WayMeta { path: Some("/ways/softwaredev/code/testing/testing.md".into()), criteria: MatchCriteria { vocabulary: Some("test tdd".into()), ..Default::default() } },
    )]
    .into_iter()
    .collect();
    r.why = Some(build_why_index(&SessionIntrospection::build(&log, SESSION, PROJECT, 200, &criteria)));
    r
}

/// The selected row of a 120x40 render.
fn selected(s: &mut Introspect) -> String {
    text(&render(s, 120, 40)).lines().find(|l| l.contains('▌')).unwrap_or("").to_string()
}

/// Injected by default, the shadow would-block marked; `f` widens the
/// table to every matched candidate, the judge-blocked row added, and back.
#[test]
fn f_toggles_the_table_between_injected_and_matched_ways() {
    let mut s = showing(judged_replay(), terminal(), Shape::PLAIN);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("◌ softwaredev/delivery/commits"), "{t}");
    assert!(t.contains("softwaredev/code/testing"));
    assert!(!t.contains("itops/incident"), "a blocked way is not injected: {t}");
    assert!(t.contains("◇ injected · 1 judged out") && t.contains("f matched"), "{t}");

    press(&mut s, &[KeyCode::Char('f')]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("⊘ itops/incident") && t.contains("judge 0.050") && t.contains("not injected"), "{t}");
    assert!(t.contains("◆ matched") && t.contains("f injected"), "{t}");
    assert!(t.contains("3 ways"), "{t}");

    press(&mut s, &[KeyCode::Char('f')]);
    assert!(!text(&render(&mut s, 120, 40)).contains("itops/incident"), "back to injected");
}

/// A blocked row is selectable and opens the why page by its id, on the
/// `judge` channel the model files it under.
#[test]
fn a_blocked_row_opens_its_why_page() {
    let mut s = showing(judged_replay(), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('f'), KeyCode::Down, KeyCode::Down]);
    assert!(selected(&mut s).contains("⊘ itops/incident"), "{}", selected(&mut s));
    press(&mut s, &[KeyCode::Enter]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("• e1 ⊘ itops/incident"), "the why index has the row: {t}");
    assert!(!t.contains("no fire record in the model"), "{t}");
    // Narrowing the view drops the row; the cursor stays in range.
    press(&mut s, &[KeyCode::Char('f')]);
    assert!(!text(&render(&mut s, 120, 40)).contains("itops/incident"));
}

/// A session the judge never saw is all injected: the header does not
/// name the view, and at 80 columns keeps the frame's timestamp.
#[test]
fn a_session_without_the_judge_keeps_its_timestamp_at_80_columns() {
    let events: Vec<WayEvent> = session_events().into_iter().filter(|e| e.event != "way_judged").collect();
    let mut s = showing(replay_of(events, false), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Right, KeyCode::Right]);
    let t = text(&render(&mut s, 80, 25));
    assert!(t.contains("window 1/2 · 2026-07-03 16:55") && !t.contains("◇ injected"), "{t}");
    let mut j = showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut j, &[KeyCode::Right, KeyCode::Right]);
    assert!(text(&render(&mut j, 80, 25)).contains("◇ injected · 1 judged out"));
}

/// The why page of a judged fire and of a judge-blocked way, from the
/// model the events build.
#[test]
fn the_why_page_shows_the_judges_verdicts() {
    let mut s = showing(judged_replay(), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('f'), KeyCode::Down, KeyCode::Enter]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("would block") && t.contains("P(yes) 0.05 < 0.30"), "{t}");
    press(&mut s, &[KeyCode::Down]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("kept out by the relevance judge"), "{t}");
}

#[test]
fn judged_golden_frames() {
    let mut g = goldens();
    let mut i = showing(judged_replay(), terminal(), Shape::PLAIN);
    check(&mut g, "judged-injected", &mut i);
    let mut m = showing(judged_replay(), terminal(), Shape::PLAIN);
    press(&mut m, &[KeyCode::Char('f')]);
    check(&mut g, "judged-matched", &mut m);
    g.finish();
}

/// A way blocked with its ancestor names the ancestor on its row.
#[test]
fn a_row_blocked_with_its_ancestor_names_it() {
    let events = vec![
        ev("2026-07-03T16:52:00Z", "session_start", "", ""),
        ev("2026-07-03T16:52:01Z", "way_judged", "itops/incident", ""),
        WayEvent { ancestor: "itops/incident".into(), ..ev("2026-07-03T16:52:01Z", "way_judged", "itops/incident/sev1", "") },
    ];
    let frames = build_frames(&events, &[], &HashMap::new(), 50);
    let mut r = Replay::new(SESSION.into(), PROJECT.into(), 200, frames, Playback::replay(1));
    r.judged = true;
    let mut s = showing(r, terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('f')]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("⊘ itops/incident/sev1 (with itops/incident)"), "{t}");
}

/// The header gives the session's judge spend, its own calls only, in
/// tokens; `$` turns it to cost, an unknown-cost call counted apart.
#[test]
fn the_header_gives_the_judges_spend_and_dollar_toggles_tokens_and_cost() {
    let log = [
        serde_json::json!({"event": "judge_call", "ts": "2026-07-03T16:52:01Z", "session": SESSION, "input_tokens": "1000", "output_tokens": "200", "cost_usd": "0.0200", "cost_source": "price_table"}),
        serde_json::json!({"event": "judge_call", "ts": "2026-07-03T16:53:01Z", "session": SESSION, "outcome": "fallback", "reason": "timeout", "cost_source": "unknown"}),
        serde_json::json!({"event": "judge_call", "ts": "2026-07-03T16:53:02Z", "session": "other", "input_tokens": "5000", "cost_usd": "0.5000", "cost_source": "provider"}),
    ]
    .map(|v| v.to_string())
    .join("\n");
    let mut r = replay(false);
    r.spend = session_spend(&log, SESSION);
    let mut s = showing(r, terminal(), Shape::PLAIN);
    let t = text(&render(&mut s, 80, 25));
    assert!(t.contains("judge ×2 · 1.2K tokens") && t.contains("$ cost"), "{t}");
    press(&mut s, &[KeyCode::Char('$')]);
    let t = text(&render(&mut s, 80, 25));
    assert!(t.contains("judge ×2 · $0.0200 + 1 unknown") && t.contains("$ tokens"), "{t}");

    assert!(session_spend(&log, "absent").is_none());
    let mut quiet = showing(replay(false), terminal(), Shape::PLAIN);
    let t = text(&render(&mut quiet, 80, 25));
    assert!(!t.contains("judge ") && !t.contains("$ cost"), "no calls, no spend: {t}");
    // `$` before the first call leaves the figure in tokens once one comes.
    press(&mut quiet, &[KeyCode::Char('$')]);
    let r = quiet.replay.as_mut().unwrap();
    assert!(!r.cost, "$ does nothing without spend");
    r.spend = session_spend(&log, SESSION);
    assert!(text(&render(&mut quiet, 80, 25)).contains("judge ×2 · 1.2K tokens"));
}

/// A heavy spend at 80 columns keeps its whole figure: the session id is
/// shortened before the figure is cut.
#[test]
fn a_heavy_spend_fits_at_80_columns() {
    let mut r = replay(false);
    r.spend = Some(ways_agent_core::spend::Group { calls: 1234, known_calls: 1222, unknown_calls: 12, cost_usd: Some(12.3456), input_tokens: 4_100_000, ..Default::default() });
    r.cost = true;
    let mut s = showing(r, terminal(), Shape::PLAIN);
    let t = text(&render(&mut s, 80, 25));
    assert!(t.contains("Session 8f3a2c1d-5e6  judge ×1234 · $12.3456 + 12 unknown"), "{t}");
    let mut wide = showing(replay(false), terminal(), Shape::PLAIN);
    assert!(text(&render(&mut wide, 80, 25)).contains(SESSION), "without spend the full id stays");
}

/// Judge calls in two projects over three days and two months; one of
/// unknown cost.
fn spend_calls() -> Vec<ways_agent_core::spend::Call> {
    let call = |ts: &str, project: &str, input: u64, cost: Option<f64>| ways_agent_core::spend::Call {
        ts: ts.into(),
        session: SESSION.into(),
        project: project.into(),
        cost_usd: cost,
        input_tokens: input,
        output_tokens: input / 10,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    };
    vec![
        call("2026-06-30T09:00:00Z", "/elsewhere", 9000, Some(0.9)),
        call("2026-07-01T10:00:00Z", PROJECT, 1000, Some(0.0110)),
        call("2026-07-03T16:52:00Z", PROJECT, 2000, Some(0.0220)),
        call("2026-07-03T16:53:00Z", PROJECT, 500, None),
    ]
}

/// The tabs are picked by digit, each named with its digit; the session
/// tabs before a session opens say how to open one; Esc goes back to the
/// picker from every tab.
#[test]
fn digits_pick_the_tabs_and_esc_goes_back_to_the_picker() {
    let mut s = picker(terminal());
    let t = text(&render(&mut s, 80, 25));
    assert!(t.contains("1 sessions") && t.contains("2 timeline") && t.contains("3 fires") && t.contains("4 spend"), "{t}");
    press(&mut s, &[KeyCode::Char('2')]);
    assert!(text(&render(&mut s, 80, 25)).contains("no session open: pick one on the sessions tab"));
    press(&mut s, &[KeyCode::Char('1'), KeyCode::Enter]);
    assert!(text(&render(&mut s, 80, 25)).contains(&format!("Session {SESSION}")), "Enter opens the timeline");
    press(&mut s, &[KeyCode::Char('3')]);
    assert!(text(&render(&mut s, 80, 25)).contains("0 semantic fires"));
    press(&mut s, &[KeyCode::Char('4')]);
    assert!(text(&render(&mut s, 80, 25)).contains("Judge spend by day"));
    press(&mut s, &[KeyCode::Char('9')]);
    assert!(text(&render(&mut s, 80, 25)).contains("Judge spend by day"), "a digit past the tabs does nothing");
    press(&mut s, &[KeyCode::Esc]);
    assert!(text(&render(&mut s, 80, 25)).contains("3 sessions in"), "back on the picker");
    // Back on the timeline, the session is where it was left: by digit, and
    // by Enter on the session already open, which does not read it again.
    press(&mut s, &[KeyCode::Char('2'), KeyCode::Right, KeyCode::Right, KeyCode::Down]);
    let left = text(&render(&mut s, 120, 40));
    assert!(left.contains("3/4"), "{left}");
    press(&mut s, &[KeyCode::Esc, KeyCode::Char('2')]);
    assert_eq!(text(&render(&mut s, 120, 40)), left);
    press(&mut s, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(text(&render(&mut s, 120, 40)), left);
}

/// Esc on the fires and spend tabs is named only where it leads back to a
/// sessions tab.
#[test]
fn the_key_bar_names_esc_only_with_a_sessions_tab() {
    let mut p = picker(terminal());
    press(&mut p, &[KeyCode::Enter, KeyCode::Char('3')]);
    assert!(text(&render(&mut p, 80, 25)).contains("esc sessions"));
    press(&mut p, &[KeyCode::Char('4')]);
    assert!(text(&render(&mut p, 80, 25)).contains("esc sessions"));
    let mut d = showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut d, &[KeyCode::Char('2')]);
    assert!(!text(&render(&mut d, 80, 25)).contains("esc"));
    press(&mut d, &[KeyCode::Char('3')]);
    assert!(!text(&render(&mut d, 80, 25)).contains("esc"));
}

/// A replay plays only while its timeline is shown; a live one keeps
/// reading the log on every tab.
#[test]
fn playback_pauses_off_the_timeline_but_live_keeps_reading() {
    let mut s = showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char(' ')]);
    assert!(s.tick_every().is_some(), "playing on the timeline");
    press(&mut s, &[KeyCode::Char('2')]);
    assert!(s.tick_every().is_none(), "no play on the fires tab");
    let mut l = showing(replay(true), terminal(), Shape::PLAIN);
    press(&mut l, &[KeyCode::Char('3')]);
    assert!(l.tick_every().is_some(), "live reads on the spend tab");
}

/// The fires read again keep the selection on its fire, though a new
/// lower-scoring one sorts in above it.
#[test]
fn a_fires_refresh_keeps_the_selection_on_its_fire() {
    let fire = |score: f64, way: &str| super::SemanticFire { score, way: way.into(), surface: "—".into(), redisclosed: false };
    let mut r = replay(false);
    r.fires.set(vec![fire(0.4, "a"), fire(0.5, "b")]);
    let mut s = showing(r, terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('2'), KeyCode::Down]);
    assert!(selected(&mut s).contains("0.500"));
    s.replay.as_mut().unwrap().fires.set(vec![fire(0.3, "c"), fire(0.4, "a"), fire(0.5, "b")]);
    assert!(selected(&mut s).contains("0.500   b"), "{}", selected(&mut s));
}

/// Opened on one session there is no sessions tab: the digits start at the
/// timeline, and Esc ends the screen from any tab.
#[test]
fn without_a_picker_the_tabs_start_at_the_timeline() {
    let mut s = showing(replay(false), terminal(), Shape::PLAIN);
    let t = text(&render(&mut s, 80, 25));
    assert!(t.contains("1 timeline") && t.contains("3 spend") && !t.contains("sessions"), "{t}");
    press(&mut s, &[KeyCode::Char('2')]);
    assert!(text(&render(&mut s, 80, 25)).contains("semantic fires"));
    assert!(!press(&mut s, &[KeyCode::Esc]), "Esc ends the screen");
}

/// The fires tab lists the session's semantic fires, lowest score first,
/// re-disclosures marked, and only this session's.
#[test]
fn the_fires_tab_lists_the_sessions_semantic_fires() {
    let log = [
        serde_json::json!({"event": "way_fired", "session": SESSION, "way": "softwaredev/code/testing", "trigger": "semantic:embedding:en", "fire_score": "0.612", "surface": "add a unit test"}),
        serde_json::json!({"event": "way_redisclosed", "session": SESSION, "way": "softwaredev/docs/adr", "trigger": "semantic:embedding:en", "fire_score": "0.405"}),
        serde_json::json!({"event": "way_fired", "session": SESSION, "way": "softwaredev/delivery/commits", "trigger": "keyword"}),
        serde_json::json!({"event": "way_fired", "session": "other", "way": "itops/incident", "trigger": "semantic:embedding:en", "fire_score": "0.300"}),
    ]
    .map(|v| v.to_string())
    .join("\n");
    let mut r = replay(false);
    r.fires.set(super::semantic_fires(&log, SESSION));
    let mut s = showing(r, terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('2')]);
    let t = text(&render(&mut s, 80, 25));
    assert!(t.contains("2 semantic fires"), "{t}");
    let adr = t.find("0.405 ↻ softwaredev/docs/adr").expect("the re-disclosure, its surface a placeholder");
    let testing = t.find("0.612   softwaredev/code/testing").expect("the first fire");
    assert!(adr < testing, "lowest score first: {t}");
    assert!(t.contains("add a unit test") && !t.contains("itops/incident"), "{t}");
}

/// The spend tab: the scope's calls by day, newest first, with a total; `m`
/// groups them by month. Another project's call is left out, and the log's
/// start is taken before the filter.
#[test]
fn the_spend_tab_groups_the_scopes_judge_calls_by_day_or_month() {
    let spend = Spend::new(spend_calls(), Some(PROJECT), PROJECT.into());
    let mut s = Introspect::showing(replay(false), with_spend(spend), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('3')]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("3 judge calls in /home/dev/proj") && t.contains("the log holds calls from 2026-06-30"), "{t}");
    let jul3 = t.find("2026-07-03").expect("newest day");
    let jul1 = t.find("2026-07-01").expect("older day");
    assert!(jul3 < jul1, "newest first: {t}");
    assert!(t.contains("$0.0220 + 1 unknown") && t.contains("$0.0330 + 1 unknown") && t.matches("2026-06-30").count() == 1, "another project's day is left out: {t}");
    // A scope written with a trailing slash matches as the sessions do.
    let slashed = Spend::new(spend_calls(), Some("/home/dev/proj/"), PROJECT.into());
    let mut d = Introspect::showing(replay(false), with_spend(slashed), terminal(), Shape::PLAIN);
    press(&mut d, &[KeyCode::Char('3')]);
    assert!(text(&render(&mut d, 120, 40)).contains("3 judge calls"));
    press(&mut s, &[KeyCode::Char('m')]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("Judge spend by month") && t.contains("2026-07 ") && !t.contains("2026-07-01"), "{t}");
}

#[test]
fn tab_golden_frames() {
    let mut g = goldens();
    let mut f = showing(judged_replay(), terminal(), Shape::PLAIN);
    f.replay.as_mut().unwrap().fires.set(vec![
        super::SemanticFire { score: 0.405, way: "softwaredev/docs/adr".into(), surface: "record the decision".into(), redisclosed: true },
        super::SemanticFire { score: 0.612, way: "softwaredev/code/testing".into(), surface: "add a unit test for the parser".into(), redisclosed: false },
    ]);
    press(&mut f, &[KeyCode::Char('2'), KeyCode::Down]);
    check(&mut g, "fires", &mut f);
    let mut s = Introspect::showing(replay(false), with_spend(Spend::new(spend_calls(), Some(PROJECT), PROJECT.into())), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('3')]);
    check(&mut g, "spend", &mut s);
    let mut u = Introspect::showing(replay(false), Reports::new(&usage_log(), Some(PROJECT), PROJECT.into()), terminal(), Shape::PLAIN);
    press(&mut u, &[KeyCode::Char('4')]);
    check(&mut g, "stats", &mut u);
    press(&mut u, &[KeyCode::Char('5')]);
    check(&mut g, "precision", &mut u);
    g.finish();
}

/// A two-digit cost with a two-digit unknown count fits the cost column
/// at 80 columns.
#[test]
fn a_heavy_spend_row_fits_at_80_columns() {
    let call = |cost: Option<f64>| ways_agent_core::spend::Call {
        ts: "2026-07-03T16:52:00Z".into(),
        session: SESSION.into(),
        project: PROJECT.into(),
        cost_usd: cost,
        input_tokens: 1,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    };
    let mut calls = vec![call(Some(12.3456))];
    calls.extend((0..12).map(|_| call(None)));
    let mut s = Introspect::showing(replay(false), with_spend(Spend::new(calls, None, "every project".into())), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('3')]);
    let t = text(&render(&mut s, 80, 25));
    assert!(t.contains("$12.3456 + 12 unknown"), "{t}");
}

/// A small log for the usage and precision tabs: two sessions in this
/// project and one elsewhere.
fn usage_log() -> String {
    let fired = |session: &str, project: &str, way: &str, trigger: &str| {
        serde_json::json!({"event": "way_fired", "ts": "2026-07-03T16:52:01Z", "session": session, "project": project, "way": way, "trigger": trigger, "scope": "agent"})
    };
    [
        serde_json::json!({"event": "session_start", "ts": "2026-07-03T16:52:00Z", "session": "s1", "project": PROJECT}),
        fired("s1", PROJECT, "softwaredev/code/testing", "semantic:embedding:en"),
        fired("s1", PROJECT, "softwaredev/delivery/commits", "bash"),
        serde_json::json!({"event": "session_start", "ts": "2026-07-04T09:00:00Z", "session": "s2", "project": PROJECT}),
        fired("s2", PROJECT, "softwaredev/code/testing", "semantic:embedding:en"),
        fired("s3", "/elsewhere", "itops/incident", "keyword"),
    ]
    .map(|v| v.to_string())
    .join("\n")
}

/// The usage tab ranks the scope's ways by fires and says how they came;
/// the precision tab lists each way with its flag, and the remedy of the
/// selected one. Both name the command an agent runs for the same data.
#[test]
fn the_stats_and_precision_tabs_report_the_scope() {
    let mut s = Introspect::showing(replay(false), Reports::new(&usage_log(), Some(PROJECT), PROJECT.into()), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('4')]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("2 sessions · 3 fires") && t.contains("2026-07-03 → 2026-07-03") && t.contains("ways tune stats --json"), "{t}");
    let testing = t.find("softwaredev/code/testing").expect("the most-fired way");
    let commits = t.find("softwaredev/delivery/commits").expect("the other way");
    assert!(testing < commits && !t.contains("itops/incident"), "ranked by fires, scope kept: {t}");
    press(&mut s, &[KeyCode::Char('5')]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("Fire precision") && t.contains("2 ways fired in /home/dev/proj") && t.contains("ways tune precision --json"), "{t}");
    assert!(t.contains("low-n") && t.contains("insufficient sample"), "under five sessions every way is low-n: {t}");
}

/// Six tabs fit the tab line at 80 columns.
#[test]
fn the_tab_line_fits_at_80_columns() {
    let mut p = picker(terminal());
    let t = text(&render(&mut p, 80, 25));
    assert!(t.lines().next().unwrap().contains("6 precision"), "{t}");
}

/// The report tabs match the scope with its trailing slash trimmed, as the
/// sessions tab does, and each border names its command at that scope.
#[test]
fn the_report_tabs_trim_the_scope_and_name_their_scoped_commands() {
    let mut s = Introspect::showing(replay(false), Reports::new(&usage_log(), Some("/home/dev/proj/"), PROJECT.into()), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('4')]);
    let t = text(&render(&mut s, 120, 40));
    assert!(t.contains("2 sessions · 3 fires") && t.contains("ways tune stats --json --project /home/dev/proj "), "{t}");
    press(&mut s, &[KeyCode::Char('5')]);
    assert!(text(&render(&mut s, 120, 40)).contains("ways tune precision --json --project /home/dev/proj "));
    press(&mut s, &[KeyCode::Char('3')]);
    assert!(text(&render(&mut s, 120, 40)).contains("ways agent cost --json --project /home/dev/proj "));
    let mut all = Introspect::showing(replay(false), Reports::new(&usage_log(), None, "every project".into()), terminal(), Shape::PLAIN);
    press(&mut all, &[KeyCode::Char('4')]);
    assert!(text(&render(&mut all, 120, 40)).contains("ways tune stats --json --global"), "every project is --global for stats");
}

/// A live reload re-ranks the ways; the cursor stays on its way, and a
/// report reads the new log only when its tab is drawn.
#[test]
fn a_report_reload_keeps_the_cursor_on_its_way() {
    let mut s = Introspect::showing(replay(false), Reports::new(&usage_log(), Some(PROJECT), PROJECT.into()), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('4'), KeyCode::Down]);
    assert!(selected(&mut s).contains("softwaredev/delivery/commits"));
    let more = (0..3)
        .map(|i| serde_json::json!({"event": "way_fired", "ts": "2026-07-05T00:00:00Z", "session": format!("n{i}"), "project": PROJECT, "way": "softwaredev/delivery/commits", "trigger": "bash", "scope": "agent"}).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    s.reports.reload(&format!("{}\n{more}", usage_log()));
    let t = selected(&mut s);
    assert!(t.contains("softwaredev/delivery/commits") && t.contains(" 4"), "now first, still selected: {t}");
}

/// Below 100 columns the precision table names the trigger by channel and
/// leaves out the spread, so the way keeps its room.
#[test]
fn precision_narrows_to_the_trigger_channel_below_100_columns() {
    let mut s = Introspect::showing(replay(false), Reports::new(&usage_log(), Some(PROJECT), PROJECT.into()), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Char('5')]);
    let wide = text(&render(&mut s, 120, 40));
    assert!(wide.contains("Spread"), "{wide}");
    let narrow = text(&render(&mut s, 80, 25));
    assert!(!narrow.contains("Spread") && narrow.contains("softwaredev/delivery/commits"), "{narrow}");
}
