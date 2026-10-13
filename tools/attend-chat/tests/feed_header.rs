//! The feed's message header through real drawing and input: one band per
//! message across the feed, in the sender's colour as the left column draws
//! it, holding the channel marks and the time, the name and the project;
//! a rule under it; the body at full width below. In its own file, so its
//! own fixture: alpha (a claude with a long project name) in #deploy, a
//! human, and messages on #open and #deploy.

mod support;

use std::time::{Duration, UNIX_EPOCH};

use agent_theme::ColorDepth;
use agent_tui::ratatui::buffer::Buffer;
use agent_tui::ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::ratatui::style::{Color, Modifier};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const NOW: u64 = 1_790_949_600;
const ALPHA_CWD: &str = "/work/the-alpha-project-with-a-very-long-name";

fn sig(id: &str, from: &str, cwd: &str, ago: u64, channel: Channel, message: &str) -> Signal {
    Signal { id: id.into(), from: from.into(), project: String::new(), cwd: cwd.into(), reply_to: None, message: message.into(), ts: NOW - ago, channel }
}

/// A chat at truecolor in the nord theme, so a band's colours can be read.
fn chat() -> Chat {
    support::fixture("feed-header", |base| {
        support::pin_alive(ALPHA_SID);
        let alpha = attend_groups::Groups::new(base, ALPHA_SID);
        alpha.create("deploy", Some("rollout")).unwrap();
        alpha.join("deploy", true).unwrap();
    });
    let (palette, _) = attend_chat::theme::palette(Some("nord"), ColorDepth::TrueColor);
    let mut c = Chat::new(None, palette, Shape::ROUND).sidebar(true).heartbeat(false).enhanced(true).dry_run(true).pinned(UNIX_EPOCH + Duration::from_secs(NOW), 0);
    let _ = Palette::default();
    c.push(sig("t-1", &format!("claude:{ALPHA_SID}"), ALPHA_CWD, 300, Channel::Open, "a message in the feed that runs long enough to wrap past the width of a narrow terminal row"));
    c.push(sig("t-2", "external:aaron@kitty", "/home/aaron", 120, Channel::Open, "from the human at the keyboard"));
    c.push(sig("t-3", &format!("claude:{ALPHA_SID}"), ALPHA_CWD, 60, Channel::Group("deploy".into()), "rollout is green"));
    c
}

fn frame(c: &mut Chat, (w, h): (u16, u16)) -> Buffer {
    testkit::render_screen(c, w, h)
}

/// The rows right of the column's rule, inside the feed's border.
fn feed_rows(buf: &Buffer) -> Vec<(usize, String)> {
    testkit::rows(buf)
        .into_iter()
        .enumerate()
        .filter_map(|(y, r)| {
            let mut parts = r.splitn(3, '│');
            let _column = parts.next()?;
            let feed = parts.next()?;
            Some((y, feed.to_string()))
        })
        .collect()
}

/// The row holding `text` in the feed, and the column it starts at.
fn find(buf: &Buffer, text: &str) -> (u16, u16) {
    let rows = testkit::rows(buf);
    for (y, r) in rows.iter().enumerate() {
        let rule = r.find('│').map_or(0, |x| x + '│'.len_utf8());
        if let Some(x) = r[rule..].find(text) {
            return (r[..rule + x].chars().count() as u16, y as u16);
        }
    }
    panic!("no {text:?} in the feed: {rows:#?}");
}

/// Whether `row` holds a whole `HH:MM`.
fn has_time(row: &str) -> bool {
    let c: Vec<char> = row.chars().collect();
    c.windows(5).any(|w| w[0].is_ascii_digit() && w[1].is_ascii_digit() && w[2] == ':' && w[3].is_ascii_digit() && w[4].is_ascii_digit())
}

/// The alpha agent's name as the column shows it.
fn alpha_name(buf: &Buffer) -> String {
    let rows = testkit::rows(buf);
    let row = rows.iter().find(|r| r.contains("the-alpha") || r.contains('●')).expect("alpha's row");
    let column = row.split('│').next().unwrap();
    column.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric())).find(|w| !w.is_empty()).expect("a name").to_string()
}

