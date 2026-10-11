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
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
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
        // A channel `_groups.yaml` still names after its directory was
        // swept: the left column lists it, set back (ADR-405).
        attend_groups::Groups::new(&base, ALPHA_SID).join("archive", false).unwrap();
        let _ = std::fs::remove_dir_all(base.join("@archive"));
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
    // The frames before the left column (ADR-405) keep the legend row;
    // the column's own tests turn it on (`chat_side`).
    let mut c = Chat::new(None, Palette { painter }, Shape::PLAIN)
        .sidebar(false)
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
    // Read at run time: a test binary reused from another checkout keeps the
    // path it was built at.
    Goldens::new(std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into())).join("tests/golden"))
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
    // Where the terminal reports Ctrl+digits, Esc is only Esc.
    let mut c = chat().enhanced(true);
    assert!(!press(&mut c, key(KeyCode::Esc)));
    // Where it does not, Ctrl+3 is this same Esc: it asks, and y quits.
    let mut c = chat();
    assert!(press(&mut c, key(KeyCode::Esc)));
    assert!(press(&mut c, key(KeyCode::Esc)), "a second Esc (or Ctrl+3) stays");
    assert!(press(&mut c, key(KeyCode::Esc)));
    assert!(!press(&mut c, key(KeyCode::Char('y'))));
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

#[test]
fn an_unchanged_screen_is_copied_not_drawn_again() {
    let mut c = chat();
    for _ in 0..50 {
        testkit::render_screen(&mut c, 80, 25);
    }
    assert_eq!(c.draws(), 1, "fifty idle frames draw once");
    let copied = testkit::frame(&testkit::render_screen(&mut c, 80, 25));
    typed(&mut c, "x");
    testkit::render_screen(&mut c, 80, 25);
    assert_eq!(c.draws(), 2, "a key draws afresh");
    testkit::render_screen(&mut c, 100, 25);
    assert_eq!(c.draws(), 3, "a resize draws afresh");
    let (tx, rx) = std::sync::mpsc::channel();
    let mut c = Chat::new(Some(rx), Palette::default(), Shape::PLAIN).heartbeat(false);
    testkit::render_screen(&mut c, 80, 25);
    c.tick();
    testkit::render_screen(&mut c, 80, 25);
    let before = c.draws();
    tx.send(sig("external:aaron@kitty", "/home/aaron", 0, Channel::Open, "new")).unwrap();
    c.tick();
    testkit::render_screen(&mut c, 80, 25);
    assert_eq!(c.draws(), before + 1, "a new message draws afresh");
    // The copy is the frame itself.
    let mut c = chat();
    let first = testkit::frame(&testkit::render_screen(&mut c, 80, 25));
    assert_eq!(testkit::frame(&testkit::render_screen(&mut c, 80, 25)), first);
    assert!(!copied.is_empty());
}

#[test]
fn an_idle_refresh_of_an_unchanged_world_draws_nothing_new() {
    let mut c = chat();
    testkit::render_screen(&mut c, 80, 25);
    for _ in 0..3 {
        c.refresh();
        testkit::render_screen(&mut c, 80, 25);
    }
    assert_eq!(c.draws(), 1, "the peers and channels read the same: no re-layout, no redraw");
}

#[test]
fn a_failed_command_leaves_the_view_paged_back_and_a_success_returns() {
    let mut c = chat();
    testkit::render_screen(&mut c, 80, 16);
    press(&mut c, key(KeyCode::PageUp));
    testkit::render_screen(&mut c, 80, 16);
    let back = c.scroll();
    assert!(back > 0);
    typed(&mut c, "/bogus");
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.scroll(), back, "an error does not move the view");
    for _ in 0..6 {
        press(&mut c, key(KeyCode::Backspace));
    }
    typed(&mut c, "/help");
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.scroll(), 0, "a success shows the newest");
}

#[test]
fn a_huge_message_reaches_its_tail() {
    let mut c = chat_at(ColorDepth::Ansi16, None);
    let body: String = (0..70_000).map(|i| format!("row {i}\n")).collect();
    c.push(sig("external:aaron@kitty", "/home/aaron", 0, Channel::Open, body.trim_end()));
    let text = testkit::text(&testkit::render_screen(&mut c, 80, 25));
    assert!(text.contains("row 69999"), "{text}");
}

#[test]
fn the_compose_box_wraps_by_word() {
    let mut c = chat();
    typed(&mut c, &"word ".repeat(20));
    let text = testkit::text(&testkit::render_screen(&mut c, 40, 25));
    let compose: Vec<&str> = text.lines().filter(|l| l.starts_with("│ >") || l.starts_with("│  ")).collect();
    assert!(compose.len() >= 2, "{text}");
    assert!(compose.iter().all(|l| !l.contains("wor ") && !l.contains(" ord")), "no word is split: {compose:?}");
}

#[test]
fn the_feed_shows_dates_once_the_day_changes() {
    // 23:59 UTC, then 00:01 the next day, with the screen idle between.
    let late = UNIX_EPOCH + Duration::from_secs(NOW + 10 * 3600 - 60);
    let mut c = chat_at(ColorDepth::Ansi16, None).pinned(late, 0);
    c.tick();
    assert!(testkit::text(&testkit::render_screen(&mut c, 80, 25)).contains("13:59"));
    let mut c = c.pinned(late + Duration::from_secs(120), 0);
    c.tick();
    let text = testkit::text(&testkit::render_screen(&mut c, 80, 25));
    assert!(text.contains("10-02 13:59"), "yesterday's times show their date: {text}");
}

#[test]
fn a_short_terminal_keeps_the_compose_cursor_in_view() {
    let mut c = chat();
    typed(&mut c, "one two three four five six seven eight nine ten eleven twelve");
    let text = testkit::text(&testkit::render_screen(&mut c, 30, 10));
    assert!(text.contains("twelve"), "the row with the cursor is shown: {text}");
}

/// Each key after a frame at `w` by `h`, as the terminal draws one before
/// it reads the next key: what a key does can depend on the frame, such
/// as a page's height.
fn drawn(c: &mut Chat, keys: &[KeyEvent], w: u16, h: u16) -> bool {
    keys.iter().all(|k| {
        testkit::render_screen(c, w, h);
        press(c, *k)
    })
}

fn chars(s: &str) -> Vec<KeyEvent> {
    s.chars().map(|ch| key(KeyCode::Char(ch))).collect()
}

