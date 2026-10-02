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
use ways_core::introspection::{FiredWay, IntrospectionSummary, JoinConfidence, MatchCriteria, MatchDetail, SessionIntrospection, Turn};

use super::frames::build_frames;
use super::model::WayEvent;
use super::screen::{reselect_by_anchor, Introspect, Picker, Replay};
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
    }
}

/// A session of two compaction windows: three ways fire, a check fires, a
/// way re-discloses and the gate keeps one out; after the compaction one
/// way re-discloses.
fn replay(live: bool) -> Replay {
    let events = vec![
        ev("2026-07-03T16:52:00Z", "session_start", "", ""),
        ev("2026-07-03T16:52:01Z", "way_fired", "softwaredev/code/testing", "semantic:embedding:en"),
        ev("2026-07-03T16:52:02Z", "way_fired", "softwaredev/delivery/commits", "keyword"),
        ev("2026-07-03T16:53:00Z", "way_fired", "softwaredev/docs/adr", "file"),
        ev("2026-07-03T16:53:01Z", "check_fired", "softwaredev/delivery/commits", ""),
        ev("2026-07-03T16:55:00Z", "way_redisclosed", "softwaredev/code/testing", "semantic:embedding:en"),
        ev("2026-07-03T16:55:01Z", "way_judged", "itops/incident", ""),
        ev("2026-07-03T17:52:00Z", "session_start", "", ""),
        ev("2026-07-03T17:52:01Z", "way_redisclosed", "softwaredev/delivery/commits", "keyword"),
    ];
    let tokens: Vec<(String, u64)> = [("2026-07-03T16:52:00Z", 18), ("2026-07-03T16:53:00Z", 64), ("2026-07-03T16:55:00Z", 132), ("2026-07-03T17:52:00Z", 31)]
        .iter()
        .map(|(t, k)| (t.to_string(), *k))
        .collect();
    let refire: HashMap<String, u64> =
        [("softwaredev/code/testing", 40), ("softwaredev/delivery/commits", 30), ("softwaredev/docs/adr", 80)].iter().map(|(w, k)| (w.to_string(), *k)).collect();
    let frames = build_frames(&events, &tokens, &refire, 50);
    let play = if live { Playback::live(frames.len()) } else { Playback::replay(frames.len()) };
    let mut r = Replay::new(SESSION.into(), PROJECT.into(), 200, frames, play);
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
        fire_score: score,
        way_path: path.map(str::to_string),
        criteria: MatchCriteria { vocabulary: Some("test tdd unit golden fixture assert".into()), ..Default::default() },
        match_detail: span.map(|s| MatchDetail { matched_span: Some(s.into()), confidence: JoinConfidence::Keyed }),
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
    Introspect::picking(Picker::new(sessions(), PROJECT.into()), open, palette, Shape::PLAIN)
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
    let mut r = Introspect::showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut r, &[KeyCode::Right, KeyCode::Right, KeyCode::Down]);
    check(&mut g, "replay", &mut r);

    // The same frame in a theme, playing.
    let mut t = Introspect::showing(replay(false), nord(), Shape::ROUND);
    press(&mut t, &[KeyCode::Right, KeyCode::Right, KeyCode::Char(' ')]);
    check(&mut g, "replay-nord", &mut t);

    // Why the first way fired: its trigger, criteria and body as markdown.
    let mut w = Introspect::showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut w, &[KeyCode::Right, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    check(&mut g, "why", &mut w);

    // Live, following the newest frame, which fired five seconds ago.
    let mut l = Introspect::showing(replay(true), terminal(), Shape::PLAIN);
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
    let mut r = Introspect::showing(replay(false), terminal(), Shape::PLAIN);
    assert!(!r.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    // A replay opened directly ends on Esc: there is no picker to go back to.
    let mut d = Introspect::showing(replay(false), terminal(), Shape::PLAIN);
    assert!(!press(&mut d, &[KeyCode::Esc]));
}

/// A scope too long for the pane's title loses its end, not the count.
#[test]
fn a_long_scope_keeps_the_session_count_in_view() {
    let open = Box::new(|_: &str| Err("none".to_string()));
    let scope = format!("/var/folders/{}/proj", "x".repeat(120));
    let mut s = Introspect::picking(Picker::new(sessions(), scope), open, terminal(), Shape::PLAIN);
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
    let mut s = Introspect::showing(replay(false), terminal(), Shape::PLAIN);
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
    let mut s = Introspect::showing(replay(false), terminal(), Shape::PLAIN);
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
    let f = &r.frames[2];
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

/// Live, new events on the log read the frames again; the why-fired
/// reader keeps its place and its index rather than going back to the top.
#[test]
fn a_live_refresh_keeps_the_why_reader_where_it_was() {
    let mut s = Introspect::showing(replay(true), terminal(), Shape::PLAIN);
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
    let mut s = Introspect::showing(replay(false), terminal(), Shape::PLAIN);
    press(&mut s, &[KeyCode::Right, KeyCode::Right, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    let top = frame(&render(&mut s, 80, 25));
    assert!(top.contains("1–"), "a long document shows its range: {top}");
    press(&mut s, &[KeyCode::Char('G')]);
    let end = text(&render(&mut s, 80, 25));
    assert!(end.contains("golden"), "the end of the body is reached: {end}");
    press(&mut s, &[KeyCode::Char('g')]);
    assert!(text(&render(&mut s, 80, 25)).contains("Trigger"), "and the top again");
}