/// One header row per message: the time, the name and the project on one
/// line, a rule under it across the feed, and the body below at the
/// feed's full width.
#[test]
fn a_message_is_a_header_line_a_rule_and_a_full_width_body() {
    for size in [(80, 25), (120, 35)] {
        let mut c = chat();
        let buf = frame(&mut c, size);
        let name = alpha_name(&buf);
        let (x, y) = find(&buf, "a message in the feed");
        let rows = testkit::rows(&buf);
        let header = &rows[y as usize - 2];
        assert!(header.contains(&name) && has_time(header) && header.contains("the-alpha"), "{size:?}: one header line with the time, name and project: {rows:#?}");
        assert!(rows[y as usize - 1].contains("──────────"), "{size:?}: a rule under it: {rows:#?}");
        let feed_left = rows[y as usize].find('│').map(|i| rows[y as usize][..i].chars().count() + 2).unwrap();
        assert!(usize::from(x) <= feed_left + 1, "{size:?}: the body starts at the feed's left edge, not beside a card: {rows:#?}");
        assert!(!rows.iter().any(|r| r.contains('╭') && r.contains("╮ a message")), "no label card");
    }
}

/// The band wears the sender's colour as the column's row does: the name
/// on the same ground.
#[test]
fn the_band_wears_the_columns_colour_for_the_sender() {
    let mut c = chat();
    let buf = frame(&mut c, (120, 35));
    let name = alpha_name(&buf);
    let rows = testkit::rows(&buf);
    let column = rows.iter().enumerate().find_map(|(y, r)| r.split('│').next().unwrap().find(&name).map(|x| (r[..x].chars().count() as u16, y as u16))).expect("the column row");
    let header = find(&buf, &name);
    let ground = |at: (u16, u16)| buf.cell(at).map(|c| c.bg).unwrap_or(Color::Reset);
    assert_ne!(ground(header), Color::Reset, "the name sits on a band");
    assert_eq!(ground(header), ground(column), "the column's colour");
}

/// At 80 columns a long name and project are cut, the time never.
#[test]
fn at_80_columns_the_time_stays_whole_and_the_rest_is_cut() {
    let mut c = chat();
    let buf = frame(&mut c, (80, 25));
    let (_, y) = find(&buf, "a message in the feed");
    let header = &testkit::rows(&buf)[y as usize - 2];
    assert!(has_time(header), "the time whole: {header:?}");
    assert!(header.contains('…'), "the project cut: {header:?}");
    let feed = feed_rows(&buf).into_iter().find(|(r, _)| *r == y as usize - 2).map(|(_, f)| f).unwrap();
    assert!(feed.chars().count() <= 80, "{feed:?}");
}

/// A human's message gets a header too, its name in the human's style.
#[test]
fn a_humans_message_has_its_header() {
    let mut c = chat();
    let buf = frame(&mut c, (120, 35));
    let (x, y) = find(&buf, "aaron");
    let rows = testkit::rows(&buf);
    assert!(rows[y as usize + 1].contains("──────────") && rows[y as usize + 2].contains("from the human"), "{rows:#?}");
    assert_ne!(buf.cell((x, y)).map(|c| c.bg), Some(Color::Reset), "on a band");
}

/// A click on a message selects it: its rule turns heavy.
#[test]
fn a_selected_message_has_a_heavy_rule() {
    let mut c = chat();
    let buf = frame(&mut c, (120, 35));
    let at = find(&buf, "from the human");
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: at.0, row: at.1, modifiers: KeyModifiers::NONE });
    let rows = testkit::rows(&frame(&mut c, (120, 35)));
    assert!(rows[at.1 as usize - 1].contains("━━━━━━"), "{rows:#?}");
    assert!(!rows.iter().any(|r| r.contains("━━━━━━") && !r.contains("─")) || rows.iter().filter(|r| r.contains("━━━━━━")).count() == 1, "one heavy rule");
}

/// The merged feed shows every message; a channel's only its own.
#[test]
fn a_channel_shows_its_own_headers_only() {
    let mut c = chat();
    let merged = testkit::rows(&frame(&mut c, (120, 35))).join("\n");
    assert!(merged.contains("a message in the feed") && merged.contains("rollout is green"));
    testkit::drive(&mut c, &[agent_tui::ratatui::crossterm::event::KeyEvent::new(agent_tui::ratatui::crossterm::event::KeyCode::Char('2'), KeyModifiers::CONTROL)]);
    let deploy = testkit::rows(&frame(&mut c, (120, 35)));
    assert!(deploy.iter().any(|r| r.contains("rollout is green")) && !deploy.iter().any(|r| r.contains("a message in the feed")), "{deploy:#?}");
    let (_, y) = find(&frame(&mut c, (120, 35)), "rollout is green");
    assert!(deploy[y as usize - 1].contains("──────"), "with its rule");
    let _ = Modifier::BOLD;
}