#[test]
fn golden_compose_and_send_to_the_foreground_channel() {
    let mut g = goldens();
    let mut c = chat();
    // Alt+3 is #deploy, where beta is a live member.
    drawn(&mut c, &[KeyEvent::new(KeyCode::Char('3'), KeyModifiers::ALT)], 80, 25);
    drawn(&mut c, &chars("ship it at 15:00"), 80, 25);
    g.check("compose-deploy-80x25", &testkit::render_screen(&mut c, 80, 25));
    drawn(&mut c, &[key(KeyCode::Enter)], 80, 25);
    assert!(c.input().is_empty());
    assert_eq!(c.status(), "sent → #deploy");
    g.check("sent-deploy-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

#[test]
fn golden_channel_switch_by_alt_digit() {
    let mut g = goldens();
    let mut c = chat();
    drawn(&mut c, &[KeyEvent::new(KeyCode::Char('4'), KeyModifiers::ALT)], 80, 25);
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()));
    g.check("channel-infra-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

#[test]
fn golden_a_slash_command_runs_and_says_so() {
    let mut g = goldens();
    let mut c = chat();
    drawn(&mut c, &chars("/help"), 80, 25);
    drawn(&mut c, &[key(KeyCode::Enter)], 80, 25);
    assert!(c.status().starts_with("available:"), "{}", c.status());
    g.check("slash-help-ran-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE }
}

/// The chat starts with the mouse off, so the terminal selects text and
/// middle-click pastes. Alt+m is the shell's toggle beside a text entry,
/// where m types; the bottom bar says which way it is, and how to select
/// while it is on.
#[test]
fn golden_alt_m_turns_the_mouse_on_and_m_types() {
    let mut g = goldens();
    let mut c = chat();
    assert!(!c.app().mouse_on(), "a chat starts with the terminal's own mouse");
    drawn(&mut c, &[alt('m')], 80, 25);
    assert!(c.app().mouse_on());
    assert!(c.input().is_empty(), "Alt+m types nothing");
    g.check("mouse-on-80x25", &testkit::render_screen(&mut c, 80, 25));
    drawn(&mut c, &[key(KeyCode::Char('m'))], 80, 25);
    assert_eq!(c.input().text(), "m", "a plain m is text");
    assert!(c.app().mouse_on());
    g.finish();
}

/// With the mouse on, a middle click pastes nothing; the bar says how to
/// paste.
#[test]
fn a_middle_click_says_how_to_paste() {
    let mut c = chat();
    drawn(&mut c, &[alt('m')], 80, 25);
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Middle), 10, 20));
    assert!(c.input().is_empty());
    let bar = testkit::rows(&testkit::render_screen(&mut c, 120, 25)).pop().expect("a bar");
    assert!(bar.contains("middle-click pastes with the mouse off"), "{bar}");
}

/// Typing on past an accidental Esc reaches the draft as typed: the guard
/// opens, and the keys that follow go into the draft instead of answering
/// it. `text` is typed after "half a thought" and an Esc.
fn typed_past_esc(text: &str) -> Chat {
    let mut c = chat();
    drawn(&mut c, &chars("half a thought"), 80, 25);
    assert!(drawn(&mut c, &[key(KeyCode::Esc)], 80, 25));
    assert!(c.app().guarding());
    assert!(drawn(&mut c, &chars(text), 80, 25), "the chat stays open");
    assert!(!c.app().guarding());
    c
}

/// D arms the quit; the i after it types the D, then itself.
#[test]
fn did_you_after_esc_keeps_every_character() {
    assert_eq!(typed_past_esc("Did you").input().text(), "half a thoughtDid you");
    assert_eq!(typed_past_esc(" Did you see the deploy?").input().text(), "half a thought Did you see the deploy?");
}

/// D then y would have confirmed the quit; beside text no letter does.
#[test]
fn dylan_here_after_esc_keeps_the_draft_and_the_chat() {
    assert_eq!(typed_past_esc("Dylan here").input().text(), "half a thoughtDylan here");
}

/// An editing key closes the guard and edits the draft.
#[test]
fn backspace_after_esc_edits_the_draft() {
    let mut c = chat();
    drawn(&mut c, &chars("half a thought"), 80, 25);
    assert!(drawn(&mut c, &[key(KeyCode::Esc)], 80, 25));
    assert!(drawn(&mut c, &[key(KeyCode::Backspace), key(KeyCode::Left)], 80, 25), "the chat stays open");
    assert!(!c.app().guarding());
    assert_eq!(c.input().text(), "half a though");
    assert_eq!(c.input().cursor(), "half a thoug".len());
}

#[test]
fn a_click_on_a_tab_shows_its_channel_and_opens_its_menu() {
    let mut c = chat();
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "3 #deploy").expect("the tab is drawn");
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    assert!(text(&mut c, 80, 25).contains("pick one: #deploy"), "one click opens the menu (ADR-405)");
    assert!(press(&mut c, key(KeyCode::Esc)), "Esc closes the menu");
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "1 merged").expect("the tab is drawn");
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    assert_eq!(c.foreground(), &Tab::Merged);
}

#[test]
fn golden_a_tab_clicked() {
    let mut g = goldens();
    let mut c = chat();
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "4 #infra").expect("the tab is drawn");
    // The glyph before the lozenge is part of the target.
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x.saturating_sub(3), y));
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()));
    g.check("clicked-infra-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

#[test]
fn the_wheel_scrolls_the_feed_and_not_outside_it() {
    let mut c = chat();
    testkit::render_screen(&mut c, 80, 16);
    c.mouse(mouse(MouseEventKind::ScrollUp, 10, 0));
    assert_eq!(c.scroll(), 0, "the tab bar is not the feed");
    c.mouse(mouse(MouseEventKind::ScrollUp, 10, 5));
    assert_eq!(c.scroll(), 3);
    c.mouse(mouse(MouseEventKind::ScrollDown, 10, 5));
    c.mouse(mouse(MouseEventKind::ScrollDown, 10, 5));
    assert_eq!(c.scroll(), 0);
}

/// Esc over a draft asks first, as the settings shell does over pending
/// edits; Esc goes back to it, D then y quits and drops it.
#[test]
fn golden_esc_over_a_draft_asks_first() {
    let mut g = goldens();
    let mut c = chat();
    drawn(&mut c, &chars("half a thought"), 80, 25);
    assert!(drawn(&mut c, &[key(KeyCode::Esc)], 80, 25), "a draft keeps the chat open");
    assert!(c.app().guarding());
    g.check("guard-draft-80x25", &testkit::render_screen(&mut c, 80, 25));
    assert!(drawn(&mut c, &[key(KeyCode::Esc)], 80, 25));
    assert!(!c.app().guarding());
    assert_eq!(c.input().text(), "half a thought", "back to the draft as it was");
    assert!(drawn(&mut c, &[KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)], 80, 25), "^C asks too");
    assert!(c.app().guarding());
    assert!(drawn(&mut c, &[key(KeyCode::Char('D'))], 80, 25));
    g.check("guard-armed-80x25", &testkit::render_screen(&mut c, 80, 25));
    assert!(!drawn(&mut c, &[key(KeyCode::Enter)], 80, 25), "D then Enter quits and drops the draft");
    g.finish();
}

