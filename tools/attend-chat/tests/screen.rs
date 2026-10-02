//! The chat screen through `agent-tui`'s test kit (ADR-504 §12): golden
//! frames of the tab strip, the legend, the message feed, the slash input
//! and the theme, at 80x25 and the reference size, and the keys driven
//! through the real key handler.
//!
//! Every test runs against one fixture home: `HOME` and the XDG
//! directories point into a temp directory set up once, before any test
//! reads them, so the real attend cache and agent-ways config are never
//! read or written. Frames pin the clock and the UTC offset.
//!
//! `AGENT_TUI_BLESS=1` records the frames for review; `AGENT_TUI_DUMP=<dir>`
//! writes every frame checked.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, UNIX_EPOCH};

use agent_theme::ColorDepth;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use agent_tui::screen::Screen;
use agent_tui::testkit::{self, Goldens};
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};
use attend_chat::tabs::Tab;

/// 2026-10-02 14:00 UTC.
const NOW: u64 = 1_790_949_600;

/// Two claudes, one of them in `#deploy`, and a human.
const ALPHA: &str = "/work/alpha";
const BETA: &str = "/work/beta";
const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";

struct Fixture {
    root: PathBuf,
}

/// A file a message attaches. Its path is in the frame, so it is one
/// every Unix test host has, not one under the fixture's temp directory.
const SHOT: &str = "/bin/sh";

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("attend-chat-screen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // Set once, before any test reads them: every test calls this first.
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
        std::env::set_var("XDG_STATE_HOME", root.join("state"));
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        std::env::set_var("USER", "aaron");
        let base = attend_chat::signal::signals_base();
        std::fs::create_dir_all(&base).unwrap();
        let g = attend_groups::Groups::new(&base, BETA_SID);
        g.join("deploy", true).unwrap();
        g.set_description("deploy", "rollout coordination").unwrap();
        attend_groups::Groups::new(&base, "").create("infra", None).unwrap();
        for sid in [ALPHA_SID, BETA_SID] {
            attend_presence::heartbeat::touch(sid).unwrap();
        }
        assert!(Path::new(SHOT).is_file(), "the attachment the frames show");
        Fixture { root }
    })
}

fn sig(from: &str, cwd: &str, ago: u64, channel: Channel, message: &str) -> Signal {
    Signal {
        id: format!("t-{ago}"),
        from: from.into(),
        project: cwd.rsplit('/').next().unwrap_or("").into(),
        cwd: cwd.into(),
        reply_to: None,
        message: message.into(),
        ts: NOW - ago,
        channel,
    }
}

/// The transcript the frames show: broadcast, a channel message, a human
/// with a dropped file, and a long message that wraps.
fn transcript() -> Vec<Signal> {
    vec![
        sig(&format!("claude:{ALPHA_SID}"), ALPHA, 600, Channel::Open, "starting the theme port; the legend moves first"),
        sig(&format!("claude:{BETA_SID}"), BETA, 300, Channel::Group("deploy".into()), "rollout is green on staging\nproduction at 15:00"),
        sig("external:aaron@kitty", "/home/aaron", 120, Channel::Open, &format!("the shell this ran under: {SHOT}")),
        sig(
            &format!("claude:{ALPHA_SID}"),
            ALPHA,
            30,
            Channel::Open,
            "a longer message, so the feed shows how a body wraps beside its chip when it runs past the width of the row it is given",
        ),
    ]
}

fn chat_at(depth: ColorDepth, theme: Option<&str>) -> Chat {
    let f = fixture();
    let dir = f.root.join("config/agent-ways/themes");
    let (painter, _) = agent_theme::Painter::named_in(theme, &dir, depth);
    let mut c = Chat::new(None, Palette { painter }, Shape::PLAIN)
        .heartbeat(false)
        .pinned(UNIX_EPOCH + Duration::from_secs(NOW), 0);
    for s in transcript() {
        c.push(s);
    }
    c
}

fn chat() -> Chat {
    chat_at(ColorDepth::Ansi16, None)
}