#[test]
fn golden_f1_shows_the_keys_from_their_declaration() {
    let mut g = goldens();
    let mut c = chat();
    drawn(&mut c, &[key(KeyCode::F(1))], 80, 25);
    let text = testkit::text(&testkit::render_screen(&mut c, 80, 25));
    for b in c.app().bindings() {
        assert!(text.contains(&b.keys), "the help lists {}: {text}", b.keys);
    }
    g.check("keys-80x25", &testkit::render_screen(&mut c, 80, 25));
    drawn(&mut c, &[key(KeyCode::Char('x'))], 80, 25);
    assert!(c.input().is_empty(), "the key that closes the help types nothing");
    g.finish();
}

/// Every binding the footer shows is one the declaration holds, in its
/// order: the footer writes no key's meaning of its own.
#[test]
fn the_footer_is_the_declaration() {
    let mut c = chat();
    let rows = testkit::rows(&testkit::render_screen(&mut c, 200, 25));
    let bar = rows.last().expect("a bottom bar");
    let shown: Vec<String> =
        c.app().bindings().iter().filter(|b| b.footer).map(|b| format!("{} {}", b.keys, b.label)).collect();
    let footer = bar.split(" │ ").last().expect("the footer").trim_end();
    assert_eq!(footer, shown.join(" · "));
}

#[test]
fn golden_the_feed_paged_back() {
    let mut g = goldens();
    let mut c = chat();
    drawn(&mut c, &[key(KeyCode::PageUp)], 80, 25);
    assert!(c.scroll() > 0);
    g.check("scrolled-back-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

// ── Clicks in the pane (#739) ───────────────────────────────────

fn left(c: &mut Chat, (x, y): (u16, u16)) {
    testkit::render_screen(c, 80, 25);
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
}

/// A click on a message cut off at the top of the feed selects it and
/// scrolls it whole into view; its box is drawn in the accent.
#[test]
fn a_click_on_a_message_selects_it_and_brings_it_into_view() {
    let mut c = chat();
    let buf = testkit::render_screen(&mut c, 80, 18);
    let rows = testkit::rows(&buf);
    // The feed's first row inside its border: part of an older message.
    let top = rows.iter().position(|r| r.starts_with('╭')).expect("the feed") as u16 + 1;
    assert_eq!(c.scroll(), 0);
    testkit::render_screen(&mut c, 80, 18);
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 10, top));
    assert!(c.scroll() > 0, "scrolled back to show the message whole: {rows:#?}");
    let after = testkit::rows(&testkit::render_screen(&mut c, 80, 18));
    assert!(after[top as usize].contains('╭'), "the message's box starts at the top: {after:?}");
}

/// A click in the compose box puts the cursor on the character under it;
/// a click on the prompt, at the start.
#[test]
fn a_click_in_the_compose_box_places_the_cursor() {
    let mut c = chat();
    typed(&mut c, "hello there");
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "> hello").expect("the compose box");
    left(&mut c, (x + 2 + 6, y));
    assert_eq!(c.input().cursor(), 6, "on the t of there");
    left(&mut c, (x, y));
    assert_eq!(c.input().cursor(), 0, "the prompt is the start");
    left(&mut c, (x + 40, y));
    assert_eq!(c.input().cursor(), 11, "past the text: its end");
    typed(&mut c, "!");
    assert_eq!(c.input().text(), "hello there!");
}

/// A click on a chip of the helper row completes what is being typed to
/// it: a channel after `#`, an agent after `@`, a slash command.
#[test]
fn a_click_on_a_chip_completes_it() {
    let mut c = chat();
    typed(&mut c, "ship it #de");
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    let helper = rows.len() - 2;
    let x = rows[helper].find("#deploy").map(|b| rows[helper][..b].chars().count() as u16).expect("the chip");
    left(&mut c, (x, helper as u16));
    assert_eq!(c.input().text(), "ship it #deploy ");
    assert_eq!(c.input().cursor(), "ship it #deploy ".chars().count());

    let mut c = chat();
    typed(&mut c, "/he");
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    let helper = rows.len() - 2;
    let x = rows[helper].find("/help").map(|b| rows[helper][..b].chars().count() as u16).expect("the chip");
    left(&mut c, (x, helper as u16));
    assert_eq!(c.input().text(), "/help ");
}

/// The mouse as a `--keys` script drives it: the mouse on, a click on the
/// first message, a click in the compose box after typing.
#[test]
fn golden_clicks_in_the_feed_and_the_compose_box() {
    let mut g = goldens();
    let mut c = chat();
    let script = testkit::parse_events(["alt-m", "text:hello", "click:6,3", "click:5,20"]).unwrap();
    assert!(testkit::play(&mut c, &script, Some((80, 25))));
    g.check("clicked-feed-compose-80x25", &testkit::render_screen(&mut c, 80, 25));
    g.finish();
}

// ── The tab menu (Ctrl+digit, F2, clicks) ───────────────────────────

fn ctrl(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

fn text(c: &mut Chat, w: u16, h: u16) -> String {
    testkit::text(&testkit::render_screen(c, w, h))
}

/// The bottom bar's row.
fn bar(c: &mut Chat) -> String {
    testkit::rows(&testkit::render_screen(c, 160, 25)).pop().expect("a bottom bar")
}

#[test]
fn ctrl_digit_shows_a_tab_and_a_second_press_opens_its_menu() {
    let mut g = goldens();
    let mut c = chat().enhanced(true);
    drawn(&mut c, &[ctrl('3')], 80, 25);
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    assert!(!text(&mut c, 80, 25).contains("pick one"), "the first press only shows the tab");
    drawn(&mut c, &[ctrl('3')], 80, 25);
    let shown = text(&mut c, 80, 25);
    for item in ["pick one: #deploy", "Add agent ▸", "Invite agent ▸", "Remove agent ▸", "Describe…", "Clear history", "Join", "Delete channel"] {
        assert!(shown.contains(item), "{item}: {shown}");
    }
    assert!(!shown.contains("Leave"), "the human is not in #deploy: Join, not Leave");
    g.check("tab-menu-deploy-80x25", &testkit::render_screen(&mut c, 80, 25));
    assert!(press(&mut c, key(KeyCode::Esc)), "Esc closes the menu, not the chat");
    assert!(!text(&mut c, 80, 25).contains("pick one"));
    assert!(c.input().is_empty(), "Ctrl+digits type nothing");
    g.finish();
}

#[test]
fn the_menus_differ_by_tab() {
    let mut c = chat().enhanced(true);
    press(&mut c, ctrl('1'));
    let merged = text(&mut c, 80, 25);
    assert!(merged.contains("pick one: merged") && merged.contains("Clear view") && !merged.contains("Clear history"), "{merged}");
    press(&mut c, key(KeyCode::Esc));
    press(&mut c, ctrl('2'));
    press(&mut c, ctrl('2'));
    let open = text(&mut c, 80, 25);
    assert!(open.contains("pick one: #open") && open.contains("Clear history") && !open.contains("Delete channel"), "{open}");
}

#[test]
fn clear_history_asks_first_and_y_runs_the_purge() {
    let mut g = goldens();
    let mut c = chat().enhanced(true).dry_run(true);
    // #open's menu puts the harmless item first: Clear view, then Clear history.
    let open_clear_history = |c: &mut Chat| {
        press(c, ctrl('2'));
        press(c, ctrl('2'));
        press(c, key(KeyCode::Down));
        press(c, key(KeyCode::Enter));
    };
    open_clear_history(&mut c);
    assert!(bar(&mut c).contains("clear #open history? deletes what every live agent has read, older than 90 s"), "{}", bar(&mut c));
    g.check("tab-menu-confirm-120x40", &testkit::render_screen(&mut c, 120, 40));
    assert!(press(&mut c, key(KeyCode::Esc)), "Esc keeps the history and the chat");
    assert_eq!(c.status(), "kept");
    press(&mut c, ctrl('2'));
    press(&mut c, key(KeyCode::Down));
    press(&mut c, key(KeyCode::Enter));
    press(&mut c, key(KeyCode::Char('y')));
    assert_eq!(c.status(), "dry run: /purge not run");
    assert!(c.input().is_empty(), "the y answered; it typed nothing");
    // Delete channel on a named channel: the last item. Another character
    // keeps the channel and goes on into the draft.
    press(&mut c, ctrl('3'));
    press(&mut c, ctrl('3'));
    press(&mut c, key(KeyCode::End));
    press(&mut c, key(KeyCode::Enter));
    assert!(bar(&mut c).contains("delete #deploy and its history?"), "{}", bar(&mut c));
    press(&mut c, key(KeyCode::Char('n')));
    assert_eq!((c.status(), c.input().text()), ("kept", "n"));
    g.finish();
}

/// A click on a tab opens its menu with the first item under the cursor:
/// merged's Clear view asks before it empties the view.
#[test]
fn a_click_then_enter_on_merged_asks_before_clearing_the_view() {
    let mut c = chat().enhanced(true);
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "1 merged").expect("the merged tab");
    left(&mut c, (x, y));
    press(&mut c, key(KeyCode::Enter));
    assert!(bar(&mut c).contains("clear the view? the bus keeps every message"), "{}", bar(&mut c));
    assert!(!c.signals().is_empty(), "nothing cleared before the y");
    press(&mut c, key(KeyCode::Esc));
    assert_eq!((c.status(), c.signals().is_empty()), ("kept", false));
    left(&mut c, (x, y));
    press(&mut c, key(KeyCode::Enter));
    press(&mut c, key(KeyCode::Char('y')));
    assert_eq!(c.status(), "transcript cleared");
    assert!(c.signals().is_empty());

    // The key help names what a click on a tab does.
    press(&mut c, key(KeyCode::F(1)));
    let help = text(&mut c, 100, 60);
    assert!(help.contains("a tab shows it and opens its menu"), "{help}");
}

#[test]
fn a_tab_change_the_tab_bar_or_a_click_drops_a_waiting_question() {
    let ask = |c: &mut Chat| {
        press(c, ctrl('3'));
        press(c, ctrl('3'));
        press(c, key(KeyCode::End));
        press(c, key(KeyCode::Enter));
        assert!(bar(c).contains("delete #deploy"), "{}", bar(c));
    };
    // A tab change.
    let mut c = chat().enhanced(true).dry_run(true);
    ask(&mut c);
    press(&mut c, ctrl('4'));
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()));
    assert!(!bar(&mut c).contains("delete #deploy"));
    press(&mut c, key(KeyCode::Char('y')));
    assert_eq!(c.input().text(), "y", "the question is gone: y is typing");
    // The tab bar's focus.
    let mut c = chat().enhanced(true).dry_run(true);
    ask(&mut c);
    press(&mut c, key(KeyCode::F(2)));
    assert_eq!(c.status(), "kept");
    press(&mut c, key(KeyCode::Esc));
    press(&mut c, key(KeyCode::Char('y')));
    assert_eq!(c.input().text(), "y");
    // A click on a tab.
    let mut c = chat().enhanced(true).dry_run(true);
    ask(&mut c);
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "2 #open").expect("the tab is drawn");
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    assert_eq!(c.status(), "kept");
    press(&mut c, key(KeyCode::Esc));
    press(&mut c, key(KeyCode::Char('y')));
    assert_eq!(c.input().text(), "y");
}

#[test]
fn the_plus_slot_takes_a_new_channels_name() {
    let mut c = chat().enhanced(true).dry_run(true);
    typed(&mut c, "a draft");
    plus_slot(&mut c);
    assert!(bar(&mut c).contains("new channel: type its name"), "{}", bar(&mut c));
    assert!(c.input().is_empty(), "the compose box is lent to the name");
    typed(&mut c, "no spaces");
    press(&mut c, key(KeyCode::Enter));
    assert!(c.status().starts_with("new channel:"), "a bad name is refused: {}", c.status());
    press(&mut c, key(KeyCode::Esc));
    assert_eq!(c.input().text(), "a draft", "Esc gives the draft back");
    plus_slot(&mut c);
    typed(&mut c, "#topic");
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.status(), "dry run: /channels create not run");
    assert_eq!(c.input().text(), "a draft");
}

/// Enter on the `+` slot goes straight to the name prompt: no menu between.
#[test]
fn golden_enter_on_the_plus_slot_asks_the_name_at_once() {
    let mut g = goldens();
    let mut c = chat().dry_run(true);
    typed(&mut c, "a draft");
    plus_slot(&mut c);
    let shown = text(&mut c, 80, 25);
    assert!(!shown.contains("pick one") && !shown.contains("New channel…"), "no menu: {shown}");
    assert!(bar(&mut c).contains("new channel: type its name"), "{}", bar(&mut c));
    g.check("new-channel-prompt-80x25", &testkit::render_screen(&mut c, 80, 25));
    // A click on the slot does the same.
    press(&mut c, key(KeyCode::Esc));
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, " + ").expect("the slot is drawn");
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x + 1, y));
    assert!(bar(&mut c).contains("new channel: type its name"), "{}", bar(&mut c));
    assert!(!text(&mut c, 80, 25).contains("pick one"));
    g.finish();
}