fn goldens() -> Goldens {
    Goldens::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"))
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn typed(c: &mut Chat, s: &str) {
    assert!(testkit::drive(c, &s.chars().map(|ch| key(KeyCode::Char(ch))).collect::<Vec<_>>()));
}

fn press(c: &mut Chat, k: KeyEvent) -> bool {
    testkit::drive(c, &[k])
}

/// The reference size beside 80x25.
const SIZES: [(u16, u16); 2] = [(80, 25), (120, 40)];

fn shoot(g: &mut Goldens, name: &str, c: &mut Chat) {
    for (w, h) in SIZES {
        g.check(&format!("{name}-{w}x{h}"), &testkit::render_screen(c, w, h));
    }
}

#[test]
fn golden_merged_feed_tabs_and_compose() {
    let mut g = goldens();
    shoot(&mut g, "merged", &mut chat());
    g.finish();
}

#[test]
fn golden_channel_tab_filters_the_feed_and_shows_its_description() {
    let mut g = goldens();
    let mut c = chat();
    // merged → #open → #deploy
    press(&mut c, key(KeyCode::Tab));
    press(&mut c, key(KeyCode::Tab));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    shoot(&mut g, "channel-deploy", &mut c);
    g.finish();
}

#[test]
fn golden_legends_mark_what_tab_completes() {
    let mut g = goldens();
    let mut c = chat();
    typed(&mut c, "@");
    g.check("legend-agents-80x25", &testkit::render_screen(&mut c, 80, 25));
    let mut c = chat();
    typed(&mut c, "#de");
    g.check("legend-groups-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

#[test]
fn golden_slash_input_legend_help_and_subcommands() {
    let mut g = goldens();
    let mut c = chat();
    typed(&mut c, "/");
    g.check("slash-80x25", &testkit::render_screen(&mut c, 80, 25));
    typed(&mut c, "chan");
    g.check("slash-partial-80x25", &testkit::render_screen(&mut c, 80, 25));
    typed(&mut c, "nels ");
    g.check("slash-subcommands-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

#[test]
fn golden_an_error_holds_the_input_and_shows_in_the_status_line() {
    let mut g = goldens();
    let mut c = chat();
    typed(&mut c, "/bogus");
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.input().text(), "/bogus", "a failure leaves the input to edit");
    assert!(c.status().contains("bogus"), "{}", c.status());
    g.check("status-error-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

#[test]
fn golden_compose_grows_with_its_lines() {
    let mut g = goldens();
    let mut c = chat();
    typed(&mut c, "first line");
    press(&mut c, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    typed(&mut c, "second line, then a path: ");
    typed(&mut c, SHOT);
    g.check("compose-multiline-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

#[test]
fn golden_theme_chosen_at_truecolor_and_without_colour() {
    let mut g = goldens();
    shoot(&mut g, "theme-nord-truecolor", &mut chat_at(ColorDepth::TrueColor, Some("nord")));
    g.check("theme-nord-256-80x25", &testkit::render_screen(&mut chat_at(ColorDepth::Ansi256, Some("nord")), 80, 25));
    g.check("no-color-80x25", &testkit::render_screen(&mut chat_at(ColorDepth::NoColor, Some("nord")), 80, 25));
    g.finish();
}

#[test]
fn a_chosen_theme_at_16_colours_is_the_terminal_palette_whole() {
    let default = testkit::frame(&testkit::render_screen(&mut chat_at(ColorDepth::Ansi16, None), 80, 25));
    let nord16 = testkit::frame(&testkit::render_screen(&mut chat_at(ColorDepth::Ansi16, Some("nord")), 80, 25));
    assert_eq!(default, nord16);
    let nord = testkit::frame(&testkit::render_screen(&mut chat_at(ColorDepth::TrueColor, Some("nord")), 80, 25));
    assert_ne!(default, nord, "at truecolor the chosen theme draws");
}

#[test]
fn without_colour_no_cell_is_coloured() {
    let buf = testkit::render_screen(&mut chat_at(ColorDepth::NoColor, Some("nord")), 80, 25);
    use agent_tui::ratatui::style::Color;
    assert!(buf.content().iter().all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
}

#[test]
fn esc_and_ctrl_c_end_the_chat() {
    let mut c = chat();
    assert!(!press(&mut c, key(KeyCode::Esc)));
    let mut c = chat();
    assert!(!press(&mut c, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
}

#[test]
fn tab_on_an_empty_input_cycles_the_tabs_and_alt_digits_jump() {
    let mut c = chat();
    let order: Vec<Tab> = (0..4)
        .map(|_| {
            press(&mut c, key(KeyCode::Tab));
            c.foreground().clone()
        })
        .collect();
    assert_eq!(order, [Tab::Channel("open".into()), Tab::Channel("deploy".into()), Tab::Channel("infra".into()), Tab::Merged]);
    press(&mut c, KeyEvent::new(KeyCode::Char('3'), KeyModifiers::ALT));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    press(&mut c, KeyEvent::new(KeyCode::Char('9'), KeyModifiers::ALT));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()), "an empty slot does nothing");
    press(&mut c, KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT));
    assert_eq!(c.foreground(), &Tab::Merged);
    assert!(c.input().is_empty(), "Alt+digit types nothing");
}

#[test]
fn tab_with_text_completes_commands_channels_and_agents() {
    let mut c = chat();
    typed(&mut c, "/he");
    press(&mut c, key(KeyCode::Tab));
    assert_eq!(c.input().text(), "/help ");
    assert_eq!(c.foreground(), &Tab::Merged, "completion does not cycle tabs");
    let mut c = chat();
    typed(&mut c, "#dep");
    press(&mut c, key(KeyCode::Tab));
    assert_eq!(c.input().text(), "#deploy ");
    let mut c = chat();
    typed(&mut c, "/channels cr");
    press(&mut c, key(KeyCode::Tab));
    assert_eq!(c.input().text(), "/channels create ");
}

#[test]
fn editing_keys_reach_the_input() {
    let mut c = chat();
    typed(&mut c, "helo");
    press(&mut c, key(KeyCode::Left));
    typed(&mut c, "l");
    press(&mut c, key(KeyCode::End));
    press(&mut c, KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    typed(&mut c, "x");
    press(&mut c, key(KeyCode::Backspace));
    press(&mut c, key(KeyCode::Home));
    press(&mut c, key(KeyCode::Delete));
    assert_eq!(c.input().text(), "hello\n");
    assert_eq!(c.input().cursor(), 6);
}

#[test]
fn enter_runs_a_command_and_clear_empties_the_transcript() {
    let mut c = chat();
    typed(&mut c, "/help");
    press(&mut c, key(KeyCode::Enter));
    assert!(c.input().is_empty());
    assert!(c.status().starts_with("available:"), "{}", c.status());
    typed(&mut c, "/clear");
    press(&mut c, key(KeyCode::Enter));
    assert!(c.signals().is_empty());
    assert_eq!(c.status(), "transcript cleared");
}

#[test]
fn enter_sends_to_the_fixture_bus_and_clears_the_input() {
    let mut c = chat();
    typed(&mut c, "hello from the screen test");
    press(&mut c, key(KeyCode::Enter));
    assert!(c.input().is_empty());
    assert!(c.status().starts_with("sent:"), "{}", c.status());
    let base = attend_chat::signal::broadcast_dir();
    assert!(base.starts_with(&fixture().root), "the send stays in the fixture");
    let found = std::fs::read_dir(&base).unwrap().flatten().any(|e| {
        std::fs::read_to_string(e.path()).is_ok_and(|t| t.contains("hello from the screen test"))
    });
    assert!(found);
}

#[test]
fn page_up_scrolls_the_feed_back_and_page_down_returns() {
    let mut c = chat();
    let bottom = testkit::text(&testkit::render_screen(&mut c, 80, 16));
    press(&mut c, key(KeyCode::PageUp));
    let back = testkit::text(&testkit::render_screen(&mut c, 80, 16));
    assert_ne!(bottom, back, "a page back moves the feed");
    for _ in 0..10 {
        press(&mut c, key(KeyCode::PageUp));
    }
    let top = testkit::text(&testkit::render_screen(&mut c, 80, 16));
    assert!(top.contains("starting the theme port"), "the scroll stops at the oldest message: {top}");
    for _ in 0..10 {
        press(&mut c, key(KeyCode::PageDown));
    }
    assert_eq!(testkit::text(&testkit::render_screen(&mut c, 80, 16)), bottom);
}

#[test]
fn the_watcher_feeds_the_screen_on_tick() {
    let (tx, rx) = std::sync::mpsc::channel();
    fixture();
    let mut c = Chat::new(Some(rx), Palette::default(), Shape::PLAIN).heartbeat(false);
    tx.send(sig("external:aaron@kitty", "/home/aaron", 0, Channel::Open, "arrived")).unwrap();
    c.tick();
    assert_eq!(c.signals().len(), 1);
    assert!(testkit::text(&testkit::render_screen(&mut c, 80, 25)).contains("arrived"));
}