/// Ctrl+N asks the name from the compose box, keeping the draft, and from
/// the tab bar.
#[test]
fn golden_ctrl_n_asks_a_new_channels_name_and_gives_the_draft_back() {
    let mut g = goldens();
    let mut c = chat().dry_run(true);
    typed(&mut c, "a draft");
    press(&mut c, ctrl('n'));
    assert!(c.input().is_empty(), "the compose box is lent to the name");
    typed(&mut c, "release");
    g.check("ctrl-n-new-channel-draft-80x25", &testkit::render_screen(&mut c, 80, 25));
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.status(), "dry run: /channels create not run");
    assert_eq!(c.input().text(), "a draft", "the draft is back");
    // Pressed again while the name is being typed, it keeps the name apart from the draft.
    press(&mut c, ctrl('n'));
    typed(&mut c, "half");
    press(&mut c, ctrl('n'));
    assert_eq!(c.input().text(), "half");
    press(&mut c, key(KeyCode::Esc));
    assert_eq!(c.input().text(), "a draft");
    // From the tab bar: the focus goes back and the prompt opens.
    press(&mut c, key(KeyCode::F(2)));
    press(&mut c, ctrl('n'));
    assert!(bar(&mut c).contains("new channel: type its name") && !bar(&mut c).contains("TABS"), "{}", bar(&mut c));
    press(&mut c, key(KeyCode::Esc));
    assert_eq!(c.input().text(), "a draft");
    g.finish();
}

/// Ctrl+N while a description is being typed keeps the description and says
/// why nothing opened.
#[test]
fn ctrl_n_never_throws_away_a_description_being_typed() {
    let mut c = chat().enhanced(true).dry_run(true);
    typed(&mut c, "a draft");
    drawn(&mut c, &[ctrl('3'), ctrl('3')], 80, 25);
    for _ in 0..3 {
        press(&mut c, key(KeyCode::Down));
    }
    press(&mut c, key(KeyCode::Enter));
    assert!(bar(&mut c).contains("describe #deploy"), "{}", bar(&mut c));
    typed(&mut c, "half a line");
    press(&mut c, ctrl('n'));
    assert_eq!(c.input().text(), "half a line", "the description stays");
    assert_eq!(c.status(), "finish the description, or Esc, first");
    press(&mut c, key(KeyCode::Esc));
    assert_eq!(c.input().text(), "a draft");
}

/// Ctrl+N asks a name; it never sends and never quits. Without the keyboard
/// enhancement the terminal sends the byte 0x0E, which crossterm's parser
/// reads as this same Ctrl+n event (`parse.rs`: `0x01..=0x1A` is
/// Ctrl+letter, apart from CR, LF, Tab, ESC and DEL); the test drives the
/// event, not the byte.
#[test]
fn ctrl_n_never_quits_or_sends() {
    for draft in ["", "hello"] {
        let mut c = chat().dry_run(true);
        typed(&mut c, draft);
        let before = c.signals().len();
        assert!(press(&mut c, ctrl('n')), "draft {draft:?}: Ctrl+N closed the chat");
        assert!(bar(&mut c).contains("new channel: type its name"), "{}", bar(&mut c));
        assert_eq!(c.signals().len(), before, "nothing was sent");
        assert!(press(&mut c, key(KeyCode::Esc)), "Esc cancels the name, not the chat");
        assert_eq!(c.input().text(), draft);
    }
}

/// The tab bar's focus on a channel tab, on `≡` and on `+`: reverse video
/// behind a ▸, and TABS in the footer.
#[test]
fn golden_the_tab_bar_focus_is_marked_on_every_kind_of_tab() {
    let mut g = goldens();
    let mut c = chat().dry_run(true);
    drawn(&mut c, &[key(KeyCode::F(2)), key(KeyCode::Right), key(KeyCode::Right)], 80, 25);
    assert!(text(&mut c, 80, 25).contains("▸3 #deploy"));
    g.check("tab-bar-focused-80x25", &testkit::render_screen(&mut c, 80, 25));
    drawn(&mut c, &[key(KeyCode::Left), key(KeyCode::Left), key(KeyCode::Left)], 80, 25);
    assert!(text(&mut c, 80, 25).contains("▸≡"));
    g.check("tab-bar-focused-common-80x25", &testkit::render_screen(&mut c, 80, 25));
    drawn(&mut c, &[key(KeyCode::Left)], 80, 25);
    assert!(text(&mut c, 80, 25).contains("▸+"));
    g.check("tab-bar-focused-plus-80x25", &testkit::render_screen(&mut c, 80, 25));
    drawn(&mut c, &[key(KeyCode::Esc)], 80, 25);
    assert!(!text(&mut c, 80, 25).contains('▸'));
    g.finish();
}

#[test]
fn f2_moves_along_the_tab_bar_and_enter_opens_the_menu() {
    // The fallback where the terminal reports no Ctrl+digits.
    let mut c = chat();
    drawn(&mut c, &[key(KeyCode::F(2))], 80, 25);
    assert!(bar(&mut c).contains("TABS") && bar(&mut c).contains("← → move · Enter menu · Ctrl+N new channel · Esc back"), "{}", bar(&mut c));
    drawn(&mut c, &[key(KeyCode::Right), key(KeyCode::Right)], 80, 25);
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    press(&mut c, key(KeyCode::Enter));
    assert!(text(&mut c, 80, 25).contains("pick one: #deploy"));
    press(&mut c, key(KeyCode::Esc));
    // To the + slot: it is pointed at, never shown, and Enter asks a name.
    press(&mut c, ctrl('t'));
    press(&mut c, key(KeyCode::Right));
    press(&mut c, key(KeyCode::Right));
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()), "the + slot is not a tab");
    press(&mut c, key(KeyCode::Enter));
    assert!(bar(&mut c).contains("new channel:"), "{}", bar(&mut c));
}

#[test]
fn a_right_click_opens_a_tabs_menu_without_showing_it() {
    let mut c = chat();
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "3 #deploy").expect("the tab is drawn");
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    assert!(text(&mut c, 80, 25).contains("pick one: #deploy"));
    press(&mut c, key(KeyCode::Esc));
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "4 #infra").expect("the tab is drawn");
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Right), x, y));
    assert!(text(&mut c, 80, 25).contains("pick one: #infra"));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()), "a right click opens the menu without showing the tab");
}

/// What a terminal without the kitty keyboard protocol sends for Ctrl+1
/// through Ctrl+9, as crossterm reads it: `1`, NUL (Ctrl+Space), ESC,
/// FS..US (Ctrl+4..7), DEL (Backspace), `9`. None may quit or send.
fn legacy_ctrl_digits() -> Vec<KeyEvent> {
    let mut out = vec![key(KeyCode::Char('1')), KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL), key(KeyCode::Esc)];
    out.extend(['4', '5', '6', '7'].map(ctrl));
    out.extend([key(KeyCode::Backspace), key(KeyCode::Char('9'))]);
    out
}

#[test]
fn legacy_ctrl_digit_bytes_never_quit_or_send() {
    for draft in ["", "hello"] {
        let mut c = chat();
        typed(&mut c, draft);
        let before = c.signals().len();
        for k in legacy_ctrl_digits() {
            assert!(press(&mut c, k), "draft {draft:?}: {k:?} closed the chat");
        }
        assert_eq!(c.signals().len(), before, "nothing was sent");
        assert!(!c.status().starts_with("sent"), "{}", c.status());
    }
    // From an empty line the ESC byte (Ctrl+3) opens the quit question, and
    // the next Ctrl+digit byte answers no.
    let mut c = chat();
    for k in legacy_ctrl_digits().into_iter().skip(2) {
        assert!(press(&mut c, k), "{k:?} closed the chat");
    }
}

#[test]
fn the_f1_view_says_whether_ctrl_digits_arrive() {
    let mut c = chat();
    press(&mut c, key(KeyCode::F(1)));
    assert!(text(&mut c, 140, 40).contains("does not report Ctrl+digits"));
    let mut c = chat().enhanced(true);
    press(&mut c, key(KeyCode::F(1)));
    assert!(text(&mut c, 140, 40).contains("Ctrl+digits arrive"));
}

/// Open the `+` slot from the keyboard: the tab bar, then left past `≡`.
fn plus_slot(c: &mut Chat) {
    for k in [key(KeyCode::F(2)), key(KeyCode::Left), key(KeyCode::Left), key(KeyCode::Enter)] {
        press(c, k);
    }
}

#[test]
fn the_common_menu_sits_before_merged_and_merged_keeps_number_one() {
    let mut g = goldens();
    let mut c = chat().enhanced(true).dry_run(true);
    let shown = text(&mut c, 80, 25);
    assert!(shown.contains("≡    1 merged"), "{shown}");
    press(&mut c, ctrl('3'));
    press(&mut c, ctrl('1'));
    assert_eq!(c.foreground(), &Tab::Merged, "Ctrl+1 is merged, as before");
    // ≡ is the slot left of merged on the focused bar.
    press(&mut c, key(KeyCode::F(2)));
    press(&mut c, key(KeyCode::Left));
    press(&mut c, key(KeyCode::Enter));
    let menu = text(&mut c, 80, 25);
    for item in ["pick one: attend-chat", "Theme ▸", "Keybinding set ▸", "Mouse on at start", "Settings…"] {
        assert!(menu.contains(item), "{item}: {menu}");
    }
    g.check("common-menu-80x25", &testkit::render_screen(&mut c, 80, 25));
    press(&mut c, key(KeyCode::End));
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.status(), "dry run: /config not run", "Settings… is /config");
    g.finish();
}

#[test]
fn the_common_menu_offers_themes_and_keybinding_sets() {
    let mut c = chat().enhanced(true).dry_run(true);
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "≡").expect("the slot is drawn");
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    press(&mut c, key(KeyCode::Enter));
    let themes = text(&mut c, 80, 30);
    assert!(themes.contains("theme for this session") && themes.contains("terminal") && themes.contains("nord"), "{themes}");
    press(&mut c, key(KeyCode::Enter));
    assert!(c.status().starts_with("theme terminal for this session"), "{}", c.status());
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    press(&mut c, key(KeyCode::Down));
    press(&mut c, key(KeyCode::Enter));
    let sets = text(&mut c, 80, 30);
    assert!(sets.contains("ctrl: Ctrl+1-9, F2, Ctrl+T") && sets.contains("fallback only: F2, Ctrl+T, Tab"), "{sets}");
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.status(), "dry run: /config not run", "a set is written through /config");
}

// ── The left column (ADR-405) ───────────────────────────────────────

/// A claude that has gone quiet: no heartbeat, so set back in the column.
const GAMMA: &str = "/work/gamma";
const GAMMA_SID: &str = "cccccccc-0000-4000-8000-000000000003";

/// The chat with the left column shown, and a sender with no heartbeat.
fn chat_side() -> Chat {
    let mut c = chat().sidebar(true);
    c.push(sig(&format!("claude:{GAMMA_SID}"), GAMMA, 900, Channel::Open, "signing off"));
    c
}

#[test]
fn golden_the_column_lists_every_agent_and_channel() {
    let mut g = goldens();
    let mut c = chat_side();
    let shown = text(&mut c, 80, 25);
    for row in ["Agents 2/4", "Channels 4", "#open", "#deploy", "#infra", "#archive"] {
        assert!(shown.contains(row), "{row}: {shown}");
    }
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    let gamma = rows.iter().find(|r| r.contains("gamma")).expect("the quiet agent is listed");
    assert!(gamma.contains('○'), "set back: {gamma}");
    assert!(!rows[rows.len() - 2].contains("@aaron"), "the legend row is the column's: {:?}", rows[rows.len() - 2]);
    for (w, h) in SIZES {
        g.check(&format!("sidebar-{w}x{h}"), &testkit::render_screen(&mut c, w, h));
    }
    g.finish();
}

#[test]
fn golden_the_sidebar_key_and_a_narrow_terminal_give_the_legend_row_back() {
    let mut g = goldens();
    let mut c = chat_side();
    press(&mut c, key(KeyCode::F(3)));
    assert!(!c.sidebar_shown());
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    assert!(rows[rows.len() - 2].contains("@"), "the legend row is back: {:?}", rows[rows.len() - 2]);
    g.check("sidebar-off-80x25", &testkit::render_screen(&mut c, 80, 25));
    press(&mut c, key(KeyCode::F(3)));
    assert!(c.sidebar_shown());
    let narrow = text(&mut c, 70, 25);
    assert!(!narrow.contains("Agents"), "too narrow for the column: {narrow}");
    g.check("sidebar-narrow-70x25", &testkit::render_screen(&mut c, 70, 25));
    g.finish();
}

#[test]
fn the_column_keeps_the_helper_row_for_commands() {
    let mut c = chat_side();
    typed(&mut c, "/he");
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    assert!(rows[rows.len() - 2].contains("/help"), "{:?}", rows[rows.len() - 2]);
}

#[test]
fn a_click_on_an_agent_row_opens_its_menu_and_on_a_channel_row_the_channels() {
    let mut c = chat_side().enhanced(true);
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "gamma").expect("the agent row");
    left(&mut c, (x, y));
    let menu = text(&mut c, 80, 25);
    assert!(menu.contains("pick one: @Lyra") && menu.contains("Address @Lyra") && menu.contains("Who is this"), "{menu}");
    assert!(!menu.contains("Remove from"), "merged shows no channel to remove from: {menu}");
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.input().text(), "@Lyra ", "the first item addresses it");
    // Who is this, for a quiet claude /whois cannot resolve.
    left(&mut c, (x, y));
    press(&mut c, key(KeyCode::Down));
    press(&mut c, key(KeyCode::Enter));
    assert!(c.status().contains("session cccccccc") && c.status().contains("quiet"), "{}", c.status());
    let mut c = chat_side().enhanced(true);

    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "#infra").expect("the channel row");
    left(&mut c, (x, y));
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()));
    assert!(text(&mut c, 80, 25).contains("pick one: #infra"));
    press(&mut c, key(KeyCode::Esc));

    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "#archive").expect("the swept channel's row");
    left(&mut c, (x, y));
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()), "a channel with no tab is not shown");
    assert!(text(&mut c, 80, 25).contains("pick one: #archive"), "its menu opens");
}

#[test]
fn a_partial_name_marks_its_rows_in_the_column() {
    let mut c = chat_side();
    typed(&mut c, "hi #de");
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "#deploy").expect("the row");
    let cell = buf.cell((x, y)).expect("a cell");
    assert!(cell.modifier.contains(agent_tui::ratatui::style::Modifier::UNDERLINED), "Tab's target is marked");
}

/// In the round shape at full colour each row is a lozenge: a cap, the
/// name on the identity colour, joins, and an inactive agent on the
/// desaturated steps of its colour.
#[test]
fn golden_the_column_draws_lozenges_in_the_round_shape() {
    let mut g = goldens();
    let f = fixture();
    let dir = f.root.join("config/agent-ways/themes");
    let (painter, _) = agent_theme::Painter::named_in(Some("nord"), &dir, ColorDepth::TrueColor);
    let mut c = Chat::new(None, Palette { painter }, Shape::ROUND).heartbeat(false).pinned(UNIX_EPOCH + Duration::from_secs(NOW), 0);
    for s in transcript() {
        c.push(s);
    }
    c.push(sig(&format!("claude:{GAMMA_SID}"), GAMMA, 900, Channel::Open, "signing off"));
    let buf = testkit::render_screen(&mut c, 80, 25);
    let rows = testkit::rows(&buf);
    let laszlo = rows.iter().position(|r| r.contains("Laszlo") && r.contains('\u{e0b6}')).expect("a capped lozenge");
    assert!(rows[laszlo].matches('\u{e0b4}').count() == 3, "three joins: {}", rows[laszlo]);
    let (x, y) = testkit::find(&buf, "Laszlo").expect("the row");
    let live_bg = buf.cell((x, y)).unwrap().bg;
    let (x, y) = testkit::find(&buf, "Lyra").expect("the quiet agent's row");
    let gone_bg = buf.cell((x, y)).unwrap().bg;
    let lyra = agent_identity::Identity::for_cwd(GAMMA, ColorDepth::TrueColor).palette.rgb;
    let faded = agent_theme::IdentitySteps::of(agent_theme::Rgb(lyra.0, lyra.1, lyra.2), false).name.bg;
    assert_eq!(gone_bg, agent_tui::ratatui::style::Color::Rgb(faded.0, faded.1, faded.2), "the desaturated step");
    assert_ne!(live_bg, gone_bg);
    g.check("sidebar-round-truecolor-80x25", &buf);
    g.finish();
}

/// The column without the mouse: F2, then Down into the column; Up and
/// Down move, Enter acts on the row, Esc goes back (ADR-405).
#[test]
fn golden_the_keyboard_walks_the_column() {
    let mut g = goldens();
    let mut c = chat_side().enhanced(true).dry_run(true);
    testkit::render_screen(&mut c, 80, 25);
    press(&mut c, key(KeyCode::F(2)));
    assert!(bar(&mut c).contains("↓ the column"), "the tab bar names the way down: {}", bar(&mut c));
    press(&mut c, key(KeyCode::Down));
    assert!(!c.app().strip_focused(), "the focus left the tab bar");
    press(&mut c, key(KeyCode::Down));
    let shown = testkit::render_screen(&mut c, 80, 25);
    g.check("sidebar-keyboard-80x25", &shown);
    let rows = testkit::rows(&shown);
    let at = rows.iter().find(|r| r.contains('▸')).expect("the cursor is drawn");
    assert!(at.contains("#deploy"), "Down from #open is #deploy: {at}");
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    assert!(text(&mut c, 80, 25).contains("pick one: #deploy"), "Enter opens the channel's menu");
    press(&mut c, key(KeyCode::Esc));

    // Up past the top goes back to the tab bar.
    press(&mut c, key(KeyCode::F(2)));
    press(&mut c, key(KeyCode::Down));
    assert!(bar(&mut c).contains("↑↓ move · Enter · Esc back"), "the bar says how the column is worked: {}", bar(&mut c));
    for _ in 0..10 {
        press(&mut c, key(KeyCode::Up));
        if c.app().strip_focused() {
            break;
        }
    }
    assert!(c.app().strip_focused(), "Up past the top focuses the tab bar");
    assert_eq!(c.input().text(), "", "typing nothing");
    press(&mut c, key(KeyCode::Down));
    press(&mut c, key(KeyCode::Home));
    press(&mut c, key(KeyCode::Enter));
    assert!(text(&mut c, 80, 25).contains("pick one: @Laszlo"), "Enter on an agent opens its menu");
    press(&mut c, key(KeyCode::Enter));
    assert!(c.input().text().starts_with("@Laszlo"), "{:?}", c.input().text());
    g.finish();
}

/// The column gives up the keyboard when it is hidden for width, when F2
/// takes the tab bar, and on a click outside it.
#[test]
fn the_column_lets_go_of_the_keyboard_when_it_goes_or_focus_moves() {
    let focused = || {
        let mut c = chat_side().enhanced(true).dry_run(true);
        testkit::render_screen(&mut c, 80, 25);
        press(&mut c, key(KeyCode::F(2)));
        press(&mut c, key(KeyCode::Down));
        assert!(cursor_row(&mut c, 80).is_some(), "the column has the keyboard");
        c
    };
    // Narrower than the column and the feed: hidden, and so unfocused.
    let mut c = focused();
    let narrow = text(&mut c, 70, 25);
    assert!(!narrow.contains("↑↓ move"), "no hint for a column not drawn: {narrow}");
    assert!(cursor_row(&mut c, 80).is_none(), "the cursor does not come back with the width");
    typed(&mut c, "x");
    assert_eq!(c.input().text(), "x", "keys go to the compose box");
    // F2 takes the tab bar from the column.
    let mut c = focused();
    press(&mut c, key(KeyCode::F(2)));
    assert!(c.app().strip_focused() && cursor_row(&mut c, 80).is_none());
    // A click in the feed.
    let mut c = focused();
    left(&mut c, (50, 5));
    assert!(cursor_row(&mut c, 80).is_none(), "a click outside the column");
    // The tab bar names the column only while it is drawn.
    let mut c = chat_side().enhanced(true);
    testkit::render_screen(&mut c, 70, 25);
    press(&mut c, key(KeyCode::F(2)));
    assert!(!bar(&mut c).contains("the column"), "{}", bar(&mut c));
    press(&mut c, key(KeyCode::Esc));
    press(&mut c, key(KeyCode::F(1)));
    assert!(!text(&mut c, 70, 40).contains("F2, then"), "the help leaves out a column not drawn");
}

/// The row the column's cursor is on, if it has one.
fn cursor_row(c: &mut Chat, w: u16) -> Option<String> {
    testkit::rows(&testkit::render_screen(c, w, 25)).into_iter().find(|r| r.contains('▸') && !r.starts_with(" ≡"))
}

/// The column with fifteen more agents than it has rows for.
fn chat_crowded() -> Chat {
    let mut c = chat_side();
    for n in 0..15 {
        c.push(sig(&format!("claude:dddddddd-0000-4000-8000-{n:012}"), &format!("/work/crowd-{n:02}"), 1000 + n, Channel::Open, "here"));
    }
    c
}

/// Every row of a long list is reachable by keyboard and by mouse: the
/// cursor scrolls the list to stay in view, and the wheel over a list
/// scrolls it.
#[test]
fn a_long_column_scrolls_with_the_cursor_and_the_wheel() {
    let mut c = chat_crowded().enhanced(true).dry_run(true);
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    let header = rows.iter().find(|r| r.contains("Agents")).expect("the header").clone();
    assert!(header.contains('↓') && !header.contains('↑'), "rows below, none above: {header}");
    let first_agent = rows.iter().position(|r| r.contains("Agents")).unwrap() + 1;
    let top_before = rows[first_agent].clone();
    // The keyboard: from the first channel, Up into the agents and on past
    // the rows shown; the cursor is always drawn.
    press(&mut c, key(KeyCode::F(2)));
    press(&mut c, key(KeyCode::Down));
    for _ in 0..19 {
        press(&mut c, key(KeyCode::Up));
        assert!(cursor_row(&mut c, 80).is_some(), "the cursor stays in view");
    }
    press(&mut c, key(KeyCode::Home));
    let at = cursor_row(&mut c, 80).expect("the cursor");
    assert!(at.contains("Laszlo"), "Home reaches the first agent: {at}");
    press(&mut c, key(KeyCode::End));
    assert!(cursor_row(&mut c, 80).expect("the cursor").contains("#archive"), "End, the last channel");
    press(&mut c, key(KeyCode::Esc));
    // The wheel over the agents scrolls them, and the feed stays put.
    let mut c = chat_crowded().enhanced(true);
    testkit::render_screen(&mut c, 80, 25);
    let ev = |kind| agent_tui::ratatui::crossterm::event::MouseEvent { kind, column: 5, row: first_agent as u16 + 1, modifiers: KeyModifiers::NONE };
    for _ in 0..30 {
        c.mouse(ev(MouseEventKind::ScrollDown));
    }
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    assert_ne!(rows[first_agent], top_before, "the list moved");
    let header = rows.iter().find(|r| r.contains("Agents")).unwrap();
    assert!(header.contains('↑') && !header.contains('↓'), "at the last row: {header}");
    assert_eq!(c.scroll(), 0, "the feed did not scroll");
    for _ in 0..30 {
        c.mouse(ev(MouseEventKind::ScrollUp));
    }
    assert_eq!(testkit::rows(&testkit::render_screen(&mut c, 80, 25))[first_agent], top_before, "back to the top");
}

/// An agent of the shown channel can be removed from its row, after a y.
#[test]
fn an_agent_row_removes_it_from_the_shown_channel() {
    let mut c = chat_side().enhanced(true).dry_run(true);
    press(&mut c, ctrl('3'));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "beta").expect("the member's row");
    left(&mut c, (x, y));
    let menu = text(&mut c, 80, 25);
    assert!(menu.contains("Remove from #deploy"), "{menu}");
    press(&mut c, key(KeyCode::End));
    press(&mut c, key(KeyCode::Enter));
    assert!(bar(&mut c).contains("from #deploy? y removes"), "{}", bar(&mut c));
    press(&mut c, key(KeyCode::Char('y')));
    assert_eq!(c.status(), "dry run: /kick not run");
}

/// A paste with newlines is one draft in the compose box: nothing sent,
/// the lines kept, the cursor after them.
#[test]
fn a_multi_line_paste_is_one_draft_and_sends_nothing() {
    let mut c = chat_side();
    typed(&mut c, "see: ");
    let before = c.signals().len();
    assert!(testkit::play(&mut c, &[agent_tui::ratatui::crossterm::event::Event::Paste("a\nb\r\nc".into())], Some((80, 25))));
    assert_eq!(c.input().text(), "see: a\nb\nc", "one draft, the lines kept");
    assert_eq!(c.input().cursor(), "see: a\nb\nc".chars().count());
    assert_eq!((c.signals().len(), c.status()), (before, ""), "nothing sent");
    let rows = testkit::rows(&testkit::render_screen(&mut c, 80, 25));
    assert!(rows.iter().any(|r| r.contains("> see: a")) && rows.iter().any(|r| r.contains("  c")), "the box grows to the lines");
}

/// The mouse alone shows and hides the column, from the common menu.
#[test]
fn the_common_menu_hides_and_shows_the_column() {
    let mut c = chat_side().enhanced(true);
    let open_common = |c: &mut Chat| {
        let buf = testkit::render_screen(c, 80, 25);
        let at = testkit::find(&buf, "≡").expect("the common slot");
        left(c, at);
        text(c, 80, 25)
    };
    let menu = open_common(&mut c);
    assert!(menu.contains("Hide column") && !menu.contains("Show column"), "{menu}");
    let buf = testkit::render_screen(&mut c, 80, 25);
    let at = testkit::find(&buf, "Hide column").expect("the item");
    c.mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1));
    assert!(!c.sidebar_shown(), "hidden");
    let menu = open_common(&mut c);
    assert!(menu.contains("Show column"), "{menu}");
    press(&mut c, key(KeyCode::Esc));
}

/// Join and Leave from the menu, with no slash command.
#[test]
fn a_channel_menu_joins_a_channel_the_human_is_not_in() {
    let mut c = chat_side().enhanced(true).dry_run(true);
    let buf = testkit::render_screen(&mut c, 80, 25);
    let (x, y) = testkit::find(&buf, "#deploy").expect("the row");
    left(&mut c, (x, y));
    let shown = text(&mut c, 80, 25);
    assert!(shown.contains("Join") && !shown.contains("Leave"), "{shown}");
    press(&mut c, key(KeyCode::End));
    press(&mut c, key(KeyCode::Up));
    press(&mut c, key(KeyCode::Enter));
    assert_eq!(c.status(), "dry run: /join not run");
}
